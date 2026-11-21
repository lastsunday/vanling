//! The pixels a page is drawn into, and the only marks there are.

use crate::light::{Rgb, invert_rgb565, rgb565};

use super::color::{Color, blend};
use super::panel::CONTENT_BOTTOM;

use super::Rect;
use super::font::{Chars, Face, FontFamily, Glyph, Metrics, Outline, VectorFace};

/// The points one glyph may hold before [`Canvas::draw_outline_at`] refuses it. The widest
/// glyph in the shipped table holds 328; a cutter that outgrows this shows up in
/// `no_outline_is_wider_than_the_canvas_scratch` rather than as a silently thin glyph.
pub(crate) const OUTLINE_POINTS: usize = 384;

/// A frame buffer and the operations every page draws with.
pub struct Canvas<'a> {
    frame: &'a mut [u8],
    width: usize,
    height: usize,
    ground: Option<Rgb>,
    color: Option<Color>,
    clips: [Rect; 4],
    depth: usize,
}

impl<'a> Canvas<'a> {
    /// A canvas over a whole frame.
    pub fn new(frame: &'a mut [u8], width: usize, height: usize, ground: Option<Rgb>) -> Self {
        debug_assert_eq!(
            frame.len(),
            width * height * 2,
            "a canvas has to cover exactly one frame"
        );
        Self {
            frame,
            width,
            height,
            ground,
            color: None,
            clips: [Rect::default(); 4],
            depth: 0,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The bytes a caller has to fold or ship after the page is drawn.
    pub fn frame(&self) -> &[u8] {
        self.frame
    }

    /// The colour this frame was painted with, if the surface still owns it.
    pub fn ground(&self) -> Option<Rgb> {
        self.ground
    }

    /// Narrows every following mark to `rect` until [`Canvas::pop_clip`].
    pub fn push_clip(&mut self, rect: Rect) {
        let rect = if self.depth == 0 {
            rect
        } else {
            self.clips[self.depth - 1].intersect(rect)
        };
        self.clips[self.depth.min(self.clips.len() - 1)] = rect;
        self.depth = (self.depth + 1).min(self.clips.len());
    }

    pub fn pop_clip(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn clipped(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return false;
        }
        if self.depth == 0 {
            return true;
        }
        self.clips[self.depth - 1].contains(x as i32, y as i32)
    }

    /// Sets the colour every following mark is laid down in; `None` turns the ground over
    /// instead, which is what a page that names no colour gets.
    pub fn set_color(&mut self, color: Option<Color>) {
        self.color = color;
    }

    /// Lays the current colour on one pixel whose bounds the caller has already checked.
    fn mark(&mut self, x: usize, y: usize) {
        let offset = (y * self.width + x) * 2;
        let packed = u16::from_be_bytes([self.frame[offset], self.frame[offset + 1]]);
        let next = match self.color {
            Some(color) => blend(packed, color),
            None => invert_rgb565(packed),
        };
        self.frame[offset..offset + 2].copy_from_slice(&next.to_be_bytes());
    }

    /// The frame's rows and columns the box at `(left, top)`, `w`×`h`, may write in: the box
    /// narrowed once to the frame and to the clip in force, so a glyph costs one intersection
    /// rather than one per pixel.
    fn reach(&self, left: isize, top: isize, w: isize, h: isize) -> (usize, usize, usize, usize) {
        let (mut x0, mut y0) = (left.max(0), top.max(0));
        let (mut x1, mut y1) = (
            (left + w).min(self.width as isize),
            (top + h).min(self.height as isize),
        );
        if self.depth > 0 {
            let clip = self.clips[self.depth - 1];
            x0 = x0.max(clip.x as isize);
            y0 = y0.max(clip.y as isize);
            x1 = x1.min(clip.right() as isize);
            y1 = y1.min(clip.bottom() as isize);
        }
        if x0 >= x1 || y0 >= y1 {
            return (0, 0, 0, 0);
        }
        (x0 as usize, y0 as usize, x1 as usize, y1 as usize)
    }

    /// Lays one pixel in the current colour.
    pub fn plot_pixel(&mut self, x: isize, y: isize) {
        if self.clipped(x, y) {
            self.mark(x as usize, y as usize);
        }
    }

    /// Lays a glyph down a pixel at a time, each set bit becoming a `scale`×`scale` block.
    /// Nothing inside the box is bounds-checked again.
    fn draw_glyph_at(&mut self, glyph: &Glyph<'_>, left: isize, top: isize, scale: usize) {
        let (x0, y0, x1, y1) = self.reach(
            left,
            top,
            (glyph.width * scale) as isize,
            (glyph.height * scale) as isize,
        );
        for y in y0..y1 {
            let row = (y as isize - top) as usize / scale;
            for x in x0..x1 {
                let column = (x as isize - left) as usize / scale;
                if glyph.ink(row, column) {
                    self.mark(x, y);
                }
            }
        }
    }

    /// Overdraws one character at a panel coordinate, `top` being the top of the line box.
    pub fn draw_char_at(&mut self, ch: char, left: isize, top: usize, metrics: Metrics) {
        if metrics.family() == FontFamily::Vector
            && let Some(outline) = VectorFace::new().outline(ch)
        {
            let baseline = top as i32 + metrics.ascent() as i32;
            self.draw_outline_at(&outline, left as i32, baseline, metrics.size());
            return;
        }
        let face = Face::new();
        let glyph = face.glyph(ch);
        let scale = metrics.scale();
        let baseline = top as isize + metrics.ascent() as isize;
        self.draw_glyph_at(
            &glyph,
            left + glyph.left * scale as isize,
            baseline - (glyph.top + glyph.height as isize) * scale as isize,
            scale,
        );
    }

    /// Fills one vector glyph's contours at a panel coordinate, its baseline on the row below
    /// the line box's ascent. Coverage is sampled on four sub-scanlines and accumulated per
    /// pixel in quarter-columns, so an edge between pixels becomes a mixed colour.
    ///
    /// The glyph's points are scaled once into [`OUTLINE_POINTS`], then every edge is filed under
    /// the first row whose samples can reach it and each row only visits the edges still crossing
    /// it. Rescaling and re-walking the whole outline on every row cost a 24 px page 238 ms per
    /// frame on the S3.
    fn draw_outline_at(&mut self, outline: &Outline<'_>, left: i32, baseline: i32, size: u16) {
        /// Coverage of a fully inked pixel: four sub-scanlines of four quarter-columns.
        const FULL: u8 = 16;
        /// The widest ink this buffer holds; the text page's face is 24 px.
        const WIDTH: usize = 64;
        /// Crossings one scanline may hold. The table's widest glyph has 17 contours.
        const CROSSINGS: usize = 48;
        /// Rows of ink one glyph may cover; a size this wide would already overrun [`WIDTH`].
        const ROWS: usize = 256;
        if size == 0 || outline.is_blank() {
            return;
        }
        let scale = |units: i32| (units * i32::from(size) * 4 + 32) / 64;
        let (ox, oy) = (left * 4, baseline * 4);
        let box_left = (ox + scale(outline.xmin as i32)).div_euclid(4);
        let box_right = (ox + scale(outline.xmax as i32) + 3).div_euclid(4);
        let box_top = (oy - scale(outline.ymax as i32)).div_euclid(4);
        let box_bottom = (oy - scale(outline.ymin as i32) + 3).div_euclid(4);
        let (x0, y0, x1, y1) = self.reach(
            box_left as isize,
            box_top as isize,
            (box_right - box_left) as isize,
            (box_bottom - box_top) as isize,
        );
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let width = x1 - x0;
        let rows = y1 - y0;
        if width > WIDTH || rows > ROWS {
            return;
        }
        let first = outline.points().start;
        let count = outline.points().len();
        debug_assert!(
            count <= OUTLINE_POINTS,
            "a glyph with {count} points overruns the canvas scratch"
        );
        let count = count.min(OUTLINE_POINTS);
        let mut scaled = [(0i16, 0i16); OUTLINE_POINTS];
        for (local, index) in (first..first + count).enumerate() {
            let (x, y) = outline.point(index);
            scaled[local] = (scale(x) as i16, -scale(y) as i16);
        }
        // Edge `local` runs to `succ[local]`; a contour closes on its own first point.
        let mut succ = [0u16; OUTLINE_POINTS];
        let mut start = first;
        for contour in outline.contours() {
            let end = outline.contour_end(contour);
            if end > start {
                let (from, to) = (start - first, end - first);
                for (offset, slot) in succ[from..to].iter_mut().enumerate() {
                    let local = from + offset;
                    *slot = if local + 1 == to {
                        from as u16
                    } else {
                        (local + 1) as u16
                    };
                }
            }
            start = end;
        }
        // File every edge under the first row whose samples can reach it. A row samples from
        // +1 to +7 quarter rows, so an edge whose top sits in a later pixel row may still cross
        // this one; `top - 4` is the row whose window ends at that top. A `u16::MAX` chain entry
        // ends the bucket.
        let mut heads = [-1i16; ROWS];
        let mut chain = [u16::MAX; OUTLINE_POINTS];
        for (local, next) in succ[..count].iter().enumerate() {
            let b = usize::from(*next);
            let top = i32::from(scaled[local].1).min(i32::from(scaled[b].1)) + oy;
            let row = (top - 4).div_euclid(4) - y0 as i32;
            if row >= rows as i32 {
                continue;
            }
            let row = row.max(0) as usize;
            chain[local] = heads[row] as u16;
            heads[row] = local as i16;
        }
        let mut active = [0u16; OUTLINE_POINTS];
        let mut active_len = 0usize;
        let mut ink = [0u8; WIDTH];
        let mut xs = [[0i32; CROSSINGS]; 4];
        for row in y0..y1 {
            let row = row as i32;
            ink[..width].fill(0);
            let scans = [row * 4 + 1, row * 4 + 3, row * 4 + 5, row * 4 + 7];
            let mut id = heads[(row as usize) - y0];
            while id >= 0 {
                let local = usize::from(id as u16);
                active[active_len] = id as u16;
                active_len += 1;
                let next = chain[local];
                if next == u16::MAX {
                    break;
                }
                id = next as i16;
            }
            let mut counts = [0usize; 4];
            // The next row's first sample is this row's +5, so an edge reaching past +5 still
            // crosses there.
            let last_needed = row * 4 + 5;
            let mut keep = 0usize;
            for at in 0..active_len {
                let local = usize::from(active[at]);
                let b = usize::from(succ[local]);
                let (ax, ay) = scaled[local];
                let (bx, by) = scaled[b];
                let (ax, ay) = (ox + i32::from(ax), oy + i32::from(ay));
                let (bx, by) = (ox + i32::from(bx), oy + i32::from(by));
                let dy = by - ay;
                for (sub, scan) in scans.iter().enumerate() {
                    if (ay <= *scan) != (by <= *scan) && counts[sub] < CROSSINGS {
                        xs[sub][counts[sub]] = ax + (*scan - ay) * (bx - ax) / dy;
                        counts[sub] += 1;
                    }
                }
                if ay.max(by) > last_needed {
                    active[keep] = active[at];
                    keep += 1;
                }
            }
            active_len = keep;
            for sub in 0..4 {
                let crossings = &mut xs[sub][..counts[sub]];
                crossings.sort_unstable();
                for pair in crossings.as_chunks::<2>().0 {
                    add_span(&mut ink, pair[0], pair[1], x0 as i32 * 4, width);
                }
            }
            for (column, coverage) in ink[..width].iter().enumerate() {
                if *coverage > 0 {
                    debug_assert!(*coverage <= FULL);
                    self.mark_coverage(x0 + column, row as usize, *coverage);
                }
            }
        }
    }

    /// Lays the current colour on one pixel at `coverage` of sixteen, blending with what is
    /// there. With no colour set the ground turns over by the same fraction.
    fn mark_coverage(&mut self, x: usize, y: usize, coverage: u8) {
        let offset = (y * self.width + x) * 2;
        let packed = u16::from_be_bytes([self.frame[offset], self.frame[offset + 1]]);
        let alpha = (u16::from(coverage) * 255 / 16) as u8;
        let next = match self.color {
            Some(color) => {
                let rgb = color.rgb888();
                let alpha = u16::from(color.alpha()) * u16::from(alpha) / 255;
                blend(packed, Color::rgba(rgb.0, rgb.1, rgb.2, alpha as u8))
            }
            None => {
                let inverted = invert_rgb565(packed);
                let r = (inverted >> 11) as u8 & 0x1F;
                let g = (inverted >> 5) as u8 & 0x3F;
                let b = inverted as u8 & 0x1F;
                blend(
                    packed,
                    Color::rgba(
                        (r << 3) | (r >> 2),
                        (g << 2) | (g >> 4),
                        (b << 3) | (b >> 2),
                        alpha,
                    ),
                )
            }
        };
        self.frame[offset..offset + 2].copy_from_slice(&next.to_be_bytes());
    }

    /// Overdraws one character at the root size.
    pub fn draw_char(&mut self, ch: char, left: isize, top: usize) {
        self.draw_char_at(ch, left, top, Metrics::default());
    }

    /// Overdraws one run of text at a panel coordinate, `top` being the top of the line box.
    pub fn draw_text_at(&mut self, text: &[u8], left: usize, top: usize, metrics: Metrics) {
        if metrics.family() == FontFamily::Vector {
            let mut pen = left as isize;
            for ch in Chars::new(text) {
                self.draw_char_at(ch, pen, top, metrics);
                pen += metrics.advance(ch) as isize;
            }
            return;
        }
        let face = Face::new();
        let scale = metrics.scale();
        let baseline = top as isize + metrics.ascent() as isize;
        let mut pen = left as isize;
        for glyph in face.glyphs(text) {
            let glyph_top = baseline - (glyph.top + glyph.height as isize) * scale as isize;
            self.draw_glyph_at(&glyph, pen + glyph.left * scale as isize, glyph_top, scale);
            pen += (glyph.advance * scale) as isize;
        }
    }

    /// Overdraws one run at the root size.
    pub fn draw_text(&mut self, text: &[u8], left: usize, top: usize) {
        self.draw_text_at(text, left, top, Metrics::default());
    }

    /// Columns a run occupies at `metrics`.
    pub fn measure_at(&self, text: &[u8], metrics: Metrics) -> usize {
        metrics.measure(text)
    }

    /// Columns a run occupies at the root size.
    pub fn measure(&self, text: &[u8]) -> usize {
        Metrics::default().measure(text)
    }

    /// [`Canvas::draw_text`] placed by its right edge, so a value that grows a digit grows
    /// leftwards into empty space.
    pub fn draw_text_right(&mut self, text: &[u8], right: isize, top: usize) {
        let left = right - self.measure(text) as isize;
        self.draw_text(text, left.max(0) as usize, top);
    }

    /// Inverts the straight run from `(x0, y0)` to `(x1, y1)` (Bresenham).
    pub fn draw_line(&mut self, x0: isize, y0: isize, x1: isize, y1: isize) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.plot_pixel(x, y);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Inverts the perimeter of the circle at `(cx, cy)` with radius `r` (midpoint
    /// algorithm).
    pub fn draw_circle(&mut self, cx: isize, cy: isize, r: isize) {
        let mut x = r;
        let mut y = 0;
        let mut err = 1 - r;
        while x >= y {
            for (dx, dy) in [
                (x, y),
                (-x, y),
                (x, -y),
                (-x, -y),
                (y, x),
                (-y, x),
                (y, -x),
                (-y, -x),
            ] {
                self.plot_pixel(cx + dx, cy + dy);
            }
            y += 1;
            if err <= 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    /// Fills `rect`: the current colour when one is set, otherwise this frame's own ground
    /// turned over, which is the panel's one way of laying a paper down for drawing.
    pub fn fill_rect(&mut self, rect: Rect) {
        let (solid, tint) = match (self.color, self.ground) {
            (Some(color), _) if color.alpha() == 0xFF => (Some(color.packed().to_be_bytes()), None),
            (Some(color), _) => (None, Some(color)),
            (None, Some(ground)) => (Some(invert_rgb565(rgb565(ground)).to_be_bytes()), None),
            (None, None) => return,
        };
        let bottom = (rect.bottom().max(0) as usize)
            .min(CONTENT_BOTTOM)
            .min(self.height);
        for y in rect.y.max(0) as usize..bottom {
            let (x0, _, x1, _) = self.reach(rect.x as isize, y as isize, rect.w as isize, 1);
            let start = (y * self.width + x0) * 2;
            let row = &mut self.frame[start..start + (x1 - x0) * 2];
            for pixel in row.as_chunks_mut::<2>().0 {
                match (solid, tint) {
                    (Some(bytes), _) => pixel.copy_from_slice(&bytes),
                    (None, Some(color)) => {
                        let packed = u16::from_be_bytes([pixel[0], pixel[1]]);
                        pixel.copy_from_slice(&blend(packed, color).to_be_bytes());
                    }
                    (None, None) => {}
                }
            }
        }
    }

    /// Draws the one-pixel outline of `rect`.
    pub fn stroke_rect(&mut self, rect: Rect) {
        if rect.w <= 0 || rect.h <= 0 {
            return;
        }
        let (left, top) = (rect.x as isize, rect.y as isize);
        let (right, bottom) = (rect.right() as isize - 1, rect.bottom() as isize - 1);
        self.draw_line(left, top, right, top);
        self.draw_line(left, bottom, right, bottom);
        self.draw_line(left, top, left, bottom);
        self.draw_line(right, top, right, bottom);
    }
}

/// Adds the coverage a scanline span from `a` to `b` gives the pixels of `ink`, in quarter
/// columns relative to `origin`. The last pixel takes only the fraction the span reaches.
fn add_span(ink: &mut [u8], a: i32, b: i32, origin: i32, width: usize) {
    let end = width as i32 * 4;
    let a = (a - origin).clamp(0, end) as usize;
    let b = (b - origin).clamp(0, end) as usize;
    if a >= b {
        return;
    }
    let (mut pixel, first) = (a / 4, (a % 4) as u8);
    let last = (b - 1) / 4;
    if pixel == last {
        ink[pixel] = ink[pixel].saturating_add((b - a) as u8);
        return;
    }
    ink[pixel] = ink[pixel].saturating_add(4 - first);
    pixel += 1;
    while pixel < last {
        ink[pixel] = ink[pixel].saturating_add(4);
        pixel += 1;
    }
    ink[last] = ink[last].saturating_add(((b - 1) % 4 + 1) as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const WIDTH: usize = 32;
    const HEIGHT: usize = 24;
    const GROUND: Rgb = Rgb(0x10, 0x20, 0x30);

    fn frame() -> alloc::vec::Vec<u8> {
        vec![0u8; WIDTH * HEIGHT * 2]
    }

    fn pixel(frame: &[u8], x: usize, y: usize) -> u16 {
        let offset = (y * WIDTH + x) * 2;
        u16::from_be_bytes([frame[offset], frame[offset + 1]])
    }

    #[test]
    fn a_draw_off_the_edge_is_clipped_rather_than_panicking() {
        let mut bytes = frame();
        let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
        canvas.plot_pixel(-1, 0);
        canvas.plot_pixel(0, -1);
        canvas.plot_pixel(WIDTH as isize, 0);
        canvas.plot_pixel(0, HEIGHT as isize);
        canvas.draw_text(b"X", WIDTH - 1, 0);
        assert!(bytes.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn a_vector_hanzi_lands_inside_its_line_box_with_soft_edges() {
        let mut bytes = frame();
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.draw_char_at('中', 0, 0, Metrics::vector(24));
        }
        let ink: vec::Vec<u16> = (0..24)
            .flat_map(|y| (0..24).map(move |x| (x, y)))
            .map(|(x, y)| pixel(&bytes, x, y))
            .filter(|pixel| *pixel != 0)
            .collect();
        assert!(!ink.is_empty(), "the glyph drew nothing");
        assert!(
            ink.iter().any(|pixel| *pixel != 0xFFFF),
            "an antialiased edge is neither empty nor opaque"
        );
        assert!(
            (24..WIDTH).all(|x| (0..HEIGHT).all(|y| pixel(&bytes, x, y) == 0)),
            "the ink stays within the 24 px advance"
        );
    }

    #[test]
    fn a_vector_run_falls_back_to_the_bitmap_face_for_a_glyph_it_lacks() {
        let mut bytes = frame();
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
            canvas.draw_char_at('\u{1F004}', 0, 0, Metrics::vector(24));
        }
        assert!(
            bytes.iter().any(|byte| *byte != 0),
            "the fallback mark drew"
        );
    }

    #[test]
    fn a_span_outside_the_ink_buffer_adds_nothing() {
        let mut ink = [0u8; 4];
        add_span(&mut ink, -30, -10, 0, 4);
        add_span(&mut ink, 20, 40, 0, 4);
        assert_eq!(ink, [0u8; 4]);
        add_span(&mut ink, -2, 30, 0, 4);
        assert_eq!(ink, [4u8; 4], "a span crossing the buffer fills it");
    }

    #[test]
    fn a_vector_glyph_is_clipped_like_any_other_mark() {
        let mut whole = frame();
        {
            let mut canvas = Canvas::new(&mut whole, WIDTH, HEIGHT, None);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.draw_char_at('中', 2, 0, Metrics::vector(24));
        }
        // The face's ink sits where only the clip can say; take the first inked pixel from the
        // unclipped mark rather than guessing a window the outline may miss.
        let (x, y) = (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .find(|(x, y)| pixel(&whole, *x, *y) != 0)
            .expect("the glyph drew ink");
        let mut bytes = frame();
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.push_clip(Rect::new(x as i32, y as i32, 1, 1));
            canvas.draw_char_at('中', 2, 0, Metrics::vector(24));
            canvas.pop_clip();
        }
        assert_eq!(pixel(&bytes, x, y), pixel(&whole, x, y), "the kept pixel");
        for row in 0..HEIGHT {
            for column in 0..WIDTH {
                if (column, row) != (x, y) {
                    assert_eq!(
                        pixel(&bytes, column, row),
                        0,
                        "({column}, {row}) is outside the clip"
                    );
                }
            }
        }
    }

    #[test]
    fn a_fill_paints_the_rect_with_the_ground_turned_over() {
        let mut bytes = frame();
        let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, Some(GROUND));
        canvas.fill_rect(Rect::new(2, 3, 4, 5));
        let expected = invert_rgb565(rgb565(GROUND)).to_be_bytes();
        for y in 3..8 {
            for x in 2..6 {
                let offset = (y * WIDTH + x) * 2;
                assert_eq!(bytes[offset..offset + 2], expected);
            }
        }
        assert_eq!(&bytes[0..2], &[0, 0], "outside the rect is untouched");
    }

    #[test]
    fn a_set_colour_lays_itself_down_and_leaves_the_rest_inverting() {
        let mut bytes = frame();
        let red = rgb565(Rgb(0xFF, 0, 0));
        let plain = invert_rgb565(rgb565(GROUND));
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, Some(GROUND));
            canvas.set_color(Some(Color::rgb(0xFF, 0, 0)));
            canvas.fill_rect(Rect::new(2, 3, 2, 2));
            canvas.draw_char('X', 8, 0);
            canvas.set_color(None);
            canvas.fill_rect(Rect::new(4, 3, 1, 1));
        }
        for y in 3..5 {
            for x in 2..4 {
                assert_eq!(pixel(&bytes, x, y), red);
            }
        }
        assert!(
            (8..14).any(|x| (0..12).any(|y| pixel(&bytes, x, y) == red)),
            "the glyph takes the colour too"
        );
        assert_eq!(pixel(&bytes, 4, 3), plain, "no colour keeps the invert");
    }

    #[test]
    fn a_translucent_fill_mixes_with_what_is_already_there() {
        let mut bytes = frame();
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, Some(GROUND));
            canvas.set_color(Some(Color::rgba(0xFF, 0xFF, 0xFF, 0x80)));
            canvas.fill_rect(Rect::new(0, 0, 2, 2));
        }
        let mixed = pixel(&bytes, 0, 0);
        assert_ne!(mixed, rgb565(GROUND));
        assert_ne!(mixed, rgb565(Rgb(0xFF, 0xFF, 0xFF)));
    }

    /// The fill is the page's surface, so drawing it over a glyph erases the glyph; a rect that
    /// reaches past the content rows must leave the footer alone.
    #[test]
    fn a_fill_covers_what_was_drawn_and_stops_at_the_footer() {
        const TALL: usize = CONTENT_BOTTOM + 8;
        let ground = rgb565(GROUND).to_be_bytes();
        let ink = invert_rgb565(rgb565(GROUND)).to_be_bytes();
        let mut bytes = vec![0u8; WIDTH * TALL * 2];
        for pixel in bytes.as_chunks_mut::<2>().0 {
            pixel.copy_from_slice(&ground);
        }
        for x in 6..12 {
            let offset = (10 * WIDTH + x) * 2;
            bytes[offset..offset + 2].copy_from_slice(&ink);
        }
        bytes[((10 * WIDTH) + 1) * 2..((10 * WIDTH) + 1) * 2 + 2].copy_from_slice(&ink);

        let mut canvas = Canvas::new(&mut bytes, WIDTH, TALL, Some(GROUND));
        canvas.fill_rect(Rect::new(4, 8, 16, CONTENT_BOTTOM as i32));

        for y in 8..CONTENT_BOTTOM {
            for x in 4..20 {
                let offset = (y * WIDTH + x) * 2;
                assert_eq!(bytes[offset..offset + 2], ink, "({x}, {y}) is the surface");
            }
        }
        assert_eq!(
            pixel(&bytes, 1, 10),
            u16::from_be_bytes(ink),
            "the word beside the box survives"
        );
        assert_eq!(
            pixel(&bytes, CONTENT_BOTTOM, 0),
            u16::from_be_bytes(ground),
            "and the footer is never filled"
        );
    }
}

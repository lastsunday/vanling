//! The pixels a page is drawn into, and the only marks there are.

use crate::drivers::surface::FrameDamage;
use crate::light::{Rgb, invert_rgb565, rgb565};

use super::color::{Color, blend};
use super::panel::CONTENT_BOTTOM;

use super::Rect;
use super::font::{Face, Glyph, Metrics};

/// The largest corner any widget asks for; the array of row insets is sized by it.
const MAX_RADIUS: u8 = 16;

/// A frame buffer and the operations every page draws with.
pub struct Canvas<'a> {
    frame: &'a mut [u8],
    width: usize,
    height: usize,
    ground: Option<Rgb>,
    color: Option<Color>,
    clips: [Rect; 4],
    depth: usize,
    /// The rectangles this frame is allowed to touch; `None` means the whole canvas, so a
    /// partial repaint still only draws what changed.
    damage: Option<FrameDamage>,
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
            damage: None,
        }
    }

    /// Restricts every mark to `damage`: the frame already holds the rest of the picture.
    #[must_use]
    pub fn with_damage(mut self, damage: FrameDamage) -> Self {
        self.damage = Some(damage);
        self
    }

    /// Narrows every following mark to `damage`, for a frame whose changed pixels are only
    /// known after the page is laid out.
    pub fn set_damage(&mut self, damage: FrameDamage) {
        self.damage = Some(damage);
    }

    /// Lays the frame's ground back inside `damage`, past both the damage window and the
    /// content clamp: the caller is about to redraw exactly these pixels, footer included.
    pub fn clear(&mut self, damage: FrameDamage) {
        let Some(ground) = self.ground else {
            return;
        };
        let packed = rgb565(ground).to_be_bytes();
        if damage.is_full() {
            for pixel in self.frame.as_chunks_mut::<2>().0 {
                pixel.copy_from_slice(&packed);
            }
            return;
        }
        for rect in damage.rects() {
            let (x0, x1) = (
                usize::from(rect.x).min(self.width),
                usize::from(rect.right()).min(self.width),
            );
            if x1 <= x0 {
                continue;
            }
            let row_bytes = (x1 - x0) * 2;
            for y in usize::from(rect.y)..usize::from(rect.bottom()).min(self.height) {
                let start = y * self.width * 2 + x0 * 2;
                for pixel in self.frame[start..start + row_bytes].as_chunks_mut::<2>().0 {
                    pixel.copy_from_slice(&packed);
                }
            }
        }
    }

    fn damaged(&self, x: usize, y: usize) -> bool {
        in_damage(self.damage, x, y)
    }

    /// The pixels this frame may draw, as the caller narrowed them.
    pub fn damage(&self) -> Option<FrameDamage> {
        self.damage
    }

    /// Whether `rect` holds a pixel this frame may draw. A node outside the damage is skipped
    /// whole, so its drawing does not walk pixels the canvas would discard.
    pub fn touches(&self, rect: Rect) -> bool {
        let Some(damage) = self.damage else {
            return true;
        };
        if damage.is_full() {
            return true;
        }
        damage.rects().iter().any(|area| {
            i32::from(area.x) < rect.right()
                && i32::from(area.right()) > rect.x
                && i32::from(area.y) < rect.bottom()
                && i32::from(area.bottom()) > rect.y
        })
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

    /// Narrows every following mark to `rect` across the axis only, keeping the vertical clip
    /// in force: a run may not overflow sideways but may let a descender out of its line box.
    pub fn push_x_clip(&mut self, rect: Rect) {
        let base = self.clip().unwrap_or(Rect {
            x: 0,
            y: 0,
            w: self.width as i32,
            h: self.height as i32,
        });
        self.push_clip(Rect {
            x: rect.x,
            y: base.y,
            w: rect.w,
            h: base.h,
        });
    }

    fn clipped(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return false;
        }
        if !self.damaged(x as usize, y as usize) {
            return false;
        }
        if self.depth == 0 {
            return true;
        }
        self.clips[self.depth - 1].contains(x as i32, y as i32)
    }

    /// Sets the colour every following mark is laid down in; `None` turns the ground over.
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
    /// `heavy` adds one column (a bit is covered when it or its left neighbour is set), the same
    /// widening two strikes give without double-striking. Nothing inside the box is bounds-checked.
    fn draw_glyph_at(
        &mut self,
        glyph: &Glyph<'_>,
        left: isize,
        top: isize,
        scale: usize,
        heavy: bool,
    ) {
        let width = glyph.width + usize::from(heavy);
        let (x0, y0, x1, y1) = self.reach(
            left,
            top,
            (width * scale) as isize,
            (glyph.height * scale) as isize,
        );
        let damage = self.damage;
        for_windows(damage, (x0, y0, x1, y1), |x0, y0, x1, y1| {
            for y in y0..y1 {
                let row = (y as isize - top) as usize / scale;
                for x in x0..x1 {
                    let column = (x as isize - left) as usize / scale;
                    let covered = column < glyph.width && glyph.coverage(row, column)
                        || heavy && column > 0 && glyph.coverage(row, column - 1);
                    if covered {
                        self.mark(x, y);
                    }
                }
            }
        });
    }

    /// Overdraws one character at a panel coordinate, `top` being the top of the line box.
    pub fn draw_char_at(&mut self, ch: char, left: isize, top: usize, metrics: Metrics) {
        self.draw_char_weighted_at(ch, left, top, metrics, false);
    }

    pub fn draw_char_heavy_at(&mut self, ch: char, left: isize, top: usize, metrics: Metrics) {
        self.draw_char_weighted_at(ch, left, top, metrics, true);
    }

    fn draw_char_weighted_at(
        &mut self,
        ch: char,
        left: isize,
        top: usize,
        metrics: Metrics,
        heavy: bool,
    ) {
        let face = Face::new();
        let glyph = face.glyph(ch);
        let scale = metrics.scale();
        let baseline = top as isize + metrics.ascent() as isize;
        self.draw_glyph_at(
            &glyph,
            left + glyph.left * scale as isize,
            baseline - (glyph.top + glyph.height as isize) * scale as isize,
            scale,
            heavy,
        );
    }

    /// The clip in force, `None` when the canvas is whole.
    fn clip(&self) -> Option<Rect> {
        (self.depth > 0).then(|| self.clips[self.depth - 1])
    }

    /// Overdraws one character at the root size.
    pub fn draw_char(&mut self, ch: char, left: isize, top: usize) {
        self.draw_char_at(ch, left, top, Metrics::default());
    }

    /// Overdraws one run of text at a panel coordinate, `top` being the top of the line box.
    pub fn draw_text_at(&mut self, text: &[u8], left: usize, top: usize, metrics: Metrics) {
        let face = Face::new();
        let scale = metrics.scale();
        let baseline = top as isize + metrics.ascent() as isize;
        let mut pen = left as isize;
        for glyph in face.glyphs(text) {
            let glyph_top = baseline - (glyph.top + glyph.height as isize) * scale as isize;
            self.draw_glyph_at(
                &glyph,
                pen + glyph.left * scale as isize,
                glyph_top,
                scale,
                false,
            );
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
        let width = self.width;
        for y in rect.y.max(0) as usize..bottom {
            let (x0, _, x1, _) = self.reach(rect.x as isize, y as isize, rect.w as isize, 1);
            if x0 >= x1 {
                continue;
            }
            let damage = self.damage;
            let frame = &mut self.frame;
            for_windows(damage, (x0, y, x1, y + 1), |x0, _, x1, _| {
                let start = (y * width + x0) * 2;
                let row = &mut frame[start..start + (x1 - x0) * 2];
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
            });
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

    /// Fills `rect` with its corners rounded by `radius` px; zero is [`Self::fill_rect`].
    pub fn fill_round_rect(&mut self, rect: Rect, radius: u8) {
        let Some(corners) = corner_rows(rect, radius) else {
            self.fill_rect(rect);
            return;
        };
        for y in 0..rect.h {
            let inset = corners.inset(y) as i32;
            self.fill_rect(Rect::new(rect.x + inset, rect.y + y, rect.w - 2 * inset, 1));
        }
    }

    /// Draws the one-pixel outline of `rect` around the same rounded corners.
    pub fn stroke_round_rect(&mut self, rect: Rect, radius: u8) {
        let Some(corners) = corner_rows(rect, radius) else {
            self.stroke_rect(rect);
            return;
        };
        let (left, top) = (rect.x as isize, rect.y as isize);
        let (right, bottom) = (rect.right() as isize - 1, rect.bottom() as isize - 1);
        for y in 0..rect.h {
            let inset = corners.inset(y) as isize;
            self.plot_pixel(left + inset, top + y as isize);
            self.plot_pixel(right - inset, top + y as isize);
        }
        let straight = corners.inset(0) as isize;
        self.draw_line(left + straight, top, right - straight, top);
        self.draw_line(left + straight, bottom, right - straight, bottom);
    }
}

/// A box's rounded corners: the first column each of the top rows leaves inside, mirrored down
/// the box. `None` when the radius rounds nothing, so the caller keeps the square path.
struct Corners {
    insets: [u8; MAX_RADIUS as usize],
    height: i32,
    rows: i32,
}

impl Corners {
    fn inset(&self, row: i32) -> u8 {
        let down = row.min(self.height - 1 - row).max(0);
        self.insets[down.min(self.rows - 1) as usize]
    }
}

fn corner_rows(rect: Rect, radius: u8) -> Option<Corners> {
    if rect.w <= 0 || rect.h <= 0 {
        return None;
    }
    let radius = radius
        .min(MAX_RADIUS)
        .min((rect.w / 2) as u8)
        .min((rect.h / 2) as u8);
    if radius == 0 {
        return None;
    }
    let r = i32::from(radius);
    let mut insets = [0u8; MAX_RADIUS as usize];
    for row in 0..r {
        // The pixel is inside the corner when its centre is: `(2r-2c-1)² + (2r-2k-1)² <= 4r²`,
        // doubled so the half-pixel centre stays a whole number.
        let down = 2 * r - 2 * row - 1;
        let mut column = 0;
        while column < r {
            let across = 2 * r - 2 * column - 1;
            if across * across + down * down <= 4 * r * r {
                break;
            }
            column += 1;
        }
        insets[row as usize] = column as u8;
    }
    Some(Corners {
        insets,
        height: rect.h,
        rows: r,
    })
}

/// Calls `f` once per rectangle of `damage` that meets the box, or once with the box itself
/// when nothing restricts it. Each window is what the caller may coverage.
fn for_windows(
    damage: Option<FrameDamage>,
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    mut f: impl FnMut(usize, usize, usize, usize),
) {
    match damage {
        Some(damage) if !damage.is_full() => {
            for rect in damage.rects() {
                let ax0 = x0.max(usize::from(rect.x));
                let ay0 = y0.max(usize::from(rect.y));
                let ax1 = x1.min(usize::from(rect.right()));
                let ay1 = y1.min(usize::from(rect.bottom()));
                if ax0 < ax1 && ay0 < ay1 {
                    f(ax0, ay0, ax1, ay1);
                }
            }
        }
        _ => f(x0, y0, x1, y1),
    }
}

fn in_damage(damage: Option<FrameDamage>, x: usize, y: usize) -> bool {
    match damage {
        None => true,
        Some(damage) => {
            damage.is_full()
                || damage
                    .rects()
                    .iter()
                    .any(|rect| rect.contains(x as u16, y as u16))
        }
    }
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
    fn a_rounded_fill_leaves_its_corners_and_its_edges_still() {
        let mut bytes = frame();
        let rect = Rect::new(4, 4, 16, 12);
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.fill_round_rect(rect, 4);
        }
        let lit = rgb565(Rgb(0xFF, 0xFF, 0xFF));
        for (x, y) in [(4, 4), (19, 4), (4, 15), (19, 15)] {
            assert_ne!(pixel(&bytes, x, y), lit, "the corner at {x},{y} was filled");
        }
        for (x, y) in [(12, 4), (12, 15), (4, 10), (19, 10), (12, 10)] {
            assert_eq!(pixel(&bytes, x, y), lit, "{x},{y} left a hole in the box");
        }
    }

    #[test]
    fn a_rounded_outline_is_one_pixel_and_closes() {
        let mut bytes = frame();
        let rect = Rect::new(4, 4, 16, 12);
        {
            let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.stroke_round_rect(rect, 4);
        }
        let lit = rgb565(Rgb(0xFF, 0xFF, 0xFF));
        for (x, y) in [(4, 9), (19, 9), (12, 4), (12, 15), (5, 5), (6, 4)] {
            assert_eq!(pixel(&bytes, x, y), lit, "{x},{y} is on the outline");
        }
        assert_ne!(pixel(&bytes, 4, 4), lit, "the corner itself is cut away");
        assert_eq!(pixel(&bytes, 12, 10), 0, "the middle stays untouched");
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
    fn damage_keeps_every_mark_inside_its_rectangles() {
        let mut bytes = frame();
        let mut damage = FrameDamage::NONE;
        damage.add(crate::drivers::surface::DamageRect::new(8, 8, 8, 8));
        {
            let mut canvas =
                Canvas::new(&mut bytes, WIDTH, HEIGHT, Some(GROUND)).with_damage(damage);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.fill_rect(Rect::new(0, 0, WIDTH as i32, HEIGHT as i32));
            canvas.plot_pixel(20, 20);
        }
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let inside = (8..16).contains(&x) && (8..16).contains(&y);
                assert_eq!(
                    pixel(&bytes, x, y),
                    if inside { 0xFFFF } else { 0 },
                    "({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_glyph_only_draws_the_damaged_rows() {
        let mut bytes = frame();
        let mut damage = FrameDamage::NONE;
        damage.add(crate::drivers::surface::DamageRect::new(
            0,
            10,
            WIDTH as u16,
            4,
        ));
        {
            let mut canvas =
                Canvas::new(&mut bytes, WIDTH, HEIGHT, Some(GROUND)).with_damage(damage);
            canvas.set_color(Some(Color::rgb(0xFF, 0xFF, 0xFF)));
            canvas.draw_char_at('A', 0, 0, Metrics::at(24));
        }
        assert!(
            (0..10).all(|y| (0..WIDTH).all(|x| pixel(&bytes, x, y) == 0)),
            "above the band stays untouched"
        );
        assert!(
            (14..HEIGHT).all(|y| (0..WIDTH).all(|x| pixel(&bytes, x, y) == 0)),
            "below the band stays untouched"
        );
        assert!(
            (10..14).any(|y| (0..WIDTH).any(|x| pixel(&bytes, x, y) != 0)),
            "the band carries the glyph's rows"
        );
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
        let coverage = invert_rgb565(rgb565(GROUND)).to_be_bytes();
        let mut bytes = vec![0u8; WIDTH * TALL * 2];
        for pixel in bytes.as_chunks_mut::<2>().0 {
            pixel.copy_from_slice(&ground);
        }
        for x in 6..12 {
            let offset = (10 * WIDTH + x) * 2;
            bytes[offset..offset + 2].copy_from_slice(&coverage);
        }
        bytes[((10 * WIDTH) + 1) * 2..((10 * WIDTH) + 1) * 2 + 2].copy_from_slice(&coverage);

        let mut canvas = Canvas::new(&mut bytes, WIDTH, TALL, Some(GROUND));
        canvas.fill_rect(Rect::new(4, 8, 16, CONTENT_BOTTOM as i32));

        for y in 8..CONTENT_BOTTOM {
            for x in 4..20 {
                let offset = (y * WIDTH + x) * 2;
                assert_eq!(
                    bytes[offset..offset + 2],
                    coverage,
                    "({x}, {y}) is the surface"
                );
            }
        }
        assert_eq!(
            pixel(&bytes, 1, 10),
            u16::from_be_bytes(coverage),
            "the word beside the box survives"
        );
        assert_eq!(
            pixel(&bytes, CONTENT_BOTTOM, 0),
            u16::from_be_bytes(ground),
            "and the footer is never filled"
        );
    }

    /// The cache may only make drawing faster: every shipped glyph drawn from a slot must land
    /// on exactly the pixels the outline would. The third pass reuses slots rasterized at a
    /// different position, proving coverage does not depend on where the glyph sits.
    #[test]
    fn a_heavy_bitmap_glyph_covers_its_plain_mark_once() {
        let mut grew = false;
        let mut overlap_total = 0usize;
        for ch in ['M', 'W', '中', '0'] {
            let mut plain = frame();
            let mut heavy = frame();
            {
                let mut canvas = Canvas::new(&mut plain, WIDTH, HEIGHT, None);
                canvas.draw_char_at(ch, 4, 0, Metrics::default());
            }
            {
                let mut canvas = Canvas::new(&mut heavy, WIDTH, HEIGHT, None);
                canvas.draw_char_heavy_at(ch, 4, 0, Metrics::default());
            }
            let mut plain_count = 0usize;
            let mut overlap = 0usize;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    if pixel(&plain, x, y) == 0 {
                        continue;
                    }
                    plain_count += 1;
                    let covered =
                        pixel(&heavy, x, y) != 0 || (x + 1 < WIDTH && pixel(&heavy, x + 1, y) != 0);
                    assert!(covered, "{ch} lost its pixel at ({x}, {y})");
                    if x > 0 && pixel(&plain, x - 1, y) != 0 {
                        overlap += 1;
                        assert!(
                            pixel(&heavy, x, y) != 0,
                            "{ch} reverted the overlap at ({x}, {y})"
                        );
                    }
                }
            }
            overlap_total += overlap;
            let heavy_count = (0..HEIGHT)
                .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
                .filter(|&(x, y)| pixel(&heavy, x, y) != 0)
                .count();
            assert!(
                heavy_count >= plain_count,
                "{ch} was narrowed by the second strike"
            );
            grew |= heavy_count > plain_count;
        }
        assert!(grew, "no glyph gained the widened column");
        assert!(overlap_total > 0, "no glyph has a two-column run");
    }
}

//! The pixels a page is drawn into, and the only marks there are.

use alloc::boxed::Box;
use alloc::vec;

use crate::drivers::surface::FrameDamage;
use crate::light::{Rgb, invert_rgb565, rgb565};

use super::color::{Color, blend};
use super::panel::CONTENT_BOTTOM;

use super::Rect;
use super::font::{Chars, Face, FontFamily, Glyph, Metrics, Outline, VectorFace};

/// The points one glyph may hold before [`Canvas::draw_outline_at`] refuses it. The widest
/// shipped glyph holds 328; a wider one trips `no_outline_is_wider_than_the_canvas_scratch`.
pub(crate) const OUTLINE_POINTS: usize = 384;

/// The coverage cache's glyph box, sized for the text page's 24 px face; larger glyphs
/// rasterize straight into the frame.
pub const GLYPH_BOX: usize = 24;

/// Glyphs one page can keep rasterized. The text sample needs 179 distinct glyphs.
pub const GLYPH_SLOTS: usize = 192;

/// Coverage is counted in sixteenths, so a pixel needs five bits: a full pixel is 16, which
/// four bits cannot hold.
const INK_BITS: usize = 5;
const GLYPH_STRIDE: usize = (GLYPH_BOX * INK_BITS).div_ceil(8);

/// The widest and tallest ink the uncached path draws; a bigger face would overrun the row
/// buffer and trips an assertion.
const MAX_GLYPH_WIDTH: usize = 64;
const MAX_GLYPH_ROWS: usize = 256;

/// Crossings one scanline may hold. The table's widest glyph has 17 contours.
const CROSSINGS: usize = 48;

/// The scanline walk's tables: on the heap with the cache, because the deepest paint already
/// stands near the main stack's ceiling (`@/records/iot/hardware-constraints.md`).
struct Scratch {
    scaled: [(i16, i16); OUTLINE_POINTS],
    succ: [u16; OUTLINE_POINTS],
    chain: [u16; OUTLINE_POINTS],
    active: [u16; OUTLINE_POINTS],
    xs: [[i32; CROSSINGS]; 4],
}

impl Scratch {
    const fn new() -> Self {
        Self {
            scaled: [(0, 0); OUTLINE_POINTS],
            succ: [0; OUTLINE_POINTS],
            chain: [u16::MAX; OUTLINE_POINTS],
            active: [0; OUTLINE_POINTS],
            xs: [[0; CROSSINGS]; 4],
        }
    }

    /// Fills freshly allocated tables in place, with no four-kilobyte temporary.
    ///
    /// # Safety
    ///
    /// `this` must be valid for writes of a whole [`Scratch`] and unread until initialised.
    unsafe fn init_in_place(this: *mut Self) {
        // SAFETY: the caller guarantees room for every table.
        unsafe {
            core::ptr::addr_of_mut!((*this).scaled).write([(0, 0); OUTLINE_POINTS]);
            core::ptr::addr_of_mut!((*this).succ).write([0; OUTLINE_POINTS]);
            core::ptr::addr_of_mut!((*this).chain).write([u16::MAX; OUTLINE_POINTS]);
            core::ptr::addr_of_mut!((*this).active).write([0; OUTLINE_POINTS]);
            core::ptr::addr_of_mut!((*this).xs).write([[0; CROSSINGS]; 4]);
        }
    }
}

/// The largest corner any widget asks for; the array of row insets is sized by it.
const MAX_RADIUS: u8 = 16;

/// Where a glyph sits and how big its ink box is, in panel pixels.
#[derive(Clone, Copy)]
struct Placement {
    /// Pen origin.
    left: i32,
    baseline: i32,
    /// Ink box top-left and size.
    x: i32,
    y: i32,
    width: usize,
    rows: usize,
}

#[derive(Clone, Copy)]
struct GlyphSlot {
    /// `u64::MAX` when free; otherwise the code point, size and weight.
    key: u64,
    ink: [u8; GLYPH_STRIDE * GLYPH_BOX],
}

impl GlyphSlot {
    const EMPTY: Self = Self {
        key: u64::MAX,
        ink: [0; GLYPH_STRIDE * GLYPH_BOX],
    };
}

/// The coverage every (code point, size, weight) has already rasterized to. A page lays a
/// glyph on whole-pixel coordinates, so a scroll can blend the slot instead of re-walking the
/// outline. Heap-only: the main stack has no room (`@/records/iot/hardware-constraints.md`).
pub struct GlyphCache {
    slots: Box<[GlyphSlot]>,
    scratch: Box<Scratch>,
    next: usize,
}

impl GlyphCache {
    pub fn new() -> Self {
        Self {
            slots: vec![GlyphSlot::EMPTY; GLYPH_SLOTS].into_boxed_slice(),
            scratch: {
                let mut slot: Box<core::mem::MaybeUninit<Scratch>> = Box::new_uninit();
                // SAFETY: the walk's tables are written in place, then the box is assumed set.
                unsafe {
                    Scratch::init_in_place(slot.as_mut_ptr());
                    slot.assume_init()
                }
            },
            next: 0,
        }
    }

    fn find(&self, key: u64) -> Option<usize> {
        self.slots.iter().position(|slot| slot.key == key)
    }

    /// The slot holding this glyph, rasterizing it first the one time it is asked for.
    fn prepare(
        &mut self,
        ch: char,
        size: u16,
        heavy: bool,
        outline: &Outline<'_>,
        at: Placement,
    ) -> usize {
        let key = (u64::from(ch as u32) << 17) | (u64::from(size) << 1) | u64::from(heavy);
        if let Some(index) = self.find(key) {
            return index;
        }
        if self.next == self.slots.len() {
            for slot in self.slots.iter_mut() {
                slot.key = u64::MAX;
            }
            self.next = 0;
        }
        let index = self.next;
        self.next += 1;
        let Self { slots, scratch, .. } = self;
        let slot = &mut slots[index];
        slot.key = key;
        slot.ink.fill(0);
        rasterize(
            outline,
            size,
            at,
            heavy,
            scratch,
            |column, row, coverage| ink_put(&mut slot.ink, GLYPH_STRIDE, column, row, coverage),
        );
        index
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

/// A frame buffer and the operations every page draws with.
pub struct Canvas<'a> {
    frame: &'a mut [u8],
    width: usize,
    height: usize,
    ground: Option<Rgb>,
    color: Option<Color>,
    clips: [Rect; 4],
    depth: usize,
    glyphs: Option<&'a mut GlyphCache>,
    /// The rectangles this frame is allowed to touch; `None` means the whole canvas, so a
    /// partial repaint still only inks what changed.
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
            glyphs: None,
            damage: None,
        }
    }

    /// Keeps every vector glyph this canvas draws in `glyphs` for the next frame.
    #[must_use]
    pub fn with_glyphs(mut self, glyphs: &'a mut GlyphCache) -> Self {
        self.glyphs = Some(glyphs);
        self
    }

    /// Restricts every mark to `damage`: the frame already holds the rest of the picture.
    #[must_use]
    pub fn with_damage(mut self, damage: FrameDamage) -> Self {
        self.damage = Some(damage);
        self
    }

    fn damaged(&self, x: usize, y: usize) -> bool {
        in_damage(self.damage, x, y)
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
    /// `heavy` adds one column (a bit is inked when it or its left neighbour is set), the same
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
                    let inked = column < glyph.width && glyph.ink(row, column)
                        || heavy && column > 0 && glyph.ink(row, column - 1);
                    if inked {
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
        if metrics.family() == FontFamily::Vector
            && let Some(outline) = VectorFace::new().outline(ch)
        {
            let baseline = top as i32 + metrics.ascent() as i32;
            self.draw_outline_at(ch, &outline, left as i32, baseline, metrics.size(), heavy);
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
            heavy,
        );
    }

    /// Fills one vector glyph's contours at a panel coordinate, its baseline on the row below
    /// the line box's ascent. Coverage is sampled on four sub-scanlines into quarter-columns,
    /// so an edge between pixels becomes a mixed colour. A glyph that fits [`GLYPH_BOX`] is
    /// rasterized once into [`GlyphCache`], and a scroll blits the slot instead of walking the
    /// outline (~1 ms a glyph on the S3); a heavy glyph adds a column, so it caches only while
    /// it still fits.
    fn draw_outline_at(
        &mut self,
        ch: char,
        outline: &Outline<'_>,
        left: i32,
        baseline: i32,
        size: u16,
        heavy: bool,
    ) {
        if size == 0 || outline.is_blank() {
            return;
        }
        let scale = |units: i32| (units * i32::from(size) * 4 + 32) / 64;
        let (xmin, xmax) = (scale(outline.xmin as i32), scale(outline.xmax as i32) + 3);
        let (ymin, ymax) = (-scale(outline.ymin as i32) + 3, -scale(outline.ymax as i32));
        let box_left = left + xmin.div_euclid(4);
        let box_top = baseline + ymax.div_euclid(4);
        let width = (xmax.div_euclid(4) - xmin.div_euclid(4)) as usize;
        let rows = (ymin.div_euclid(4) - ymax.div_euclid(4)) as usize;
        if width == 0 || rows == 0 {
            return;
        }
        let out_width = width + usize::from(heavy);
        let (x0, y0, x1, y1) = self.reach(
            box_left as isize,
            box_top as isize,
            out_width as isize,
            rows as isize,
        );
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let clip = self.clip().unwrap_or(Rect {
            x: 0,
            y: 0,
            w: self.width as i32,
            h: self.height as i32,
        });
        let damage = self.damage;
        let Self {
            frame,
            width: frame_width,
            color,
            glyphs,
            ..
        } = self;
        if let Some(cache) = glyphs
            && out_width <= GLYPH_BOX
            && rows <= GLYPH_BOX
        {
            let index = cache.prepare(
                ch,
                size,
                heavy,
                outline,
                Placement {
                    left,
                    baseline,
                    x: box_left,
                    y: box_top,
                    width,
                    rows,
                },
            );
            let ink = &cache.slots[index].ink;
            for_windows(damage, (x0, y0, x1, y1), |x0, y0, x1, y1| {
                blit(
                    frame,
                    *frame_width,
                    *color,
                    (x0, y0, x1, y1),
                    (box_left, box_top),
                    GLYPH_STRIDE,
                    ink,
                );
            });
            return;
        }
        debug_assert!(
            out_width <= MAX_GLYPH_WIDTH && rows <= MAX_GLYPH_ROWS,
            "a {size} px glyph fills {out_width}x{rows} px, past the raster's limits"
        );
        if out_width > MAX_GLYPH_WIDTH || rows > MAX_GLYPH_ROWS {
            return;
        }
        let at = Placement {
            left,
            baseline,
            x: x0 as i32,
            y: y0 as i32,
            width: x1 - x0,
            rows: y1 - y0,
        };
        let put = |column, row, coverage| {
            let (x, y) = (x0 + column, y0 + row);
            if !clip.contains(x as i32, y as i32) || !in_damage(damage, x, y) {
                return;
            }
            blend_pixel(frame, *frame_width, *color, x, y, coverage)
        };
        match glyphs {
            Some(cache) => rasterize(outline, size, at, heavy, &mut cache.scratch, put),
            // No cache to borrow tables from: this canvas rasters a handful of glyphs, so its
            // own frame may hold them.
            None => rasterize_owning(outline, size, at, heavy, put),
        }
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

/// Adds the coverage a scanline span from `a` to `b` gives the pixels of `ink`, in quarter
/// columns relative to `origin`; the last pixel takes only the fraction the span reaches.
/// Walks `outline`'s contours once, sampling four sub-scanlines per row and handing every
/// inked pixel to `put`. Points are scaled into [`OUTLINE_POINTS`] once and each edge is filed
/// under the first row that can reach it, so a row visits only the edges still crossing it —
/// rescaling per row cost 238 ms a frame.
fn rasterize(
    outline: &Outline<'_>,
    size: u16,
    at: Placement,
    heavy: bool,
    scratch: &mut Scratch,
    mut put: impl FnMut(usize, usize, u8),
) {
    let Placement {
        left,
        baseline,
        x: x0,
        y: y0,
        width,
        rows,
    } = at;
    /// Coverage of a fully inked pixel: four sub-scanlines of four quarter-columns.
    const FULL: u8 = 16;
    let scale = |units: i32| (units * i32::from(size) * 4 + 32) / 64;
    let (ox, oy) = (left * 4, baseline * 4);
    let first = outline.points().start;
    let count = outline.points().len();
    debug_assert!(
        count <= OUTLINE_POINTS,
        "a glyph with {count} points overruns the canvas scratch"
    );
    let count = count.min(OUTLINE_POINTS);
    let Scratch {
        scaled,
        succ,
        chain,
        active,
        xs,
    } = scratch;
    scaled.fill((0, 0));
    succ.fill(0);
    chain.fill(u16::MAX);
    for (local, index) in (first..first + count).enumerate() {
        let (x, y) = outline.point(index);
        scaled[local] = (scale(x) as i16, -scale(y) as i16);
    }
    // Edge `local` runs to `succ[local]`; a contour closes on its own first point.
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
    // File every edge under the first row whose samples can reach it: a row samples +1 to +7
    // quarter rows, so `top - 4` is the row whose window ends at that top. `u16::MAX` ends the
    // bucket chain.
    let mut heads = [-1i16; MAX_GLYPH_ROWS];
    for (local, next) in succ[..count].iter().enumerate() {
        let b = usize::from(*next);
        let top = i32::from(scaled[local].1).min(i32::from(scaled[b].1)) + oy;
        let file = (top - 4).div_euclid(4) - y0;
        if file >= rows as i32 {
            continue;
        }
        let file = file.max(0) as usize;
        chain[local] = heads[file] as u16;
        heads[file] = local as i16;
    }
    let mut active_len = 0usize;
    let mut ink = [0u8; MAX_GLYPH_WIDTH];
    let columns = (width + usize::from(heavy)).min(MAX_GLYPH_WIDTH);
    let merged = heavy && width < MAX_GLYPH_WIDTH;
    debug_assert!(
        !heavy || merged,
        "a heavy {width} px glyph overruns the row buffer"
    );
    for (row, head) in heads.iter().enumerate().take(rows) {
        let pixel = y0 + row as i32;
        ink[..width].fill(0);
        let scans = [pixel * 4 + 1, pixel * 4 + 3, pixel * 4 + 5, pixel * 4 + 7];
        let mut id = *head;
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
        let last_needed = pixel * 4 + 5;
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
                add_span(&mut ink, pair[0], pair[1], x0 * 4, width);
            }
        }
        if merged {
            ink[width] = 0;
            for column in (0..=width).rev() {
                let left = if column == 0 { 0 } else { ink[column - 1] };
                ink[column] = over(ink[column], left);
            }
        }
        for (column, coverage) in ink[..columns].iter().enumerate() {
            if *coverage > 0 {
                debug_assert!(*coverage <= FULL);
                put(column, row, *coverage);
            }
        }
    }
}

/// [`rasterize`] with its tables in the caller's frame: only a canvas without a glyph cache
/// gets here, which means a handful of glyphs per picture.
#[inline(never)]
fn rasterize_owning(
    outline: &Outline<'_>,
    size: u16,
    at: Placement,
    heavy: bool,
    put: impl FnMut(usize, usize, u8),
) {
    let mut scratch = Scratch::new();
    rasterize(outline, size, at, heavy, &mut scratch, put);
}

/// Two strikes of the same mark, merged: the coverage of one pass over another, in sixteenths.
fn over(a: u8, b: u8) -> u8 {
    let (a, b) = (u16::from(a), u16::from(b));
    (a + b - a * b / 16) as u8
}

/// Reads one five-bit sample from a packed row.
fn ink_get(ink: &[u8], stride: usize, x: usize, y: usize) -> u8 {
    let bit = x * INK_BITS;
    let index = y * stride + bit / 8;
    let shift = bit % 8;
    let mut word = u16::from(ink[index]) >> shift;
    if shift + INK_BITS > 8 && index + 1 < ink.len() {
        word |= u16::from(ink[index + 1]) << (8 - shift);
    }
    (word & 0x1F) as u8
}

/// Writes one five-bit sample into a packed row.
fn ink_put(ink: &mut [u8], stride: usize, x: usize, y: usize, value: u8) {
    let bit = x * INK_BITS;
    let index = y * stride + bit / 8;
    let shift = bit % 8;
    let mask = 0x1Fu16 << shift;
    let mut word = u16::from(ink[index]);
    if index + 1 < ink.len() {
        word |= u16::from(ink[index + 1]) << 8;
    }
    word = (word & !mask) | (u16::from(value) << shift);
    ink[index] = word as u8;
    if shift + INK_BITS > 8 {
        ink[index + 1] = (word >> 8) as u8;
    }
}

/// Lays one cached glyph's coverage onto the frame inside `window`; `origin` is the glyph's
/// top-left in panel pixels.
fn blit(
    frame: &mut [u8],
    frame_width: usize,
    color: Option<Color>,
    window: (usize, usize, usize, usize),
    origin: (i32, i32),
    stride: usize,
    ink: &[u8],
) {
    let (x0, y0, x1, y1) = window;
    for y in y0..y1 {
        let row = (y as i32 - origin.1) as usize;
        for x in x0..x1 {
            let column = (x as i32 - origin.0) as usize;
            let coverage = ink_get(ink, stride, column, row);
            if coverage > 0 {
                blend_pixel(frame, frame_width, color, x, y, coverage);
            }
        }
    }
}

/// Lays the current colour on one pixel at `coverage` of sixteen, blending with what is
/// there. With no colour set the ground turns over by the same fraction.
fn blend_pixel(
    frame: &mut [u8],
    frame_width: usize,
    color: Option<Color>,
    x: usize,
    y: usize,
    coverage: u8,
) {
    let offset = (y * frame_width + x) * 2;
    let packed = u16::from_be_bytes([frame[offset], frame[offset + 1]]);
    let alpha = (u16::from(coverage) * 255 / 16) as u8;
    let next = match color {
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
    frame[offset..offset + 2].copy_from_slice(&next.to_be_bytes());
}

/// Calls `f` once per rectangle of `damage` that meets the box, or once with the box itself
/// when nothing restricts it. Each window is what the caller may ink.
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
    fn a_glyph_only_inks_the_damaged_rows() {
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
            canvas.draw_char_at('A', 0, 0, Metrics::vector(24));
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

    /// The cache may only make drawing faster: every shipped glyph drawn from a slot must land
    /// on exactly the pixels the outline would. The third pass reuses slots rasterized at a
    /// different position, proving coverage does not depend on where the glyph sits.
    #[test]
    fn a_cached_glyph_lands_on_the_outline_s_pixels() {
        const W: usize = 48;
        const H: usize = 40;
        let glyphs: Vec<char> = VectorFace::new().code_points().collect();
        assert!(glyphs.len() >= 181, "the table shrank: {}", glyphs.len());
        let mut cache = GlyphCache::new();
        for (pass, left) in [(0, 4), (1, 4), (2, 5)] {
            for &ch in &glyphs {
                let mut plain = vec![0u8; W * H * 2];
                let mut cached = vec![0u8; W * H * 2];
                {
                    let mut canvas = Canvas::new(&mut plain, W, H, Some(GROUND));
                    canvas.set_color(Some(Color::rgb(0xE0, 0xE0, 0xE0)));
                    canvas.draw_char_at(ch, left, 4, Metrics::vector(24));
                }
                {
                    let mut canvas =
                        Canvas::new(&mut cached, W, H, Some(GROUND)).with_glyphs(&mut cache);
                    canvas.set_color(Some(Color::rgb(0xE0, 0xE0, 0xE0)));
                    canvas.draw_char_at(ch, left, 4, Metrics::vector(24));
                }
                assert_eq!(plain, cached, "{ch:?} differs on pass {pass}");
            }
        }
    }

    /// One strike over another in sixteenths: full over full stays full, and the merge is
    /// symmetric because two passes commute.
    #[test]
    fn a_second_strike_merges_coverage_instead_of_replacing_it() {
        assert_eq!(over(0, 0), 0);
        assert_eq!(over(16, 0), 16);
        assert_eq!(over(16, 16), 16);
        assert_eq!(over(8, 8), 12);
        assert_eq!(over(4, 8), over(8, 4));
    }

    /// A heavy bitmap glyph is its plain mark plus the same mark shifted one column right,
    /// laid down once: striking twice on the inverted ground would cancel where they overlap.
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

    #[test]
    fn a_heavy_vector_glyph_is_not_erased_where_its_strokes_overlap() {
        const W: usize = 32;
        const H: usize = 32;
        let mut plain = vec![0u8; W * H * 2];
        let mut heavy = vec![0u8; W * H * 2];
        {
            let mut canvas = Canvas::new(&mut plain, W, H, None);
            canvas.draw_char_at('中', 2, 2, Metrics::vector(24));
        }
        {
            let mut canvas = Canvas::new(&mut heavy, W, H, None);
            canvas.draw_char_heavy_at('中', 2, 2, Metrics::vector(24));
        }
        let pixel = |frame: &[u8], x: usize, y: usize| {
            let offset = (y * W + x) * 2;
            u16::from_be_bytes([frame[offset], frame[offset + 1]])
        };
        let mut solid = 0usize;
        for y in 0..H {
            for x in 1..W {
                if pixel(&plain, x, y) == 0xFFFF && pixel(&plain, x - 1, y) == 0xFFFF {
                    solid += 1;
                    assert_eq!(
                        pixel(&heavy, x, y),
                        0xFFFF,
                        "the second strike reverted the overlap at ({x}, {y})"
                    );
                }
            }
        }
        assert!(solid > 0, "the sample has no solid two-column interior");
    }

    /// A cached heavy glyph has to land exactly where the uncached one does, at every
    /// position and after the slot was rasterized elsewhere.
    #[test]
    fn a_cached_heavy_glyph_lands_on_the_uncached_one() {
        const W: usize = 48;
        const H: usize = 40;
        let glyphs: Vec<char> = VectorFace::new().code_points().collect();
        let mut cache = GlyphCache::new();
        for (pass, left) in [(0, 4), (1, 5)] {
            for &ch in &glyphs {
                let mut plain = vec![0u8; W * H * 2];
                let mut cached = vec![0u8; W * H * 2];
                {
                    let mut canvas = Canvas::new(&mut plain, W, H, Some(GROUND));
                    canvas.set_color(Some(Color::rgb(0xE0, 0xE0, 0xE0)));
                    canvas.draw_char_heavy_at(ch, left, 4, Metrics::vector(24));
                }
                {
                    let mut canvas =
                        Canvas::new(&mut cached, W, H, Some(GROUND)).with_glyphs(&mut cache);
                    canvas.set_color(Some(Color::rgb(0xE0, 0xE0, 0xE0)));
                    canvas.draw_char_heavy_at(ch, left, 4, Metrics::vector(24));
                }
                assert_eq!(plain, cached, "{ch:?} differs on pass {pass}");
            }
        }
    }
}

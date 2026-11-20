//! The pixels a page is drawn into, and the only marks there are.

use crate::light::{Rgb, invert_rgb565, rgb565};

use super::panel::CONTENT_BOTTOM;

use super::Rect;
use super::font::{Face, Glyph};

/// A frame buffer and the operations every page draws with.
pub struct Canvas<'a> {
    frame: &'a mut [u8],
    width: usize,
    height: usize,
    ground: Option<Rgb>,
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

    /// Inverts one pixel whose bounds the caller has already checked.
    fn invert(&mut self, x: usize, y: usize) {
        let offset = (y * self.width + x) * 2;
        let packed = u16::from_be_bytes([self.frame[offset], self.frame[offset + 1]]);
        self.frame[offset..offset + 2].copy_from_slice(&invert_rgb565(packed).to_be_bytes());
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

    /// Inverts one pixel.
    pub fn invert_pixel(&mut self, x: isize, y: isize) {
        if self.clipped(x, y) {
            self.invert(x as usize, y as usize);
        }
    }

    /// Lays a glyph down a row at a time: a blank byte of the row skips eight pixels, which is
    /// most of a 6×12 cell, and nothing inside the box is checked again.
    fn draw_glyph(&mut self, glyph: &Glyph<'_>, left: isize, top: isize) {
        let (x0, y0, x1, y1) = self.reach(left, top, glyph.width as isize, glyph.height as isize);
        let skip = (x0 as isize - left) as usize;
        for y in y0..y1 {
            let bits = glyph.row((y as isize - top) as usize);
            let mut x = x0;
            while x < x1 {
                let at = skip + (x - x0);
                let byte = bits[at / 8];
                let first = at % 8;
                let span = (8 - first).min(x1 - x);
                if byte != 0 {
                    for i in 0..span {
                        if byte & (0x80 >> (first + i)) != 0 {
                            self.invert(x + i, y);
                        }
                    }
                }
                x += span;
            }
        }
    }

    /// Overdraws one character at a panel coordinate, `top` being the top of the line box.
    pub fn draw_char(&mut self, ch: char, left: isize, top: usize) {
        let face = Face::new();
        let glyph = face.glyph(ch);
        let baseline = top as isize + face.ascent() as isize;
        self.draw_glyph(
            &glyph,
            left + glyph.left,
            baseline - glyph.top - glyph.height as isize,
        );
    }

    /// Overdraws one run of text at a panel coordinate, `top` being the top of the line box.
    pub fn draw_text(&mut self, text: &[u8], left: usize, top: usize) {
        let face = Face::new();
        let baseline = top as isize + face.ascent() as isize;
        let mut pen = left as isize;
        for glyph in face.glyphs(text) {
            let glyph_top = baseline - glyph.top - glyph.height as isize;
            self.draw_glyph(&glyph, pen + glyph.left, glyph_top);
            pen += glyph.advance as isize;
        }
    }

    /// Columns a run occupies.
    pub fn measure(&self, text: &[u8]) -> usize {
        Face::new().measure(text)
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
            self.invert_pixel(x, y);
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
                self.invert_pixel(cx + dx, cy + dy);
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

    /// Fills `rect` with this frame's own ground turned over: the panel's one way of laying a
    /// paper down for drawing.
    pub fn fill_rect(&mut self, rect: Rect) {
        let Some(color) = self.ground else {
            return;
        };
        let surface = invert_rgb565(rgb565(color)).to_be_bytes();
        let bottom = (rect.bottom().max(0) as usize)
            .min(CONTENT_BOTTOM)
            .min(self.height);
        for y in rect.y.max(0) as usize..bottom {
            let (x0, _, x1, _) = self.reach(rect.x as isize, y as isize, rect.w as isize, 1);
            let start = (y * self.width + x0) * 2;
            let row = &mut self.frame[start..start + (x1 - x0) * 2];
            for pixel in row.as_chunks_mut::<2>().0 {
                pixel.copy_from_slice(&surface);
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
        canvas.invert_pixel(-1, 0);
        canvas.invert_pixel(0, -1);
        canvas.invert_pixel(WIDTH as isize, 0);
        canvas.invert_pixel(0, HEIGHT as isize);
        canvas.draw_text(b"X", WIDTH - 1, 0);
        assert!(bytes.iter().any(|byte| *byte != 0));
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

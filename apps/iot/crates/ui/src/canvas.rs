//! The pixels a page is drawn into, and the only ink there is.

use iot_core::drivers::light::{Fill, Rgb};
use iot_core::keyboard::ControlRect;
use iot_core::overlay::{FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP};
use iot_core::stamp::{fill_surface, invert_rgb565};

use crate::font::FONT;

/// A frame buffer and the operations every page draws with.
pub struct Canvas<'a> {
    frame: &'a mut [u8],
    width: usize,
    height: usize,
    ground: Option<(Fill, Rgb)>,
}

impl<'a> Canvas<'a> {
    /// A canvas over a whole frame.
    pub fn new(
        frame: &'a mut [u8],
        width: usize,
        height: usize,
        ground: Option<(Fill, Rgb)>,
    ) -> Self {
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

    /// The fill this frame was painted with, if the surface still owns it.
    pub fn ground(&self) -> Option<(Fill, Rgb)> {
        self.ground
    }

    /// Inverts one pixel.
    pub fn stamp_pixel(&mut self, x: isize, y: isize) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let offset = (y as usize * self.width + x as usize) * 2;
        let packed = u16::from_be_bytes([self.frame[offset], self.frame[offset + 1]]);
        self.frame[offset..offset + 2].copy_from_slice(&invert_rgb565(packed).to_be_bytes());
    }

    fn stamp_char(&mut self, ch: u8, left: usize, top: usize) {
        if !(0x20..=0x7E).contains(&ch) {
            return;
        }
        let base = usize::from(ch - 0x20) * FONT_W;
        for (col, &bits) in FONT[base..base + FONT_W].iter().enumerate() {
            for row in 0..FONT_H {
                if bits & (1 << row) == 0 {
                    continue;
                }
                self.stamp_pixel(left as isize + col as isize, top as isize + row as isize);
            }
        }
    }

    /// Overdraws one run of text at a panel coordinate.
    pub fn stamp_text(&mut self, text: &[u8], left: usize, top: usize) {
        for (glyph, &ch) in text.iter().enumerate() {
            self.stamp_char(ch, left + glyph * GLYPH_PITCH, top);
        }
    }

    /// [`Canvas::stamp_text`] measured from the text's last column rather than its first,
    /// so a value that grows a digit grows leftwards into empty space.
    pub fn stamp_text_right(&mut self, text: &[u8], right: isize, top: usize) {
        let left = right + OVERLAY_GAP as isize - text.len() as isize * GLYPH_PITCH as isize;
        self.stamp_text(text, left.max(0) as usize, top);
    }

    /// Inverts the straight run from `(x0, y0)` to `(x1, y1)` (Bresenham).
    pub fn stamp_line(&mut self, x0: isize, y0: isize, x1: isize, y1: isize) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.stamp_pixel(x, y);
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
    pub fn stamp_circle(&mut self, cx: isize, cy: isize, r: isize) {
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
                self.stamp_pixel(cx + dx, cy + dy);
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

    /// Fills `rect` with this frame's own ground turned over: the panel's one way of laying
    /// a paper down for ink.
    pub fn fill(&mut self, rect: ControlRect) {
        let Some((fill, color)) = self.ground else {
            return;
        };
        fill_surface(self.frame, rect, fill, color, self.width, self.height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const WIDTH: usize = 32;
    const HEIGHT: usize = 24;

    fn frame() -> alloc::vec::Vec<u8> {
        vec![0u8; WIDTH * HEIGHT * 2]
    }

    #[test]
    fn a_stamp_off_the_edge_is_clipped_rather_than_panicking() {
        let mut bytes = frame();
        let mut canvas = Canvas::new(&mut bytes, WIDTH, HEIGHT, None);
        canvas.stamp_pixel(-1, 0);
        canvas.stamp_pixel(0, -1);
        canvas.stamp_pixel(WIDTH as isize, 0);
        canvas.stamp_pixel(0, HEIGHT as isize);
        canvas.stamp_text(b"X", WIDTH - 1, 0);
        assert!(bytes.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn a_fill_paints_the_rect_with_the_ground_turned_over() {
        let mut bytes = frame();
        let mut canvas = Canvas::new(
            &mut bytes,
            WIDTH,
            HEIGHT,
            Some((Fill::Uniform, Rgb(0x10, 0x20, 0x30))),
        );
        canvas.fill(ControlRect {
            x: 2,
            y: 3,
            w: 4,
            h: 5,
        });
        let expected =
            iot_core::stamp::invert_rgb565(iot_core::stamp::rgb565(Rgb(0x10, 0x20, 0x30)))
                .to_be_bytes();
        for y in 3..8 {
            for x in 2..6 {
                let offset = (y * WIDTH + x) * 2;
                assert_eq!(bytes[offset..offset + 2], expected);
            }
        }
        assert_eq!(&bytes[0..2], &[0, 0], "outside the rect is untouched");
    }
}

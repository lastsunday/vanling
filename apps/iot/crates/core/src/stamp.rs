//! The panel's ink and paper: how a fill is written into a frame of RGB565 pixels.

use crate::drivers::light::{Fill, Rgb, scale_brightness, vertical_brightness};
use crate::keyboard::ControlRect;
use crate::overlay::CONTENT_BOTTOM;

/// Packs a colour into the panel's big-endian RGB565 word.
pub fn rgb565(color: Rgb) -> u16 {
    let r = color.0 as u16;
    let g = color.1 as u16;
    let b = color.2 as u16;
    ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
}

/// Per-channel bitwise inversion of an RGB565 word: a glyph drawn this way is visible on
/// any fill, matching the "off" side of the contrast regardless of base colour.
pub fn invert_rgb565(packed: u16) -> u16 {
    let r = (packed >> 11) & 0x1F;
    let g = (packed >> 5) & 0x3F;
    let b = packed & 0x1F;
    ((0x1F - r) << 11) | ((0x3F - g) << 5) | (0x1F - b)
}

/// Writes `rect` of `frame` as the surface: the ground the page was painted with, turned
/// over.
pub fn fill_surface(
    frame: &mut [u8],
    rect: ControlRect,
    fill: Fill,
    color: Rgb,
    width: usize,
    height: usize,
) {
    let right = (rect.x + rect.w).min(width);
    let bottom = (rect.y + rect.h).min(CONTENT_BOTTOM).min(height);
    for y in rect.y..bottom {
        let ground = match fill {
            Fill::Uniform => color,
            Fill::VerticalGradient => {
                scale_brightness(color, vertical_brightness(y as u16, height as u16))
            }
        };
        let surface = invert_rgb565(rgb565(ground)).to_be_bytes();
        for x in rect.x..right {
            let offset = (y * width + x) * 2;
            frame[offset..offset + 2].copy_from_slice(&surface);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 40;
    const HEIGHT: usize = CONTENT_BOTTOM + 8;
    const GROUND: Rgb = Rgb(0x10, 0x20, 0x30);

    fn ink() -> u16 {
        invert_rgb565(rgb565(GROUND))
    }

    fn ground() -> u16 {
        rgb565(GROUND)
    }

    fn painted() -> alloc::vec::Vec<u8> {
        let mut frame = alloc::vec![0u8; WIDTH * HEIGHT * 2];
        let ground = rgb565(GROUND).to_be_bytes();
        for pixel in frame.as_chunks_mut::<2>().0 {
            pixel.copy_from_slice(&ground);
        }
        frame
    }

    fn pixel(frame: &[u8], x: usize, y: usize) -> u16 {
        let offset = (y * WIDTH + x) * 2;
        u16::from_be_bytes([frame[offset], frame[offset + 1]])
    }

    fn box_of(x: usize, y: usize, w: usize, h: usize) -> ControlRect {
        ControlRect { x, y, w, h }
    }

    #[test]
    fn a_fill_covers_whatever_was_already_drawn_there() {
        let mut frame = painted();
        let rect = box_of(4, 8, 16, 12);

        let words = |frame: &mut alloc::vec::Vec<u8>, x: usize| {
            frame[((10 * WIDTH) + x) * 2..((10 * WIDTH) + x) * 2 + 2]
                .copy_from_slice(&ink().to_be_bytes());
        };
        for x in 6..12 {
            words(&mut frame, x);
        }
        words(&mut frame, 1);
        assert_eq!(
            pixel(&frame, 6, 10),
            ink(),
            "the words are there to begin with"
        );

        fill_surface(&mut frame, rect, Fill::Uniform, GROUND, WIDTH, HEIGHT);

        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                assert_eq!(
                    pixel(&frame, x, y),
                    ink(),
                    "({x}, {y}) is the surface and not what was under it"
                );
            }
        }
        assert_eq!(
            pixel(&frame, 1, 10),
            ink(),
            "and a word the box does not reach is still there"
        );
        assert_eq!(
            pixel(&frame, rect.x + rect.w, 10),
            ground(),
            "beside the page's own ground"
        );
        assert_eq!(pixel(&frame, rect.x, rect.y + rect.h), ground());
    }

    #[test]
    fn a_fill_of_a_gradient_page_follows_it_a_row_at_a_time() {
        let mut frame = painted();
        let rect = box_of(0, 0, WIDTH, 20);
        fill_surface(
            &mut frame,
            rect,
            Fill::VerticalGradient,
            GROUND,
            WIDTH,
            HEIGHT,
        );

        let at = |row: u16| {
            invert_rgb565(rgb565(scale_brightness(
                GROUND,
                vertical_brightness(row, HEIGHT as u16),
            )))
        };
        assert_eq!(pixel(&frame, 0, 0), at(0), "the top row is the page's top");
        assert_eq!(pixel(&frame, 0, 19), at(19), "and the last is its own");
        assert_ne!(
            at(0),
            at(19),
            "which is not the same colour, or this proves nothing"
        );
    }

    #[test]
    fn a_fill_leaves_the_footer_alone() {
        let mut frame = painted();
        fill_surface(
            &mut frame,
            box_of(0, CONTENT_BOTTOM - 2, WIDTH, 8),
            Fill::Uniform,
            GROUND,
            WIDTH,
            HEIGHT,
        );

        assert_ne!(
            pixel(&frame, 0, CONTENT_BOTTOM - 1),
            ground(),
            "the last content row is filled"
        );
        assert_eq!(
            pixel(&frame, 0, CONTENT_BOTTOM),
            ground(),
            "and the footer is not"
        );
    }
}

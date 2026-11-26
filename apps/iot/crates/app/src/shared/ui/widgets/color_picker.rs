//! An HSV picker: a saturation/value square, a hue strip, a swatch and its hex. Every block is
//! drawn from the same sampling `point_at` inverts, so a finger lands on the colour it sees.

use iot_core::light::{Rgb, hsv_to_rgb};
use iot_core::ui::{Canvas, Color, Rect};

use crate::shared::ui::theme::Theme;

pub const SQUARE: i16 = 132;
pub const STRIP: i16 = 16;
pub const SWATCH: i16 = 44;
pub const GAP: i16 = 8;
pub const BLOCKS: i16 = 12;
pub const WIDTH: i16 = SQUARE + GAP + STRIP + GAP + SWATCH;
pub const HEIGHT: i16 = SQUARE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picker {
    pub hue: u8,
    pub sat: u8,
    pub val: u8,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            hue: 170,
            sat: 200,
            val: 240,
        }
    }
}

/// What a panel point asks of the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    /// Saturation and value, from inside the square.
    Square { sat: u8, val: u8 },
    /// Hue, from the strip.
    Hue(u8),
}

pub fn square(rect: Rect) -> Rect {
    Rect::new(rect.x, rect.y, i32::from(SQUARE), i32::from(SQUARE))
}

pub fn strip(rect: Rect) -> Rect {
    Rect::new(
        rect.x + i32::from(SQUARE + GAP),
        rect.y,
        i32::from(STRIP),
        i32::from(SQUARE),
    )
}

pub fn swatch(rect: Rect) -> Rect {
    Rect::new(
        rect.x + i32::from(SQUARE + GAP + STRIP + GAP),
        rect.y,
        i32::from(SWATCH),
        i32::from(SWATCH),
    )
}

pub fn point_at(rect: Rect, x: i32, y: i32) -> Option<Sample> {
    let square = square(rect);
    if square.contains(x, y) {
        let sat = ((x - square.x) * 255 / (square.w - 1).max(1)).clamp(0, 255);
        let val = (255 - (y - square.y) * 255 / (square.h - 1).max(1)).clamp(0, 255);
        return Some(Sample::Square {
            sat: sat as u8,
            val: val as u8,
        });
    }
    let strip = strip(rect);
    if strip.contains(x, y) {
        let hue = ((y - strip.y) * 255 / (strip.h - 1).max(1)).clamp(0, 255);
        return Some(Sample::Hue(hue as u8));
    }
    None
}

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, picker: Picker, theme: &Theme) {
    let square = square(rect);
    let block = (square.w + i32::from(BLOCKS) - 1) / i32::from(BLOCKS);
    for row in 0..i32::from(BLOCKS) {
        for column in 0..i32::from(BLOCKS) {
            let sat = ((column * 255) / (i32::from(BLOCKS) - 1).max(1)) as u8;
            let val = (255 - (row * 255) / (i32::from(BLOCKS) - 1).max(1)) as u8;
            let Rgb(r, g, b) = hsv_to_rgb(picker.hue, sat, val);
            canvas.set_color(Some(Color::rgb(r, g, b)));
            canvas.fill_rect(Rect::new(
                square.x + column * block,
                square.y + row * block,
                block,
                block,
            ));
        }
    }

    let strip = strip(rect);
    let band = (strip.h + i32::from(BLOCKS) - 1) / i32::from(BLOCKS);
    for row in 0..i32::from(BLOCKS) {
        let hue = ((row * 255) / (i32::from(BLOCKS) - 1).max(1)) as u8;
        let Rgb(r, g, b) = hsv_to_rgb(hue, 255, 255);
        canvas.set_color(Some(Color::rgb(r, g, b)));
        canvas.fill_rect(Rect::new(strip.x, strip.y + row * band, strip.w, band));
    }
    canvas.set_color(Some(theme.foreground));
    let mark = strip.y + i32::from(picker.hue) * (strip.h - 1) / 255;
    canvas.draw_line(
        strip.x as isize,
        mark as isize,
        strip.right() as isize,
        mark as isize,
    );

    let swatch = swatch(rect);
    let Rgb(r, g, b) = hsv_to_rgb(picker.hue, picker.sat, picker.val);
    canvas.set_color(Some(Color::rgb(r, g, b)));
    canvas.fill_round_rect(swatch, 4);
    canvas.set_color(Some(theme.muted));
    canvas.stroke_round_rect(swatch, 4);
    canvas.set_color(Some(theme.foreground));
    let mut buf = [0u8; 6];
    let hex = hex(picker, &mut buf);
    canvas.draw_text(
        hex,
        swatch.x.max(0) as usize,
        (swatch.bottom() + 4).max(0) as usize,
    );
    canvas.set_color(None);
}

/// `RRGGBB` of the picker's current colour.
pub fn hex(picker: Picker, buf: &mut [u8; 6]) -> &[u8] {
    let Rgb(r, g, b) = hsv_to_rgb(picker.hue, picker.sat, picker.val);
    let bytes = [r, g, b];
    for (index, byte) in bytes.into_iter().enumerate() {
        buf[index * 2] = HEX[usize::from(byte >> 4)];
        buf[index * 2 + 1] = HEX[usize::from(byte & 0x0F)];
    }
    buf
}

const HEX: [u8; 16] = *b"0123456789ABCDEF";

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(10, 20, WIDTH as i32, HEIGHT as i32);

    #[test]
    fn the_square_maps_its_corners_to_the_hsv_extremes() {
        let square = square(BOX);
        assert_eq!(
            point_at(BOX, square.x, square.y),
            Some(Sample::Square { sat: 0, val: 255 }),
            "top left is white"
        );
        assert_eq!(
            point_at(BOX, square.right() - 1, square.bottom() - 1),
            Some(Sample::Square { sat: 255, val: 0 }),
            "bottom right is black"
        );
    }

    #[test]
    fn the_strip_covers_every_hue_top_to_bottom() {
        let strip = strip(BOX);
        assert_eq!(point_at(BOX, strip.x + 2, strip.y), Some(Sample::Hue(0)));
        assert_eq!(
            point_at(BOX, strip.x + 2, strip.bottom() - 1),
            Some(Sample::Hue(255))
        );
        assert_eq!(point_at(BOX, swatch(BOX).x + 2, BOX.y + 2), None);
    }

    #[test]
    fn the_swatch_and_hex_follow_the_same_colour() {
        let mut buf = [0u8; 6];
        for (picker, want) in [
            (
                Picker {
                    hue: 0,
                    sat: 0,
                    val: 255,
                },
                &b"FFFFFF"[..],
            ),
            (
                Picker {
                    hue: 0,
                    sat: 0,
                    val: 0,
                },
                &b"000000"[..],
            ),
            (
                Picker {
                    hue: 170,
                    sat: 255,
                    val: 255,
                },
                &b"0004FF"[..],
            ),
        ] {
            assert_eq!(hex(picker, &mut buf), want, "{picker:?}");
        }
    }

    #[test]
    fn the_picker_fits_beside_the_page_margin() {
        assert_eq!(WIDTH, 208);
        assert!(WIDTH <= 306);
    }
}

//! ARGB colours as a page states them, and the palette one declaration interns them in.
//!
//! The panel writes RGB565; a colour keeps its alpha so a translucent mark blends with what is
//! already in the frame. A declaration holds at most [`MAX_COLORS`] distinct colours, and a
//! node names one by index, which is what keeps a colour one byte in a node's style.

use crate::light::{Rgb, rgb565};

/// Distinct colours one declaration may name.
pub const MAX_COLORS: usize = 16;

/// A colour in ARGB32: the same order Flutter's `Color` uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Self = Self(0);

    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self(0xFF00_0000 | Self::pack(red, green, blue))
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self((alpha as u32) << 24 | Self::pack(red, green, blue))
    }

    const fn pack(red: u8, green: u8, blue: u8) -> u32 {
        (red as u32) << 16 | (green as u32) << 8 | blue as u32
    }

    pub const fn alpha(self) -> u8 {
        (self.0 >> 24) as u8
    }

    pub const fn rgb888(self) -> Rgb {
        Rgb((self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8)
    }

    /// The panel's own encoding, alpha aside.
    pub fn packed(self) -> u16 {
        rgb565(self.rgb888())
    }
}

/// A colour's slot in its declaration's palette; `0` means the node names none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorId(pub u8);

impl ColorId {
    pub const NONE: Self = Self(0);
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    colors: [Color; MAX_COLORS],
    len: u8,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            colors: [Color::TRANSPARENT; MAX_COLORS],
            len: 0,
        }
    }
}

impl Palette {
    /// The slot for `color`, adding it when it is new.
    pub fn intern(&mut self, color: Color) -> ColorId {
        if let Some(index) = self.colors[..usize::from(self.len)]
            .iter()
            .position(|held| *held == color)
        {
            return ColorId(index as u8 + 1);
        }
        if usize::from(self.len) >= MAX_COLORS {
            debug_assert!(false, "the declaration ran out of colours");
            return ColorId::NONE;
        }
        self.colors[usize::from(self.len)] = color;
        self.len += 1;
        ColorId(self.len)
    }

    pub fn get(&self, id: ColorId) -> Option<Color> {
        let index = usize::from(id.0).checked_sub(1)?;
        self.colors[..usize::from(self.len)].get(index).copied()
    }
}

/// One source-over step onto a packed RGB565 pixel. An opaque colour never reads the frame.
pub fn blend(dst: u16, color: Color) -> u16 {
    let alpha = u32::from(color.alpha());
    if alpha == 0xFF {
        return color.packed();
    }
    if alpha == 0 {
        return dst;
    }
    let src = color.rgb888();
    let (r, g, b) = expand(dst);
    let mix = |under: u8, over: u8| {
        ((u32::from(over) * alpha + u32::from(under) * (0xFF - alpha)) / 0xFF) as u8
    };
    rgb565(Rgb(mix(r, src.0), mix(g, src.1), mix(b, src.2)))
}

fn expand(packed: u16) -> (u8, u8, u8) {
    let r = (packed >> 11) as u8 & 0x1F;
    let g = (packed >> 5) as u8 & 0x3F;
    let b = packed as u8 & 0x1F;
    (
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_hands_the_same_slot_back_for_the_same_colour() {
        let mut palette = Palette::default();
        let red = Color::rgb(0xFF, 0, 0);
        let first = palette.intern(red);
        assert_eq!(palette.intern(Color::rgb(0xFF, 0, 0)), first);
        assert_eq!(palette.get(first), Some(red));
        assert_eq!(palette.get(ColorId::NONE), None, "zero names no colour");
        assert_ne!(palette.intern(Color::rgb(0, 0xFF, 0)), first);
    }

    #[test]
    fn an_opaque_colour_replaces_the_pixel_and_a_clear_one_leaves_it() {
        let under = rgb565(Rgb(0x10, 0x20, 0x30));
        assert_eq!(
            blend(under, Color::rgb(0xFF, 0, 0)),
            rgb565(Rgb(0xFF, 0, 0))
        );
        assert_eq!(blend(under, Color::rgba(0xFF, 0, 0, 0)), under);
    }

    #[test]
    fn a_half_transparent_white_sits_between_the_two() {
        let under = rgb565(Rgb(0, 0, 0));
        let over = blend(under, Color::rgba(0xFF, 0xFF, 0xFF, 0x80));
        let (r, _, _) = expand(over);
        assert!((0x70..=0x90).contains(&r), "blended red is {r:#04x}");
    }
}

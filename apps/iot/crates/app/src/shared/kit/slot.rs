//! The optional pieces of a cell: a leading mark, a trailing badge, or nothing.

use iot_core::ui::font::Face;
use iot_core::ui::{Canvas, Rect};

use super::theme::Theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Slot<'a> {
    #[default]
    None,
    Text(&'a [u8]),
    Badge(&'a [u8]),
    Mark(&'a [u8]),
    Glyph(char),
}

impl<'a> Slot<'a> {
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub fn width(self, canvas: &Canvas<'_>) -> i32 {
        match self {
            Self::None => 0,
            Self::Text(run) | Self::Badge(run) | Self::Mark(run) => canvas.measure(run) as i32,
            Self::Glyph(glyph) => Face::new().advance(glyph) as i32,
        }
    }

    /// Draws the slot at `(left, top)` and answers the width it took.
    pub fn draw(self, canvas: &mut Canvas<'_>, left: i32, top: i32, theme: &Theme) -> i32 {
        let width = self.width(canvas);
        match self {
            Self::None => {}
            Self::Text(run) | Self::Mark(run) => {
                let ink = if matches!(self, Self::Mark(_)) {
                    theme.muted
                } else {
                    theme.ink
                };
                canvas.set_color(Some(ink));
                canvas.draw_text(run, left.max(0) as usize, top.max(0) as usize);
            }
            Self::Badge(run) => {
                let rect = Rect::new(left, top, width + 4, 12);
                canvas.set_color(Some(theme.accent));
                canvas.fill_round_rect(rect, 3);
                canvas.set_color(Some(theme.on_accent));
                canvas.draw_text(run, (left + 2).max(0) as usize, top.max(0) as usize);
            }
            Self::Glyph(glyph) => {
                canvas.set_color(Some(theme.ink));
                canvas.draw_char(glyph, left as isize, top.max(0) as usize);
            }
        }
        canvas.set_color(None);
        width
    }
}

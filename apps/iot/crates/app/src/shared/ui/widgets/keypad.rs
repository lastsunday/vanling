//! A 4x3 keypad drawn as one cell; drawing and hit testing read the same grid.

use iot_core::ui::panel::LINE;
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const KEYS: [&[u8; 3]; 4] = [b"123", b"456", b"789", b"*0#"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keypad {
    pub pressed: Option<u8>,
}

impl Keypad {
    /// The key index under a panel point, `0..12`, when the point lands on a key.
    pub fn key_at(rect: Rect, x: i32, y: i32) -> Option<u8> {
        if !rect.contains(x, y) {
            return None;
        }
        let column = (x - rect.x) * 3 / rect.w.max(1);
        let row = (y - rect.y) * 4 / rect.h.max(1);
        if !(0..3).contains(&column) || !(0..4).contains(&row) {
            return None;
        }
        u8::try_from(row * 3 + column).ok()
    }

    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        let edge = theme.muted;
        let fg = theme.foreground;
        let paper = theme.selection;
        let cell_w = rect.w / 3;
        let cell_h = rect.h / 4;
        for (index, key) in KEYS.iter().enumerate() {
            let column = i32::try_from(index % 3).unwrap_or(0);
            let row = i32::try_from(index / 3).unwrap_or(0);
            let cell = Rect::new(
                rect.x + column * cell_w + 2,
                rect.y + row * cell_h + 1,
                cell_w - 4,
                cell_h - 2,
            );
            if self.pressed == u8::try_from(index).ok() {
                canvas.set_color(Some(paper));
                canvas.fill_round_rect(cell, 3);
            } else {
                canvas.set_color(Some(edge));
                canvas.stroke_round_rect(cell, 3);
            }
            let glyph = [key[0]];
            let left = cell.x + (cell.w - canvas.measure(&glyph) as i32) / 2;
            let top = cell.y + (cell.h - LINE as i32) / 2;
            canvas.set_color(Some(fg));
            canvas.draw_text(&glyph, left.max(0) as usize, top.max(0) as usize);
        }
        canvas.set_color(None);
    }
}

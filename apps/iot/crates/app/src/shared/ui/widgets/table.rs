//! A header plus fixed cells, drawn on one canvas: columns share the width equally.

use iot_core::ui::panel::{LINE, TOUCH_MIN};
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

/// One text line plus the rule's breathing room.
pub const HEADER_H: i16 = LINE as i16 + 4;
pub const ROW_H: i16 = TOUCH_MIN as i16;

#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    pub headers: [&'a [u8]; 3],
    pub rows: [[&'a [u8]; 3]; 4],
}

impl Table<'_> {
    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        let column = rect.w / 3;
        let muted = theme.muted;
        let ink = theme.ink;
        canvas.set_color(Some(muted));
        canvas.draw_line(
            rect.x as isize,
            (rect.y + i32::from(HEADER_H)) as isize,
            rect.right() as isize,
            (rect.y + i32::from(HEADER_H)) as isize,
        );
        for (index, header) in self.headers.iter().enumerate() {
            let left = rect.x + i32::try_from(index).unwrap_or(0) * column + 2;
            canvas.draw_text(header, left.max(0) as usize, rect.y.max(0) as usize);
        }
        for (row, cells) in self.rows.iter().enumerate() {
            let top =
                rect.y + i32::from(HEADER_H) + i32::try_from(row).unwrap_or(0) * i32::from(ROW_H);
            let text_top = top + (i32::from(ROW_H) - LINE as i32) / 2;
            canvas.set_color(Some(if row % 2 == 0 { ink } else { muted }));
            for (index, cell) in cells.iter().enumerate() {
                let left = rect.x + i32::try_from(index).unwrap_or(0) * column + 2;
                canvas.draw_text(cell, left.max(0) as usize, text_top.max(0) as usize);
            }
        }
        canvas.set_color(None);
    }
}

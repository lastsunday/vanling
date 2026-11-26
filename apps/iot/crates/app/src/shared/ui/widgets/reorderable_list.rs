//! A list whose rows move: a 44 px handle at the right edge owns the drag, the lifted row is
//! drawn raised, and a rule marks where it will land.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const ROW_H: i16 = 32;
pub const HANDLE: i16 = 44;
pub const GLYPH: i16 = 12;
pub const PAD: i16 = 6;

#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    pub label: &'a [u8],
    pub detail: &'a [u8],
}

#[derive(Clone, Copy, Debug)]
pub struct ReorderableList<'a> {
    pub rows: &'a [Row<'a>],
    pub lifted: Option<u8>,
    pub target: Option<u8>,
}

pub const fn height(count: usize) -> i16 {
    ROW_H * count as i16
}

pub fn row_at(rect: Rect, count: usize, x: i32, y: i32) -> Option<u8> {
    if !rect.contains(x, y) {
        return None;
    }
    let row = (y - rect.y) / i32::from(ROW_H);
    (row < count as i32).then_some(row as u8)
}

/// The handle a panel point lands on: the rightmost [`HANDLE`] pixels of a row.
pub fn handle_at(rect: Rect, count: usize, x: i32, y: i32) -> Option<u8> {
    if x < rect.right() - i32::from(HANDLE) {
        return None;
    }
    row_at(rect, count, x, y)
}

impl ReorderableList<'_> {
    pub fn draw(&self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        let pitch = i32::from(ROW_H);
        for (index, row) in self.rows.iter().enumerate() {
            let top = rect.y + index as i32 * pitch;
            if top + pitch > rect.bottom() {
                break;
            }
            let box_ = Rect::new(rect.x, top + 1, rect.w, pitch - 2);
            if self.lifted == Some(index as u8) {
                canvas.set_color(Some(theme.surface));
                canvas.fill_round_rect(box_, 4);
                canvas.set_color(Some(theme.accent));
                canvas.stroke_round_rect(box_, 4);
            }
            canvas.set_color(Some(theme.foreground));
            canvas.draw_text(
                row.label,
                (rect.x + i32::from(PAD)).max(0) as usize,
                (top + 4).max(0) as usize,
            );
            canvas.set_color(Some(theme.muted));
            canvas.draw_text(
                row.detail,
                (rect.x + i32::from(PAD)).max(0) as usize,
                (top + 18).max(0) as usize,
            );
            let grip = rect.right() - i32::from(HANDLE) / 2;
            let middle = top + pitch / 2;
            canvas.set_color(Some(theme.muted));
            for step in -1..=1 {
                canvas.draw_line(
                    (grip - i32::from(GLYPH) / 2) as isize,
                    (middle + step * 3) as isize,
                    (grip + i32::from(GLYPH) / 2) as isize,
                    (middle + step * 3) as isize,
                );
            }
        }
        if let Some(target) = self.target {
            let top = rect.y + i32::from(target) * pitch;
            canvas.set_color(Some(theme.accent));
            canvas.fill_rect(Rect::new(rect.x, top, rect.w, 2));
        }
        canvas.set_color(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(10, 20, 300, height(5) as i32);

    #[test]
    fn the_handle_is_the_right_edge_and_the_row_is_anywhere() {
        let y = BOX.y + i32::from(ROW_H) + 4;
        assert_eq!(handle_at(BOX, 5, BOX.right() - 2, y), Some(1));
        assert_eq!(handle_at(BOX, 5, BOX.x + 4, y), None, "the label side");
        assert_eq!(row_at(BOX, 5, BOX.x + 4, y), Some(1));
        assert_eq!(row_at(BOX, 5, BOX.x + 4, BOX.bottom()), None);
    }

    #[test]
    fn five_rows_fit_the_page() {
        assert_eq!(height(5), 160);
        assert!(i32::from(HANDLE) >= 44);
    }
}

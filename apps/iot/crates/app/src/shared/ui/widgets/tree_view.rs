//! A tree: rows at one pitch, an indent per level, a chevron that says whether a branch is
//! open. Drawing and hit testing share the row pitch, so a finger lands on the row it sees.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const ROW_H: i16 = 24;
pub const INDENT: i16 = 10;
pub const MARK: i16 = 12;
pub const PAD: i16 = 4;

#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    pub depth: u8,
    pub label: &'a [u8],
    pub open: bool,
    pub leaf: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TreeView<'a> {
    pub rows: &'a [Row<'a>],
    pub selected: Option<u8>,
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

impl TreeView<'_> {
    pub fn draw(&self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        let pitch = i32::from(ROW_H);
        for (index, row) in self.rows.iter().enumerate() {
            let top = rect.y + index as i32 * pitch;
            if top + pitch > rect.bottom() {
                break;
            }
            if self.selected == Some(index as u8) {
                canvas.set_color(Some(theme.selection));
                canvas.fill_rect(Rect::new(rect.x, top, rect.w, pitch));
            }
            let left = rect.x + i32::from(PAD) + i32::from(row.depth) * i32::from(INDENT);
            if !row.leaf {
                canvas.set_color(Some(theme.muted));
                let middle = top + pitch / 2;
                if row.open {
                    canvas.draw_line(
                        (left + 1) as isize,
                        (middle - 2) as isize,
                        (left + 4) as isize,
                        (middle + 1) as isize,
                    );
                    canvas.draw_line(
                        (left + 4) as isize,
                        (middle + 1) as isize,
                        (left + 7) as isize,
                        (middle - 2) as isize,
                    );
                } else {
                    canvas.draw_line(
                        (left + 2) as isize,
                        (middle - 3) as isize,
                        (left + 5) as isize,
                        middle as isize,
                    );
                    canvas.draw_line(
                        (left + 5) as isize,
                        middle as isize,
                        (left + 2) as isize,
                        (middle + 3) as isize,
                    );
                }
            }
            canvas.set_color(Some(if row.leaf {
                theme.foreground
            } else {
                theme.accent
            }));
            canvas.draw_text(
                row.label,
                (left + i32::from(MARK)).max(0) as usize,
                (top + (pitch - 12) / 2).max(0) as usize,
            );
        }
        canvas.set_color(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(10, 20, 300, height(6) as i32);

    #[test]
    fn every_row_takes_its_own_pitch() {
        for row in 0..6u8 {
            let y = BOX.y + i32::from(row) * i32::from(ROW_H) + 2;
            assert_eq!(row_at(BOX, 6, BOX.x + 4, y), Some(row));
        }
        assert_eq!(row_at(BOX, 6, BOX.x + 4, BOX.bottom()), None);
        assert_eq!(row_at(BOX, 6, BOX.x - 1, BOX.y + 2), None);
    }

    #[test]
    fn six_rows_fit_the_page() {
        assert_eq!(height(6), 144);
        assert!(height(6) <= 306);
    }
}

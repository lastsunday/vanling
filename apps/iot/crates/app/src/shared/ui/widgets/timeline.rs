//! A single-sided timeline: a rail, one dot per entry, the time above the words.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const ROW_H: i16 = 40;
pub const RAIL: i16 = 5;
pub const DOT: isize = 3;
pub const TEXT: i16 = 18;

#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    pub time: &'a [u8],
    pub text: &'a [u8],
    pub lit: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Timeline<'a> {
    pub entries: &'a [Entry<'a>],
}

pub const fn height(count: usize) -> i16 {
    ROW_H * count as i16
}

/// The entry a panel point picks, on the same pitch the draw uses.
pub fn row_at(rect: Rect, count: usize, x: i32, y: i32) -> Option<u8> {
    if !rect.contains(x, y) {
        return None;
    }
    let row = (y - rect.y) / i32::from(ROW_H);
    (row < count as i32).then_some(row as u8)
}

impl Timeline<'_> {
    pub fn draw(&self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        if self.entries.is_empty() {
            return;
        }
        let rail = rect.x + i32::from(RAIL);
        let pitch = i32::from(ROW_H);
        let last = rect.y + (self.entries.len() as i32 - 1) * pitch + pitch / 2;
        canvas.set_color(Some(theme.muted));
        canvas.draw_line(
            rail as isize,
            (rect.y + pitch / 2) as isize,
            rail as isize,
            last as isize,
        );
        for (index, entry) in self.entries.iter().enumerate() {
            let top = rect.y + index as i32 * pitch;
            canvas.set_color(Some(if entry.lit { theme.accent } else { theme.muted }));
            canvas.draw_circle(rail as isize, (top + pitch / 2) as isize, DOT);
            canvas.set_color(Some(theme.muted));
            canvas.draw_text(
                entry.time,
                (rect.x + i32::from(TEXT)).max(0) as usize,
                (top + 3).max(0) as usize,
            );
            canvas.set_color(Some(theme.foreground));
            canvas.draw_text(
                entry.text,
                (rect.x + i32::from(TEXT)).max(0) as usize,
                (top + 17).max(0) as usize,
            );
        }
        canvas.set_color(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_entries_fit_the_page() {
        assert_eq!(height(4), 160);
        assert!(height(4) <= 306);
        assert!(i32::from(TEXT) + 5 * 6 <= 300, "a five-digit time fits");
    }
}

//! A month grid: seven columns, six rows, day cells drawn and hit by one geometry.

use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const COLUMNS: usize = 7;
pub const ROWS: usize = 6;
pub const CELL_W: i16 = 42;
pub const CELL_H: i16 = 32;
pub const GAP: i16 = 1;
pub const HEADER_H: i16 = 12;

pub const WIDTH: i16 = COLUMNS as i16 * CELL_W + (COLUMNS as i16 - 1) * GAP;
pub const HEIGHT: i16 = HEADER_H + ROWS as i16 * CELL_H;

const WEEKDAYS: [u8; COLUMNS] = *b"MTWTFSS";

/// Days in `month`, with February following the Gregorian rule.
pub const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const fn leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// The column the first day falls on, Monday as zero.
pub const fn first_weekday(year: u16, month: u8) -> u8 {
    (weekday(year, month, 1) + 6) % 7
}

/// Sakamoto's day of week: Sunday as zero.
const fn weekday(year: u16, month: u8, day: u8) -> u8 {
    const SHIFT: [u8; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let year = if month < 3 {
        year as u32 - 1
    } else {
        year as u32
    };
    ((year + year / 4 - year / 100 + year / 400 + SHIFT[month as usize - 1] as u32 + day as u32)
        % 7) as u8
}

pub fn cell_rect(rect: Rect, index: u8) -> Rect {
    let column = i32::from(index % COLUMNS as u8);
    let row = i32::from(index / COLUMNS as u8);
    Rect::new(
        rect.x + column * (i32::from(CELL_W) + i32::from(GAP)),
        rect.y + i32::from(HEADER_H) + row * i32::from(CELL_H),
        i32::from(CELL_W),
        i32::from(CELL_H),
    )
}

/// The day a panel point picks, or `None` on the header, a gap, or a blank cell.
pub fn day_at(rect: Rect, year: u16, month: u8, x: i32, y: i32) -> Option<u8> {
    let top = rect.y + i32::from(HEADER_H);
    if x < rect.x
        || x >= rect.x + i32::from(WIDTH)
        || y < top
        || y >= top + i32::from(HEIGHT) - i32::from(HEADER_H)
    {
        return None;
    }
    let pitch = i32::from(CELL_W + GAP);
    if (x - rect.x) % pitch >= i32::from(CELL_W) {
        return None;
    }
    let index = (y - top) / i32::from(CELL_H) * COLUMNS as i32 + (x - rect.x) / pitch;
    let day = index - i32::from(first_weekday(year, month)) + 1;
    (1..=i32::from(days_in_month(year, month)))
        .contains(&day)
        .then_some(day as u8)
}

pub fn draw(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    year: u16,
    month: u8,
    selected: Option<u8>,
    theme: &Theme,
) {
    let muted = theme.muted;
    canvas.set_color(Some(muted));
    for (column, letter) in WEEKDAYS.iter().enumerate() {
        let cell = cell_rect(rect, u8::try_from(column).unwrap_or(0));
        let glyph = [*letter];
        let width = canvas.measure(&glyph) as i32;
        canvas.draw_text(
            &glyph,
            (cell.x + (cell.w - width) / 2).max(0) as usize,
            rect.y.max(0) as usize,
        );
    }
    let first = i32::from(first_weekday(year, month));
    let days = i32::from(days_in_month(year, month));
    for day in 1..=days {
        let index = first + day - 1;
        let cell = cell_rect(rect, u8::try_from(index).unwrap_or(0));
        let picked = selected == Some(day as u8);
        if picked {
            canvas.set_color(Some(theme.accent));
            canvas.fill_round_rect(cell, 4);
        }
        let mut buf = [0u8; 6];
        let text = format_u16(day as u16, &mut buf);
        let width = canvas.measure(text) as i32;
        canvas.set_color(Some(if picked {
            theme.on_accent
        } else {
            theme.foreground
        }));
        canvas.draw_text(
            text,
            (cell.x + (cell.w - width) / 2).max(0) as usize,
            (cell.y + (cell.h - 12) / 2).max(0) as usize,
        );
    }
    canvas.set_color(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(10, 20, WIDTH as i32, HEIGHT as i32);

    #[test]
    fn the_gregorian_month_length_and_leap_rule_hold() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 11), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn the_first_weekday_is_monday_zero() {
        // 2026-11-01 is a Sunday.
        assert_eq!(first_weekday(2026, 11), 6);
        // 2026-06-01 is a Monday.
        assert_eq!(first_weekday(2026, 6), 0);
    }

    #[test]
    fn every_day_of_a_month_lands_on_its_own_cell_and_no_blank_one_answers() {
        let (year, month) = (2026, 11);
        let first = i32::from(first_weekday(year, month));
        for day in 1..=days_in_month(year, month) {
            let index = u8::try_from(first + i32::from(day) - 1).unwrap();
            let cell = cell_rect(BOX, index);
            assert_eq!(
                day_at(BOX, year, month, cell.x + cell.w / 2, cell.y + cell.h / 2),
                Some(day)
            );
        }
        let blank = cell_rect(BOX, 0);
        assert_eq!(
            day_at(BOX, year, month, blank.x + 1, blank.y + 1),
            None,
            "the first cell is still October"
        );
        assert_eq!(
            day_at(BOX, year, month, BOX.x + 1, BOX.y + 1),
            None,
            "the header"
        );
    }

    #[test]
    fn the_gap_between_cells_belongs_to_nobody() {
        let cell = cell_rect(BOX, 7);
        assert_eq!(
            day_at(BOX, 2026, 11, cell.right(), cell.y + 2),
            None,
            "the one-pixel gutter"
        );
    }

    #[test]
    fn the_grid_is_the_width_the_page_reserves() {
        assert_eq!(WIDTH, 300);
        assert_eq!(HEIGHT, 204);
        assert!(WIDTH <= 306, "the page's content width");
    }
}

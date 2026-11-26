//! A month grid: seven columns, six rows, every geometry derived from the rect it is given.
//!
//! Drawing and hit testing share [`Metrics`], so the cell a finger lands on is the cell drawn.

use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const COLUMNS: usize = 7;
pub const ROWS: usize = 6;
pub const GAP: i16 = 1;
pub const HEADER_H: i16 = 12;
pub const MIN_CELL: i16 = 28;
pub const MAX_CELL: i16 = 48;
pub const MIN_WIDTH: i16 = COLUMNS as i16 * MIN_CELL + (COLUMNS as i16 - 1) * GAP;
pub const MIN_HEIGHT: i16 = HEADER_H + ROWS as i16 * MIN_CELL;

/// A height that fills a 306 px content column at the panel's 42 px cell.
pub const HEIGHT: i16 = 204;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub header_h: i16,
    pub cell_w: i16,
    pub cell_h: i16,
    pub gap: i16,
    pub left: i32,
    pub top: i32,
}

pub fn metrics(rect: Rect) -> Metrics {
    let gap = GAP;
    let cell_w = ((rect.w - i32::from(gap) * (COLUMNS as i32 - 1)) / COLUMNS as i32)
        .clamp(1, i32::from(MAX_CELL)) as i16;
    let cell_h =
        ((rect.h - i32::from(HEADER_H)) / ROWS as i32).clamp(1, i32::from(MAX_CELL)) as i16;
    let grid_w = COLUMNS as i32 * i32::from(cell_w) + (COLUMNS as i32 - 1) * i32::from(gap);
    let grid_h = i32::from(HEADER_H) + ROWS as i32 * i32::from(cell_h);
    Metrics {
        header_h: HEADER_H,
        cell_w,
        cell_h,
        gap,
        left: rect.x + (rect.w - grid_w) / 2,
        top: rect.y + (rect.h - grid_h) / 2,
    }
}

/// Whether the rect can hold a comfortable cell in both axes.
pub fn fits(rect: Rect) -> bool {
    let m = metrics(rect);
    m.cell_w >= MIN_CELL && m.cell_h >= MIN_CELL
}

pub fn cell_rect(rect: Rect, index: u8) -> Rect {
    let m = metrics(rect);
    let column = i32::from(index % COLUMNS as u8);
    let row = i32::from(index / COLUMNS as u8);
    Rect::new(
        m.left + column * (i32::from(m.cell_w) + i32::from(m.gap)),
        m.top + i32::from(m.header_h) + row * i32::from(m.cell_h),
        i32::from(m.cell_w),
        i32::from(m.cell_h),
    )
}

/// The day a panel point picks, or `None` on the header, a gap, or a blank cell.
pub fn day_at(rect: Rect, year: u16, month: u8, x: i32, y: i32) -> Option<u8> {
    let m = metrics(rect);
    let top = m.top + i32::from(m.header_h);
    let pitch = i32::from(m.cell_w + m.gap);
    let bottom = top + ROWS as i32 * i32::from(m.cell_h);
    if x < m.left
        || x >= m.left + COLUMNS as i32 * pitch - i32::from(m.gap)
        || y < top
        || y >= bottom
    {
        return None;
    }
    if (x - m.left) % pitch >= i32::from(m.cell_w) {
        return None;
    }
    let index = (y - top) / i32::from(m.cell_h) * COLUMNS as i32 + (x - m.left) / pitch;
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
    let m = metrics(rect);
    canvas.set_color(Some(theme.muted));
    for (column, letter) in WEEKDAYS.iter().enumerate() {
        let cell = cell_rect(rect, u8::try_from(column).unwrap_or(0));
        let glyph = [*letter];
        let width = canvas.measure(&glyph) as i32;
        canvas.draw_text(
            &glyph,
            (cell.x + (cell.w - width) / 2).max(0) as usize,
            m.top.max(0) as usize,
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

    const SIZES: [Rect; 4] = [
        Rect::new(10, 20, 306, HEIGHT as i32),
        Rect::new(0, 0, 300, 204),
        Rect::new(0, 0, 700, 500),
        Rect::new(0, 0, 202, 180),
    ];

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
        assert_eq!(first_weekday(2026, 11), 6, "2026-11-01 is a Sunday");
        assert_eq!(first_weekday(2026, 6), 0, "2026-06-01 is a Monday");
    }

    #[test]
    fn every_day_lands_on_its_own_cell_in_every_size() {
        let (year, month) = (2026, 11);
        for rect in SIZES {
            let first = i32::from(first_weekday(year, month));
            for day in 1..=days_in_month(year, month) {
                let index = u8::try_from(first + i32::from(day) - 1).unwrap();
                let cell = cell_rect(rect, index);
                assert_eq!(
                    day_at(rect, year, month, cell.x + cell.w / 2, cell.y + cell.h / 2),
                    Some(day),
                    "{rect:?}"
                );
            }
            let blank = cell_rect(rect, 0);
            assert_eq!(day_at(rect, year, month, blank.x + 1, blank.y + 1), None);
            assert_eq!(
                day_at(rect, year, month, rect.x, rect.y),
                None,
                "the header"
            );
        }
    }

    #[test]
    fn the_gap_between_cells_belongs_to_nobody() {
        for rect in SIZES {
            let cell = cell_rect(rect, 7);
            assert_eq!(day_at(rect, 2026, 11, cell.right(), cell.y + 2), None);
        }
    }

    #[test]
    fn the_grid_answers_whether_it_fits() {
        assert!(fits(SIZES[0]));
        assert!(fits(SIZES[2]));
        assert!(!fits(Rect::new(0, 0, 201, 180)), "one pixel short");
        assert!(!fits(Rect::new(0, 0, 202, 179)), "one row short");
        let m = metrics(Rect::new(0, 0, 700, 500));
        assert_eq!(
            (m.cell_w, m.cell_h),
            (MAX_CELL, MAX_CELL),
            "cells are capped"
        );
        assert_eq!(m.left, (700 - 342) / 2, "a capped grid centres");
    }
}

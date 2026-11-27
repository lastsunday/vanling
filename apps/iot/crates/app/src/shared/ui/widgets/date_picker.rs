//! Date pickers: a month grid when the body can hold one, three wheels when it cannot.
//!
//! The parent asks [`kind`]; each kind shares the dialog frame in [`super::picker`].

use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect};

use super::wheel::{self, Labels, WheelState, WheelView};
use super::{calendar, picker};
use crate::shared::ui::theme::Theme;

pub const MIN_YEAR: u16 = 1970;
pub const MAX_YEAR: u16 = 2100;
pub const COLUMNS: u8 = 3;
pub const NAV_W: i16 = 44;
pub const HEADER_H: i16 = 32;

pub const YEAR_LABELS: Labels<'static> = Labels::Numbers {
    min: MIN_YEAR,
    max: MAX_YEAR,
    step: 1,
    pad: 4,
};
pub const MONTH_LABELS: Labels<'static> = Labels::Numbers {
    min: 1,
    max: 12,
    step: 1,
    pad: 2,
};
pub const DAY_LABELS: Labels<'static> = Labels::Numbers {
    min: 1,
    max: 31,
    step: 1,
    pad: 2,
};

pub fn labels(column: u8) -> Labels<'static> {
    match column {
        0 => YEAR_LABELS,
        1 => MONTH_LABELS,
        _ => DAY_LABELS,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Month,
    Wheels,
}

/// Which date dialog the overlay can show: the grid only when its body holds 28 px cells.
pub fn kind(overlay: Rect) -> Kind {
    if calendar::fits(picker::frame(overlay).body) {
        Kind::Month
    } else {
        Kind::Wheels
    }
}

fn year_count() -> u16 {
    MAX_YEAR - MIN_YEAR + 1
}

/// The three date wheels, kept together so the day range follows the month.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateWheels {
    year: WheelState,
    month: WheelState,
    day: WheelState,
}

impl DateWheels {
    pub fn new(year: u16, month: u8, day: u8) -> Self {
        let year = year.clamp(MIN_YEAR, MAX_YEAR);
        let month = month.clamp(1, 12);
        let days = calendar::days_in_month(year, month).max(1);
        Self {
            year: WheelState::new(year - MIN_YEAR, year_count(), false),
            month: WheelState::new(u16::from(month - 1), 12, true),
            day: WheelState::new(u16::from(day.clamp(1, days) - 1), u16::from(days), true),
        }
    }

    pub fn date(self) -> (u16, u8, u8) {
        (
            MIN_YEAR + self.year.index(),
            self.month.index() as u8 + 1,
            self.day.index() as u8 + 1,
        )
    }

    pub fn views(self) -> [WheelView; COLUMNS as usize] {
        [self.year.view(), self.month.view(), self.day.view()]
    }

    pub fn moving(self) -> bool {
        self.year.moving() || self.month.moving() || self.day.moving()
    }

    pub fn tick(&mut self, step_ms: u16) {
        self.year.tick(step_ms);
        self.month.tick(step_ms);
        self.day.tick(step_ms);
        self.sync();
    }

    pub fn press(&mut self, column: u8) {
        self.wheel_mut(column).press();
    }

    pub fn drag(&mut self, column: u8, dy: i16, row_h: i16) {
        self.wheel_mut(column).drag(dy, row_h);
        self.sync();
    }

    pub fn release(&mut self, column: u8, speed: i16, row_h: i16) {
        self.wheel_mut(column).release(speed, row_h);
        self.sync();
    }

    pub fn tap_row(&mut self, column: u8, offset: i32) {
        self.wheel_mut(column).tap_row(offset);
        self.sync();
    }

    /// A shorter month pulls the day back into it.
    pub fn sync(&mut self) {
        let (year, month, _) = self.date();
        let days = calendar::days_in_month(year, month).max(1);
        self.day.clamp_count(u16::from(days));
    }

    fn wheel_mut(&mut self, column: u8) -> &mut WheelState {
        match column {
            0 => &mut self.year,
            1 => &mut self.month,
            _ => &mut self.day,
        }
    }
}

pub fn column_rect(rect: Rect, column: u8) -> Rect {
    wheel::column_rect(rect, COLUMNS, column)
}

pub fn column_at(rect: Rect, x: i32, y: i32) -> Option<u8> {
    wheel::column_at(rect, COLUMNS, x, y)
}

pub fn draw_wheels(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    wheels: [WheelView; COLUMNS as usize],
    theme: &Theme,
) {
    for column in 0..COLUMNS {
        draw_column(canvas, rect, wheels, column, theme);
    }
    canvas.set_color(None);
}

/// One column and the separator at its right edge: everything inside that column's rectangle,
/// so a frame can repaint the column alone.
pub fn draw_column(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    wheels: [WheelView; COLUMNS as usize],
    column: u8,
    theme: &Theme,
) {
    let column = column.min(COLUMNS - 1);
    let col = column_rect(rect, column);
    wheel::draw(
        canvas,
        col,
        wheels[usize::from(column)],
        labels(column),
        theme,
    );
    if column + 1 < COLUMNS {
        wheel::separator(canvas, col, theme);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonthView {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonthLayout {
    pub header: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub grid: Rect,
}

pub fn month_layout(body: Rect) -> MonthLayout {
    let header_h = HEADER_H.min((body.h / 3).max(1) as i16);
    let nav_w = NAV_W.min((body.w / 3).max(1) as i16);
    let top = body.y + i32::from(header_h);
    MonthLayout {
        header: Rect::new(body.x, body.y, body.w, i32::from(header_h)),
        prev: Rect::new(body.x, body.y, i32::from(nav_w), i32::from(header_h)),
        next: Rect::new(
            body.right() - i32::from(nav_w),
            body.y,
            i32::from(nav_w),
            i32::from(header_h),
        ),
        grid: Rect::new(body.x, top, body.w, body.h - i32::from(header_h)),
    }
}

pub fn draw_month(canvas: &mut Canvas<'_>, body: Rect, view: MonthView, theme: &Theme) {
    let layout = month_layout(body);
    let mut buf = [0u8; 6];
    let year = format_u16(view.year, &mut buf);
    let mut text = [0u8; 9];
    let len = year.len().min(4);
    text[..len].copy_from_slice(&year[..len]);
    text[len] = b'-';
    text[len + 1] = b'0' + view.month / 10;
    text[len + 2] = b'0' + view.month % 10;
    let end = len + 3;
    let width = canvas.measure(&text[..end]) as i32;
    canvas.set_color(Some(theme.foreground));
    canvas.draw_text(
        &text[..end],
        (layout.header.x + (layout.header.w - width) / 2).max(0) as usize,
        (layout.header.y + (layout.header.h - 12) / 2).max(0) as usize,
    );
    for (rect, glyph) in [
        (layout.prev, b"<".as_slice()),
        (layout.next, b">".as_slice()),
    ] {
        let width = canvas.measure(glyph) as i32;
        canvas.set_color(Some(theme.muted));
        canvas.draw_text(
            glyph,
            (rect.x + (rect.w - width) / 2).max(0) as usize,
            (rect.y + (rect.h - 12) / 2).max(0) as usize,
        );
    }
    calendar::draw(
        canvas,
        layout.grid,
        view.year,
        view.month,
        Some(view.day),
        theme,
    );
    canvas.set_color(None);
}

pub fn nav_at(body: Rect, x: i32, y: i32) -> Option<i8> {
    let layout = month_layout(body);
    if layout.prev.contains(x, y) {
        Some(-1)
    } else if layout.next.contains(x, y) {
        Some(1)
    } else {
        None
    }
}

pub fn day_at(body: Rect, view: MonthView, x: i32, y: i32) -> Option<u8> {
    calendar::day_at(month_layout(body).grid, view.year, view.month, x, y)
}

pub fn shift_month(view: &mut MonthView, delta: i32) {
    let mut month = i32::from(view.month) + delta;
    let mut year = i32::from(view.year);
    while month < 1 {
        month += 12;
        year -= 1;
    }
    while month > 12 {
        month -= 12;
        year += 1;
    }
    view.year = year.clamp(i32::from(MIN_YEAR), i32::from(MAX_YEAR)) as u16;
    view.month = month as u8;
    view.day = view
        .day
        .min(calendar::days_in_month(view.year, view.month).max(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Rect = Rect::new(0, 0, 320, 228);
    const MEDIUM: Rect = Rect::new(0, 0, 480, 320);
    const LARGE: Rect = Rect::new(0, 0, 800, 600);
    const SMALL: Rect = Rect::new(0, 0, 240, 180);

    fn body(overlay: Rect) -> Rect {
        picker::frame(overlay).body
    }

    #[test]
    fn a_short_body_takes_wheels_and_a_tall_one_takes_the_grid() {
        assert_eq!(kind(PANEL), Kind::Wheels);
        assert_eq!(kind(SMALL), Kind::Wheels);
        assert_eq!(kind(MEDIUM), Kind::Month);
        assert_eq!(kind(LARGE), Kind::Month);
    }

    #[test]
    fn the_wheels_round_trip_a_date() {
        let wheels = DateWheels::new(2026, 11, 26);
        assert_eq!(wheels.date(), (2026, 11, 26));
        let views = wheels.views();
        assert_eq!(views[0].count, MAX_YEAR - MIN_YEAR + 1);
        assert_eq!(views[1].count, 12);
        assert_eq!(views[2].count, 30);
    }

    #[test]
    fn a_shorter_month_pulls_the_day_back() {
        let mut wheels = DateWheels::new(2026, 1, 31);
        assert_eq!(wheels.date(), (2026, 1, 31));
        wheels.month.set_index(1);
        wheels.sync();
        assert_eq!(wheels.date(), (2026, 2, 28));
        wheels.month.set_index(8);
        wheels.year.set_index(2024 - MIN_YEAR);
        wheels.sync();
        assert_eq!(wheels.date(), (2024, 9, 28));
    }

    #[test]
    fn a_tap_row_moves_one_column_only() {
        let mut wheels = DateWheels::new(2026, 11, 26);
        wheels.tap_row(0, -1);
        assert_eq!(wheels.date(), (2025, 11, 26));
        wheels.tap_row(1, 1);
        assert_eq!(wheels.date(), (2025, 12, 26));
        wheels.tap_row(2, 4);
        assert_eq!(wheels.date(), (2025, 12, 30));
    }

    #[test]
    fn the_month_view_picks_days_and_turns_months() {
        let body = body(MEDIUM);
        let mut view = MonthView {
            year: 2026,
            month: 11,
            day: 26,
        };
        let layout = month_layout(body);
        let index =
            u8::try_from(i32::from(calendar::first_weekday(view.year, view.month)) + 14).unwrap();
        let cell = calendar::cell_rect(layout.grid, index);
        assert_eq!(
            day_at(body, view, cell.x + cell.w / 2, cell.y + cell.h / 2),
            Some(15)
        );
        assert_eq!(nav_at(body, layout.prev.x + 2, layout.prev.y + 2), Some(-1));
        assert_eq!(nav_at(body, layout.next.x + 2, layout.next.y + 2), Some(1));
        shift_month(&mut view, -1);
        assert_eq!((view.year, view.month), (2026, 10));
        view.day = 31;
        shift_month(&mut view, 1);
        assert_eq!((view.year, view.month, view.day), (2026, 11, 30));
    }

    #[test]
    fn a_column_point_finds_its_wheel() {
        let body = body(PANEL);
        assert_eq!(column_at(body, body.x + 2, body.y + 2), Some(0));
        assert_eq!(column_at(body, body.x + body.w / 2, body.y + 2), Some(1));
        assert_eq!(column_at(body, body.right() - 2, body.y + 2), Some(2));
    }
}

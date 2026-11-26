//! Two or three wheel columns: hour, minute, and optionally second.
//!
//! Every column is a [`wheel`] slice of the same rect, so a screen that changes size only
//! changes the rect the dialog hands in.

use iot_core::ui::{Canvas, Rect};

use super::wheel::{self, Labels, WheelState, WheelView};
use crate::shared::ui::theme::Theme;

pub const MAX_COLUMNS: u8 = 3;

pub const HOUR_LABELS: Labels<'static> = Labels::Numbers {
    min: 0,
    max: 23,
    step: 1,
    pad: 2,
};
pub const MINUTE_LABELS: Labels<'static> = Labels::Numbers {
    min: 0,
    max: 59,
    step: 1,
    pad: 2,
};
pub const SECOND_LABELS: Labels<'static> = MINUTE_LABELS;

pub fn labels(column: u8) -> Labels<'static> {
    match column {
        0 => HOUR_LABELS,
        1 => MINUTE_LABELS,
        _ => SECOND_LABELS,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeView {
    pub wheels: [WheelView; MAX_COLUMNS as usize],
    pub columns: u8,
}

impl Default for TimeView {
    fn default() -> Self {
        Self {
            wheels: [WheelView::default(); MAX_COLUMNS as usize],
            columns: 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeState {
    hour: WheelState,
    minute: WheelState,
    second: WheelState,
    columns: u8,
}

impl TimeState {
    pub fn new(hour: u8, minute: u8, second: u8, columns: u8) -> Self {
        Self {
            hour: WheelState::new(u16::from(hour.min(23)), 24, true),
            minute: WheelState::new(u16::from(minute.min(59)), 60, true),
            second: WheelState::new(u16::from(second.min(59)), 60, true),
            columns: columns.clamp(2, MAX_COLUMNS),
        }
    }

    pub fn value(self) -> (u8, u8, u8) {
        (
            self.hour.index() as u8,
            self.minute.index() as u8,
            self.second.index() as u8,
        )
    }

    pub fn view(self) -> TimeView {
        TimeView {
            wheels: [self.hour.view(), self.minute.view(), self.second.view()],
            columns: self.columns,
        }
    }

    pub fn columns(self) -> u8 {
        self.columns
    }

    pub fn moving(self) -> bool {
        self.hour.moving() || self.minute.moving() || self.second.moving()
    }

    pub fn tick(&mut self, step_ms: u16) {
        self.hour.tick(step_ms);
        self.minute.tick(step_ms);
        self.second.tick(step_ms);
    }

    pub fn press(&mut self, column: u8) {
        self.wheel_mut(column).press();
    }

    pub fn drag(&mut self, column: u8, dy: i16, row_h: i16) {
        self.wheel_mut(column).drag(dy, row_h);
    }

    pub fn release(&mut self, column: u8, speed: i16, row_h: i16) {
        self.wheel_mut(column).release(speed, row_h);
    }

    pub fn tap_row(&mut self, column: u8, offset: i32) {
        self.wheel_mut(column).tap_row(offset);
    }

    fn wheel_mut(&mut self, column: u8) -> &mut WheelState {
        match column {
            0 => &mut self.hour,
            1 => &mut self.minute,
            _ => &mut self.second,
        }
    }
}

pub fn column_rect(rect: Rect, columns: u8, column: u8) -> Rect {
    wheel::column_rect(rect, columns, column)
}

pub fn column_at(rect: Rect, columns: u8, x: i32, y: i32) -> Option<u8> {
    wheel::column_at(rect, columns, x, y)
}

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, view: TimeView, theme: &Theme) {
    let columns = view.columns.clamp(1, MAX_COLUMNS);
    for column in 0..columns {
        let col = column_rect(rect, columns, column);
        wheel::draw(
            canvas,
            col,
            view.wheels[usize::from(column)],
            labels(column),
            theme,
        );
        if column > 0 {
            let m = wheel::metrics(col);
            let width = canvas.measure(b":") as i32;
            canvas.set_color(Some(theme.muted));
            canvas.draw_text(
                b":",
                (col.x - width / 2).max(0) as usize,
                (m.centre + (i32::from(m.row_h) - 12) / 2).max(0) as usize,
            );
        }
    }
    canvas.set_color(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Rect = Rect::new(10, 40, 300, 140);

    #[test]
    fn a_time_round_trips_and_wraps_its_ends() {
        let mut time = TimeState::new(9, 5, 30, 3);
        assert_eq!(time.value(), (9, 5, 30));
        time.tap_row(0, -9);
        assert_eq!(time.value(), (0, 5, 30));
        time.tap_row(0, -1);
        assert_eq!(time.value(), (23, 5, 30), "midnight wraps backwards");
        time.tap_row(1, 1);
        assert_eq!(time.value(), (23, 6, 30));
        time.tap_row(2, 30);
        assert_eq!(time.value(), (23, 6, 0), "seconds wrap at sixty");
    }

    #[test]
    fn the_seconds_column_is_optional() {
        let two = TimeState::new(9, 5, 30, 2);
        assert_eq!(two.columns(), 2);
        let three = TimeState::new(9, 5, 30, 3);
        assert_eq!(three.columns(), 3);
        assert_eq!(three.view().wheels[2].count, 60);
    }

    #[test]
    fn each_column_owns_a_third_of_the_body() {
        assert_eq!(column_at(BODY, 3, BODY.x + 2, BODY.y + 2), Some(0));
        assert_eq!(column_at(BODY, 3, BODY.x + 150, BODY.y + 2), Some(1));
        assert_eq!(column_at(BODY, 3, BODY.right() - 2, BODY.y + 2), Some(2));
        assert_eq!(column_at(BODY, 3, BODY.right(), BODY.y + 2), None);
    }

    #[test]
    fn a_number_column_pads_to_two_cells() {
        let mut buf = [0u8; 16];
        assert_eq!(HOUR_LABELS.write(5, &mut buf), b"05");
        assert_eq!(MINUTE_LABELS.write(59, &mut buf), b"59");
    }
}

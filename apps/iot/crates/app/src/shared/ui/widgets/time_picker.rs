//! A three-row time wheel: the centre value between two guide rules, its neighbours above and
//! below. Every row is a whole touch target; drawing and hit testing share the pitch.

use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub const ROW_H: i16 = 44;
pub const VISIBLE: usize = 3;
pub const COL_W: i16 = 56;
pub const HEIGHT: i16 = ROW_H * VISIBLE as i16;

/// One wheel's resting value and the last value it may show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wheel {
    pub value: u8,
    pub max: u8,
}

/// The value a panel point picks: the row above, the centre, or the row below.
pub fn row_at(rect: Rect, wheel: Wheel, x: i32, y: i32) -> Option<u8> {
    if !rect.contains(x, y) {
        return None;
    }
    let row = (y - rect.y) / i32::from(ROW_H);
    let value = i32::from(wheel.value) + row - 1;
    (0..=i32::from(wheel.max))
        .contains(&value)
        .then_some(value as u8)
}

/// The wheel turned `rows` steps, clamped to its range.
pub fn step(wheel: Wheel, rows: i32) -> Wheel {
    let moved = i32::from(wheel.value) + rows;
    Wheel {
        value: moved.clamp(0, i32::from(wheel.max)) as u8,
        max: wheel.max,
    }
}

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, wheel: Wheel, theme: &Theme) {
    let step = i32::from(ROW_H);
    canvas.push_clip(rect);
    for row in 0..VISIBLE as i32 {
        let value = i32::from(wheel.value) + row - 1;
        if value < 0 || value > i32::from(wheel.max) {
            continue;
        }
        let top = rect.y + row * step;
        let mut buf = [0u8; 6];
        let text = format_u16(value as u16, &mut buf);
        let width = canvas.measure(text) as i32;
        canvas.set_color(Some(if row == 1 { theme.accent } else { theme.muted }));
        canvas.draw_text(
            text,
            (rect.x + (rect.w - width) / 2).max(0) as usize,
            (top + (step - 12) / 2).max(0) as usize,
        );
    }
    canvas.set_color(Some(theme.muted));
    for edge in [rect.y + step, rect.y + 2 * step] {
        canvas.draw_line(
            rect.x as isize,
            edge as isize,
            rect.right() as isize,
            edge as isize,
        );
    }
    canvas.pop_clip();
    canvas.set_color(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(0, 0, COL_W as i32, HEIGHT as i32);

    fn wheel(value: u8, max: u8) -> Wheel {
        Wheel { value, max }
    }

    #[test]
    fn a_tap_picks_the_row_it_lands_on() {
        for (row, value) in [(0, 4), (1, 5), (2, 6)] {
            let y = BOX.y + row * i32::from(ROW_H) + 4;
            assert_eq!(row_at(BOX, wheel(5, 23), 4, y), Some(value as u8));
        }
        assert_eq!(row_at(BOX, wheel(0, 23), 4, BOX.y + 4), None, "above zero");
        assert_eq!(
            row_at(BOX, wheel(23, 23), 4, BOX.bottom() - 4),
            None,
            "past the last hour"
        );
    }

    #[test]
    fn the_ends_stop_rather_than_wrap() {
        assert_eq!(step(wheel(23, 23), 1), wheel(23, 23));
        assert_eq!(step(wheel(0, 23), -1), wheel(0, 23));
        assert_eq!(step(wheel(5, 23), 1), wheel(6, 23));
    }

    #[test]
    fn three_rows_are_three_touch_targets_tall() {
        assert_eq!(HEIGHT, 132);
        assert!(i32::from(ROW_H) >= 44);
    }
}

//! Carousel dots: one filled at `index`, the rest hollow. Drawing and hit testing share the
//! same pitch, so a finger lands on the dot it sees.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

/// Radius of one dot, the gap between dots, and the pitch they sit on.
pub const DOT: isize = 3;
pub const GAP: i32 = 4;
pub const PITCH: i32 = 2 * DOT as i32 + GAP;

/// How many dots a strip holds at most, so a finger's cell stays as wide as [`TOUCH_MIN`].
pub fn fit(rect: Rect) -> u8 {
    u8::try_from((rect.w / TOUCH_MIN as i32).max(1)).unwrap_or(u8::MAX)
}

/// The dot a panel point picks: within half a touch target of a dot's centre, the nearest dot.
/// Targets may overlap on a short strip, so "nearest" is the whole rule.
pub fn dot_at(rect: Rect, count: u8, x: i32, y: i32) -> Option<u8> {
    if !rect.contains(x, y) || count == 0 {
        return None;
    }
    let count = i32::from(count);
    let span = (count - 1) * PITCH;
    let left = rect.x + (rect.w - span) / 2;
    let nearest = ((x - left + PITCH / 2) / PITCH).clamp(0, count - 1);
    let centre = left + nearest * PITCH;
    ((x - centre).abs() <= TOUCH_MIN as i32 / 2).then_some(nearest as u8)
}

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, count: u8, index: u8, theme: &Theme) {
    let count = count.max(1) as i32;
    let span = (count - 1) * PITCH;
    let left = rect.x + (rect.w - span) / 2;
    let middle = rect.y + rect.h / 2;
    let muted = theme.muted;
    let accent = theme.accent;
    for step in 0..count {
        let cx = left + step * PITCH;
        if step == i32::from(index).min(count - 1) {
            canvas.set_color(Some(accent));
            canvas.fill_round_rect(
                Rect::new(
                    cx - DOT as i32,
                    middle - DOT as i32,
                    2 * DOT as i32 + 1,
                    2 * DOT as i32 + 1,
                ),
                DOT as u8,
            );
        } else {
            canvas.set_color(Some(muted));
            canvas.draw_circle(cx as isize, middle as isize, DOT);
        }
    }
    canvas.set_color(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: Rect = Rect::new(0, 100, 300, TOUCH_MIN as i32);

    #[test]
    fn a_dot_takes_a_touch_wide_target_around_its_centre() {
        let half = TOUCH_MIN as i32 / 2;
        let span = 2 * PITCH;
        let left = STRIP.x + (STRIP.w - span) / 2;
        for step in 0..3 {
            let cx = left + step * PITCH;
            assert_eq!(dot_at(STRIP, 3, cx, 110), Some(step as u8));
        }
        assert_eq!(
            dot_at(STRIP, 3, left - half, 110),
            Some(0),
            "the left edge still belongs to the first dot"
        );
        assert_eq!(
            dot_at(STRIP, 3, left + span + half, 110),
            Some(2),
            "the right edge still belongs to the last dot"
        );
        assert_eq!(
            dot_at(STRIP, 3, STRIP.x + 10, 110),
            None,
            "far from any dot"
        );
        assert_eq!(dot_at(STRIP, 3, left, 99), None, "outside the strip");
    }

    #[test]
    fn a_narrow_strip_offers_fewer_dots_than_a_wide_one() {
        assert_eq!(fit(Rect::new(0, 0, 300, 44)), 6);
        assert_eq!(fit(Rect::new(0, 0, 30, 44)), 1);
    }
}

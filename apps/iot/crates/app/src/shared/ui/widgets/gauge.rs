//! A half-dial: an arc for the range, the lit part for the value, a needle on top.
//!
//! The arc is a table of unit points rather than trigonometry: the core keeps no float libm.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

/// Unit semicircle, 180° to 0° in 22.5° steps, scaled by 1000.
const ARC: [(i32, i32); 9] = [
    (-1000, 0),
    (-924, 383),
    (-707, 707),
    (-383, 924),
    (0, 1000),
    (383, 924),
    (707, 707),
    (924, 383),
    (1000, 0),
];

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, value: u8, theme: &Theme) {
    let radius = (rect.w / 2).min(rect.h - 4).max(4);
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h - 2;
    let point = |step: i32| {
        (
            cx + ARC[step as usize].0 * radius / 1000,
            cy - ARC[step as usize].1 * radius / 1000,
        )
    };
    let lit = i32::from(value.min(100)) * 8 / 100;
    for step in 0..8 {
        let (x0, y0) = point(step);
        let (x1, y1) = point(step + 1);
        canvas.set_color(Some(if step < lit {
            theme.accent
        } else {
            theme.muted
        }));
        canvas.draw_line(x0 as isize, y0 as isize, x1 as isize, y1 as isize);
    }
    let (x, y) = point(lit);
    canvas.set_color(Some(theme.ink));
    canvas.draw_line(cx as isize, cy as isize, x as isize, y as isize);
    canvas.set_color(None);
}

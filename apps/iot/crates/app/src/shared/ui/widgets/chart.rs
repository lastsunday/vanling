//! Bars over a baseline: the smallest honest chart, one pixel per bar per value.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, values: &[u8], theme: &Theme) {
    if values.is_empty() || rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let base = rect.bottom() - 1;
    let count = i32::try_from(values.len()).unwrap_or(1).max(1);
    let slot = rect.w / count;
    let bar = (slot - 2).max(1);
    canvas.set_color(Some(theme.muted));
    canvas.draw_line(
        rect.x as isize,
        base as isize,
        rect.right() as isize,
        base as isize,
    );
    for (index, value) in values.iter().enumerate() {
        let height = (rect.h - 4) * i32::from(*value).min(100) / 100;
        if height <= 0 {
            continue;
        }
        let left = rect.x + i32::try_from(index).unwrap_or(0) * slot + 1;
        canvas.set_color(Some(if index % 2 == 0 {
            theme.accent
        } else {
            theme.accent_pressed
        }));
        canvas.fill_rect(Rect::new(left, base - height, bar, height));
    }
    canvas.set_color(None);
}

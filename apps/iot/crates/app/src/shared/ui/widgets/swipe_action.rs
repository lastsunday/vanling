//! One row that slides left to uncover its action; the page owns how far it slid.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

/// How wide the action band can be uncovered, in px.
pub const REVEAL_MAX: i16 = 72;

#[derive(Clone, Copy, Debug)]
pub struct SwipeAction<'a> {
    pub title: &'a [u8],
    pub detail: &'a [u8],
    pub mark: char,
    pub selected: bool,
}

impl SwipeAction<'_> {
    pub fn draw(
        self,
        canvas: &mut Canvas<'_>,
        rect: Rect,
        reveal: i16,
        action: &[u8],
        theme: &Theme,
    ) {
        let reveal = reveal.clamp(0, REVEAL_MAX).min(rect.w.max(0) as i16);
        let shift = i32::from(reveal);
        if reveal > 0 {
            let band = Rect::new(rect.right() - shift, rect.y, shift, rect.h);
            canvas.set_color(Some(theme.danger));
            canvas.fill_rect(band);
            canvas.set_color(Some(theme.on_accent));
            let width = canvas.measure(action) as i32;
            canvas.draw_text(
                action,
                (band.x + (band.w - width) / 2).max(0) as usize,
                (band.y + (band.h - 12) / 2).max(0) as usize,
            );
        }
        canvas.push_clip(Rect::new(rect.x, rect.y, rect.w - shift, rect.h));
        if self.selected {
            canvas.set_color(Some(theme.selection));
            canvas.fill_rect(rect);
        }
        let left = rect.x - shift + 6;
        canvas.set_color(Some(theme.accent));
        canvas.draw_char(self.mark, left as isize, (rect.y + 10).max(0) as usize);
        canvas.set_color(Some(theme.foreground));
        canvas.draw_text(
            self.title,
            (left + 12).max(0) as usize,
            rect.y.max(0) as usize,
        );
        canvas.set_color(Some(theme.muted));
        canvas.draw_text(
            self.detail,
            (left + 12).max(0) as usize,
            (rect.y + 14).max(0) as usize,
        );
        canvas.draw_line(
            rect.x as isize,
            rect.bottom() as isize,
            rect.right() as isize,
            rect.bottom() as isize,
        );
        canvas.pop_clip();
        canvas.set_color(None);
    }
}

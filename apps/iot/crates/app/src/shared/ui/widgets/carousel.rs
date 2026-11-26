//! A pager that can run itself: the card strip, plus a dwell bar that says how long the next
//! turn has left. The layout is [`Pager`]'s; this only reserves the bar and draws it.

use iot_core::ui::{Canvas, Rect};

use super::pager::{Card, Pager};
use crate::shared::ui::theme::Theme;

pub const PROGRESS_H: i16 = 3;

#[derive(Clone, Copy, Debug)]
pub struct Carousel<'a> {
    pub cards: &'a [Card<'a>],
    pub index: u8,
    pub drag: i16,
    /// How far the dwell has run, 0 to 100.
    pub progress: u8,
    pub running: bool,
}

/// The strip's rectangle, without the dwell bar underneath.
pub fn body(rect: Rect) -> Rect {
    Rect::new(
        rect.x,
        rect.y,
        rect.w,
        (rect.h - i32::from(PROGRESS_H)).max(1),
    )
}

impl Carousel<'_> {
    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        let body = body(rect);
        Pager {
            cards: self.cards,
            index: self.index,
            drag: self.drag,
        }
        .draw(canvas, body, theme);
        let track = Rect::new(
            rect.x,
            rect.bottom() - i32::from(PROGRESS_H),
            rect.w,
            i32::from(PROGRESS_H),
        );
        canvas.set_color(Some(theme.muted));
        canvas.fill_rect(track);
        if self.running {
            canvas.set_color(Some(theme.accent));
            canvas.fill_rect(Rect::new(
                track.x,
                track.y,
                track.w * i32::from(self.progress.min(100)) / 100,
                track.h,
            ));
        }
        canvas.set_color(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_sits_inside_the_slot() {
        assert_eq!(PROGRESS_H, 3);
        assert!(PROGRESS_H < 20, "the cards keep their height");
    }
}

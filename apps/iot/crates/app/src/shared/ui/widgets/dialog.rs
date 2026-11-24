//! A modal question on a scrim: title, one line of body, two answers.
//!
//! Drawing and hit testing share [`card`], so the button a finger lands on is the one drawn.

use iot_core::ui::panel::{LINE, MARGIN, TOUCH_MIN};
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

const CARD_W: i32 = 240;
const ACTION_H: i32 = TOUCH_MIN as i32;
const TITLE_TOP: i32 = LINE as i32;
const BODY_TOP: i32 = TITLE_TOP + LINE as i32 + MARGIN as i32;
const CARD_H: i32 = ACTION_H + BODY_TOP + LINE as i32 + MARGIN as i32;

#[derive(Clone, Copy, Debug)]
pub struct Dialog<'a> {
    pub title: &'a [u8],
    pub body: &'a [u8],
    pub ok: &'a [u8],
    pub cancel: &'a [u8],
}

/// The card's rectangle inside `rect`, clamped to what the panel has.
pub fn card(rect: Rect) -> Rect {
    let w = CARD_W.min(rect.w);
    let h = CARD_H.min(rect.h);
    Rect::new(rect.x + (rect.w - w) / 2, rect.y + (rect.h - h) / 2, w, h)
}

impl Dialog<'_> {
    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        canvas.set_color(Some(theme.overlay));
        canvas.fill_rect(rect);
        let card = card(rect);
        canvas.set_color(Some(theme.ground));
        canvas.fill_round_rect(card, 4);
        canvas.set_color(Some(theme.muted));
        canvas.stroke_round_rect(card, 4);
        let width = canvas.measure(self.title) as i32;
        canvas.set_color(Some(theme.ink));
        canvas.draw_text(
            self.title,
            (card.x + (card.w - width) / 2).max(0) as usize,
            (card.y + TITLE_TOP).max(0) as usize,
        );
        let width = canvas.measure(self.body) as i32;
        canvas.set_color(Some(theme.muted));
        canvas.draw_text(
            self.body,
            (card.x + (card.w - width) / 2).max(0) as usize,
            (card.y + BODY_TOP).max(0) as usize,
        );
        canvas.set_color(Some(theme.muted));
        canvas.draw_line(
            card.x as isize,
            (card.bottom() - ACTION_H) as isize,
            card.right() as isize,
            (card.bottom() - ACTION_H) as isize,
        );
        let half = card.w / 2;
        self.label(
            canvas,
            Rect::new(card.x, card.bottom() - ACTION_H, half, ACTION_H),
            self.cancel,
            theme,
            false,
        );
        self.label(
            canvas,
            Rect::new(
                card.x + half,
                card.bottom() - ACTION_H,
                card.w - half,
                ACTION_H,
            ),
            self.ok,
            theme,
            true,
        );
        canvas.set_color(None);
    }

    fn label(&self, canvas: &mut Canvas<'_>, rect: Rect, run: &[u8], theme: &Theme, accent: bool) {
        let width = canvas.measure(run) as i32;
        canvas.set_color(Some(if accent { theme.accent } else { theme.muted }));
        canvas.draw_text(
            run,
            (rect.x + (rect.w - width) / 2).max(0) as usize,
            (rect.y + (rect.h - LINE as i32) / 2).max(0) as usize,
        );
    }
}

/// The answer a point picks: `Some(true)` for OK, `Some(false)` for cancel, `None` elsewhere.
pub fn action_at(rect: Rect, x: i32, y: i32) -> Option<bool> {
    let card = card(rect);
    let row = Rect::new(card.x, card.bottom() - ACTION_H, card.w, ACTION_H);
    if !row.contains(x, y) {
        return None;
    }
    Some(x >= card.x + card.w / 2)
}

//! A short list of verbs that hangs under its anchor; the page decides where and why.

use iot_core::ui::panel::{LINE, TOUCH_MIN};
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::{CORNER, Theme};

const WIDTH: i32 = 200;
const ROW_H: i32 = TOUCH_MIN as i32;
const PAD: i32 = 3;
const ROWS: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct PopupMenu<'a> {
    pub items: [&'a [u8]; ROWS],
    pub selected: u8,
}

/// The card's rectangle, hung from `anchor` and clamped inside `rect`.
pub fn card(rect: Rect, anchor: i16) -> Rect {
    let w = WIDTH.min(rect.w - 2 * PAD);
    let h = (ROW_H * ROWS as i32 + 2 * PAD).min(rect.h);
    let top = i32::from(anchor).clamp(rect.y, rect.bottom() - h);
    Rect::new(rect.x + (rect.w - w) / 2, top, w, h)
}

/// The item a point picks, or `None` on the frame or the padding.
pub fn item_at(card: Rect, x: i32, y: i32) -> Option<u8> {
    let inner = Rect::new(
        card.x + PAD,
        card.y + PAD,
        card.w - 2 * PAD,
        card.h - 2 * PAD,
    );
    if !inner.contains(x, y) {
        return None;
    }
    u8::try_from((y - inner.y) / ROW_H)
        .ok()
        .filter(|row| usize::from(*row) < ROWS)
}

impl PopupMenu<'_> {
    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, anchor: i16, theme: &Theme) {
        canvas.set_color(Some(theme.ground));
        canvas.fill_rect(rect);
        let card = card(rect, anchor);
        canvas.set_color(Some(theme.surface));
        canvas.fill_round_rect(card, CORNER);
        canvas.set_color(Some(theme.muted));
        canvas.stroke_round_rect(card, CORNER);
        for (index, item) in self.items.iter().enumerate() {
            let row = Rect::new(
                card.x + PAD,
                card.y + PAD + index as i32 * ROW_H,
                card.w - 2 * PAD,
                ROW_H,
            );
            if index == usize::from(self.selected) {
                canvas.set_color(Some(theme.selection));
                canvas.fill_rect(row);
            }
            canvas.set_color(Some(theme.ink));
            canvas.push_clip(row);
            canvas.draw_text(
                item,
                (row.x + PAD).max(0) as usize,
                (row.y + (ROW_H - LINE as i32) / 2).max(0) as usize,
            );
            canvas.pop_clip();
        }
        canvas.set_color(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Rect = Rect::new(0, 0, 320, 228);

    #[test]
    fn the_card_hangs_from_the_anchor_and_every_row_reads_back() {
        let card = card(PANEL, 40);
        assert_eq!(card.y, 40);
        for row in 0..ROWS as u8 {
            let y = card.y + PAD + i32::from(row) * ROW_H + ROW_H / 2;
            assert_eq!(item_at(card, card.x + card.w / 2, y), Some(row));
        }
        assert_eq!(item_at(card, card.x + 1, card.y + 1), None);
    }

    #[test]
    fn the_card_stays_inside_the_panel_whatever_the_anchor() {
        for anchor in [-40i16, 0, 100, 400] {
            let card = card(PANEL, anchor);
            assert!(card.y >= PANEL.y, "{anchor}");
            assert!(card.bottom() <= PANEL.bottom(), "{anchor}");
        }
    }
}

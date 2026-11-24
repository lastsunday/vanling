//! A horizontal card strip: one card centred, its neighbours peeking. The page owns the index
//! and the live drag; [`turn`] says which card a released drag picks.

use iot_core::ui::panel::{LINE, MARGIN};
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

/// One card's words and its one-character mark.
#[derive(Clone, Copy, Debug)]
pub struct Card<'a> {
    pub title: &'a [u8],
    pub detail: &'a [u8],
    pub mark: char,
}

#[derive(Clone, Copy, Debug)]
pub struct Pager<'a> {
    pub cards: &'a [Card<'a>],
    pub index: u8,
    /// Live horizontal travel in panel pixels, positive when the finger moved right.
    pub drag: i16,
}

/// Card width as a share of the strip: two thirds leaves the neighbours' edges visible.
pub fn card_width(rect: Rect) -> i32 {
    (rect.w * 2 / 3).max(1)
}

/// Distance between card origins: three quarters of the strip, so a neighbour peeks by a
/// quarter and the gap between cards stays legible.
pub fn pitch(rect: Rect) -> i32 {
    (rect.w * 3 / 4).max(1)
}

impl Pager<'_> {
    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
        let count = self.cards.len();
        if count == 0 || rect.w <= 0 || rect.h <= 0 {
            return;
        }
        let index = usize::from(self.index).min(count - 1) as i32;
        let card_w = card_width(rect);
        let card_h = (rect.h - 2 * MARGIN as i32).max(1);
        let step = pitch(rect);
        let top = rect.y + MARGIN as i32;
        let origin = rect.x + (rect.w - card_w) / 2;
        let middle = top + card_h / 2;
        canvas.push_clip(rect);
        for (step_index, card) in self.cards.iter().enumerate() {
            let left = origin + (step_index as i32 - index) * step + i32::from(self.drag);
            if left + card_w < rect.x || left > rect.right() {
                continue;
            }
            let card_rect = Rect::new(left, top, card_w, card_h);
            let focused = step_index as i32 == index;
            canvas.set_color(Some(theme.ground));
            canvas.fill_round_rect(card_rect, 6);
            canvas.set_color(Some(if focused { theme.accent } else { theme.muted }));
            canvas.stroke_round_rect(card_rect, 6);

            // One card is four pixels of air plus its own words: nothing it says may reach the
            // neighbour, whatever the language does to the length.
            canvas.push_clip(card_rect);
            canvas.set_color(Some(if focused { theme.ink } else { theme.muted }));
            canvas.draw_text(
                card.title,
                (card_rect.x + MARGIN as i32).max(0) as usize,
                (card_rect.y + 6).max(0) as usize,
            );
            let glyph = [card.mark as u8];
            let width = canvas.measure(&glyph) as i32;
            canvas.set_color(Some(if focused { theme.accent } else { theme.muted }));
            canvas.draw_text(
                &glyph,
                (card_rect.x + (card_rect.w - width) / 2).max(0) as usize,
                (middle - LINE as i32 / 2).max(0) as usize,
            );
            canvas.set_color(Some(theme.muted));
            canvas.draw_text(
                card.detail,
                (card_rect.x + MARGIN as i32).max(0) as usize,
                (card_rect.bottom() - LINE as i32 - 6).max(0) as usize,
            );
            canvas.pop_clip();
            if focused {
                canvas.set_color(Some(theme.accent));
                canvas.draw_line(
                    card_rect.x as isize,
                    card_rect.bottom() as isize,
                    card_rect.right() as isize,
                    card_rect.bottom() as isize,
                );
            }
        }
        canvas.pop_clip();
        canvas.set_color(None);
    }
}

/// The card a point picks at rest, `None` in the gap between two visible cards.
pub fn page_at(rect: Rect, index: u8, count: u8, x: i32, y: i32) -> Option<u8> {
    if count == 0 || !rect.contains(x, y) {
        return None;
    }
    let card_w = card_width(rect);
    let step = pitch(rect);
    let origin = rect.x + (rect.w - card_w) / 2;
    (0..count).find(|card| {
        let left = origin + (i32::from(*card) - i32::from(index)) * step;
        (left..left + card_w).contains(&x)
    })
}

/// The card a released drag picks: a quarter of the strip is a deliberate page turn.
pub fn turn(index: u8, count: u8, rect: Rect, dx: i16) -> u8 {
    if count == 0 {
        return 0;
    }
    let threshold = (rect.w / 4).max(1);
    let step = if i32::from(dx) <= -threshold {
        1
    } else if i32::from(dx) >= threshold {
        -1
    } else {
        0
    };
    (i32::from(index) + step).clamp(0, i32::from(count) - 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: Rect = Rect::new(0, 0, 300, 88);

    #[test]
    fn a_quarter_strip_turns_one_card_and_a_shorter_drag_snaps_back() {
        assert_eq!(turn(0, 3, STRIP, -80), 1);
        assert_eq!(turn(0, 3, STRIP, 80), 0, "the first card has nowhere to go");
        assert_eq!(turn(2, 3, STRIP, -80), 2, "and neither has the last");
        assert_eq!(turn(1, 3, STRIP, 74), 1, "below a quarter is a snap back");
        assert_eq!(turn(1, 3, STRIP, -75), 2);
    }

    #[test]
    fn a_point_picks_the_card_it_covers_and_the_gap_picks_none() {
        assert_eq!(page_at(STRIP, 1, 3, 60, 10), Some(1));
        assert_eq!(
            page_at(STRIP, 1, 3, 20, 10),
            Some(0),
            "the left peek is card 0"
        );
        assert_eq!(
            page_at(STRIP, 1, 3, 280, 10),
            Some(2),
            "the right peek is card 2"
        );
        assert_eq!(page_at(STRIP, 1, 3, 260, 10), None, "the gap is not a card");
        assert_eq!(page_at(STRIP, 1, 3, 60, 100), None, "outside the strip");
    }

    #[test]
    fn a_card_is_wide_enough_to_read_and_leaves_its_neighbours_peeking() {
        let card = card_width(STRIP);
        let step = pitch(STRIP);
        assert!(card >= STRIP.w / 2, "a card is {card} px on {STRIP:?}");
        assert!(step < STRIP.w, "the next card starts inside the strip");
        assert!(step > card, "cards sit beside each other, not on top");
    }
}

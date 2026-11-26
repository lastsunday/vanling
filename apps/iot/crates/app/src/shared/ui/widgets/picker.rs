//! The frame every modal picker shares: scrim, centred card, title, body, two answers.
//!
//! The card is capped rather than stretched, so a large screen keeps a readable dialog and a
//! small one keeps a usable body. Drawing and hit testing share [`frame`].

use iot_core::ui::{Canvas, Color, Rect};

use crate::shared::ui::theme::Theme;

pub const MARGIN: i32 = 8;
pub const MAX_W: i32 = 360;
pub const MAX_H: i32 = 480;
pub const TITLE_H: i32 = 24;
pub const MIN_ACTION_H: i32 = 32;
pub const MAX_ACTION_H: i32 = 44;
pub const PAD: i32 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub card: Rect,
    pub title: Rect,
    pub body: Rect,
    pub cancel: Rect,
    pub ok: Rect,
}

pub fn frame(rect: Rect) -> Frame {
    let w = (rect.w - 2 * MARGIN).clamp(0, MAX_W);
    let h = (rect.h - 2 * MARGIN).clamp(0, MAX_H);
    let card = Rect::new(rect.x + (rect.w - w) / 2, rect.y + (rect.h - h) / 2, w, h);
    let action_h = (h / 5).clamp(MIN_ACTION_H, MAX_ACTION_H).min(h);
    let title_h = TITLE_H.min(h - action_h);
    let half = card.w / 2;
    Frame {
        card,
        title: Rect::new(card.x, card.y, card.w, title_h),
        body: Rect::new(
            card.x + PAD,
            card.y + title_h,
            card.w - 2 * PAD,
            (h - title_h - action_h - PAD).max(0),
        ),
        cancel: Rect::new(card.x, card.bottom() - action_h, half, action_h),
        ok: Rect::new(
            card.x + half,
            card.bottom() - action_h,
            card.w - half,
            action_h,
        ),
    }
}

/// The card's corner radius, and the square each corner's arc is cut from.
const RADIUS: u8 = 4;

/// Dims the page around the card. The card is opaque and paints over its own pixels, so the
/// scrim is laid in the four bands around it plus the corner squares its arcs leave showing:
/// a frame that repaints only the card's body then costs no blend at all.
fn scrim(canvas: &mut Canvas<'_>, rect: Rect, card: Rect, color: Color) {
    canvas.set_color(Some(color));
    let r = i32::from(RADIUS);
    for band in [
        Rect::new(rect.x, rect.y, rect.w, card.y - rect.y),
        Rect::new(rect.x, card.bottom(), rect.w, rect.bottom() - card.bottom()),
        Rect::new(rect.x, card.y, card.x - rect.x, card.h),
        Rect::new(card.right(), card.y, rect.right() - card.right(), card.h),
        Rect::new(card.x, card.y, r, r),
        Rect::new(card.right() - r, card.y, r, r),
        Rect::new(card.x, card.bottom() - r, r, r),
        Rect::new(card.right() - r, card.bottom() - r, r, r),
    ] {
        canvas.fill_rect(band);
    }
}

/// `Some(true)` for OK, `Some(false)` for cancel or a tap on the scrim.
pub fn action_at(rect: Rect, x: i32, y: i32) -> Option<bool> {
    let frame = frame(rect);
    if frame.ok.contains(x, y) {
        Some(true)
    } else if frame.cancel.contains(x, y) {
        Some(false)
    } else if frame.card.contains(x, y) {
        None
    } else {
        Some(false)
    }
}

pub fn draw(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    title: &[u8],
    cancel: &[u8],
    ok: &[u8],
    theme: &Theme,
) {
    let frame = frame(rect);
    scrim(canvas, rect, frame.card, theme.overlay);
    canvas.set_color(Some(theme.surface));
    canvas.fill_round_rect(frame.card, RADIUS);
    canvas.set_color(Some(theme.muted));
    canvas.stroke_round_rect(frame.card, RADIUS);
    let width = canvas.measure(title) as i32;
    canvas.set_color(Some(theme.foreground));
    canvas.draw_text(
        title,
        (frame.title.x + (frame.title.w - width) / 2).max(0) as usize,
        (frame.title.y + (frame.title.h - 12) / 2).max(0) as usize,
    );
    canvas.set_color(Some(theme.muted));
    canvas.draw_line(
        frame.card.x as isize,
        frame.cancel.y as isize,
        frame.card.right() as isize,
        frame.cancel.y as isize,
    );
    label(canvas, frame.cancel, cancel, theme.muted);
    label(canvas, frame.ok, ok, theme.accent);
    canvas.set_color(None);
}

fn label(canvas: &mut Canvas<'_>, rect: Rect, run: &[u8], color: iot_core::ui::Color) {
    let width = canvas.measure(run) as i32;
    canvas.push_clip(rect);
    canvas.set_color(Some(color));
    canvas.draw_text(
        run,
        (rect.x + (rect.w - width) / 2).max(0) as usize,
        (rect.y + (rect.h - 12) / 2).max(0) as usize,
    );
    canvas.pop_clip();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [Rect; 4] = [
        Rect::new(0, 0, 320, 228),
        Rect::new(0, 0, 480, 320),
        Rect::new(0, 0, 800, 600),
        Rect::new(0, 0, 240, 180),
    ];

    #[test]
    fn the_card_stays_inside_every_screen_and_keeps_a_body() {
        for rect in SIZES {
            let f = frame(rect);
            assert!(rect.contains(f.card.x, f.card.y), "{rect:?}");
            assert!(f.card.right() <= rect.right() && f.card.bottom() <= rect.bottom());
            assert!(f.card.w <= MAX_W && f.card.h <= MAX_H, "{rect:?}");
            assert!(f.body.h > 0, "{rect:?}: {:?}", f.body);
            assert!(f.cancel.h >= MIN_ACTION_H, "{rect:?}");
        }
    }

    #[test]
    fn the_two_answers_split_the_action_band() {
        let f = frame(SIZES[0]);
        assert_eq!(f.cancel.w + f.ok.w, f.card.w);
        assert_eq!(f.cancel.bottom(), f.card.bottom());
        assert_eq!(f.ok.x, f.cancel.right());
    }

    #[test]
    fn every_answer_point_reaches_its_own_action() {
        for rect in SIZES {
            let f = frame(rect);
            assert_eq!(
                action_at(rect, f.ok.x + f.ok.w / 2, f.ok.y + f.ok.h / 2),
                Some(true)
            );
            assert_eq!(
                action_at(
                    rect,
                    f.cancel.x + f.cancel.w / 2,
                    f.cancel.y + f.cancel.h / 2
                ),
                Some(false)
            );
            assert_eq!(action_at(rect, 0, 0), Some(false), "the scrim cancels");
            assert_eq!(
                action_at(rect, f.body.x + 2, f.body.y + 2),
                None,
                "the body is not an answer"
            );
        }
    }
}

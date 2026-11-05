//! The confirmation box: two answers laid along its own bottom edge.

use iot_core::geometry::ControlRect;
use iot_core::overlay::{FONT_H, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, ROW_PITCH};

use super::TOUCH_MIN;
use super::list::LIST_TOP;
use super::toolbar::TOOLBAR_TOP;

/// Which of a confirmation's two answers a tap landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogChoice {
    Keep,
    Act,
}

/// The confirmation's box: a page-wide margin either side, centred on the rows the list
/// would have been showing.
pub const DIALOG_W: usize = 256;
pub const DIALOG_H: usize = 118;
pub const DIALOG_X: usize = (PANEL_COLUMNS - DIALOG_W) / 2;
pub const DIALOG_TOP: usize = LIST_TOP + (TOOLBAR_TOP - LIST_TOP - DIALOG_H) / 2;
/// Margin between the box's edge and what it holds, and between its two answers.
pub const DIALOG_INSET: usize = 24;
pub const DIALOG_GAP: usize = 8;

/// Glyphs one line of the box holds: what fits between its own margins.
pub const DIALOG_GLYPHS: usize = (DIALOG_W - 2 * DIALOG_INSET + OVERLAY_GAP) / GLYPH_PITCH;

/// The rows the box's two lines are stamped on: the question, and the consequence under it.
pub const DIALOG_TITLE_TOP: usize = DIALOG_TOP + DIALOG_INSET / 2;
pub const DIALOG_BODY_TOP: usize = DIALOG_TITLE_TOP + ROW_PITCH;

/// One answer's box: two of them, side by side along the box's own bottom edge.
pub const DIALOG_ANSWER_W: usize = (DIALOG_W - 2 * DIALOG_INSET - DIALOG_GAP) / 2;

/// The box one answer of a confirmation is drawn in and tested against.
pub const fn dialog_answer_box(choice: DialogChoice) -> ControlRect {
    let x = match choice {
        DialogChoice::Keep => DIALOG_X + DIALOG_INSET,
        DialogChoice::Act => DIALOG_X + DIALOG_INSET + DIALOG_ANSWER_W + DIALOG_GAP,
    };
    ControlRect {
        x,
        y: DIALOG_TOP + DIALOG_H - DIALOG_INSET - TOUCH_MIN,
        w: DIALOG_ANSWER_W,
        h: TOUCH_MIN,
    }
}

/// The answer a tap at `(x, y)` landed on, or `None` for a point that missed both.
pub fn dialog_choice_at(x: u16, y: u16) -> Option<DialogChoice> {
    [DialogChoice::Keep, DialogChoice::Act]
        .into_iter()
        .find(|choice| holds(dialog_answer_box(*choice), x, y))
}

fn holds(rect: ControlRect, x: u16, y: u16) -> bool {
    let (x, y) = (usize::from(x), usize::from(y));
    (rect.x..rect.x + rect.w).contains(&x) && (rect.y..rect.y + rect.h).contains(&y)
}

const _: () = assert!(
    DIALOG_TOP >= LIST_TOP && DIALOG_TOP + DIALOG_H <= TOOLBAR_TOP,
    "a confirmation is drawn over rows the list does not own"
);
const _: () = assert!(
    DIALOG_TITLE_TOP >= DIALOG_TOP
        && DIALOG_BODY_TOP >= DIALOG_TITLE_TOP + FONT_H
        && DIALOG_BODY_TOP + ROW_PITCH + FONT_H <= dialog_answer_box(DialogChoice::Keep).y
        && DIALOG_TITLE_TOP + FONT_H <= dialog_answer_box(DialogChoice::Act).y,
    "the box's own words run into its answers"
);
const _: () = assert!(
    dialog_answer_box(DialogChoice::Keep).x >= DIALOG_X
        && dialog_answer_box(DialogChoice::Act).x + DIALOG_ANSWER_W <= DIALOG_X + DIALOG_W,
    "an answer is drawn outside the box that holds it"
);
const _: () = assert!(
    dialog_answer_box(DialogChoice::Keep).w >= TOUCH_MIN
        && dialog_answer_box(DialogChoice::Keep).h >= TOUCH_MIN,
    "an answer to a confirmation is smaller than a finger"
);
const _: () = assert!(
    DIALOG_GLYPHS >= 16,
    "a line of a confirmation is too narrow to ask anything in"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_confirmation_answers_from_two_boxes_that_do_not_touch() {
        let keep = dialog_answer_box(DialogChoice::Keep);
        let act = dialog_answer_box(DialogChoice::Act);
        assert_eq!(keep.y, act.y);
        assert_eq!(
            act.x,
            keep.x + keep.w + DIALOG_GAP,
            "the two answers are one gap apart"
        );
        for choice in [DialogChoice::Keep, DialogChoice::Act] {
            let rect = dialog_answer_box(choice);
            for x in rect.x..rect.x + rect.w {
                for y in rect.y..rect.y + rect.h {
                    assert_eq!(dialog_choice_at(x as u16, y as u16), Some(choice));
                }
            }
        }
        let seam = (keep.x + keep.w) as u16;
        assert_eq!(dialog_choice_at(seam, keep.y as u16), None);
        assert_eq!(dialog_choice_at(keep.x as u16, DIALOG_TOP as u16), None);
        assert_eq!(dialog_choice_at(0, 0), None);
    }
}

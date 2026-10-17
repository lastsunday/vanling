//! What the Network page is made of, and which of its parts are showing.
//!
//! The order is the layering, and it is the whole of it: a part is drawn over every part before
//! it, so the question at the end covers the list without either of them knowing about the other.
//! Which parts are drawn at all is [`Part::hidden`], and both live here rather than in the panel
//! because the panel is only reachable on the device: a part that stopped being drawn, or moved in
//! the order, is otherwise a bug nobody can read as anything but a design decision.
//!
//! Hiding is not only a drawing matter. A part that is not drawn is not there to be tapped either,
//! and the two have to agree: [`Part::hidden`] is what the panel draws, [`Part::showing`] is what
//! [`crate::intent`] aims at, and they are one answer rather than two — a keyboard that took the
//! strip down without the page agreeing was a strip stamped across the keys that end an entry.

use crate::drivers::network::{LinkPhase, LinkStatus};
use crate::state::NetworkSetup;

/// One part of the page, in the order the page draws them.
///
/// The panel's paper is the ground turned over, so a part that lays down a surface merges with any
/// surface already under it. A part that has to read as its own rectangle is therefore drawn over
/// a page that has no other surface on it, which is what [`Part::hidden`] is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// The title and the readings under it: the phase, the network the page is about, and the
    /// address — or the draft, while one is being typed.
    Header,
    /// The page's content: the scan's bands, or the keyboard a picked row opened in their place.
    Content,
    /// The action strip along the bottom.
    Controls,
    /// The box over the list: the question a confirmation is asking, or what the radio is doing
    /// while an attempt is in flight.
    Overlay,
}

impl Part {
    /// Every part, in the order they are drawn.
    pub const IN_ORDER: [Part; 4] = [Part::Header, Part::Content, Part::Controls, Part::Overlay];

    /// Whether `setup` is not showing this part at all.
    pub fn hidden(self, setup: NetworkSetup, link: LinkStatus) -> bool {
        let question = setup.confirm.is_some();
        let joining = joining(setup, link);
        match self {
            // The question holds the only two answers there are, so the strip's own verb would be
            // a verb the page is not offering. The list goes with it: the question is about one of
            // its rows, and that row's surface is the same paper as the question's, so a band
            // sticking out either side of the box would read as part of the box.
            //
            // The keyboard takes the strip down for a reason of its own, and it is geometry: the
            // strip's box and the keyboard's last row are the same band of the panel, so a page
            // that drew both would stamp the strip's own word across the keys that end an entry.
            // An entry in progress is what the finger is doing, and the strip is not a control
            // while it is typed on — [`crate::intent`] sends every point to the keys.
            Part::Controls => question || setup.keyboard,
            // The box the radio's own attempt is drawn in covers the list the same way a question
            // does. What stays under it is the strip: the box asks nothing, and `LEAVE` is still
            // the page's one way off a link.
            Part::Content => question || joining,
            // A question is the finger's own screen and the join box is the radio's, so when both
            // are up it is the question the box holds.
            Part::Overlay => !question && !joining,
            Part::Header => false,
        }
    }

    /// Whether `setup` is showing this part: the same fact as [`Part::hidden`], read the way a
    /// gesture asks it.
    pub fn showing(self, setup: NetworkSetup, link: LinkStatus) -> bool {
        !self.hidden(setup, link)
    }
}

/// Whether the page is showing the box that says an attempt is in flight.
///
/// Two facts, and the second is not about the radio: an attempt is in flight, and no keyboard is
/// over it. The keyboard is the page's own subject while it is up — the attempt is the radio's —
/// and a draft being typed is what the finger is doing, so the box is not drawn under one. What
/// the two stages of the attempt are called is the panel's business; which of them it is, is
/// [`LinkStatus::ap`], which the radio fills in when it has accepted the credential.
pub fn joining(setup: NetworkSetup, link: LinkStatus) -> bool {
    !setup.keyboard && link.phase == LinkPhase::Connecting
}

/// Hands `draw` the parts `setup` is showing, in the order they are drawn.
///
/// The loop is here rather than in the panel for the same reason the parts are: which of them is
/// asked for, and in what order, is the whole of the layering, and the panel is the only place a
/// wrong answer to it cannot be read back. The panel's `draw` is a `match` over the parts, so what
/// is left to it is what each part looks like.
pub fn walk<F: FnMut(Part)>(setup: NetworkSetup, link: LinkStatus, mut draw: F) {
    for part in Part::IN_ORDER {
        if part.showing(setup, link) {
            draw(part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::network::AccessPoint;
    use crate::keyboard;
    use crate::state::{Confirm, Entry};
    use alloc::vec;
    use alloc::vec::Vec;

    fn asking() -> NetworkSetup {
        NetworkSetup {
            confirm: Some(Confirm::Forget(AccessPoint::default())),
            ..NetworkSetup::default()
        }
    }

    /// The link while an attempt is in flight and the radio has not accepted it yet.
    fn attempting() -> LinkStatus {
        LinkStatus {
            phase: LinkPhase::Connecting,
            ..LinkStatus::default()
        }
    }

    fn typing() -> NetworkSetup {
        NetworkSetup {
            keyboard: true,
            entry: Some(Entry::default()),
            ..NetworkSetup::default()
        }
    }

    #[test]
    fn the_order_is_the_layering() {
        // Written out rather than derived, because this list *is* the z-order: a part moved in it
        // is a part drawn under something it used to be over.
        assert_eq!(
            Part::IN_ORDER,
            [Part::Header, Part::Content, Part::Controls, Part::Overlay]
        );
    }

    #[test]
    fn a_page_that_is_only_showing_the_page_has_no_box_over_it() {
        let (setup, link) = (NetworkSetup::default(), LinkStatus::default());
        assert!(Part::Header.showing(setup, link));
        assert!(
            Part::Content.showing(setup, link),
            "the list is the content"
        );
        assert!(
            Part::Controls.showing(setup, link),
            "and the strip is offered"
        );
        assert!(
            Part::Overlay.hidden(setup, link),
            "there is nothing over the list to draw"
        );
    }

    #[test]
    fn a_question_hides_the_content_and_the_strip_and_nothing_else() {
        let setup = asking();
        let link = LinkStatus::default();
        assert!(
            Part::Content.hidden(setup, link),
            "the list is under the box"
        );
        assert!(
            Part::Controls.hidden(setup, link),
            "and the strip is not offered"
        );
        assert!(!Part::Header.hidden(setup, link), "the readings stay");
        assert!(
            Part::Overlay.showing(setup, link),
            "and the box is what is drawn"
        );
    }

    #[test]
    fn a_join_in_flight_is_a_box_over_the_list_with_the_strip_still_offered() {
        // The box the radio's attempt is drawn in is not a question: it wants nothing from the
        // finger, so the strip stays where it is and `LEAVE` stays the way off the link.
        let setup = NetworkSetup::default();
        let link = attempting();
        assert!(
            Part::Content.hidden(setup, link),
            "the box covers the list it is about"
        );
        assert!(
            Part::Controls.showing(setup, link),
            "the strip is not the box's to take"
        );
        assert!(Part::Overlay.showing(setup, link));
    }

    #[test]
    fn a_question_is_what_the_box_holds_when_a_link_is_joining_under_it() {
        // Both are the same rectangle in the same place, so one of them has to win: a question is
        // the finger's own screen and the attempt is the radio's, so the question is the box.
        let setup = asking();
        let link = attempting();
        assert!(Part::Content.hidden(setup, link));
        assert!(
            Part::Controls.hidden(setup, link),
            "the question holds the strip, whatever the radio is doing"
        );
        assert!(Part::Overlay.showing(setup, link));
    }

    #[test]
    fn the_page_under_a_keyboard_has_no_strip_and_no_box() {
        let setup = typing();
        let link = LinkStatus::default();
        assert!(
            Part::Content.showing(setup, link),
            "the keyboard is the content"
        );
        assert!(
            Part::Controls.hidden(setup, link),
            "the strip is not a control while the keys are the page"
        );
        assert!(
            Part::Overlay.hidden(setup, link),
            "and there is nothing for the box to say over a draft"
        );
    }

    #[test]
    fn a_draft_is_the_pages_own_subject_over_an_attempt_too() {
        // A row picked before the radio associated leaves a keyboard up over the attempt: what the
        // finger is doing is the entry, so the attempt is not drawn over it — the header's phase
        // word is where the radio says where it got to.
        let setup = typing();
        let link = attempting();
        assert!(Part::Content.showing(setup, link));
        assert!(Part::Controls.hidden(setup, link));
        assert!(Part::Overlay.hidden(setup, link));
    }

    #[test]
    fn the_strip_is_hidden_under_a_keyboard_because_the_two_are_the_same_band() {
        // The rule above is geometry rather than taste, and this is the geometry: the keyboard's
        // rows run from [`keyboard::KEYBOARD_TOP`] to `CONTENT_BOTTOM`, which is where the strip
        // is, so a page that drew the strip with the keys would stamp its own word into the middle
        // of the row that ends an entry. A panel whose two no longer overlap is a panel whose
        // keyboard could be drawn with the strip — until then, the two are one band.
        let strip = keyboard::toolbar_box();
        let last = keyboard::key_box(keyboard::COLUMNS * (keyboard::ROWS - 1))
            .expect("the keyboard's last row is a row of keys");
        assert!(
            strip.y < last.y + last.h,
            "the strip ({strip:?}) starts below the keys ({last:?}), so nothing is stamped over"
        );
        let label_top = strip.y + (strip.h - crate::overlay::FONT_H) / 2;
        assert!(
            label_top < last.y + last.h,
            "and the strip's own word lands inside the last key row"
        );
    }

    /// The names of the parts `walk` hands out, in the order it hands them out.
    fn walked(setup: NetworkSetup, link: LinkStatus) -> Vec<Part> {
        let mut asked = Vec::new();
        walk(setup, link, |part| asked.push(part));
        asked
    }

    #[test]
    fn the_walk_hands_out_the_page_and_leaves_the_box_out_while_there_is_nothing_to_say() {
        assert_eq!(
            walked(NetworkSetup::default(), LinkStatus::default()),
            vec![Part::Header, Part::Content, Part::Controls]
        );
    }

    #[test]
    fn the_walk_never_asks_for_what_a_question_is_hiding() {
        // What the panel's own loop would have to get right and could not be caught getting wrong:
        // the parts a question hides are never handed to the drawing at all.
        assert_eq!(
            walked(asking(), LinkStatus::default()),
            vec![Part::Header, Part::Overlay]
        );
    }

    #[test]
    fn the_walk_leaves_the_strip_out_and_keeps_the_box_in_while_a_finger_types() {
        assert_eq!(
            walked(typing(), LinkStatus::default()),
            vec![Part::Header, Part::Content]
        );
    }

    #[test]
    fn the_walk_puts_the_join_box_over_the_list_and_leaves_the_strip_under_it() {
        assert_eq!(
            walked(NetworkSetup::default(), attempting()),
            vec![Part::Header, Part::Controls, Part::Overlay]
        );
    }
}

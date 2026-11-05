//! The Network page's bands: which parts of the panel are showing, and in what layering.

use super::NetworkPage;
use iot_core::drivers::network::{LinkPhase, LinkStatus};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Header,
    Content,
    Controls,
    Overlay,
}

impl Band {
    pub const IN_ORDER: [Band; 4] = [Band::Header, Band::Content, Band::Controls, Band::Overlay];

    pub fn hidden(self, draft: NetworkPage, link: LinkStatus) -> bool {
        let question = draft.confirm.is_some();
        let boxed = question || link.shows_setup_box();
        let joining = joining(draft, link);
        match self {
            Band::Controls => question || draft.keyboard,
            Band::Content => boxed || joining,
            Band::Overlay => !boxed && !joining,
            Band::Header => false,
        }
    }

    pub fn showing(self, draft: NetworkPage, link: LinkStatus) -> bool {
        !self.hidden(draft, link)
    }
}

pub fn joining(draft: NetworkPage, link: LinkStatus) -> bool {
    !draft.keyboard && link.phase == LinkPhase::Connecting
}

pub fn each_band<F: FnMut(Band)>(draft: NetworkPage, link: LinkStatus, mut draw: F) {
    for band in Band::IN_ORDER {
        if band.showing(draft, link) {
            draw(band);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls;
    use crate::model::state::{Confirm, Entry};
    use alloc::vec;
    use alloc::vec::Vec;
    use iot_core::drivers::network::{AccessPoint, LinkPhase, LinkStatus, NetworkMode, SetupHost};

    fn asking() -> NetworkPage {
        NetworkPage {
            confirm: Some(Confirm::Forget(AccessPoint::default())),
            ..NetworkPage::default()
        }
    }

    fn attempting() -> LinkStatus {
        LinkStatus {
            phase: LinkPhase::Connecting,
            ..LinkStatus::default()
        }
    }

    fn typing() -> NetworkPage {
        NetworkPage {
            keyboard: true,
            entry: Some(Entry::default()),
            ..NetworkPage::default()
        }
    }

    #[test]
    fn the_order_is_the_layering() {
        assert_eq!(
            Band::IN_ORDER,
            [Band::Header, Band::Content, Band::Controls, Band::Overlay]
        );
    }

    #[test]
    fn a_page_that_is_only_showing_the_page_has_no_box_over_it() {
        let (draft, link) = (NetworkPage::default(), LinkStatus::default());
        assert!(Band::Header.showing(draft, link));
        assert!(
            Band::Content.showing(draft, link),
            "the list is the content"
        );
        assert!(
            Band::Controls.showing(draft, link),
            "and the strip is offered"
        );
        assert!(
            Band::Overlay.hidden(draft, link),
            "there is nothing over the list to draw"
        );
    }

    #[test]
    fn a_question_hides_the_content_and_the_strip_and_nothing_else() {
        let draft = asking();
        let link = LinkStatus::default();
        assert!(
            Band::Content.hidden(draft, link),
            "the list is under the box"
        );
        assert!(
            Band::Controls.hidden(draft, link),
            "and the strip is not offered"
        );
        assert!(!Band::Header.hidden(draft, link), "the readings stay");
        assert!(
            Band::Overlay.showing(draft, link),
            "and the box is what is drawn"
        );
    }

    #[test]
    fn a_join_in_flight_is_a_box_over_the_list_with_the_strip_still_offered() {
        let draft = NetworkPage::default();
        let link = attempting();
        assert!(
            Band::Content.hidden(draft, link),
            "the box covers the list it is about"
        );
        assert!(
            Band::Controls.showing(draft, link),
            "the strip is not the box's to take"
        );
        assert!(Band::Overlay.showing(draft, link));
    }

    #[test]
    fn the_setup_box_covers_the_list_and_leaves_the_strip_offered() {
        let draft = NetworkPage::default();
        let link = LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::AccessPoint,
                cancellable: false,
            },
            ..LinkStatus::default()
        };
        assert!(Band::Content.hidden(draft, link));
        assert!(
            Band::Controls.showing(draft, link),
            "the strip is how the panel gets back"
        );
        assert!(
            !Band::Header.hidden(draft, link),
            "the mode's name stays on the header behind the box"
        );
        assert!(Band::Overlay.showing(draft, link));
    }

    #[test]
    fn a_phone_less_front_has_no_box_over_the_list() {
        let draft = NetworkPage::default();
        let link = LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::Panel,
                cancellable: false,
            },
            ..LinkStatus::default()
        };
        assert!(!link.shows_setup_box());
        assert!(Band::Content.showing(draft, link));
        assert!(Band::Controls.showing(draft, link));
        assert!(Band::Overlay.hidden(draft, link));
    }

    #[test]
    fn a_question_is_what_the_box_holds_when_a_link_is_joining_under_it() {
        let draft = asking();
        let link = attempting();
        assert!(Band::Content.hidden(draft, link));
        assert!(
            Band::Controls.hidden(draft, link),
            "the question holds the strip, whatever the radio is doing"
        );
        assert!(Band::Overlay.showing(draft, link));
    }

    #[test]
    fn the_page_under_a_keyboard_has_no_strip_and_no_box() {
        let draft = typing();
        let link = LinkStatus::default();
        assert!(
            Band::Content.showing(draft, link),
            "the keyboard is the content"
        );
        assert!(
            Band::Controls.hidden(draft, link),
            "the strip is not a control while the keys are the page"
        );
        assert!(
            Band::Overlay.hidden(draft, link),
            "and there is nothing for the box to say over a draft"
        );
    }

    #[test]
    fn a_draft_is_the_pages_own_subject_over_an_attempt_too() {
        let draft = typing();
        let link = attempting();
        assert!(Band::Content.showing(draft, link));
        assert!(Band::Controls.hidden(draft, link));
        assert!(Band::Overlay.hidden(draft, link));
    }

    #[test]
    fn the_strip_is_hidden_under_a_keyboard_because_the_two_are_the_same_band() {
        let strip = controls::toolbar_box();
        let last = controls::key_box(controls::COLUMNS * (controls::ROWS - 1))
            .expect("the keyboard's last row is a row of keys");
        assert!(
            strip.y < last.y + last.h,
            "the strip ({strip:?}) starts below the keys ({last:?}), so nothing is stamped over"
        );
        let label_top = strip.y + (strip.h - iot_core::overlay::FONT_H) / 2;
        assert!(
            label_top < last.y + last.h,
            "and the strip's own word lands inside the last key row"
        );
    }

    fn bands(draft: NetworkPage, link: LinkStatus) -> Vec<Band> {
        let mut asked = Vec::new();
        each_band(draft, link, |band| asked.push(band));
        asked
    }

    #[test]
    fn the_bands_hand_out_the_page_and_leave_the_box_out_while_there_is_nothing_to_say() {
        assert_eq!(
            bands(NetworkPage::default(), LinkStatus::default()),
            vec![Band::Header, Band::Content, Band::Controls]
        );
    }

    #[test]
    fn the_bands_never_ask_for_what_a_question_is_hiding() {
        assert_eq!(
            bands(asking(), LinkStatus::default()),
            vec![Band::Header, Band::Overlay]
        );
    }

    #[test]
    fn the_bands_leave_the_strip_out_and_keep_the_box_in_while_a_finger_types() {
        assert_eq!(
            bands(typing(), LinkStatus::default()),
            vec![Band::Header, Band::Content]
        );
    }

    #[test]
    fn the_bands_put_the_join_box_over_the_list_and_leave_the_strip_under_it() {
        assert_eq!(
            bands(NetworkPage::default(), attempting()),
            vec![Band::Header, Band::Controls, Band::Overlay]
        );
    }
}

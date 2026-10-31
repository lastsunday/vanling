use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::controls;
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;
use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::network::{AccessPoint, LinkPhase, LinkStatus};
use crate::intent::{ActionIntent, InputIntent, NetworkIntent};
use crate::keyboard;
use crate::state::{DeviceState, NetworkDraft};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Header,
    Content,
    Controls,
    Overlay,
}

impl Band {
    pub const IN_ORDER: [Band; 4] = [Band::Header, Band::Content, Band::Controls, Band::Overlay];

    pub fn hidden(self, draft: NetworkDraft, link: LinkStatus) -> bool {
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

    pub fn showing(self, draft: NetworkDraft, link: LinkStatus) -> bool {
        !self.hidden(draft, link)
    }
}

pub fn joining(draft: NetworkDraft, link: LinkStatus) -> bool {
    !draft.keyboard && link.phase == LinkPhase::Connecting
}

pub fn each_band<F: FnMut(Band)>(draft: NetworkDraft, link: LinkStatus, mut draw: F) {
    for band in Band::IN_ORDER {
        if band.showing(draft, link) {
            draw(band);
        }
    }
}

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}

pub(super) fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) => {
            network_tap(current, x, y, at_ms)
        }
        InputEvent::Gesture(GestureEvent::LongPress { x, y, .. }) => network_hold(current, x, y),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. })
            if !current.network.draft.keyboard =>
        {
            if current.network.draft.confirm.is_some() {
                return ActionIntent::Network(NetworkIntent::Dismiss);
            }
            if !Band::Content.showing(current.network.draft, current.network.link) {
                return ActionIntent::Invalid;
            }
            match direction {
                SwipeDirection::Up => ActionIntent::Network(NetworkIntent::Scroll { rows: 1 }),
                SwipeDirection::Down => ActionIntent::Network(NetworkIntent::Scroll { rows: -1 }),
                _ => ActionIntent::Invalid,
            }
        }
        InputEvent::Button(ButtonEvent::Click) => {
            controls::strip(current.network.draft, current.network.link)
                .command(keyboard::ToolbarCell::Right)
        }
        _ => ActionIntent::Invalid,
    }
}

pub(super) fn repaint(prev: &Diagnostics, next: &Diagnostics, _overlay: bool) -> bool {
    prev.link != next.link
        || prev.aps != next.aps
        || prev.draft != next.draft
        || prev.mac != next.mac
}

fn network_tap(current: &DeviceState, x: u16, y: u16, at_ms: u32) -> ActionIntent {
    if current.network.draft.keyboard {
        let Some(entry) = current.network.draft.entry else {
            return ActionIntent::Invalid;
        };
        return keyboard::key_at(entry.layer(), x, y).map_or(ActionIntent::Invalid, |key| {
            ActionIntent::Network(NetworkIntent::Key { key, at_ms })
        });
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if let Some(cell) = keyboard::toolbar_cell_at(x, y) {
        return controls::strip(current.network.draft, current.network.link).command(cell);
    }
    if !Band::Content.showing(current.network.draft, current.network.link) {
        return ActionIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(ActionIntent::Invalid, |ap| {
            ActionIntent::Network(NetworkIntent::Choose(ap))
        }),
        None => ActionIntent::Invalid,
    }
}

fn network_hold(current: &DeviceState, x: u16, y: u16) -> ActionIntent {
    if current.network.draft.keyboard {
        return ActionIntent::Invalid;
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if !Band::Content.showing(current.network.draft, current.network.link) {
        return ActionIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(ActionIntent::Invalid, |ap| {
            ActionIntent::Network(NetworkIntent::AskForget(ap))
        }),
        None => ActionIntent::Invalid,
    }
}

fn answer_at(current: &DeviceState, x: u16, y: u16) -> Option<ActionIntent> {
    if current.network.link.shows_setup_box() {
        let answers = controls::answers(current.network.link);
        return match keyboard::dialog_choice_at(x, y) {
            Some(keyboard::DialogChoice::Keep) => Some(answers.keep.command),
            Some(keyboard::DialogChoice::Act) => Some(answers.act.command),
            None => None,
        };
    }
    let confirm = current.network.draft.confirm?;
    Some(match keyboard::dialog_choice_at(x, y) {
        Some(keyboard::DialogChoice::Act) => ActionIntent::Network(NetworkIntent::Answer(confirm)),
        Some(keyboard::DialogChoice::Keep) | None => ActionIntent::Network(NetworkIntent::Dismiss),
    })
}

fn ap_at(current: &DeviceState, band: usize) -> Option<AccessPoint> {
    let index = usize::from(current.network.draft.list_top) + band;
    current.network.aps.and_then(|list| list.get(index))
}

#[cfg(test)]
mod parts {
    use super::*;
    use crate::drivers::network::AccessPoint;
    use crate::keyboard;
    use crate::state::{Confirm, Entry};
    use alloc::vec;
    use alloc::vec::Vec;

    fn asking() -> NetworkDraft {
        NetworkDraft {
            confirm: Some(Confirm::Forget(AccessPoint::default())),
            ..NetworkDraft::default()
        }
    }

    fn attempting() -> LinkStatus {
        LinkStatus {
            phase: LinkPhase::Connecting,
            ..LinkStatus::default()
        }
    }

    fn typing() -> NetworkDraft {
        NetworkDraft {
            keyboard: true,
            entry: Some(Entry::default()),
            ..NetworkDraft::default()
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
        let (draft, link) = (NetworkDraft::default(), LinkStatus::default());
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
        let draft = NetworkDraft::default();
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
        let draft = NetworkDraft::default();
        let link = LinkStatus {
            setup: true,
            host: crate::drivers::network::SetupHost::AccessPoint,
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
        let draft = NetworkDraft::default();
        let link = LinkStatus {
            setup: true,
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

    fn bands(draft: NetworkDraft, link: LinkStatus) -> Vec<Band> {
        let mut asked = Vec::new();
        each_band(draft, link, |band| asked.push(band));
        asked
    }

    #[test]
    fn the_bands_hand_out_the_page_and_leave_the_box_out_while_there_is_nothing_to_say() {
        assert_eq!(
            bands(NetworkDraft::default(), LinkStatus::default()),
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
            bands(NetworkDraft::default(), attempting()),
            vec![Band::Header, Band::Controls, Band::Overlay]
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
    use crate::drivers::network::{LinkStatus, SetupHost};
    use crate::intent::{ActionIntent, NetworkIntent, translate};
    use crate::page::test_support::{DEFAULT_BREATH, op, state_on, with_network};
    use crate::state::{DeviceState, DisplayPage};

    mod network_page {
        use super::*;
        use crate::drivers::network::{AccessPoint, ApList, Ssid};
        use crate::keyboard::{self, Key};

        fn ap(ssid: &[u8], rssi: i8) -> AccessPoint {
            ap_with(ssid, rssi, 6)
        }

        fn ap_with(ssid: &[u8], rssi: i8, last: u8) -> AccessPoint {
            AccessPoint {
                ssid: Ssid::from_octets(ssid),
                bssid: [1, 2, 3, 4, 5, last],
                channel: 6,
                rssi,
                secured: true,
            }
        }

        fn on_network() -> DeviceState {
            with_network(state_on(DisplayPage::Network, DEFAULT_BREATH), |net| {
                net.aps = Some(ApList::from_ranked(&[ap(b"neighbour", -55)]));
            })
        }

        fn typing() -> DeviceState {
            with_network(on_network(), |net| {
                net.draft = crate::state::NetworkDraft {
                    selected: Some(ap(b"neighbour", -55)),
                    entry: Some(crate::state::Entry::default()),
                    keyboard: true,
                    ..Default::default()
                };
            })
        }

        fn tap_at(x: u16, y: u16) -> InputEvent {
            InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms: 1_000,
            })
        }

        fn long_press_at(x: u16, y: u16) -> InputEvent {
            InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x,
                y,
                held_ms: u16::from(crate::drivers::input::FINGER_LONG_PRESS) + 100,
            })
        }

        fn asking() -> DeviceState {
            with_network(on_network(), |net| {
                net.draft = crate::state::NetworkDraft {
                    confirm: Some(crate::state::Confirm::Forget(ap(b"neighbour", -55))),
                    ..Default::default()
                };
            })
        }

        fn swipe(direction: SwipeDirection) -> InputEvent {
            InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction,
                x: 100,
                y: 100,
                end_x: 100,
                end_y: 40,
                held_ms: 60,
                distance_px: 60,
            })
        }

        fn key_tap(cell: usize) -> InputEvent {
            let rect = keyboard::key_box(cell).expect("a cell of the grid");
            tap_at((rect.x + rect.w / 2) as u16, (rect.y + rect.h / 2) as u16)
        }

        #[test]
        fn a_tap_on_a_band_is_that_band_of_the_list() {
            let state = with_network(on_network(), |net| {
                net.aps = Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                ]));
            });
            let first = keyboard::LIST_TOP as u16;
            assert_eq!(
                translate(&op(tap_at(30, first)), &state),
                ActionIntent::Network(NetworkIntent::Choose(ap(b"neighbour", -55))),
                "the list's first band is the first network"
            );
            assert_eq!(
                translate(&op(tap_at(30, first + keyboard::LIST_ROW_H as u16)), &state),
                ActionIntent::Network(NetworkIntent::Choose(ap_with(b"cafe", -60, 7))),
                "a band down is the next network"
            );
            assert_eq!(
                translate(
                    &op(tap_at(30, first + keyboard::LIST_ROW_H as u16 - 1)),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Choose(ap(b"neighbour", -55))),
                "and the pixels below a band's glyphs still belong to that band"
            );
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                ActionIntent::Invalid,
                "while the page's own header is not a band"
            );
        }

        #[test]
        fn a_slide_moves_the_list_and_only_while_the_keyboard_is_down() {
            let state = on_network();
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                ActionIntent::Network(NetworkIntent::Scroll { rows: 1 }),
                "a finger pushing the list up walks down it"
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Down)), &state),
                ActionIntent::Network(NetworkIntent::Scroll { rows: -1 })
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Left)), &state),
                ActionIntent::Invalid,
                "the list is one column, so there is nothing to slide sideways to"
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &typing()),
                ActionIntent::Invalid
            );
        }

        #[test]
        fn the_strip_is_the_page_s_own_control_and_the_bands_stop_above_it() {
            let state = with_network(on_network(), |net| {
                net.aps = Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                    ap_with(b"bakery", -65, 8),
                ]));
            });
            let strip = keyboard::toolbar_box();
            for (x, _cell) in [
                (0, keyboard::ToolbarCell::Left),
                (strip.w / 2 - 1, keyboard::ToolbarCell::Left),
                (strip.w / 2, keyboard::ToolbarCell::Right),
                (strip.w - 1, keyboard::ToolbarCell::Right),
            ] {
                for y in [strip.y, strip.y + strip.h / 2, strip.y + strip.h - 1] {
                    assert_eq!(
                        translate(&op(tap_at(x as u16, y as u16)), &state),
                        ActionIntent::Network(NetworkIntent::ScanNow),
                        "({x}, {y}) is the strip"
                    );
                }
            }
            assert_eq!(
                translate(&op(tap_at(160, (keyboard::TOOLBAR_TOP - 1) as u16)), &state),
                ActionIntent::Network(NetworkIntent::Choose(ap_with(b"bakery", -65, 8))),
                "the row above the strip is still the list's own last band"
            );
        }

        #[test]
        fn the_button_is_the_strip_verb_and_the_setup_box_asks_about_its_passphrase() {
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_network()),
                ActionIntent::Network(NetworkIntent::ScanNow)
            );

            let announcing = with_network(on_network(), |net| {
                net.link = LinkStatus {
                    setup: true,
                    host: SetupHost::AccessPoint,
                    setup_password: Some(crate::drivers::network::SetupPass::from_array(
                        *b"k3mq9t7w2x",
                    )),
                    ..Default::default()
                };
            });
            let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
            let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
            assert_eq!(
                translate(
                    &op(tap_at(keep.x as u16 + 1, keep.y as u16 + 1)),
                    &announcing
                ),
                ActionIntent::Network(NetworkIntent::SetOpenAccessPoint(true))
            );
            assert_eq!(
                translate(&op(tap_at(act.x as u16 + 1, act.y as u16 + 1)), &announcing),
                ActionIntent::Network(NetworkIntent::SwitchHost)
            );
            assert_eq!(
                translate(&op(tap_at(10, 10)), &announcing),
                ActionIntent::Invalid
            );
            assert_eq!(
                translate(&op(long_press_at(10, 10)), &announcing),
                ActionIntent::Invalid
            );
        }

        #[test]
        fn a_hold_on_a_band_is_that_band_asked_about() {
            let state = with_network(on_network(), |net| {
                net.aps = Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                ]));
            });
            let first = keyboard::LIST_TOP as u16;
            assert_eq!(
                translate(&op(long_press_at(30, first)), &state),
                ActionIntent::Network(NetworkIntent::AskForget(ap(b"neighbour", -55)))
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, first + keyboard::LIST_ROW_H as u16)),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::AskForget(ap_with(b"cafe", -60, 7)))
            );
            assert_eq!(
                translate(&op(long_press_at(30, 5)), &state),
                ActionIntent::Invalid,
                "the header is not a row to ask about"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(160, keyboard::TOOLBAR_TOP as u16)),
                    &state
                ),
                ActionIntent::Invalid,
                "and neither is the strip"
            );
            assert_eq!(
                translate(&op(long_press_at(30, first)), &typing()),
                ActionIntent::Invalid,
                "while the keyboard is up there is no list under the finger to hold"
            );
        }

        #[test]
        fn a_question_is_answered_by_where_a_finger_lands_and_by_nothing_else() {
            let state = asking();
            let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
            let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
            assert_eq!(
                translate(
                    &op(tap_at(
                        (act.x + act.w / 2) as u16,
                        (act.y + act.h / 2) as u16
                    )),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Answer(crate::state::Confirm::Forget(ap(
                    b"neighbour",
                    -55
                )))),
                "the action's own button is the whole of the yes"
            );
            assert_eq!(
                translate(
                    &op(tap_at(
                        (keep.x + keep.w / 2) as u16,
                        (keep.y + keep.h / 2) as u16
                    )),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Dismiss),
                "and the button that says no says no"
            );
            for (x, y) in [
                (30, 5),
                (30, keyboard::LIST_TOP as u16 + 2),
                (160, keyboard::TOOLBAR_TOP as u16 + 2),
                (
                    (keyboard::DIALOG_X + 2) as u16,
                    (keyboard::DIALOG_TOP + 2) as u16,
                ),
            ] {
                assert_eq!(
                    translate(&op(tap_at(x, y)), &state),
                    ActionIntent::Network(NetworkIntent::Dismiss),
                    "({x}, {y}) leaves the question undone"
                );
            }
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                ActionIntent::Network(NetworkIntent::Dismiss),
                "a swipe under the box does not slide a list it cannot see"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, keyboard::LIST_TOP as u16 + 2)),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Dismiss)
            );
            assert_eq!(
                translate(&op(tap_at(160, keyboard::TOOLBAR_TOP as u16 + 2)), &state),
                ActionIntent::Network(NetworkIntent::Dismiss)
            );
        }

        #[test]
        fn a_tap_off_the_page_s_own_content_is_nothing() {
            let header = tap_at(30, 5);
            assert_eq!(
                translate(&op(header), &on_network()),
                ActionIntent::Invalid,
                "the page's header is not a control and not a colour walk"
            );
            let state = typing();
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                ActionIntent::Invalid,
                "and the keyboard's own header is not a key"
            );
        }

        #[test]
        fn a_tap_while_the_keyboard_is_up_types_the_key_under_it() {
            let state = typing();
            assert_eq!(
                translate(&op(key_tap(1)), &state),
                ActionIntent::Network(NetworkIntent::Key {
                    key: Key::Char(b'2'),
                    at_ms: 1_000,
                }),
                "the phone layer's second key is the one printed 2"
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Shift).expect("a control's own cell")
                    )),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Key {
                    key: Key::Shift,
                    at_ms: 1_000,
                })
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Cancel).expect("a control's own cell")
                    )),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Key {
                    key: Key::Cancel,
                    at_ms: 1_000,
                })
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Reveal).expect("a control's own cell")
                    )),
                    &state
                ),
                ActionIntent::Network(NetworkIntent::Key {
                    key: Key::Reveal,
                    at_ms: 1_000,
                })
            );
            assert_eq!(translate(&op(tap_at(30, 5)), &state), ActionIntent::Invalid);
            assert_eq!(
                translate(
                    &op(tap_at(
                        30,
                        (keyboard::KEYBOARD_TOP + keyboard::ROWS * keyboard::KEY_H) as u16
                    )),
                    &state
                ),
                ActionIntent::Invalid
            );
        }

        #[test]
        fn the_clock_at_the_lift_is_what_the_key_carries() {
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::Tap {
                        id: 0,
                        x: keyboard::KEY_W as u16 / 2,
                        y: (keyboard::KEYBOARD_TOP + keyboard::KEY_H / 2) as u16,
                        held_ms: 40,
                        at_ms: 4_321,
                    })),
                    &typing()
                ),
                ActionIntent::Network(NetworkIntent::Key {
                    key: Key::Char(b'1'),
                    at_ms: 4_321,
                })
            );
        }
    }
}

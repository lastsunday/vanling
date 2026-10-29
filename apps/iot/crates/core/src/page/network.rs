use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::controls;
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;
use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::network::{AccessPoint, LinkPhase, LinkStatus};
use crate::intent::{BusinessIntent, OperationIntent};
use crate::keyboard;
use crate::state::{DeviceState, NetworkSetup};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Header,
    Content,
    Controls,
    Overlay,
}

impl Part {
    pub const IN_ORDER: [Part; 4] = [Part::Header, Part::Content, Part::Controls, Part::Overlay];

    pub fn hidden(self, setup: NetworkSetup, link: LinkStatus) -> bool {
        let question = setup.confirm.is_some();
        let boxed = question || link.shows_setup_box();
        let joining = joining(setup, link);
        match self {
            Part::Controls => question || setup.keyboard,
            Part::Content => boxed || joining,
            Part::Overlay => !boxed && !joining,
            Part::Header => false,
        }
    }

    pub fn showing(self, setup: NetworkSetup, link: LinkStatus) -> bool {
        !self.hidden(setup, link)
    }
}

pub fn joining(setup: NetworkSetup, link: LinkStatus) -> bool {
    !setup.keyboard && link.phase == LinkPhase::Connecting
}

pub fn walk<F: FnMut(Part)>(setup: NetworkSetup, link: LinkStatus, mut draw: F) {
    for part in Part::IN_ORDER {
        if part.showing(setup, link) {
            draw(part);
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

pub(super) fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) => {
            network_tap(current, x, y, at_ms)
        }
        InputEvent::Gesture(GestureEvent::LongPress { x, y, .. }) => network_hold(current, x, y),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) if !current.setup.keyboard => {
            if current.setup.confirm.is_some() {
                return BusinessIntent::Dismiss;
            }
            if !Part::Content.showing(current.setup, current.link) {
                return BusinessIntent::Invalid;
            }
            match direction {
                SwipeDirection::Up => BusinessIntent::Scroll { rows: 1 },
                SwipeDirection::Down => BusinessIntent::Scroll { rows: -1 },
                _ => BusinessIntent::Invalid,
            }
        }
        InputEvent::Button(ButtonEvent::Click) => {
            controls::strip(current.setup, current.link).command(keyboard::ToolbarCell::Right)
        }
        _ => BusinessIntent::Invalid,
    }
}

pub(super) fn repaint(prev: &Diagnostics, next: &Diagnostics, _overlay: bool) -> bool {
    prev.link != next.link
        || prev.aps != next.aps
        || prev.setup != next.setup
        || prev.mac != next.mac
}

fn network_tap(current: &DeviceState, x: u16, y: u16, at_ms: u32) -> BusinessIntent {
    if current.setup.keyboard {
        let Some(entry) = current.setup.entry else {
            return BusinessIntent::Invalid;
        };
        return keyboard::key_at(entry.layer(), x, y).map_or(BusinessIntent::Invalid, |key| {
            BusinessIntent::Key { key, at_ms }
        });
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if let Some(cell) = keyboard::toolbar_cell_at(x, y) {
        return controls::strip(current.setup, current.link).command(cell);
    }
    if !Part::Content.showing(current.setup, current.link) {
        return BusinessIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(BusinessIntent::Invalid, BusinessIntent::Choose),
        None => BusinessIntent::Invalid,
    }
}

fn network_hold(current: &DeviceState, x: u16, y: u16) -> BusinessIntent {
    if current.setup.keyboard {
        return BusinessIntent::Invalid;
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if !Part::Content.showing(current.setup, current.link) {
        return BusinessIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => {
            ap_at(current, band).map_or(BusinessIntent::Invalid, BusinessIntent::AskForget)
        }
        None => BusinessIntent::Invalid,
    }
}

fn answer_at(current: &DeviceState, x: u16, y: u16) -> Option<BusinessIntent> {
    if current.link.shows_setup_box() {
        let answers = controls::answers(current.link);
        return match keyboard::dialog_choice_at(x, y) {
            Some(keyboard::DialogChoice::Keep) => Some(answers.keep.command),
            Some(keyboard::DialogChoice::Act) => Some(answers.act.command),
            None => None,
        };
    }
    let confirm = current.setup.confirm?;
    Some(match keyboard::dialog_choice_at(x, y) {
        Some(keyboard::DialogChoice::Act) => BusinessIntent::Answer(confirm),
        Some(keyboard::DialogChoice::Keep) | None => BusinessIntent::Dismiss,
    })
}

fn ap_at(current: &DeviceState, band: usize) -> Option<AccessPoint> {
    let index = usize::from(current.setup.list_top) + band;
    current.aps.and_then(|list| list.get(index))
}

#[cfg(test)]
mod parts {
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
    fn the_setup_box_covers_the_list_and_leaves_the_strip_offered() {
        let setup = NetworkSetup::default();
        let link = LinkStatus {
            setup: true,
            front: crate::drivers::network::Front::AccessPoint,
            ..LinkStatus::default()
        };
        assert!(Part::Content.hidden(setup, link));
        assert!(
            Part::Controls.showing(setup, link),
            "the strip is how the panel gets back"
        );
        assert!(
            !Part::Header.hidden(setup, link),
            "the mode's name stays on the header behind the box"
        );
        assert!(Part::Overlay.showing(setup, link));
    }

    #[test]
    fn a_phone_less_front_has_no_box_over_the_list() {
        let setup = NetworkSetup::default();
        let link = LinkStatus {
            setup: true,
            ..LinkStatus::default()
        };
        assert!(!link.shows_setup_box());
        assert!(Part::Content.showing(setup, link));
        assert!(Part::Controls.showing(setup, link));
        assert!(Part::Overlay.hidden(setup, link));
    }

    #[test]
    fn a_question_is_what_the_box_holds_when_a_link_is_joining_under_it() {
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
        let setup = typing();
        let link = attempting();
        assert!(Part::Content.showing(setup, link));
        assert!(Part::Controls.hidden(setup, link));
        assert!(Part::Overlay.hidden(setup, link));
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

#[cfg(test)]
mod tests {
    use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
    use crate::drivers::network::{Front, LinkStatus};
    use crate::intent::{BusinessIntent, translate};
    use crate::page::test_support::{DEFAULT_BREATH, op, state_on};
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
            DeviceState {
                aps: Some(ApList::from_ranked(&[ap(b"neighbour", -55)])),
                ..state_on(DisplayPage::Network, DEFAULT_BREATH)
            }
        }

        fn typing() -> DeviceState {
            DeviceState {
                setup: crate::state::NetworkSetup {
                    selected: Some(ap(b"neighbour", -55)),
                    entry: Some(crate::state::Entry::default()),
                    keyboard: true,
                    ..Default::default()
                },
                ..on_network()
            }
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
            DeviceState {
                setup: crate::state::NetworkSetup {
                    confirm: Some(crate::state::Confirm::Forget(ap(b"neighbour", -55))),
                    ..Default::default()
                },
                ..on_network()
            }
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
            let state = DeviceState {
                aps: Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                ])),
                ..on_network()
            };
            let first = keyboard::LIST_TOP as u16;
            assert_eq!(
                translate(&op(tap_at(30, first)), &state),
                BusinessIntent::Choose(ap(b"neighbour", -55)),
                "the list's first band is the first network"
            );
            assert_eq!(
                translate(&op(tap_at(30, first + keyboard::LIST_ROW_H as u16)), &state),
                BusinessIntent::Choose(ap_with(b"cafe", -60, 7)),
                "a band down is the next network"
            );
            assert_eq!(
                translate(
                    &op(tap_at(30, first + keyboard::LIST_ROW_H as u16 - 1)),
                    &state
                ),
                BusinessIntent::Choose(ap(b"neighbour", -55)),
                "and the pixels below a band's glyphs still belong to that band"
            );
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                BusinessIntent::Invalid,
                "while the page's own header is not a band"
            );
        }

        #[test]
        fn a_slide_moves_the_list_and_only_while_the_keyboard_is_down() {
            let state = on_network();
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                BusinessIntent::Scroll { rows: 1 },
                "a finger pushing the list up walks down it"
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Down)), &state),
                BusinessIntent::Scroll { rows: -1 }
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Left)), &state),
                BusinessIntent::Invalid,
                "the list is one column, so there is nothing to slide sideways to"
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &typing()),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn the_strip_is_the_page_s_own_control_and_the_bands_stop_above_it() {
            let state = DeviceState {
                aps: Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                    ap_with(b"bakery", -65, 8),
                ])),
                ..on_network()
            };
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
                        BusinessIntent::ScanNow,
                        "({x}, {y}) is the strip"
                    );
                }
            }
            assert_eq!(
                translate(&op(tap_at(160, (keyboard::TOOLBAR_TOP - 1) as u16)), &state),
                BusinessIntent::Choose(ap_with(b"bakery", -65, 8)),
                "the row above the strip is still the list's own last band"
            );
        }

        #[test]
        fn the_button_is_the_strip_verb_and_the_setup_box_asks_about_its_passphrase() {
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_network()),
                BusinessIntent::ScanNow
            );

            let announcing = DeviceState {
                link: LinkStatus {
                    setup: true,
                    front: Front::AccessPoint,
                    setup_password: Some(crate::drivers::network::SetupPass::from_array(
                        *b"k3mq9t7w2x",
                    )),
                    ..Default::default()
                },
                ..on_network()
            };
            let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
            let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
            assert_eq!(
                translate(
                    &op(tap_at(keep.x as u16 + 1, keep.y as u16 + 1)),
                    &announcing
                ),
                BusinessIntent::SetOpenAccessPoint(true)
            );
            assert_eq!(
                translate(&op(tap_at(act.x as u16 + 1, act.y as u16 + 1)), &announcing),
                BusinessIntent::SwitchFront
            );
            assert_eq!(
                translate(&op(tap_at(10, 10)), &announcing),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(&op(long_press_at(10, 10)), &announcing),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn a_hold_on_a_band_is_that_band_asked_about() {
            let state = DeviceState {
                aps: Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                ])),
                ..on_network()
            };
            let first = keyboard::LIST_TOP as u16;
            assert_eq!(
                translate(&op(long_press_at(30, first)), &state),
                BusinessIntent::AskForget(ap(b"neighbour", -55))
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, first + keyboard::LIST_ROW_H as u16)),
                    &state
                ),
                BusinessIntent::AskForget(ap_with(b"cafe", -60, 7))
            );
            assert_eq!(
                translate(&op(long_press_at(30, 5)), &state),
                BusinessIntent::Invalid,
                "the header is not a row to ask about"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(160, keyboard::TOOLBAR_TOP as u16)),
                    &state
                ),
                BusinessIntent::Invalid,
                "and neither is the strip"
            );
            assert_eq!(
                translate(&op(long_press_at(30, first)), &typing()),
                BusinessIntent::Invalid,
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
                BusinessIntent::Answer(crate::state::Confirm::Forget(ap(b"neighbour", -55))),
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
                BusinessIntent::Dismiss,
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
                    BusinessIntent::Dismiss,
                    "({x}, {y}) leaves the question undone"
                );
            }
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                BusinessIntent::Dismiss,
                "a swipe under the box does not slide a list it cannot see"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, keyboard::LIST_TOP as u16 + 2)),
                    &state
                ),
                BusinessIntent::Dismiss
            );
            assert_eq!(
                translate(&op(tap_at(160, keyboard::TOOLBAR_TOP as u16 + 2)), &state),
                BusinessIntent::Dismiss
            );
        }

        #[test]
        fn a_tap_off_the_page_s_own_content_is_nothing() {
            let header = tap_at(30, 5);
            assert_eq!(
                translate(&op(header), &on_network()),
                BusinessIntent::Invalid,
                "the page's header is not a control and not a colour walk"
            );
            let state = typing();
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                BusinessIntent::Invalid,
                "and the keyboard's own header is not a key"
            );
        }

        #[test]
        fn a_tap_while_the_keyboard_is_up_types_the_key_under_it() {
            let state = typing();
            assert_eq!(
                translate(&op(key_tap(1)), &state),
                BusinessIntent::Key {
                    key: Key::Char(b'2'),
                    at_ms: 1_000,
                },
                "the phone layer's second key is the one printed 2"
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Shift).expect("a control's own cell")
                    )),
                    &state
                ),
                BusinessIntent::Key {
                    key: Key::Shift,
                    at_ms: 1_000,
                }
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Cancel).expect("a control's own cell")
                    )),
                    &state
                ),
                BusinessIntent::Key {
                    key: Key::Cancel,
                    at_ms: 1_000,
                }
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        keyboard::cell_of(Key::Reveal).expect("a control's own cell")
                    )),
                    &state
                ),
                BusinessIntent::Key {
                    key: Key::Reveal,
                    at_ms: 1_000,
                }
            );
            assert_eq!(
                translate(&op(tap_at(30, 5)), &state),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(
                    &op(tap_at(
                        30,
                        (keyboard::KEYBOARD_TOP + keyboard::ROWS * keyboard::KEY_H) as u16
                    )),
                    &state
                ),
                BusinessIntent::Invalid
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
                BusinessIntent::Key {
                    key: Key::Char(b'1'),
                    at_ms: 4_321,
                }
            );
        }
    }
}

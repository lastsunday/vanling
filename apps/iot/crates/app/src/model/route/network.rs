use super::{MOTION_BACKGROUND, ON_SCREEN};
use iot_core::demand::{Claim, DemandKind};

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}

#[cfg(test)]
mod tests {
    use crate::model::intent::{ActionIntent, NetworkPageIntent};
    use crate::model::route::test_support::{
        DEFAULT_BREATH, op, state_on, with_network, with_network_page,
    };
    use crate::model::state::translate;
    use crate::model::state::{DeviceState, DisplayPage};
    use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
    use iot_core::drivers::network::{LinkStatus, SetupHost};

    mod network_page {
        use super::*;
        use crate::controls::{self, Key};
        use iot_core::drivers::network::{AccessPoint, ApList, Ssid};

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
            with_network(state_on(DisplayPage::Network, DEFAULT_BREATH), |network| {
                network.aps = Some(ApList::from_ranked(&[ap(b"neighbour", -55)]));
            })
        }

        fn typing() -> DeviceState {
            with_network_page(on_network(), |page| {
                *page = crate::model::state::NetworkPage {
                    selected: Some(ap(b"neighbour", -55)),
                    entry: Some(crate::model::state::Entry::default()),
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
                held_ms: u16::from(iot_core::drivers::input::FINGER_LONG_PRESS) + 100,
            })
        }

        fn asking() -> DeviceState {
            with_network_page(on_network(), |page| {
                *page = crate::model::state::NetworkPage {
                    confirm: Some(crate::model::state::Confirm::Forget(ap(b"neighbour", -55))),
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
            let rect = controls::key_box(cell).expect("a cell of the grid");
            tap_at((rect.x + rect.w / 2) as u16, (rect.y + rect.h / 2) as u16)
        }

        #[test]
        fn a_tap_on_a_band_is_that_band_of_the_list() {
            let state = with_network(on_network(), |network| {
                network.aps = Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                ]));
            });
            let first = controls::LIST_TOP as u16;
            assert_eq!(
                translate(&op(tap_at(30, first)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::Choose(ap(b"neighbour", -55))),
                "the list's first band is the first network"
            );
            assert_eq!(
                translate(&op(tap_at(30, first + controls::LIST_ROW_H as u16)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::Choose(ap_with(b"cafe", -60, 7))),
                "a band down is the next network"
            );
            assert_eq!(
                translate(
                    &op(tap_at(30, first + controls::LIST_ROW_H as u16 - 1)),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Choose(ap(b"neighbour", -55))),
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
                ActionIntent::NetworkPage(NetworkPageIntent::Scroll { rows: 1 }),
                "a finger pushing the list up walks down it"
            );
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Down)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::Scroll { rows: -1 })
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
            let state = with_network(on_network(), |network| {
                network.aps = Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                    ap_with(b"bakery", -65, 8),
                ]));
            });
            let strip = controls::toolbar_box();
            for (x, _cell) in [
                (0, controls::ToolbarCell::Left),
                (strip.w / 2 - 1, controls::ToolbarCell::Left),
                (strip.w / 2, controls::ToolbarCell::Right),
                (strip.w - 1, controls::ToolbarCell::Right),
            ] {
                for y in [strip.y, strip.y + strip.h / 2, strip.y + strip.h - 1] {
                    assert_eq!(
                        translate(&op(tap_at(x as u16, y as u16)), &state),
                        ActionIntent::NetworkPage(NetworkPageIntent::ScanNow),
                        "({x}, {y}) is the strip"
                    );
                }
            }
            assert_eq!(
                translate(&op(tap_at(160, (controls::TOOLBAR_TOP - 1) as u16)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::Choose(ap_with(b"bakery", -65, 8))),
                "the row above the strip is still the list's own last band"
            );
        }

        #[test]
        fn the_button_is_the_strip_verb_and_the_setup_box_asks_about_its_passphrase() {
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_network()),
                ActionIntent::NetworkPage(NetworkPageIntent::ScanNow)
            );

            let announcing = with_network(on_network(), |network| {
                network.link = LinkStatus {
                    mode: iot_core::drivers::network::NetworkMode::Setup {
                        host: SetupHost::AccessPoint,
                        cancellable: false,
                    },
                    setup_password: Some(iot_core::drivers::network::SetupPass::from_array(
                        *b"k3mq9t7w2x",
                    )),
                    ..Default::default()
                };
            });
            let keep = controls::dialog_answer_box(controls::DialogChoice::Keep);
            let act = controls::dialog_answer_box(controls::DialogChoice::Act);
            assert_eq!(
                translate(
                    &op(tap_at(keep.x as u16 + 1, keep.y as u16 + 1)),
                    &announcing
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::SetOpenAccessPoint(true))
            );
            assert_eq!(
                translate(&op(tap_at(act.x as u16 + 1, act.y as u16 + 1)), &announcing),
                ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)
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
            let state = with_network(on_network(), |network| {
                network.aps = Some(ApList::from_ranked(&[
                    ap(b"neighbour", -55),
                    ap_with(b"cafe", -60, 7),
                ]));
            });
            let first = controls::LIST_TOP as u16;
            assert_eq!(
                translate(&op(long_press_at(30, first)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::AskForget(ap(b"neighbour", -55)))
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, first + controls::LIST_ROW_H as u16)),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::AskForget(ap_with(b"cafe", -60, 7)))
            );
            assert_eq!(
                translate(&op(long_press_at(30, 5)), &state),
                ActionIntent::Invalid,
                "the header is not a row to ask about"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(160, controls::TOOLBAR_TOP as u16)),
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
            let act = controls::dialog_answer_box(controls::DialogChoice::Act);
            let keep = controls::dialog_answer_box(controls::DialogChoice::Keep);
            assert_eq!(
                translate(
                    &op(tap_at(
                        (act.x + act.w / 2) as u16,
                        (act.y + act.h / 2) as u16
                    )),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Answer(
                    crate::model::state::Confirm::Forget(ap(b"neighbour", -55))
                )),
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
                ActionIntent::NetworkPage(NetworkPageIntent::Dismiss),
                "and the button that says no says no"
            );
            for (x, y) in [
                (30, 5),
                (30, controls::LIST_TOP as u16 + 2),
                (160, controls::TOOLBAR_TOP as u16 + 2),
                (
                    (controls::DIALOG_X + 2) as u16,
                    (controls::DIALOG_TOP + 2) as u16,
                ),
            ] {
                assert_eq!(
                    translate(&op(tap_at(x, y)), &state),
                    ActionIntent::NetworkPage(NetworkPageIntent::Dismiss),
                    "({x}, {y}) leaves the question undone"
                );
            }
            assert_eq!(
                translate(&op(swipe(SwipeDirection::Up)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::Dismiss),
                "a swipe under the box does not slide a list it cannot see"
            );
            assert_eq!(
                translate(
                    &op(long_press_at(30, controls::LIST_TOP as u16 + 2)),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Dismiss)
            );
            assert_eq!(
                translate(&op(tap_at(160, controls::TOOLBAR_TOP as u16 + 2)), &state),
                ActionIntent::NetworkPage(NetworkPageIntent::Dismiss)
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
                ActionIntent::NetworkPage(NetworkPageIntent::Key {
                    key: Key::Char(b'2'),
                    at_ms: 1_000,
                }),
                "the phone layer's second key is the one printed 2"
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        controls::cell_of(Key::Shift).expect("a control's own cell")
                    )),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Key {
                    key: Key::Shift,
                    at_ms: 1_000,
                })
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        controls::cell_of(Key::Cancel).expect("a control's own cell")
                    )),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Key {
                    key: Key::Cancel,
                    at_ms: 1_000,
                })
            );
            assert_eq!(
                translate(
                    &op(key_tap(
                        controls::cell_of(Key::Reveal).expect("a control's own cell")
                    )),
                    &state
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Key {
                    key: Key::Reveal,
                    at_ms: 1_000,
                })
            );
            assert_eq!(translate(&op(tap_at(30, 5)), &state), ActionIntent::Invalid);
            assert_eq!(
                translate(
                    &op(tap_at(
                        30,
                        (controls::KEYBOARD_TOP + controls::ROWS * controls::KEY_H) as u16
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
                        x: controls::KEY_W as u16 / 2,
                        y: (controls::KEYBOARD_TOP + controls::KEY_H / 2) as u16,
                        held_ms: 40,
                        at_ms: 4_321,
                    })),
                    &typing()
                ),
                ActionIntent::NetworkPage(NetworkPageIntent::Key {
                    key: Key::Char(b'1'),
                    at_ms: 4_321,
                })
            );
        }
    }
}

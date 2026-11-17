//! The Network page's input contract: what a finger on a band, the strip, a key or the
//! confirmation box asks for — and what is nothing.

use crate::pages::PageSet;
use crate::pages::network::NetworkPage;
use crate::pages::network::layout;
use crate::shared::intent::{BusinessIntent, Confirm, Key, NetworkPageIntent};
use crate::shared::model::{DeviceState, Entry, NetworkView};
use crate::state::test_support::*;
use iot_core::drivers::network::{AccessPoint, ApList, LinkStatus, SetupHost, Ssid};

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

/// The page as the devices hold it: the state it reads, and the pages that answer.
struct Fixture {
    state: DeviceState,
    pages: PageSet,
}

impl Fixture {
    fn ask(&self, event: InputEvent) -> BusinessIntent {
        translate(&op(event), &self.state, &self.pages)
    }
}

fn with_draft(draft: NetworkView) -> PageSet {
    PageSet {
        network: NetworkPage { draft },
        ..PageSet::default()
    }
}

fn on_network() -> Fixture {
    Fixture {
        state: with_network(state_on(PageId::Network, BREATH), |network| {
            network.aps = Some(ApList::from_ranked(&[ap(b"neighbour", -55)]));
        }),
        pages: PageSet::default(),
    }
}

fn typing() -> Fixture {
    let mut fixture = on_network();
    fixture.pages = with_draft(NetworkView {
        selected: Some(ap(b"neighbour", -55)),
        entry: Some(Entry::default()),
        keyboard: true,
        ..Default::default()
    });
    fixture
}

fn asking() -> Fixture {
    let mut fixture = on_network();
    fixture.pages = with_draft(NetworkView {
        confirm: Some(Confirm::Forget(ap(b"neighbour", -55))),
        ..Default::default()
    });
    fixture
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
    let rect = layout::key_box(cell).expect("a cell of the grid");
    tap_at((rect.x + rect.w / 2) as u16, (rect.y + rect.h / 2) as u16)
}

#[test]
fn a_tap_on_a_band_is_that_band_of_the_list() {
    let mut fixture = on_network();
    fixture.state.network.aps = Some(ApList::from_ranked(&[
        ap(b"neighbour", -55),
        ap_with(b"cafe", -60, 7),
    ]));
    let first = layout::LIST_TOP as u16;
    assert_eq!(
        fixture.ask(tap_at(30, first)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Choose(ap(b"neighbour", -55))),
        "the list's first band is the first network"
    );
    assert_eq!(
        fixture.ask(tap_at(30, first + layout::LIST_ROW_H as u16)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Choose(ap_with(b"cafe", -60, 7))),
        "a band down is the next network"
    );
    assert_eq!(
        fixture.ask(tap_at(30, first + layout::LIST_ROW_H as u16 - 1)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Choose(ap(b"neighbour", -55))),
        "and the pixels below a band's glyphs still belong to that band"
    );
    assert_eq!(
        fixture.ask(tap_at(30, 5)),
        BusinessIntent::Invalid,
        "while the page's own header is not a band"
    );
}

#[test]
fn a_slide_moves_the_list_and_only_while_the_keyboard_is_down() {
    let fixture = on_network();
    assert_eq!(
        fixture.ask(swipe(SwipeDirection::Up)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Scroll { rows: 1 }),
        "a finger pushing the list up walks down it"
    );
    assert_eq!(
        fixture.ask(swipe(SwipeDirection::Down)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Scroll { rows: -1 })
    );
    assert_eq!(
        fixture.ask(swipe(SwipeDirection::Left)),
        BusinessIntent::Invalid,
        "the list is one column, so there is nothing to slide sideways to"
    );
    assert_eq!(
        typing().ask(swipe(SwipeDirection::Up)),
        BusinessIntent::Invalid
    );
}

#[test]
fn the_strip_is_the_page_s_own_control_and_the_bands_stop_above_it() {
    let mut fixture = on_network();
    fixture.state.network.aps = Some(ApList::from_ranked(&[
        ap(b"neighbour", -55),
        ap_with(b"cafe", -60, 7),
        ap_with(b"bakery", -65, 8),
    ]));
    let strip = layout::toolbar_box();
    for (x, _cell) in [
        (0, layout::ToolbarCell::Left),
        (strip.w / 2 - 1, layout::ToolbarCell::Left),
        (strip.w / 2, layout::ToolbarCell::Right),
        (strip.w - 1, layout::ToolbarCell::Right),
    ] {
        for y in [strip.y, strip.y + strip.h / 2, strip.y + strip.h - 1] {
            assert_eq!(
                fixture.ask(tap_at(x as u16, y as u16)),
                BusinessIntent::NetworkPage(NetworkPageIntent::ScanNow),
                "({x}, {y}) is the strip"
            );
        }
    }
    assert_eq!(
        fixture.ask(tap_at(160, (layout::TOOLBAR_TOP - 1) as u16)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Choose(ap_with(b"bakery", -65, 8))),
        "the row above the strip is still the list's own last band"
    );
}

#[test]
fn the_button_is_the_strip_verb_and_the_setup_box_asks_about_its_passphrase() {
    assert_eq!(
        on_network().ask(InputEvent::Button(ButtonEvent::Click)),
        BusinessIntent::NetworkPage(NetworkPageIntent::ScanNow)
    );

    let mut announcing = on_network();
    announcing.state.network.link = LinkStatus {
        mode: iot_core::drivers::network::NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable: false,
        },
        setup_password: Some(iot_core::drivers::network::SetupPass::from_array(
            *b"k3mq9t7w2x",
        )),
        ..Default::default()
    };
    let keep = layout::dialog_answer_box(layout::DialogChoice::Keep);
    let act = layout::dialog_answer_box(layout::DialogChoice::Act);
    assert_eq!(
        announcing.ask(tap_at(keep.x as u16 + 1, keep.y as u16 + 1)),
        BusinessIntent::NetworkPage(NetworkPageIntent::SetOpenAccessPoint(true))
    );
    assert_eq!(
        announcing.ask(tap_at(act.x as u16 + 1, act.y as u16 + 1)),
        BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost)
    );
    assert_eq!(announcing.ask(tap_at(10, 10)), BusinessIntent::Invalid);
    assert_eq!(
        announcing.ask(long_press_at(10, 10)),
        BusinessIntent::Invalid
    );
}

#[test]
fn a_hold_on_a_band_is_that_band_asked_about() {
    let mut fixture = on_network();
    fixture.state.network.aps = Some(ApList::from_ranked(&[
        ap(b"neighbour", -55),
        ap_with(b"cafe", -60, 7),
    ]));
    let first = layout::LIST_TOP as u16;
    assert_eq!(
        fixture.ask(long_press_at(30, first)),
        BusinessIntent::NetworkPage(NetworkPageIntent::AskForget(ap(b"neighbour", -55)))
    );
    assert_eq!(
        fixture.ask(long_press_at(30, first + layout::LIST_ROW_H as u16)),
        BusinessIntent::NetworkPage(NetworkPageIntent::AskForget(ap_with(b"cafe", -60, 7)))
    );
    assert_eq!(
        fixture.ask(long_press_at(30, 5)),
        BusinessIntent::Invalid,
        "the header is not a row to ask about"
    );
    assert_eq!(
        fixture.ask(long_press_at(160, layout::TOOLBAR_TOP as u16)),
        BusinessIntent::Invalid,
        "and neither is the strip"
    );
    assert_eq!(
        typing().ask(long_press_at(30, first)),
        BusinessIntent::Invalid,
        "while the keyboard is up there is no list under the finger to hold"
    );
}

#[test]
fn a_question_is_answered_by_where_a_finger_lands_and_by_nothing_else() {
    let fixture = asking();
    let act = layout::dialog_answer_box(layout::DialogChoice::Act);
    let keep = layout::dialog_answer_box(layout::DialogChoice::Keep);
    assert_eq!(
        fixture.ask(tap_at(
            (act.x + act.w / 2) as u16,
            (act.y + act.h / 2) as u16
        )),
        BusinessIntent::NetworkPage(NetworkPageIntent::Answer(Confirm::Forget(ap(
            b"neighbour",
            -55
        )))),
        "the action's own button is the whole of the yes"
    );
    assert_eq!(
        fixture.ask(tap_at(
            (keep.x + keep.w / 2) as u16,
            (keep.y + keep.h / 2) as u16
        )),
        BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss),
        "and the button that says no says no"
    );
    for (x, y) in [
        (30, 5),
        (30, layout::LIST_TOP as u16 + 2),
        (160, layout::TOOLBAR_TOP as u16 + 2),
        (
            (layout::DIALOG_X + 2) as u16,
            (layout::DIALOG_TOP + 2) as u16,
        ),
    ] {
        assert_eq!(
            fixture.ask(tap_at(x, y)),
            BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss),
            "({x}, {y}) leaves the question undone"
        );
    }
    assert_eq!(
        fixture.ask(swipe(SwipeDirection::Up)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss),
        "a swipe under the box does not slide a list it cannot see"
    );
    assert_eq!(
        fixture.ask(long_press_at(30, layout::LIST_TOP as u16 + 2)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss)
    );
    assert_eq!(
        fixture.ask(tap_at(160, layout::TOOLBAR_TOP as u16 + 2)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss)
    );
}

#[test]
fn a_tap_off_the_page_s_own_content_is_nothing() {
    assert_eq!(
        on_network().ask(tap_at(30, 5)),
        BusinessIntent::Invalid,
        "the page's header is not a control and not a colour walk"
    );
    assert_eq!(
        typing().ask(tap_at(30, 5)),
        BusinessIntent::Invalid,
        "and the keyboard's own header is not a key"
    );
}

#[test]
fn a_tap_while_the_keyboard_is_up_types_the_key_under_it() {
    let fixture = typing();
    assert_eq!(
        fixture.ask(key_tap(1)),
        BusinessIntent::NetworkPage(NetworkPageIntent::Key {
            key: Key::Char(b'2'),
            at_ms: 1_000,
        }),
        "the phone layer's second key is the one printed 2"
    );
    assert_eq!(
        fixture.ask(key_tap(
            layout::cell_of(Key::Shift).expect("a control's own cell")
        )),
        BusinessIntent::NetworkPage(NetworkPageIntent::Key {
            key: Key::Shift,
            at_ms: 1_000,
        })
    );
    assert_eq!(
        fixture.ask(key_tap(
            layout::cell_of(Key::Cancel).expect("a control's own cell")
        )),
        BusinessIntent::NetworkPage(NetworkPageIntent::Key {
            key: Key::Cancel,
            at_ms: 1_000,
        })
    );
    assert_eq!(
        fixture.ask(key_tap(
            layout::cell_of(Key::Reveal).expect("a control's own cell")
        )),
        BusinessIntent::NetworkPage(NetworkPageIntent::Key {
            key: Key::Reveal,
            at_ms: 1_000,
        })
    );
    assert_eq!(fixture.ask(tap_at(30, 5)), BusinessIntent::Invalid);
    assert_eq!(
        fixture.ask(tap_at(
            30,
            (layout::KEYBOARD_TOP + layout::ROWS * layout::KEY_H) as u16
        )),
        BusinessIntent::Invalid
    );
}

#[test]
fn the_clock_at_the_lift_is_what_the_key_carries() {
    assert_eq!(
        typing().ask(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: layout::KEY_W as u16 / 2,
            y: (layout::KEYBOARD_TOP + layout::KEY_H / 2) as u16,
            held_ms: 40,
            at_ms: 4_321,
        })),
        BusinessIntent::NetworkPage(NetworkPageIntent::Key {
            key: Key::Char(b'1'),
            at_ms: 4_321,
        })
    );
}

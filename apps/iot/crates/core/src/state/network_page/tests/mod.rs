use super::*;
use crate::drivers::input::FINGER_LONG_PRESS;
use crate::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::network::{ApList, Credentials, PASSWORD_BYTES, SetupHost};
use crate::drivers::playback::Sound;
use crate::intent::*;
use crate::keyboard::MULTITAP_WINDOW_MS;
use crate::keyboard::{self, Key, Layer};
use crate::state::test_support::*;
use crate::state::{Boot, DeviceManager, DisplayPage};

fn ap_named(ssid: &[u8], rssi: i8, secured: bool) -> AccessPoint {
    let first = ssid.first().copied().unwrap_or(0);
    AccessPoint {
        ssid: Ssid::from_octets(ssid),
        bssid: [first, 2, 3, 4, 5, ssid.len() as u8],
        channel: 6,
        rssi,
        secured,
    }
}

fn ap(ssid: &[u8]) -> AccessPoint {
    ap_named(ssid, -55, true)
}

fn open(ssid: &[u8]) -> AccessPoint {
    ap_named(ssid, -55, false)
}

fn marked(manager: &mut DeviceManager, names: &[&str]) {
    let mut networks = SavedNetworks::empty();
    for name in names {
        let credentials =
            Credentials::from_text(name, "latte").expect("a credential the radio takes");
        networks.remember(&credentials);
    }
    let Some(mut list) = manager.state().network.aps else {
        return;
    };
    list.mark_saved(networks.mask(&list));
    manager.apply_scan(list);
}

fn booted(boot: Boot) -> DeviceManager {
    let mut manager = landed(DeviceManager::boot(boot));
    manager.apply_scan(ApList::from_ranked(&[ap(b"neighbour"), ap(b"cafe")]));
    for _ in 0..CYCLE.len() {
        if manager.state().page() == DisplayPage::Network {
            break;
        }
        manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
    }
    assert_eq!(manager.state().page(), DisplayPage::Network);
    manager
}

fn on_network_page() -> DeviceManager {
    booted(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    })
}

fn tap_on_at(manager: &mut DeviceManager, x: u16, y: u16, at_ms: u32) -> Option<NetworkCommand> {
    let operation = op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x,
        y,
        held_ms: 40,
        at_ms,
    }));
    manager.apply_input(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_action(business)
}

fn tap_on(manager: &mut DeviceManager, x: u16, y: u16) -> Option<NetworkCommand> {
    tap_on_at(manager, x, y, 40)
}

fn tap_band(manager: &mut DeviceManager, band: usize) -> Option<NetworkCommand> {
    let y = (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16;
    tap_on(manager, 30, y)
}

fn key_at(manager: &mut DeviceManager, key: Key, at_ms: u32) -> Option<NetworkCommand> {
    manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Key {
        key,
        at_ms,
    }))
}

fn key(manager: &mut DeviceManager, key: Key) -> Option<NetworkCommand> {
    key_at(manager, key, 40)
}

fn handed_over(command: Option<NetworkCommand>) -> Option<Credentials> {
    match command {
        Some(NetworkCommand::Join(JoinRequest::Typed(credentials))) => Some(credentials),
        Some(NetworkCommand::Join(JoinRequest::Held(_))) => {
            panic!("the page handed over a name where a passphrase was expected")
        }
        _ => None,
    }
}

fn type_letter(manager: &mut DeviceManager, wanted: u8, at_ms: u32) {
    for digit in 0..keyboard::CHARS {
        let candidates = Layer::Abc.candidates(digit);
        let Some(taps) = candidates.iter().position(|candidate| *candidate == wanted) else {
            continue;
        };
        let face = Layer::Abc.face(digit);
        for tap in 0..=taps {
            key_at(manager, Key::Char(face), at_ms + 100 * tap as u32);
        }
        return;
    }
    panic!("{wanted:?} is not a key of the phone layer");
}

fn eight_rows() -> DeviceManager {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    }));
    let aps: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
    manager.apply_scan(ApList::from_ranked(&aps));
    manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
    manager
}

fn slide_list(manager: &mut DeviceManager) -> Option<NetworkCommand> {
    let operation = op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        direction: SwipeDirection::Up,
        x: 30,
        y: (keyboard::LIST_TOP + keyboard::LIST_ROW_H * 2) as u16,
        end_x: 30,
        end_y: keyboard::LIST_TOP as u16,
        held_ms: 40,
        distance_px: (keyboard::LIST_ROW_H * 2) as u16,
    }));
    manager.apply_input(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_action(business)
}

fn tap_strip(manager: &mut DeviceManager) -> Option<NetworkCommand> {
    tap_on(
        manager,
        160,
        (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
    )
}

fn tap_strip_left(manager: &mut DeviceManager) -> Option<NetworkCommand> {
    tap_on(
        manager,
        40,
        (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
    )
}

fn hold_band(manager: &mut DeviceManager, band: usize) -> Option<NetworkCommand> {
    let y = (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16;
    let operation = op(InputEvent::Gesture(GestureEvent::LongPress {
        id: 0,
        x: 30,
        y,
        held_ms: u16::from(FINGER_LONG_PRESS) + 20,
    }));
    manager.apply_input(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_action(business)
}

fn tap_answer(
    manager: &mut DeviceManager,
    choice: keyboard::DialogChoice,
) -> Option<NetworkCommand> {
    let box_ = keyboard::dialog_answer_box(choice);
    tap_on(
        manager,
        (box_.x + box_.w / 2) as u16,
        (box_.y + box_.h / 2) as u16,
    )
}

fn link_up(manager: &mut DeviceManager, ssid: &[u8]) {
    link_at(manager, LinkPhase::Connected, ssid);
}

fn link_at(manager: &mut DeviceManager, phase: LinkPhase, ssid: &[u8]) {
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        phase,
        ap: Some(ap(ssid)),
        address: None,
        ..LinkStatus::default()
    })));
}

fn long_press_the_button(manager: &mut DeviceManager) -> Option<NetworkCommand> {
    let operation = op(InputEvent::Button(ButtonEvent::LongPress));
    manager.apply_input(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_action(business)
}

fn setup_mode(manager: &mut DeviceManager, cancellable: bool) {
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
        mode: NetworkMode::Setup {
            host: SetupHost::AccessPoint,
            cancellable,
        },
        setup_password: Some(crate::drivers::network::SetupPass::from_array(
            *b"k3mq9t7w2x",
        )),
        ..LinkStatus::default()
    })));
}

mod boot;
mod front;
mod join;
mod list;
mod strip;
mod typing;

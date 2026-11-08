//! The page's moves, walked exhaustively rather than along the paths a test happened to pick.
//!
//! The tests above are examples: one path somebody thought of, with the state it expected beside
//! it. That is the wrong shape for a move whose *precondition* an earlier move took away — such a
//! rule can be green for every state its own test walks into while being wrong for a state none of
//! them does. So this walks the reachable states instead: a breadth-first sweep over every move the
//! page has, from the state a booted device is in, with `broken`'s invariants checked on every
//! state a move lands on. The moves that reached a broken state are printed with it, so a failure
//! here is a reproduction rather than a verdict.
//!
//! Two limits of the alphabet decide what the sweep's answer is worth. Every report it sends is one
//! the radio really makes — `Connected` carries an address, `Connecting` may or may not have a name
//! yet, `Off` and `Scanning` carry neither — because a sweep free to invent a report would find
//! states the device cannot be in and call them reachable. And states are compared through `key`:
//! a field left out that a move reads would merge two states the page tells apart.

use super::*;
use crate::pages::network::layout::{self, DialogChoice};
use crate::pages::network::model::SetupWait;
use crate::shared::intent::Confirm;
use crate::shared::keyboard::Key;
use alloc::collections::{BTreeSet, VecDeque};
use alloc::string::String;
use iot_core::drivers::network::{NetworkMode, SetupHost, SetupPass};

const LIST_X: u16 = 30;

const SWEEP_CAP: usize = 20_000;

fn key(state: &DeviceState) -> Vec<u8> {
    let mut key = Vec::with_capacity(64);
    key.push(state.nav.entries().len() as u8);
    for page in state.nav.entries() {
        key.push(page.code());
    }
    match state.network.link.mode {
        NetworkMode::Link => key.push(0),
        NetworkMode::Browsing => key.push(1),
        NetworkMode::Setup { host, cancellable } => {
            key.push(2);
            key.push(match host {
                SetupHost::Panel => 0,
                SetupHost::AccessPoint => 1,
            });
            key.push(u8::from(cancellable));
            key.push(u8::from(state.network.link.setup_password.is_some()));
        }
    }
    key.push(u8::from(state.network.link.is_leavable()));
    encode_ap(&mut key, state.network.link.ap);
    key.push(u8::from(state.network.link.address.is_some()));
    match state.network.aps {
        None => key.push(0),
        Some(list) => {
            key.push(1);
            key.push(list.len() as u8);
            for index in 0..list.len() {
                let Some(ap) = list.get(index) else {
                    break;
                };
                encode_ap(&mut key, Some(ap));
                key.push(u8::from(list.is_saved(index)));
            }
        }
    }
    encode_ap(&mut key, state.network_page.selected);
    key.push(u8::from(state.network_page.entry.is_some()));
    key.push(u8::from(state.network_page.keyboard));
    key.push(u8::from(state.network_page.join_wait));
    key.push(state.network_page.list_top);
    match state.network_page.setup_wait {
        None => key.push(0),
        Some(SetupWait::Host(SetupHost::Panel)) => key.push(1),
        Some(SetupWait::Host(SetupHost::AccessPoint)) => key.push(2),
        Some(SetupWait::Open(open)) => {
            key.push(3);
            key.push(u8::from(open));
        }
    }
    match state.network_page.confirm {
        None => key.push(0),
        Some(Confirm::Leave) => key.push(1),
        Some(Confirm::Forget(ap)) => {
            key.push(2);
            encode_ap(&mut key, Some(ap));
        }
    }
    key
}

fn encode_ap(bytes: &mut Vec<u8>, ap: Option<AccessPoint>) {
    match ap {
        None => bytes.push(0),
        Some(ap) => {
            bytes.push(1);
            bytes.extend_from_slice(&ap.bssid);
            bytes.push(u8::from(ap.secured));
        }
    }
}

// The invariants the page's own drawing and hit rules rest on, read off a state some move
// reached: `None` when they all hold.
//
// Each one is the negation of a state that draws nothing, offers nothing, or offers
// something that is not there — the failures this page has no way to be loud about, because
// what breaks is what the panel puts on the glass.
fn broken(state: &DeviceState) -> Option<&'static str> {
    if state.network_page.keyboard && state.network_page.entry.is_none() {
        return Some(
            "the keyboard is up with no draft under it. The keys are drawn from the draft \
                 and the strip is hidden with them, so the page draws nothing between the \
                 header and the bottom edge, and no control on it can be reached",
        );
    }
    if state.network_page.entry.is_some() && state.network_page.selected.is_none() {
        return Some(
            "a draft is kept for no row, so nothing names the network it would be handed \
                 to",
        );
    }
    if state.network_page.confirm == Some(Confirm::Leave) && !state.network.link.is_leavable() {
        return Some(
            "the page is asking about leaving a link that is not up, so the box has no \
                 answer left to take",
        );
    }
    if state.network_page.rescan_wait && state.network.link.is_leavable() {
        return Some("the strip is waiting for a scan round under a link it would offer to leave");
    }
    if state.network_page.join_wait && state.network_page.keyboard {
        return Some(
            "a join is already with the radio while the keys are still up, so the keyboard's \
                 OK would ask for a second one over the first",
        );
    }
    if matches!(state.network_page.setup_wait, Some(SetupWait::Open(_)))
        && !state.network.link.mode.is_setup()
    {
        return Some(
            "a passphrase change is waiting outside setup mode, so no report the radio can \
                 send will ever clear it",
        );
    }
    None
}

fn path(origin: &[(Option<usize>, &'static str)], at: usize, this: &str) -> String {
    let mut names = Vec::new();
    let mut here = Some(at);
    while let Some(index) = here {
        let (parent, name) = origin[index];
        names.push(name);
        here = parent;
    }
    names.reverse();
    let mut text = String::new();
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(name);
    }
    if !text.is_empty() {
        text.push_str(", ");
    }
    text.push_str(this);
    text
}

fn band_y(band: usize) -> u16 {
    (layout::LIST_TOP + band * layout::LIST_ROW_H + layout::LIST_ROW_H / 2) as u16
}

fn applied(manager: &mut DeviceManager, event: InputEvent) -> Option<Command> {
    let operation = op(event);
    manager.apply_input(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_action(business)
}

fn tap_at(manager: &mut DeviceManager, x: u16, y: u16) -> Option<Command> {
    applied(
        manager,
        InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x,
            y,
            held_ms: 40,
            at_ms: 40,
        }),
    )
}

fn hold_at(manager: &mut DeviceManager, x: u16, y: u16) -> Option<Command> {
    applied(
        manager,
        InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x,
            y,
            held_ms: 500,
        }),
    )
}

fn swipe(manager: &mut DeviceManager, direction: SwipeDirection, end_y: u16) -> Option<Command> {
    let y = band_y(1);
    applied(
        manager,
        InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction,
            x: LIST_X,
            y,
            end_x: LIST_X,
            end_y,
            held_ms: 40,
            distance_px: 60,
        }),
    )
}

fn tap_key(manager: &mut DeviceManager, key: Key) -> Option<Command> {
    let cell = layout::cell_of(key)
        .or_else(|| (0..layout::KEYS).find(|cell| layout::key_of(Layer::Abc, *cell) == Some(key)))
        .expect("the grid holds every key");
    let rect = layout::key_box(cell).expect("a cell of the grid");
    tap_at(
        manager,
        (rect.x + rect.w / 2) as u16,
        (rect.y + rect.h / 2) as u16,
    )
}

fn tap_answer(manager: &mut DeviceManager, choice: DialogChoice) -> Option<Command> {
    let choice = layout::dialog_answer_box(choice);
    tap_at(
        manager,
        (choice.x + choice.w / 2) as u16,
        (choice.y + choice.h / 2) as u16,
    )
}

fn ap_named(ssid: &[u8], last: u8) -> AccessPoint {
    AccessPoint {
        ssid: Ssid::from_octets(ssid),
        bssid: [0x02, 0, 0, 0, 0, last],
        channel: 6,
        rssi: -55,
        secured: true,
    }
}

fn neighbour() -> AccessPoint {
    ap_named(b"neighbour", 1)
}

fn cafe() -> AccessPoint {
    ap_named(b"cafe", 2)
}

fn link_to(manager: &mut DeviceManager, status: LinkStatus) -> Option<Command> {
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(status)))
}

fn scan_of(manager: &mut DeviceManager, rows: &[AccessPoint]) {
    manager.apply_scan(ApList::from_ranked(rows));
}

fn tap_first_band(manager: &mut DeviceManager) -> Option<Command> {
    tap_at(manager, LIST_X, band_y(0))
}
fn tap_second_band(manager: &mut DeviceManager) -> Option<Command> {
    tap_at(manager, LIST_X, band_y(1))
}
fn tap_third_band(manager: &mut DeviceManager) -> Option<Command> {
    tap_at(manager, LIST_X, band_y(2))
}
fn tap_the_strip(manager: &mut DeviceManager) -> Option<Command> {
    tap_at(
        manager,
        160,
        (layout::TOOLBAR_TOP + layout::TOOLBAR_H / 2) as u16,
    )
}
fn tap_the_header(manager: &mut DeviceManager) -> Option<Command> {
    tap_at(manager, 2, 2)
}
fn tap_keep(manager: &mut DeviceManager) -> Option<Command> {
    tap_answer(manager, DialogChoice::Keep)
}
fn tap_the_action(manager: &mut DeviceManager) -> Option<Command> {
    tap_answer(manager, DialogChoice::Act)
}
fn hold_first_band(manager: &mut DeviceManager) -> Option<Command> {
    hold_at(manager, LIST_X, band_y(0))
}
fn hold_second_band(manager: &mut DeviceManager) -> Option<Command> {
    hold_at(manager, LIST_X, band_y(1))
}
fn slide_down(manager: &mut DeviceManager) -> Option<Command> {
    swipe(manager, SwipeDirection::Down, band_y(2))
}
fn slide_up(manager: &mut DeviceManager) -> Option<Command> {
    swipe(manager, SwipeDirection::Up, 0)
}
fn tap_a_character(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Char(Layer::Abc.face(0)))
}
fn tap_shift(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Shift)
}
fn tap_backspace(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Backspace)
}
fn tap_layer(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Layer)
}
fn tap_reveal(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Reveal)
}
fn tap_connect(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Connect)
}
fn tap_cancel(manager: &mut DeviceManager) -> Option<Command> {
    tap_key(manager, Key::Cancel)
}
fn triple_click(manager: &mut DeviceManager) -> Option<Command> {
    applied(manager, InputEvent::Button(ButtonEvent::TripleClick))
}
fn splash_over(manager: &mut DeviceManager) -> Option<Command> {
    manager.apply_action(ActionIntent::Page(PageIntent::SplashOver))
}
fn click(manager: &mut DeviceManager) -> Option<Command> {
    applied(manager, InputEvent::Button(ButtonEvent::Click))
}
fn hold_the_button(manager: &mut DeviceManager) -> Option<Command> {
    applied(manager, InputEvent::Button(ButtonEvent::LongPress))
}

fn nothing_on_the_air(manager: &mut DeviceManager) -> Option<Command> {
    link_to(manager, LinkStatus::default())
}
fn looking_around(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            phase: LinkPhase::Scanning,
            ..LinkStatus::default()
        },
    )
}
fn attempt_without_a_name(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            phase: LinkPhase::Connecting,
            ap: None,
            address: None,
            ..LinkStatus::default()
        },
    )
}
fn associated(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            phase: LinkPhase::Connecting,
            ap: Some(neighbour()),
            address: None,
            ..LinkStatus::default()
        },
    )
}
fn connected(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            phase: LinkPhase::Connected,
            ap: Some(neighbour()),
            address: Some(Ipv4::from_octets([10, 0, 0, 2])),
            ..LinkStatus::default()
        },
    )
}
fn refused(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ap: Some(neighbour()),
            address: None,
            ..LinkStatus::default()
        },
    )
}
fn the_driver_refused(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Driver),
            ap: None,
            address: None,
            ..LinkStatus::default()
        },
    )
}
fn setup_on_the_panel(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::Panel,
                cancellable: false,
            },
            ..LinkStatus::default()
        },
    )
}
fn setup_access_point(manager: &mut DeviceManager) -> Option<Command> {
    link_to(
        manager,
        LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::AccessPoint,
                cancellable: false,
            },
            setup_password: Some(SetupPass::from_array(*b"k3mq9t7w2x")),
            ..LinkStatus::default()
        },
    )
}

fn scan_two(manager: &mut DeviceManager) -> Option<Command> {
    scan_of(manager, &[neighbour(), cafe()]);
    None
}
fn scan_one_open(manager: &mut DeviceManager) -> Option<Command> {
    scan_of(
        manager,
        &[AccessPoint {
            secured: false,
            ..neighbour()
        }],
    );
    None
}
fn scan_with_a_saved_row(manager: &mut DeviceManager) -> Option<Command> {
    let mut list = ApList::from_ranked(&[neighbour(), cafe()]);
    let mut store = SavedNetworks::empty();
    let credentials =
        Credentials::from_text("neighbour", "latte").expect("a credential the radio takes");
    store.remember(&credentials);
    let marks = store.mask(&list);
    list.mark_saved(marks);
    manager.apply_scan(list);
    None
}

/// The task on the other end of a command: what it does with one, so the sweep walks into
/// the state its work leaves behind.
fn carry_out(manager: &mut DeviceManager, command: NetworkCommand) {
    match command {
        NetworkCommand::Join(JoinRequest::Typed(_)) => {
            attempt_without_a_name(manager);
        }
        NetworkCommand::Join(JoinRequest::Held(ssid)) => {
            if manager.state().network.aps.is_some_and(|list| {
                (0..list.len()).any(|index| list.get(index).is_some_and(|ap| ap.ssid == ssid))
            }) {
                associated(manager);
            } else {
                manager.apply_action(ActionIntent::NetworkDraft(NetworkPageIntent::NoCopy(ssid)));
            }
        }
        NetworkCommand::Leave => {
            nothing_on_the_air(manager);
        }
        NetworkCommand::ScanNow => {
            looking_around(manager);
            scan_two(manager);
        }
        NetworkCommand::Forget(_) => {
            let mut list = manager.state().network.aps.unwrap_or_else(ApList::empty);
            list.mark_saved(0);
            manager.apply_scan(list);
        }
        NetworkCommand::LeaveSetup { .. }
        | NetworkCommand::SwitchHost { .. }
        | NetworkCommand::SetupOpen { .. }
        | NetworkCommand::SplashOver { .. } => {}
    }
}

struct Move {
    name: &'static str,
    run: fn(&mut DeviceManager) -> Option<Command>,
}

fn alphabet() -> Vec<Move> {
    alloc::vec![
        Move {
            name: "a tap on the first band",
            run: tap_first_band
        },
        Move {
            name: "a tap on the second band",
            run: tap_second_band
        },
        Move {
            name: "a tap on the third band",
            run: tap_third_band
        },
        Move {
            name: "a tap on the strip",
            run: tap_the_strip
        },
        Move {
            name: "a tap on the header",
            run: tap_the_header
        },
        Move {
            name: "a tap on the box's KEEP",
            run: tap_keep
        },
        Move {
            name: "a tap on the box's action",
            run: tap_the_action
        },
        Move {
            name: "a hold on the first band",
            run: hold_first_band
        },
        Move {
            name: "a hold on the second band",
            run: hold_second_band
        },
        Move {
            name: "a slide down the list",
            run: slide_down
        },
        Move {
            name: "a slide up the list",
            run: slide_up
        },
        Move {
            name: "a tap on a character key",
            run: tap_a_character
        },
        Move {
            name: "a tap on shift",
            run: tap_shift
        },
        Move {
            name: "a tap on backspace",
            run: tap_backspace
        },
        Move {
            name: "a tap on the layer key",
            run: tap_layer
        },
        Move {
            name: "a tap on reveal",
            run: tap_reveal
        },
        Move {
            name: "a tap on connect",
            run: tap_connect
        },
        Move {
            name: "a tap on cancel",
            run: tap_cancel
        },
        Move {
            name: "a triple click",
            run: triple_click
        },
        Move {
            name: "a click",
            run: click
        },
        Move {
            name: "a long press of the button",
            run: hold_the_button
        },
        Move {
            name: "the splash's clock",
            run: splash_over
        },
        Move {
            name: "the radio reporting nothing on the air",
            run: nothing_on_the_air
        },
        Move {
            name: "the radio looking around",
            run: looking_around
        },
        Move {
            name: "an attempt with no name yet",
            run: attempt_without_a_name
        },
        Move {
            name: "the radio associating",
            run: associated
        },
        Move {
            name: "the radio connected",
            run: connected
        },
        Move {
            name: "the credential refused",
            run: refused
        },
        Move {
            name: "the driver refusing",
            run: the_driver_refused
        },
        Move {
            name: "a scan of two networks",
            run: scan_two
        },
        Move {
            name: "a scan of an open network",
            run: scan_one_open
        },
        Move {
            name: "a scan with a saved row",
            run: scan_with_a_saved_row
        },
        Move {
            name: "the setup mode offered on the panel",
            run: setup_on_the_panel
        },
        Move {
            name: "the setup access point up",
            run: setup_access_point
        },
    ]
}

#[test]
fn every_state_the_page_can_reach_keeps_its_invariants() {
    let moves = alphabet();
    let start = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    });
    assert_eq!(
        broken(&start.state()),
        None,
        "a booted device is already in a state the page cannot draw"
    );

    let mut states: Vec<DeviceManager> = alloc::vec![start];
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    seen.insert(key(&states[0].state()));
    let mut queue = VecDeque::from([0usize]);
    let mut origin: Vec<(Option<usize>, &'static str)> = alloc::vec![(None, "the device at boot")];
    let mut walked = 0usize;
    // Every request the page made, so the sweep says which ones the task was ever handed.
    let mut asked: Vec<NetworkCommand> = Vec::new();

    while let Some(here) = queue.pop_front() {
        walked += 1;
        assert!(
            walked <= SWEEP_CAP,
            "the sweep reached {SWEEP_CAP} states without settling: raise the cap, or cut \
                 the alphabet down to what the page really reads"
        );
        for step in &moves {
            let mut next = states[here].clone();
            let command = (step.run)(&mut next);
            // A move that asks the task for something leaves two states behind: the page
            // waiting, and the page after the task answered. The command itself is not
            // state, so the sweep carries it out here.
            let mut outcomes = alloc::vec![next];
            if let Some(command) = command {
                let Command::Network(command) = command;
                let mut answered = outcomes[0].clone();
                carry_out(&mut answered, command.clone());
                outcomes.push(answered);
                asked.push(command);
            }
            for next in outcomes {
                let state = next.state();
                if let Some(why) = broken(&state) {
                    panic!(
                        "the page can reach a state where {why}\nreached by {}",
                        path(&origin, here, step.name),
                    );
                }
                if seen.insert(key(&state)) {
                    states.push(next);
                    origin.push((Some(here), step.name));
                    queue.push_back(states.len() - 1);
                }
            }
        }
    }

    let reached =
        |wanted: fn(&DeviceState) -> bool| states.iter().any(|manager| wanted(&manager.state()));
    assert!(
        reached(|state| state.network_page.keyboard && state.network_page.entry.is_some()),
        "the keyboard never came up over a draft"
    );
    assert!(
        reached(|state| state.network_page.join_wait),
        "a join was never held while the radio answered"
    );
    assert!(
        reached(|state| state.network_page.setup_wait.is_some()),
        "a setup tap was never left waiting for the radio"
    );
    assert!(
        reached(|state| state.network_page.selected.is_some() && state.network_page.entry.is_none()),
        "a row was never left picked with no draft: the state a link that came up leaves \
             behind, and the one the pick after it reads"
    );
    assert!(
        reached(|state| state.network_page.confirm == Some(Confirm::Leave)),
        "the strip never asked about leaving"
    );
    assert!(
        reached(|state| matches!(state.network_page.confirm, Some(Confirm::Forget(_)))),
        "a hold never asked about a saved row"
    );
    for (wanted, seen) in [
        (
            "a leave",
            asked
                .iter()
                .any(|command| matches!(command, NetworkCommand::Leave)),
        ),
        (
            "a typed credential",
            asked
                .iter()
                .any(|command| matches!(command, NetworkCommand::Join(JoinRequest::Typed(_)))),
        ),
        (
            "a saved row asked for by name",
            asked
                .iter()
                .any(|command| matches!(command, NetworkCommand::Join(JoinRequest::Held(_)))),
        ),
        (
            "a scan",
            asked
                .iter()
                .any(|command| matches!(command, NetworkCommand::ScanNow)),
        ),
        (
            "a forget",
            asked
                .iter()
                .any(|command| matches!(command, NetworkCommand::Forget(_))),
        ),
    ] {
        assert!(seen, "the page never asked for {wanted}");
    }
    assert!(
        reached(|state| state.network.link.is_leavable()),
        "the link was never up"
    );
    assert!(
        reached(|state| state.page() != Page::Network),
        "the page cycle was never walked"
    );
}

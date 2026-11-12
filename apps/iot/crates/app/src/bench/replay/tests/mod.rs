//! The walk, exercised end to end.

use super::*;
use crate::shared::intent::Layer;
use crate::shared::intent::{BusinessIntent, LinkIntent, NavigationIntent, NetworkPageIntent};
use crate::state::{Backed, Boot, Command, NetworkCommand};
use iot_core::drivers::network::{
    AccessPoint, ApList, Credentials, LinkPhase, LinkStatus, SavedNetworks,
};

fn ap(name: &str) -> AccessPoint {
    AccessPoint {
        ssid: Ssid::from_octets(name.as_bytes()),
        bssid: [7; 6],
        channel: 11,
        rssi: -50,
        secured: true,
    }
}

fn marked(names: &[&str], saved: &[&str]) -> ApList {
    let rows: Vec<AccessPoint> = names.iter().map(|name| ap(name)).collect();
    let mut list = ApList::from_ranked(&rows);
    let mut store = SavedNetworks::empty();
    for name in saved {
        store.remember(&Credentials::from_text(name, "ab").expect("a credential"));
    }
    list.mark_saved(store.mask(&list));
    list
}

fn showing(names: &[&str]) -> DeviceState {
    let rows: Vec<AccessPoint> = names.iter().map(|name| ap(name)).collect();
    DeviceState {
        nav: crate::state::Navigator::new(Page::Network),
        network: crate::state::NetworkState {
            aps: Some(ApList::from_ranked(&rows)),
            ..Default::default()
        },
        ..DeviceState::default()
    }
}

fn key_center(key: Key) -> Touch {
    let cell = layout::cell_of(key).expect("the grid holds every control");
    let rect = layout::key_box(cell).expect("a cell of the grid");
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

fn point(touch: Touch) -> (u16, u16) {
    match touch {
        Touch::At { x, y, .. } => (x, y),
        Touch::NextPage => panic!("a slide is not a point"),
        Touch::Hold { .. } => panic!("a hold is not a point"),
    }
}

#[test]
fn a_run_taps_the_row_the_named_network_is_on() {
    let state = showing(&["aaa", "bbb", "ccc", "target", "eee"]);
    let mut bench = PanelBench::new();
    let plan = bench.plan(&state, "target", "ab").expect("a run");

    assert_eq!(plan[0], Touch::NextPage);
    assert_eq!(
        plan[1],
        Touch::At {
            x: BAND_X,
            y: band_center(2),
            at_ms: 0
        }
    );
    assert_eq!(layout::list_band_at(band_center(2)), Some(2));
}

#[test]
fn every_row_a_scan_can_carry_is_reachable_by_the_bench() {
    let names = [
        "net0", "net1", "net2", "net3", "net4", "net5", "net6", "net7",
    ];
    let rows: Vec<AccessPoint> = names
        .iter()
        .enumerate()
        .map(|(index, name)| AccessPoint {
            ssid: Ssid::from_octets(name.as_bytes()),
            rssi: -50 - index as i8,
            ..ap("taken")
        })
        .collect();
    let state = DeviceState {
        nav: crate::state::Navigator::new(Page::Network),
        network: crate::state::NetworkState {
            aps: Some(ApList::from_ranked(&rows)),
            ..Default::default()
        },
        ..DeviceState::default()
    };

    for (index, name) in names.iter().enumerate() {
        let mut bench = PanelBench::new();
        let plan = bench
            .plan(&state, name, "ab")
            .unwrap_or_else(|| panic!("row {index} has no run"));
        let slides = plan
            .iter()
            .take_while(|touch| matches!(touch, Touch::NextPage))
            .count();
        let (_, y) = point(plan[slides]);
        assert_eq!(
            layout::list_band_at(y),
            Some(index - slides),
            "row {index} is the band its tap lands on after {slides} slides"
        );
    }
}

#[test]
fn the_row_the_page_keeps_picked_is_the_band_its_own_tap_lands_on() {
    let rows: Vec<AccessPoint> = ["aaa", "bbb", "target", "ddd", "eee"]
        .iter()
        .enumerate()
        .map(|(index, name)| AccessPoint {
            bssid: [7, 0, 0, 0, 0, index as u8 + 1],
            ..ap(name)
        })
        .collect();
    let mut state = DeviceState {
        nav: crate::state::Navigator::new(Page::Network),
        network: crate::state::NetworkState {
            aps: Some(ApList::from_ranked(&rows)),
            ..Default::default()
        },
        ..DeviceState::default()
    };
    assert_eq!(picked_band(&state), None, "nothing is picked yet");

    state.network_page.selected = Some(rows[2]);
    assert_eq!(picked_band(&state), Some(2), "the row is the band it is on");

    state.network_page.list_top = 2;
    assert_eq!(picked_band(&state), Some(0), "with the window slid onto it");

    state.network_page.selected = Some(rows[0]);
    assert_eq!(
        picked_band(&state),
        None,
        "and a row the window has scrolled past is one the bench cannot tap either"
    );
}

#[test]
fn the_walk_asks_before_it_leaves_forgets_and_looks_around() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    });
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    manager.apply_scan(marked(&["target"], &[]));
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::NextPage));
    let picked = manager
        .state()
        .network
        .aps
        .and_then(|list| list.get(0))
        .expect("the row the bench's scan published");
    manager.apply_business(BusinessIntent::NetworkPage(NetworkPageIntent::Choose(
        picked,
    )));
    assert!(
        manager.state_ref().network_page.keyboard,
        "the pick is there to begin with"
    );
    manager.apply_scan(marked(&["target"], &["target"]));
    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
        phase: LinkPhase::Connected,
        ap: Some(ap("target")),
        address: None,
        ..LinkStatus::default()
    })));
    assert!(
        manager.state_ref().network_page.selected.is_some()
            && !manager.state_ref().network_page.keyboard,
        "the link cleared the draft and kept the row it was for"
    );

    let bus = IntentBus::new();
    let mut bench = PanelBench::new();
    bench.answered = true;
    let (mut leaves, mut forgets, mut rounds, mut joins) = (0, 0, 0, 0);
    let mut reopened = 0;
    let mut visited: Vec<Act> = Vec::new();
    for _ in 0..ACT_PASSES * 6 {
        bench.walk(&manager, &bus);
        if visited.last() != Some(&bench.act) {
            visited.push(bench.act);
        }
        let mut asked: Vec<Command> = Vec::new();
        while let Ok(intent) = bus.try_receive() {
            if let Some(command) = manager.apply_intent(intent) {
                asked.push(command);
            }
        }
        for command in asked {
            let Command::Network(command) = command;
            match command {
                NetworkCommand::Leave => {
                    leaves += 1;
                    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(
                        LinkStatus::default(),
                    )));
                }
                NetworkCommand::Forget(_) => {
                    forgets += 1;
                    manager.apply_scan(marked(&["target"], &[]));
                }
                NetworkCommand::Join(iot_core::drivers::network::JoinRequest::Held(ssid)) => {
                    joins += 1;
                    assert_eq!(
                        ssid,
                        Ssid::from_octets(b"target"),
                        "the row the walk tapped"
                    );
                    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(LinkStatus {
                        phase: LinkPhase::Connecting,
                        ap: Some(ap("target")),
                        address: None,
                        ..LinkStatus::default()
                    })));
                }
                NetworkCommand::ScanNow => {
                    rounds += 1;
                    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(
                        LinkStatus::default(),
                    )));
                }
                NetworkCommand::Join(_)
                | NetworkCommand::LeaveSetup { .. }
                | NetworkCommand::SwitchHost { .. }
                | NetworkCommand::SetupOpen { .. }
                | NetworkCommand::SplashOver { .. } => {}
            }
        }
        if bench.act == Act::ReopenTheRowJustLeft && manager.state_ref().network_page.keyboard {
            assert!(
                manager.state_ref().network_page.entry.is_some(),
                "the keyboard came up over no draft, which is a page with nothing on it"
            );
            reopened += 1;
        }
        if bench.act == Act::Done {
            break;
        }
        assert!(
            !manager.state_ref().network.link.is_leavable()
                || !manager.state_ref().network_page.keyboard,
            "a row took a pick while a link was up"
        );
    }

    assert_eq!(bench.act, Act::Done, "the walk reached the end of the page");
    assert_eq!(
        visited,
        alloc::vec![
            Act::TapRowUnderLink,
            Act::AskToLeave,
            Act::KeepTheLink,
            Act::LeaveAsk,
            Act::LeaveGo,
            Act::RejoinTheRowJustLeft,
            Act::AskToLeave,
            Act::KeepTheLink,
            Act::LeaveAsk,
            Act::LeaveGo,
            Act::AskToForget,
            Act::Forget,
            Act::ReopenTheRowJustLeft,
            Act::DropTheKeyboard,
            Act::Rescan,
            Act::Done,
        ],
        "the acts the walk was in, in the order it was in them"
    );
    assert_eq!(
        leaves, 2,
        "KEEP left the link up and the action took it down, twice: the build's own network and \
             the one the saved row brought back"
    );
    assert_eq!(joins, 1, "the saved row was asked for by name, and once");
    assert_eq!(forgets, 1, "the hold's own answer is the one forget");
    assert_eq!(rounds, 1, "and the strip's other verb asked for one round");
    assert!(
        reopened > 0,
        "the row the link was left from never opened an entry for the walk to read"
    );
    let state = manager.state_ref();
    assert!(!state.network.link.is_leavable(), "the link is down");
    assert!(
        !state.network.aps.expect("the list is still up").is_saved(0),
        "and the saved mark is gone"
    );
    assert_eq!(
        state.network_page.confirm, None,
        "with no question left open"
    );
    assert!(
        !state.network_page.rescan_wait,
        "and the strip is not waiting"
    );
}

#[test]
fn a_run_ends_on_the_key_that_hands_the_passphrase_over() {
    let state = showing(&["target"]);
    let mut bench = PanelBench::new();
    let plan = bench.plan(&state, "target", "ab").expect("a run");

    let last = plan.last().copied().expect("a run has taps");
    assert_eq!(point(last), point(key_center(Key::Connect)));
    assert_eq!(
        layout::key_at(Layer::Abc, point(last).0, point(last).1),
        Some(Key::Connect),
        "the last tap is the key that hands the credential over, not a cell beside it"
    );
    for tap in &plan[..plan.len() - 1] {
        let (x, y) = point(*tap);
        assert!(
            layout::key_at(Layer::Abc, x, y).is_some(),
            "the tap at ({x}, {y}) is on a key"
        );
    }
}

#[test]
fn a_row_that_is_not_listed_is_reported_rather_than_silently_skipped() {
    let state = showing(&["aaa"]);
    let mut bench = PanelBench::new();
    assert!(bench.plan(&state, "target", "ab").is_none());
    assert!(bench.missing_reported, "the bench says the row is missing");
    assert!(
        !bench.answered,
        "and it is waiting to be told what the row is"
    );
}

#[test]
fn an_open_network_is_refused_rather_than_typed_into() {
    let mut state = showing(&["target"]);
    state.network.aps = Some(ApList::from_ranked(&[AccessPoint {
        secured: false,
        ..ap("target")
    }]));
    let mut bench = PanelBench::new();
    assert!(bench.plan(&state, "target", "ab").is_none());
    assert!(bench.answered, "there is no run to wait for");
}

#[test]
fn no_run_is_built_before_the_list_is_there() {
    let mut state = DeviceState {
        nav: crate::state::Navigator::new(Page::Network),
        ..DeviceState::default()
    };
    let mut bench = PanelBench::new();
    assert!(bench.plan(&state, "target", "ab").is_none());
    assert!(!bench.missing_reported, "nothing has been looked at yet");
    assert!(!bench.absent_reported, "nothing has been looked at yet");

    state.network.aps = Some(ApList::from_ranked(&[ap("target")]));
    state.network_page.keyboard = true;
    assert!(bench.plan(&state, "target", "ab").is_none());
    assert!(!bench.missing_reported);
}

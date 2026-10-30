//! The panel's own controls, replayed by a bench build that has no finger.

use alloc::vec::Vec;

use iot_core::drivers::input::{FINGER_LONG_PRESS, GestureEvent, InputEvent, SwipeDirection};
use iot_core::drivers::network::{ApList, LinkPhase, Ssid};
use iot_core::intent::{Intent, recognize};
use iot_core::keyboard::{self, DialogChoice, Key, LIST_ROWS};
use iot_core::page::Part;
use iot_core::state::{Confirm, DeviceManager, DeviceState, DisplayPage};

use crate::input::IntentBus;

/// Whether this build replays a credential through the panel rather than being handed one.
pub const BENCH: bool = option_env!("IOT_PANEL_BENCH").is_some();

const BAND_X: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    At { x: u16, y: u16, at_ms: u32 },
    Hold { x: u16, y: u16 },
    NextPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    TapRowUnderLink,
    AskToLeave,
    KeepTheLink,
    LeaveAsk,
    LeaveGo,
    ReopenTheRowJustLeft,
    RejoinTheRowJustLeft,
    DropTheKeyboard,
    AskToForget,
    Forget,
    Rescan,
    Done,
}

const ACT_PASSES: u16 = 120;

pub struct PanelBench {
    script: Option<Vec<Touch>>,
    next: usize,
    answered: bool,
    typed: Option<usize>,
    missing_reported: bool,
    absent_reported: bool,
    typed_parts_read: bool,
    joining_read: bool,
    draftless_read: bool,
    rejoined: bool,
    act: Act,
    sent: bool,
    picked: Option<(bool, bool)>,
    passes: u16,
}

impl PanelBench {
    pub const fn new() -> Self {
        Self {
            script: None,
            next: 0,
            answered: false,
            typed: None,
            missing_reported: false,
            absent_reported: false,
            typed_parts_read: false,
            joining_read: false,
            draftless_read: false,
            rejoined: false,
            act: Act::TapRowUnderLink,
            sent: false,
            picked: None,
            passes: 0,
        }
    }

    pub fn step(&mut self, manager: &DeviceManager, bus: &IntentBus) {
        if !BENCH {
            return;
        }
        let state = manager.state();
        self.read_parts(&state);
        if self.script.is_none() {
            let Some((ssid, psk)) = ssid().zip(psk()) else {
                if state.page == DisplayPage::Network && state.aps.is_some() {
                    self.absent("this build was named no network and no passphrase to replay");
                }
                return;
            };
            let Some(script) = self.script(&state, ssid, psk) else {
                return;
            };
            self.script = Some(script);
        }
        let script = self.script.as_ref().expect("built just above");
        if let Some(touch) = script.get(self.next) {
            if self.next + 1 == script.len() {
                self.typed = Some(state.setup.entry.map_or(0, |entry| entry.len()));
            }
            self.publish(bus, *touch);
            self.next += 1;
            return;
        }
        if self.answered {
            self.walk(manager, bus);
            return;
        }
        match state.link.phase {
            LinkPhase::Connected => {
                let typed = self.typed.unwrap_or(0);
                let wanted = psk().map_or(0, |psk| psk.len());
                self.answered = true;
                if typed == wanted {
                    log::info!(
                        "[BENCH] the panel's own credential is on the air: {typed} characters typed, \
                         {} taps, and the radio has an address",
                        script.len()
                    );
                } else {
                    log::error!(
                        "[BENCH] the radio is up, but the draft was {typed} characters of {wanted} \
                         when OK was pressed: the radio accepted something the keyboard did not type"
                    );
                }
            }
            LinkPhase::Failed(kind) => {
                self.answered = true;
                log::error!(
                    "[BENCH] the radio refused what the panel typed: {kind:?} after {} taps",
                    script.len()
                );
            }
            LinkPhase::Off | LinkPhase::Scanning | LinkPhase::Connecting => {}
        }
    }

    fn read_parts(&mut self, state: &DeviceState) {
        if state.page != DisplayPage::Network {
            return;
        }
        let showing = |part: Part| part.showing(state.setup, state.link);
        if state.setup.keyboard && state.setup.entry.is_none() && !self.draftless_read {
            self.draftless_read = true;
            log::error!(
                "[BENCH] the keyboard is up with no draft under it: the keys are drawn from the \
                 draft and the strip is hidden with them, so the page has nothing to put below its \
                 header and no control left on it"
            );
        }
        if state.setup.keyboard && !self.typed_parts_read {
            self.typed_parts_read = true;
            if showing(Part::Content) && !showing(Part::Controls) && !showing(Part::Overlay) {
                log::info!("[BENCH] a finger is typing, and the strip is not drawn with the keys");
            } else {
                log::error!(
                    "[BENCH] the page under the keyboard is drawn wrong: content {}, strip {}, \
                     box {}",
                    showing(Part::Content),
                    showing(Part::Controls),
                    showing(Part::Overlay),
                );
            }
        }
        if state.link.phase == LinkPhase::Connecting && !self.joining_read {
            self.joining_read = true;
            if showing(Part::Overlay) && !showing(Part::Content) && showing(Part::Controls) {
                log::info!(
                    "[BENCH] an attempt is in flight: the box is over the list and the strip is \
                     still offered"
                );
            } else {
                log::error!(
                    "[BENCH] the radio is joining and the page is not saying so: box {}, list \
                     hidden {}, strip {}",
                    showing(Part::Overlay),
                    !showing(Part::Content),
                    showing(Part::Controls),
                );
            }
        }
    }

    fn script(&mut self, state: &DeviceState, ssid: &str, psk: &str) -> Option<Vec<Touch>> {
        if state.page != DisplayPage::Network || state.setup.keyboard {
            return None;
        }
        let list = state.aps?;
        let wanted = Ssid::from_octets(ssid.as_bytes());
        let Some(index) = list.iter().position(|ap| ap.ssid == wanted) else {
            if !list.is_empty() && !self.missing_reported {
                self.missing_reported = true;
                log::warn!(
                    "[BENCH] the network this build was named is not among the {} listed yet; \
                     a later round may carry it",
                    list.len()
                );
            }
            return None;
        };
        if !list.get(index).is_some_and(|ap| ap.secured) {
            log::error!(
                "[BENCH] the network this build was named asks for no passphrase, so the panel path \
                 this bench exists for is not being exercised"
            );
            self.answered = true;
            return None;
        }
        let script = keyboard::script_for(psk.as_bytes())?;
        let mut touches = Vec::with_capacity(script.len() + 2);
        let slides = index
            .saturating_sub(LIST_ROWS - 1)
            .min(list.len().saturating_sub(LIST_ROWS));
        let band = index - slides;
        for _ in 0..slides {
            touches.push(Touch::NextPage);
        }
        touches.push(Touch::At {
            x: BAND_X,
            y: band_center(band),
            at_ms: 0,
        });
        for tap in &script {
            let rect = keyboard::key_box(tap.cell).expect("a script only taps cells the grid has");
            touches.push(Touch::At {
                x: (rect.x + rect.w / 2) as u16,
                y: (rect.y + rect.h / 2) as u16,
                at_ms: tap.at_ms,
            });
        }
        let ok = keyboard::key_box(keyboard::cell_of(Key::Connect).expect("the grid has connect"))
            .expect("a cell of the grid");
        touches.push(Touch::At {
            x: (ok.x + ok.w / 2) as u16,
            y: (ok.y + ok.h / 2) as u16,
            at_ms: script.last().map_or(0, |tap| tap.at_ms) + 100,
        });
        log::info!(
            "[BENCH] replaying a credential through the panel: {index} is band {band} after \
             {slides} slides, {} taps in all",
            touches.len()
        );
        Some(touches)
    }

    fn walk(&mut self, manager: &DeviceManager, bus: &IntentBus) {
        let state = manager.state();
        self.passes = self.passes.saturating_add(1);
        match self.act {
            Act::TapRowUnderLink => {
                if !self.sent {
                    if !state.link.is_leavable() {
                        return self.unwalkable(&state, "no link is up");
                    }
                    log::info!("[BENCH] tapping band 0 under the link, which must pick nothing");
                    self.picked = Some(a_pick_showing(&state));
                    return self.send(bus, row_point(0));
                }
                if self.picked != Some(a_pick_showing(&state)) {
                    log::error!(
                        "[BENCH] the row under the link took a pick: a link to leave is not a row \
                         to pick"
                    );
                    self.act = Act::Done;
                    return;
                }
                log::info!("[BENCH] the row under the link took no pick, and no keyboard came up");
                self.advance(Act::AskToLeave);
            }
            Act::AskToLeave => {
                if !self.sent {
                    if !state.link.is_leavable() {
                        return self.unwalkable(&state, "no link is up");
                    }
                    log::info!("[BENCH] tapping the strip, which asks to leave the link it is on");
                    return self.send(bus, strip_point());
                }
                if state.setup.confirm == Some(Confirm::Leave) {
                    log::info!("[BENCH] the strip asked about leaving, and nothing is down yet");
                    return self.advance(Act::KeepTheLink);
                }
                self.stop_if_stale(format_args!(
                    "the strip was tapped with a link up and asked nothing"
                ));
            }
            Act::KeepTheLink => {
                if !self.sent {
                    return self.send(bus, answer_point(DialogChoice::Keep));
                }
                if state.setup.confirm.is_none() {
                    if state.link.is_leavable() {
                        log::info!("[BENCH] KEEP left the question undone and the link up");
                    } else {
                        log::error!(
                            "[BENCH] KEEP took the link down, which is what the action is for"
                        );
                    }
                    return self.advance(Act::LeaveAsk);
                }
                self.stop_if_stale(format_args!(
                    "the box's own KEEP did not take the question away, which leaves it saying {:?}",
                    state.setup.confirm,
                ));
            }
            Act::LeaveAsk => {
                if !self.sent {
                    return self.send(bus, strip_point());
                }
                if state.setup.confirm == Some(Confirm::Leave) {
                    log::info!("[BENCH] the strip asked again; the action's own answer is next");
                    return self.advance(Act::LeaveGo);
                }
                self.stop_if_stale(format_args!(
                    "the strip was tapped again with a link up and asked nothing"
                ));
            }
            Act::LeaveGo => {
                if !self.sent {
                    return self.send(bus, answer_point(DialogChoice::Act));
                }
                if !state.link.is_leavable() && state.link.address.is_none() {
                    log::info!("[BENCH] the link is down and the page is back on the list");
                    return self.advance(if self.rejoined {
                        Act::AskToForget
                    } else {
                        Act::RejoinTheRowJustLeft
                    });
                }
                self.stop_if_stale(format_args!(
                    "the action's own tap did not take the link down (it says {:?} with the \
                     address {:?})",
                    state.link.phase, state.link.address,
                ));
            }
            Act::RejoinTheRowJustLeft => {
                let Some(band) = picked_band(&state) else {
                    return self
                        .unwalkable(&state, "the row the link was on is not on a band showing");
                };
                if !self.sent {
                    log::info!(
                        "[BENCH] tapping band {band}, the row the link was just taken off and this \
                         device has the passphrase for"
                    );
                    return self.send(bus, row_point(band));
                }
                if state.link.is_leavable() {
                    if state.setup.keyboard || state.setup.entry.is_some() {
                        log::error!(
                            "[BENCH] a row this device has the passphrase for opened the keyboard: \
                             the copy on the part is what such a row is joined with, and asking for \
                             one is asking the user to type a secret the device already has"
                        );
                        self.act = Act::Done;
                        return;
                    }
                    if saved_band(&state).is_none() {
                        log::error!(
                            "[BENCH] the row that brought the link back is not one this device has \
                             saved, so the copy it joined with came from nowhere"
                        );
                        self.act = Act::Done;
                        return;
                    }
                    log::info!(
                        "[BENCH] the saved row brought the link back up with the copy on the part, \
                         and the page asked for nothing"
                    );
                    self.rejoined = true;
                    return self.advance(Act::AskToLeave);
                }
                self.stop_if_stale(format_args!(
                    "tapping the saved row did not bring the link up (it says {:?} with the address \
                     {:?})",
                    state.link.phase, state.link.address,
                ));
            }
            Act::ReopenTheRowJustLeft => {
                let Some(band) = picked_band(&state) else {
                    return self
                        .unwalkable(&state, "the row the link was on is not on a band showing");
                };
                if !self.sent {
                    log::info!("[BENCH] tapping band {band}, the row the link was just taken off");
                    return self.send(bus, row_point(band));
                }
                match state.setup.entry {
                    None if state.setup.keyboard => {
                        log::error!(
                            "[BENCH] the row the link was left from opened the keyboard over no \
                             draft: the page drew nothing below its header"
                        );
                        self.act = Act::Done;
                    }
                    None => self.stop_if_stale(format_args!(
                        "tapping the row the link was left from opened nothing"
                    )),
                    Some(entry) => {
                        log::info!(
                            "[BENCH] the row the link was left from opens a fresh entry: the \
                             keyboard is up over {} characters, and the draft the link dropped is \
                             not back",
                            entry.len(),
                        );
                        self.advance(Act::DropTheKeyboard);
                    }
                }
            }
            Act::DropTheKeyboard => {
                if !self.sent {
                    return self.send(bus, cancel_point());
                }
                if !state.setup.keyboard && state.setup.selected.is_none() {
                    log::info!("[BENCH] the entry's own cancel puts the page back on the list");
                    return self.advance(Act::Rescan);
                }
                self.stop_if_stale(format_args!(
                    "the keyboard's cancel left the entry where it was: keyboard up {}, row \
                     picked {}",
                    state.setup.keyboard,
                    state.setup.selected.is_some(),
                ));
            }
            Act::AskToForget => {
                let Some(band) = saved_band(&state) else {
                    return self.unwalkable(&state, "no saved row is on one of the bands showing");
                };
                if !self.sent {
                    log::info!("[BENCH] holding band {band}, which asks to forget that network");
                    return self.send(bus, hold_point(band));
                }
                if let Some(Confirm::Forget(ap)) = state.setup.confirm {
                    log::info!(
                        "[BENCH] the hold asked to forget {:?}",
                        ap.ssid.as_display_text().unwrap_or("<unprintable>"),
                    );
                    return self.advance(Act::Forget);
                }
                self.stop_if_stale(format_args!(
                    "the hold asked nothing, which leaves the question {:?}",
                    state.setup.confirm,
                ));
            }
            Act::Forget => {
                if !self.sent {
                    return self.send(bus, answer_point(DialogChoice::Act));
                }
                if saved_band(&state).is_none() && state.setup.confirm.is_none() {
                    log::info!("[BENCH] the saved mark is gone from the row the finger held");
                    return self.advance(Act::ReopenTheRowJustLeft);
                }
                self.stop_if_stale(format_args!(
                    "the action's own tap left the row the finger held marked ({:#010b} over {} \
                     rows) with the question {:?}",
                    marks(&state),
                    state.aps.map_or(0, |list| list.len()),
                    state.setup.confirm,
                ));
            }
            Act::Rescan => {
                if !self.sent {
                    log::info!("[BENCH] tapping the strip with no link, which asks to look around");
                    return self.send(bus, strip_point());
                }
                if !state.setup.rescan_wait {
                    log::info!("[BENCH] the strip's own round came back");
                    return self.advance(Act::Done);
                }
                self.stop_if_stale(format_args!(
                    "the strip asked to look around and no round came back"
                ));
            }
            Act::Done => {}
        }
    }

    fn unwalkable(&mut self, state: &DeviceState, why: &str) {
        log::error!(
            "[BENCH] the page's own controls cannot be walked: {why} (phase {:?})",
            state.link.phase
        );
        self.act = Act::Done;
    }

    fn send(&mut self, bus: &IntentBus, touch: Touch) {
        self.sent = true;
        self.passes = 0;
        self.publish(bus, touch);
    }

    fn advance(&mut self, act: Act) {
        self.act = act;
        self.sent = false;
        self.passes = 0;
    }

    fn stop_if_stale(&mut self, why: core::fmt::Arguments<'_>) {
        if self.passes < ACT_PASSES {
            return;
        }
        log::error!("[BENCH] {why} in {ACT_PASSES} passes; the walk stops here");
        self.act = Act::Done;
    }

    fn absent(&mut self, why: &str) {
        if self.absent_reported {
            return;
        }
        self.absent_reported = true;
        log::error!("[BENCH] {why}");
    }

    fn publish(&self, bus: &IntentBus, touch: Touch) {
        let event = match touch {
            Touch::At { x, y, at_ms } => InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms,
            }),
            Touch::Hold { x, y } => InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x,
                y,
                held_ms: u16::from(FINGER_LONG_PRESS) + 100,
            }),
            Touch::NextPage => InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Up,
                x: BAND_X + 10,
                y: band_center(LIST_ROWS - 1),
                end_x: BAND_X + 10,
                end_y: 0,
                held_ms: 40,
                distance_px: band_center(LIST_ROWS - 1),
            }),
        };
        let _ = bus.try_send(Intent::Operation(recognize(event, 0)));
    }
}

fn strip_point() -> Touch {
    Touch::At {
        x: BAND_X,
        y: (keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H / 2) as u16,
        at_ms: 0,
    }
}

fn answer_point(choice: DialogChoice) -> Touch {
    let rect = keyboard::dialog_answer_box(choice);
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

fn row_point(band: usize) -> Touch {
    Touch::At {
        x: BAND_X,
        y: band_center(band),
        at_ms: 0,
    }
}

fn hold_point(band: usize) -> Touch {
    Touch::Hold {
        x: BAND_X,
        y: band_center(band),
    }
}

fn a_pick_showing(state: &DeviceState) -> (bool, bool) {
    (state.setup.keyboard, state.setup.selected.is_some())
}

fn band_where(state: &DeviceState, matches: impl Fn(&ApList, usize) -> bool) -> Option<usize> {
    let list = state.aps?;
    let top = usize::from(state.setup.list_top);
    (top..top + LIST_ROWS)
        .find(|index| matches(&list, *index))
        .map(|index| index - top)
}

fn saved_band(state: &DeviceState) -> Option<usize> {
    band_where(state, |list, index| list.is_saved(index))
}

fn marks(state: &DeviceState) -> u8 {
    let Some(list) = state.aps else {
        return 0;
    };
    (0..list.len()).fold(0, |marks, index| {
        marks | u8::from(list.is_saved(index)) << index
    })
}

fn picked_band(state: &DeviceState) -> Option<usize> {
    let picked = state.setup.selected?;
    band_where(state, |list, index| {
        list.get(index)
            .is_some_and(|ap| ap.is_same_network(&picked))
    })
}

fn cancel_point() -> Touch {
    let cell = keyboard::cell_of(Key::Cancel).expect("the grid holds cancel");
    let rect = keyboard::key_box(cell).expect("a cell of the grid");
    Touch::At {
        x: (rect.x + rect.w / 2) as u16,
        y: (rect.y + rect.h / 2) as u16,
        at_ms: 0,
    }
}

impl Default for PanelBench {
    fn default() -> Self {
        Self::new()
    }
}

fn band_center(band: usize) -> u16 {
    (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16
}

fn ssid() -> Option<&'static str> {
    option_env!("IOT_WIFI_SSID")
}

fn psk() -> Option<&'static str> {
    option_env!("IOT_WIFI_PSK")
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::network::{
        AccessPoint, ApList, Credentials, LinkPhase, LinkStatus, SavedNetworks,
    };
    use iot_core::intent::{BusinessIntent, translate};
    use iot_core::keyboard::Layer;

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
            page: DisplayPage::Network,
            aps: Some(ApList::from_ranked(&rows)),
            ..DeviceState::default()
        }
    }

    fn key_center(key: Key) -> Touch {
        let cell = keyboard::cell_of(key).expect("the grid holds every control");
        let rect = keyboard::key_box(cell).expect("a cell of the grid");
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
        let script = bench.script(&state, "target", "ab").expect("a run");

        assert_eq!(script[0], Touch::NextPage);
        assert_eq!(
            script[1],
            Touch::At {
                x: BAND_X,
                y: band_center(2),
                at_ms: 0
            }
        );
        assert_eq!(keyboard::list_band_at(band_center(2)), Some(2));
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
            page: DisplayPage::Network,
            aps: Some(ApList::from_ranked(&rows)),
            ..DeviceState::default()
        };

        for (index, name) in names.iter().enumerate() {
            let mut bench = PanelBench::new();
            let script = bench
                .script(&state, name, "ab")
                .unwrap_or_else(|| panic!("row {index} has no run"));
            let slides = script
                .iter()
                .take_while(|touch| matches!(touch, Touch::NextPage))
                .count();
            let (_, y) = point(script[slides]);
            assert_eq!(
                keyboard::list_band_at(y),
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
            page: DisplayPage::Network,
            aps: Some(ApList::from_ranked(&rows)),
            ..DeviceState::default()
        };
        assert_eq!(picked_band(&state), None, "nothing is picked yet");

        state.setup.selected = Some(rows[2]);
        assert_eq!(picked_band(&state), Some(2), "the row is the band it is on");

        state.setup.list_top = 2;
        assert_eq!(picked_band(&state), Some(0), "with the window slid onto it");

        state.setup.selected = Some(rows[0]);
        assert_eq!(
            picked_band(&state),
            None,
            "and a row the window has scrolled past is one the bench cannot tap either"
        );
    }

    #[test]
    fn the_walk_asks_before_it_leaves_forgets_and_looks_around() {
        let mut manager = DeviceManager::new().with_network(true);
        manager.apply_business(BusinessIntent::SplashOver);
        manager.apply_scan(marked(&["target"], &[]));
        manager.apply_business(BusinessIntent::NextPage);
        let picked = manager
            .state()
            .aps
            .and_then(|list| list.get(0))
            .expect("the row the bench's scan published");
        manager.apply_business(BusinessIntent::Choose(picked));
        assert!(
            manager.state().setup.keyboard,
            "the pick is there to begin with"
        );
        manager.apply_scan(marked(&["target"], &["target"]));
        manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Connected,
            ap: Some(ap("target")),
            address: None,
            ..LinkStatus::default()
        }));
        assert!(
            manager.state().setup.selected.is_some() && !manager.state().setup.keyboard,
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
            while let Ok(intent) = bus.try_receive() {
                match intent {
                    Intent::Operation(op) => {
                        manager.apply_operation(op);
                        let business = translate(&op, &manager.state());
                        manager.apply_business(business);
                    }
                    Intent::Business(business) => manager.apply_business(business),
                }
            }
            if manager.take_leave() {
                leaves += 1;
                manager.apply_business(BusinessIntent::LinkReport(LinkStatus::default()));
            }
            if manager.take_forget().is_some() {
                forgets += 1;
                manager.apply_scan(marked(&["target"], &[]));
            }
            if let Some(iot_core::drivers::network::JoinRequest::Held(ssid)) = manager.take_join() {
                joins += 1;
                assert_eq!(
                    ssid,
                    Ssid::from_octets(b"target"),
                    "the row the walk tapped"
                );
                manager.apply_business(BusinessIntent::LinkReport(LinkStatus {
                    phase: LinkPhase::Connecting,
                    ap: Some(ap("target")),
                    address: None,
                    ..LinkStatus::default()
                }));
            }
            if manager.take_scan_now() {
                rounds += 1;
                manager.apply_business(BusinessIntent::LinkReport(LinkStatus::default()));
            }
            if bench.act == Act::ReopenTheRowJustLeft && manager.state().setup.keyboard {
                assert!(
                    manager.state().setup.entry.is_some(),
                    "the keyboard came up over no draft, which is a page with nothing on it"
                );
                reopened += 1;
            }
            if bench.act == Act::Done {
                break;
            }
            assert!(
                !manager.state().link.is_leavable() || !manager.state().setup.keyboard,
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
        let state = manager.state();
        assert!(!state.link.is_leavable(), "the link is down");
        assert!(
            !state.aps.expect("the list is still up").is_saved(0),
            "and the saved mark is gone"
        );
        assert_eq!(state.setup.confirm, None, "with no question left open");
        assert!(!state.setup.rescan_wait, "and the strip is not waiting");
    }

    #[test]
    fn a_run_ends_on_the_key_that_hands_the_passphrase_over() {
        let state = showing(&["target"]);
        let mut bench = PanelBench::new();
        let script = bench.script(&state, "target", "ab").expect("a run");

        let last = script.last().copied().expect("a run has taps");
        assert_eq!(point(last), point(key_center(Key::Connect)));
        assert_eq!(
            keyboard::key_at(Layer::Abc, point(last).0, point(last).1),
            Some(Key::Connect),
            "the last tap is the key that hands the credential over, not a cell beside it"
        );
        for tap in &script[..script.len() - 1] {
            let (x, y) = point(*tap);
            assert!(
                keyboard::key_at(Layer::Abc, x, y).is_some(),
                "the tap at ({x}, {y}) is on a key"
            );
        }
    }

    #[test]
    fn a_row_that_is_not_listed_is_reported_rather_than_silently_skipped() {
        let state = showing(&["aaa"]);
        let mut bench = PanelBench::new();
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(bench.missing_reported, "the bench says the row is missing");
        assert!(
            !bench.answered,
            "and it is waiting to be told what the row is"
        );
    }

    #[test]
    fn an_open_network_is_refused_rather_than_typed_into() {
        let mut state = showing(&["target"]);
        state.aps = Some(ApList::from_ranked(&[AccessPoint {
            secured: false,
            ..ap("target")
        }]));
        let mut bench = PanelBench::new();
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(bench.answered, "there is no run to wait for");
    }

    #[test]
    fn no_run_is_built_before_the_list_is_there() {
        let mut state = DeviceState {
            page: DisplayPage::Network,
            ..DeviceState::default()
        };
        let mut bench = PanelBench::new();
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(!bench.missing_reported, "nothing has been looked at yet");
        assert!(!bench.absent_reported, "nothing has been looked at yet");

        state.aps = Some(ApList::from_ranked(&[ap("target")]));
        state.setup.keyboard = true;
        assert!(bench.script(&state, "target", "ab").is_none());
        assert!(!bench.missing_reported);
    }
}

//! The Network page's own state: the row a passphrase is being typed for, the draft itself,
//! the question the page is asking, and where the list is scrolled. The link is not here —
//! this record reads [`NetworkState`](super::NetworkState) and never writes it, so a report
//! from the worker cannot land in a page field and a page edit cannot change what the radio
//! said.

use crate::drivers::network::{
    AccessPoint, JoinRequest, LinkPhase, LinkStatus, NetworkError, NetworkMode, Ssid,
};
use crate::intent::NetworkPageIntent;
use crate::keyboard::{Key, LIST_ROWS};

use super::entry::Entry;
use super::network::{NetworkCommand, NetworkState};

/// What the Network page is in the middle of: the row a passphrase is being typed for, the
/// draft itself, and the last refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetworkPage {
    /// The scanned row the user picked, whose name the draft goes with.
    pub selected: Option<AccessPoint>,
    /// The draft, and with it what a passphrase is being typed for.
    pub entry: Option<Entry>,
    /// Whether the keyboard is up, which is what the page's two states are.
    pub keyboard: bool,
    /// Whether the draft is drawn as it was typed rather than masked.
    pub revealed: bool,
    /// The last association the radio refused, kept until another attempt starts.
    pub refused: Option<NetworkError>,
    /// The first scan row the list is showing.
    pub list_top: u8,
    /// The confirmation the page is asking, if it is asking one.
    pub confirm: Option<Confirm>,
    /// Whether the action strip is waiting for a scan round it asked for.
    pub rescan_wait: bool,
}

/// The action a confirmation is standing in front of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Leave,
    Forget(AccessPoint),
}

impl Confirm {
    /// The network this question is about, which is the row the page highlights behind it.
    pub fn network(&self, link: LinkStatus) -> Option<AccessPoint> {
        match self {
            Confirm::Leave => link.ap,
            Confirm::Forget(ap) => Some(*ap),
        }
    }
}

impl NetworkPage {
    /// The phase the page should show.
    pub fn phase(&self, link: LinkPhase) -> LinkPhase {
        match (self.refused, link) {
            (Some(kind), LinkPhase::Off | LinkPhase::Scanning) => LinkPhase::Failed(kind),
            (_, link) => link,
        }
    }

    /// The page's intents: every ask the panel makes of the link, answered with the command
    /// the caller hands to the worker. The link is read, never written.
    pub(super) fn apply(
        &mut self,
        intent: NetworkPageIntent,
        link: &NetworkState,
        has_pages: bool,
    ) -> Option<NetworkCommand> {
        match intent {
            NetworkPageIntent::Key { key, at_ms } => self.apply_key(key, at_ms),
            NetworkPageIntent::Choose(ap) => self.pick_row(ap, link),
            NetworkPageIntent::AskForget(ap) => {
                self.hold_row(ap, link);
                None
            }
            NetworkPageIntent::Scroll { rows } => {
                self.scroll(rows, link);
                None
            }
            NetworkPageIntent::NoCopy(ssid) => {
                self.no_copy(ssid);
                None
            }
            NetworkPageIntent::Leave => self.leave_link(link, has_pages),
            NetworkPageIntent::Answer(confirm) => self.answer(confirm, link),
            NetworkPageIntent::CloseSetup => self.close_setup(link),
            NetworkPageIntent::SwitchHost => self.switch_host(link, has_pages),
            NetworkPageIntent::ScanNow => self.scan_now(link),
            NetworkPageIntent::SetOpenAccessPoint(open) => self.set_open(open, link),
            NetworkPageIntent::Dismiss => {
                self.confirm = None;
                None
            }
        }
    }

    /// What one link report does to the page. Only the report decides; the page never asks
    /// the link to change.
    pub(super) fn on_link(&mut self, after: LinkStatus) {
        match after.phase {
            LinkPhase::Failed(kind) => {
                self.refused = Some(kind);
                self.reopen_entry();
            }
            LinkPhase::Connecting | LinkPhase::Connected => self.refused = None,
            LinkPhase::Off | LinkPhase::Scanning => {}
        }
        if after.phase == LinkPhase::Connected {
            self.entry = None;
            self.keyboard = false;
            self.revealed = false;
        }
        if !after.is_leavable() && self.confirm == Some(Confirm::Leave) {
            self.confirm = None;
        }
        self.rescan_wait = false;
    }

    /// A new list can be shorter than the row the page was scrolled to.
    pub(super) fn clamp_top(&mut self, rows: usize) {
        self.list_top = self.list_top.min(Self::last_list_top(rows));
    }

    fn apply_key(&mut self, key: Key, at_ms: u32) -> Option<NetworkCommand> {
        if !self.keyboard {
            return None;
        }
        match key {
            Key::Connect => {
                let (Some(ap), Some(entry)) = (self.selected, self.entry) else {
                    return None;
                };
                self.keyboard = false;
                self.revealed = false;
                Some(NetworkCommand::Join(JoinRequest::Typed(
                    entry.credentials(ap.ssid),
                )))
            }
            Key::Cancel => {
                self.keyboard = false;
                self.revealed = false;
                self.entry = None;
                self.selected = None;
                None
            }
            Key::Reveal => {
                self.revealed = !self.revealed;
                None
            }
            typing => {
                if let Some(entry) = self.entry.as_mut() {
                    entry.edit(typing, at_ms);
                }
                None
            }
        }
    }

    fn reopen_entry(&mut self) {
        self.confirm = None;
        if self.selected.is_some() {
            self.entry.get_or_insert_with(Entry::default);
        }
        self.keyboard = self.entry.is_some();
    }

    fn pick_row(&mut self, ap: AccessPoint, link: &NetworkState) -> Option<NetworkCommand> {
        if link.link.is_leavable() {
            return None;
        }
        let index = link.index_of(ap)?;
        if ap.secured && link.is_saved(index) {
            self.selected = Some(ap);
            self.entry = None;
            self.revealed = false;
            self.keyboard = false;
            return Some(NetworkCommand::Join(JoinRequest::Held(ap.ssid)));
        }
        let same = self.entry.is_some()
            && self
                .selected
                .is_some_and(|picked| picked.is_same_network(&ap));
        if !same {
            self.entry = Some(Entry::default());
            self.revealed = false;
        }
        self.selected = Some(ap);
        if ap.secured {
            self.keyboard = true;
            return None;
        }
        self.keyboard = false;
        let entry = self.entry.unwrap_or_default();
        Some(NetworkCommand::Join(JoinRequest::Typed(
            entry.credentials(ap.ssid),
        )))
    }

    fn no_copy(&mut self, ssid: Ssid) {
        if !self.selected.is_some_and(|picked| picked.ssid == ssid) {
            return;
        }
        self.reopen_entry();
    }

    fn hold_row(&mut self, ap: AccessPoint, link: &NetworkState) {
        let Some(index) = link.index_of(ap) else {
            return;
        };
        if !link.is_saved(index) {
            return;
        }
        self.confirm = Some(Confirm::Forget(ap));
    }

    fn leave_link(&mut self, link: &NetworkState, has_pages: bool) -> Option<NetworkCommand> {
        if !link.link.is_leavable() {
            return None;
        }
        if has_pages {
            self.confirm = Some(Confirm::Leave);
            None
        } else {
            Some(NetworkCommand::Leave)
        }
    }

    fn switch_host(&mut self, link: &NetworkState, has_pages: bool) -> Option<NetworkCommand> {
        let from = link.setup_host()?;
        has_pages.then_some(NetworkCommand::SwitchHost { from })
    }

    fn close_setup(&mut self, link: &NetworkState) -> Option<NetworkCommand> {
        let NetworkMode::Setup {
            host,
            cancellable: true,
        } = link.link.mode
        else {
            return None;
        };
        Some(NetworkCommand::LeaveSetup { from: host })
    }

    fn scan_now(&mut self, link: &NetworkState) -> Option<NetworkCommand> {
        if link.link.is_leavable() {
            return None;
        }
        self.rescan_wait = true;
        Some(NetworkCommand::ScanNow)
    }

    fn set_open(&mut self, open: bool, link: &NetworkState) -> Option<NetworkCommand> {
        if !link.link.shows_setup_box() {
            return None;
        }
        Some(NetworkCommand::SetupOpen {
            from: link.setup_host()?,
            open,
        })
    }

    fn answer(&mut self, question: Confirm, link: &NetworkState) -> Option<NetworkCommand> {
        if self.confirm != Some(question) {
            return None;
        }
        self.confirm = None;
        match question {
            Confirm::Leave if link.link.is_leavable() => Some(NetworkCommand::Leave),
            Confirm::Leave => None,
            Confirm::Forget(ap) => {
                let index = link.index_of(ap)?;
                link.is_saved(index)
                    .then_some(NetworkCommand::Forget(ap.ssid))
            }
        }
    }

    /// The topmost row the list can be scrolled to with `LIST_ROWS` shown.
    fn last_list_top(rows: usize) -> u8 {
        rows.saturating_sub(LIST_ROWS) as u8
    }

    fn scroll(&mut self, rows: i8, link: &NetworkState) {
        let last = Self::last_list_top(link.len());
        let top = i16::from(self.list_top) + i16::from(rows);
        self.list_top = top.clamp(0, i16::from(last)) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::FINGER_LONG_PRESS;
    use crate::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
    use crate::drivers::network::{ApList, Credentials, PASSWORD_BYTES, SetupHost};
    use crate::drivers::playback::Sound;
    use crate::intent::*;
    use crate::keyboard::MULTITAP_WINDOW_MS;
    use crate::state::test_support::*;
    use crate::state::{Boot, DeviceManager, DisplayPage};

    use crate::keyboard::{self, Key, Layer};

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
            if manager.state().page == DisplayPage::Network {
                break;
            }
            manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
        }
        assert_eq!(manager.state().page, DisplayPage::Network);
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

    /// A tap, answered with the command the page asked for by it, if any.
    fn tap_on_at(
        manager: &mut DeviceManager,
        x: u16,
        y: u16,
        at_ms: u32,
    ) -> Option<NetworkCommand> {
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
        let y =
            (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16;
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

    /// The passphrase the page just handed over, read off the command it answered with.
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

    #[test]
    fn the_address_the_radio_reports_is_the_one_the_page_draws() {
        let mut manager = on_network_page();
        assert_eq!(
            manager.state().network.mac,
            None,
            "nothing has reported one yet"
        );
        let mac = [0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef];
        manager.apply_action(ActionIntent::Link(LinkIntent::Mac(mac)));
        assert_eq!(manager.state().network.mac, Some(mac));
    }

    #[test]
    fn a_script_types_its_own_text_through_the_keyboard() {
        for text in [b"pass".as_slice(), b"aZ9.# 0_", b"00", b"z9", b"AaA", b" "] {
            let mut entry = Entry::default();
            for tap in keyboard::script_for(text).expect("a text the keyboard carries") {
                let key = keyboard::key_of(entry.layer(), tap.cell).expect("a cell of the grid");
                entry.edit(key, tap.at_ms);
            }
            assert_eq!(entry.as_bytes(), text, "{text:?} did not come back");
        }
    }

    #[test]
    fn a_tap_on_a_band_opens_the_keyboard_on_the_network_shown_in_it() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"cafe")),
            "the keyboard goes with the network the band was showing"
        );
        assert_eq!(
            manager.state().network_page.entry,
            Some(Entry::default()),
            "and starts empty"
        );
        assert_eq!(
            manager
                .state()
                .network_page
                .entry
                .map(|entry| entry.layer()),
            Some(Layer::Abc),
            "on the layer a network's name is spelled with"
        );
    }

    #[test]
    fn a_band_the_list_has_nothing_in_is_not_a_selection() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 3);
        assert_eq!(
            manager.state().network_page.entry,
            None,
            "a band past the last network is the page's own background"
        );
    }

    #[test]
    fn the_list_slides_a_row_at_a_time_and_a_band_means_the_row_it_shows() {
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

        tap_band(&mut manager, 0);
        assert_eq!(manager.state().network_page.selected, Some(ap(b"a")));
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Key {
            key: Key::Cancel,
            at_ms: 1,
        }));

        assert_eq!(manager.state().network_page.list_top, 0);
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
            rows: 1,
        }));
        assert_eq!(
            manager.state().network_page.list_top,
            1,
            "a slide forward moves the window one row"
        );
        tap_band(&mut manager, 0);
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"b")),
            "and the first band now holds the second network"
        );
        for _ in 0..(8 - keyboard::LIST_ROWS) {
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
                rows: 1,
            }));
        }
        assert_eq!(
            manager.state().network_page.list_top,
            (8 - keyboard::LIST_ROWS) as u8,
            "eight rows through a three-row window stop five rows down"
        );
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
            rows: 1,
        }));
        assert_eq!(
            manager.state().network_page.list_top,
            (8 - keyboard::LIST_ROWS) as u8,
            "and the end of the list is as far as it goes"
        );
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
            rows: -1,
        }));
        assert_eq!(
            manager.state().network_page.list_top,
            (8 - keyboard::LIST_ROWS - 1) as u8
        );
        for _ in 0..8 {
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
                rows: -1,
            }));
        }
        assert_eq!(
            manager.state().network_page.list_top,
            0,
            "and the top of it is as far back"
        );
    }

    #[test]
    fn a_pick_is_read_only_while_there_is_a_link_to_take_down() {
        for phase in [LinkPhase::Connecting, LinkPhase::Connected] {
            let mut manager = on_network_page();
            link_at(&mut manager, phase, b"neighbour");
            let asked = tap_band(&mut manager, 1);
            assert_eq!(
                manager.state().network_page.selected,
                None,
                "{phase:?} is a link, so the band is not a pick"
            );
            assert!(
                !manager.state().network_page.keyboard,
                "{phase:?}: no keyboard"
            );
            assert_eq!(
                manager.state().network_page.entry,
                None,
                "{phase:?}: no draft"
            );
            assert_eq!(
                handed_over(asked),
                None,
                "{phase:?}: and nothing reaches the radio"
            );
        }
        for phase in [
            LinkPhase::Off,
            LinkPhase::Scanning,
            LinkPhase::Failed(NetworkError::Credentials),
        ] {
            let mut manager = on_network_page();
            link_at(&mut manager, phase, b"neighbour");
            tap_band(&mut manager, 1);
            assert_eq!(
                manager.state().network_page.selected,
                Some(ap(b"cafe")),
                "{phase:?} is not a link, so the band picks the row it shows"
            );
            assert!(
                manager.state().network_page.keyboard,
                "{phase:?}: and opens the keyboard"
            );
        }
    }

    #[test]
    fn a_pick_of_an_open_row_is_read_only_under_a_link_too() {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            ..Default::default()
        });
        manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
        manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
        link_at(&mut manager, LinkPhase::Connected, b"neighbour");

        let asked = tap_band(&mut manager, 0);
        assert_eq!(
            manager.state().network_page.selected,
            None,
            "the row is not a pick"
        );
        assert_eq!(handed_over(asked), None, "and nothing reaches the radio");
    }

    #[test]
    fn a_hold_on_the_list_under_the_join_box_asks_nothing() {
        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        link_at(&mut manager, LinkPhase::Connecting, b"cafe");

        let asked = hold_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "a hold under the join box asks nothing"
        );
        assert_eq!(asked, None, "and nothing reaches the store");
    }

    #[test]
    fn a_slide_under_the_join_box_does_not_move_the_list() {
        let mut manager = eight_rows();
        link_at(&mut manager, LinkPhase::Connecting, b"a");
        slide_list(&mut manager);
        assert_eq!(
            manager.state().network_page.list_top,
            0,
            "the window stayed where the box was drawn over it"
        );

        let mut manager = eight_rows();
        slide_list(&mut manager);
        assert_eq!(manager.state().network_page.list_top, 1);
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

    #[test]
    fn a_hold_on_a_saved_row_still_asks_while_the_link_is_up() {
        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        link_up(&mut manager, b"neighbour");

        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "a tap is not a question"
        );

        hold_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.confirm,
            Some(Confirm::Forget(ap(b"cafe"))),
            "a hold still is"
        );
    }

    #[test]
    fn a_draft_picked_before_the_radio_associated_is_still_open_under_it() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 1);
        assert!(
            manager.state().network_page.keyboard,
            "the pick opened the keyboard"
        );

        link_at(&mut manager, LinkPhase::Connecting, b"neighbour");
        assert!(
            manager.state().network_page.keyboard,
            "and the association does not close it"
        );
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"cafe")),
            "the draft is still the row it was opened on"
        );
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
        let y =
            (keyboard::LIST_TOP + band * keyboard::LIST_ROW_H + keyboard::LIST_ROW_H / 2) as u16;
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

    #[test]
    fn the_action_strip_leaves_while_a_link_is_up_and_looks_around_while_it_is_not() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");

        let asked = tap_strip(&mut manager);
        assert_eq!(
            manager.state().network_page.confirm,
            Some(Confirm::Leave),
            "a link that is up is one to take down, and taking it down is asked about"
        );
        assert_eq!(asked, None, "asked about, not done");

        let mut manager = on_network_page();
        assert_eq!(
            tap_strip(&mut manager),
            Some(NetworkCommand::ScanNow),
            "with no link there is something to look around for"
        );
        assert!(
            manager.state().network_page.rescan_wait,
            "and the strip waits for the round it asked for"
        );
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "nothing to confirm"
        );
    }

    #[test]
    fn a_link_that_is_coming_up_is_already_one_to_leave() {
        for named in [None, Some(ap(b"neighbour"))] {
            for phase in [LinkPhase::Connecting, LinkPhase::Connected] {
                let mut manager = on_network_page();
                manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
                    phase,
                    ap: named,
                    address: None,
                    ..LinkStatus::default()
                })));
                tap_strip(&mut manager);
                assert_eq!(
                    manager.state().network_page.confirm,
                    Some(Confirm::Leave),
                    "{phase:?} is a link to leave, named {named:?}"
                );
            }
        }
        for phase in [
            LinkPhase::Off,
            LinkPhase::Scanning,
            LinkPhase::Failed(NetworkError::Credentials),
        ] {
            let mut manager = on_network_page();
            link_at(&mut manager, phase, b"neighbour");
            assert_eq!(
                tap_strip(&mut manager),
                Some(NetworkCommand::ScanNow),
                "{phase:?} is not a link, so the strip looks around instead"
            );
        }
    }

    fn long_press_the_button(manager: &mut DeviceManager) -> Option<NetworkCommand> {
        let operation = op(InputEvent::Button(ButtonEvent::LongPress));
        manager.apply_input(operation);
        let business = translate(&operation, &manager.state());
        manager.apply_action(business)
    }

    #[test]
    fn a_splash_long_press_asks_for_setup_mode_on_every_board() {
        for kinds in [
            [SurfaceRole::Pages, SurfaceRole::Absent],
            [SurfaceRole::Lamp, SurfaceRole::Absent],
        ] {
            let mut manager = DeviceManager::boot(Boot {
                backed: Backed {
                    network: true,
                    ..Backed::NONE
                },
                surface_roles: kinds,
                ..Default::default()
            });
            let asked = long_press_the_button(&mut manager);
            assert!(manager.setup_wanted(), "{kinds:?}: the request is latched");
            assert_eq!(
                manager.state().network_page.confirm,
                None,
                "{kinds:?}: the splash carries no question"
            );
            assert_eq!(asked, None, "{kinds:?}: the request is not a leave");
            assert_eq!(
                manager.state().page,
                DisplayPage::Splash,
                "{kinds:?}: the page keeps its window"
            );
        }
    }

    #[test]
    fn the_splash_lands_on_the_page_that_offers_setup_when_it_was_asked_for() {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                camera: true,
                ..Backed::NONE
            },
            boot_page: DisplayPage::Camera,
            ..Default::default()
        });
        long_press_the_button(&mut manager);
        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
        assert_eq!(manager.state().page, DisplayPage::Network);
    }

    #[test]
    fn the_splash_lands_on_the_build_page_when_nothing_was_asked() {
        let manager = landed(DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                camera: true,
                ..Backed::NONE
            },
            boot_page: DisplayPage::Camera,
            ..Default::default()
        }));
        assert_eq!(manager.state().page, DisplayPage::Camera);
        assert!(!manager.setup_wanted());
    }

    #[test]
    fn a_board_with_no_radio_lands_on_its_page_even_after_the_hold() {
        let mut manager = DeviceManager::new();
        long_press_the_button(&mut manager);
        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn a_long_press_after_the_splash_is_the_pages_own_verb() {
        let mut manager = landed(DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
            ..Default::default()
        }));
        link_up(&mut manager, b"neighbour");
        let before = manager.state().lights[0];

        let asked = long_press_the_button(&mut manager);
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "the press never raises the question the strip raises"
        );
        assert_eq!(asked, None, "and it takes nothing down");
        assert_ne!(
            manager.state().lights[0],
            before,
            "the press is still the page's own verb"
        );
    }

    #[test]
    fn a_strip_leave_on_a_board_with_no_screen_is_done_rather_than_asked_about() {
        let mut manager = booted(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
            ..Default::default()
        });
        link_up(&mut manager, b"neighbour");

        assert_eq!(
            tap_strip(&mut manager),
            Some(NetworkCommand::Leave),
            "the strip's verb is the leave itself"
        );
        assert_eq!(manager.state().network_page.confirm, None);
    }

    #[test]
    fn leaving_happens_when_the_question_is_answered_and_not_when_it_is_left_alone() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");

        tap_strip(&mut manager);
        let asked = tap_answer(&mut manager, keyboard::DialogChoice::Keep);
        assert_eq!(manager.state().network_page.confirm, None);
        assert_eq!(asked, None, "the safe answer takes nothing down");
        assert_eq!(
            manager.state().network.link.phase,
            LinkPhase::Connected,
            "and the link is still the one it was"
        );

        tap_strip(&mut manager);
        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Act),
            Some(NetworkCommand::Leave),
            "and the request is on its way"
        );
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "the question is done"
        );
    }

    #[test]
    fn a_question_about_the_link_does_not_outlive_the_link() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");
        tap_strip(&mut manager);
        assert!(manager.state().network_page.confirm.is_some());

        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::NotFound),
            ap: None,
            address: None,
            ..LinkStatus::default()
        })));
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "a link that is gone takes its own question with it"
        );
        assert_eq!(
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Leave)),
            None
        );
    }

    #[test]
    fn a_scan_round_ends_the_strips_wait() {
        let mut manager = on_network_page();
        tap_strip(&mut manager);
        assert!(manager.state().network_page.rescan_wait);
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(
            LinkStatus::default(),
        )));
        assert!(
            !manager.state().network_page.rescan_wait,
            "the round came back, so the strip stops waiting"
        );
    }

    #[test]
    fn a_hold_on_a_saved_row_asks_to_forget_it() {
        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        hold_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.confirm,
            Some(Confirm::Forget(ap(b"cafe"))),
            "the band held is the network the box asks about"
        );
        assert!(
            !manager.state().network_page.keyboard,
            "a hold is not a tap: nothing is picked and nothing is typed"
        );
        assert_eq!(manager.state().network_page.selected, None);

        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Act),
            Some(NetworkCommand::Forget(Ssid::from_octets(b"cafe"))),
            "the answer hands the name to the task that holds the store"
        );
        assert_eq!(manager.state().network_page.confirm, None, "once");
    }

    #[test]
    fn each_panel_request_answers_with_its_own_command_on_the_spot() {
        let mut manager = on_network_page();
        assert_eq!(
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
                rows: 1
            })),
            None,
            "an intent that asks the radio for nothing answers with nothing"
        );

        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        assert_eq!(
            tap_band(&mut manager, 0),
            Some(NetworkCommand::Join(JoinRequest::Held(Ssid::from_octets(
                b"neighbour"
            )))),
            "a saved row is the credential the task is handed"
        );

        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");
        tap_strip(&mut manager);
        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Act),
            Some(NetworkCommand::Leave),
            "the answered question is the leave"
        );

        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        assert_eq!(
            tap_strip_left(&mut manager),
            Some(NetworkCommand::LeaveSetup {
                from: SetupHost::AccessPoint
            })
        );
        assert_eq!(
            tap_strip(&mut manager),
            Some(NetworkCommand::SwitchHost {
                from: SetupHost::AccessPoint
            })
        );

        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Keep),
            Some(NetworkCommand::SetupOpen {
                from: SetupHost::AccessPoint,
                open: true
            })
        );

        let mut manager = on_network_page();
        assert_eq!(
            tap_strip(&mut manager),
            Some(NetworkCommand::ScanNow),
            "with no link the strip is the scan"
        );

        let mut manager = on_network_page();
        marked(&mut manager, &["cafe"]);
        hold_band(&mut manager, 1);
        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Act),
            Some(NetworkCommand::Forget(Ssid::from_octets(b"cafe")))
        );
    }

    #[test]
    fn the_splash_lands_once_and_says_whether_setup_was_asked_for() {
        let mut manager = on_network_page();
        manager.state.page = DisplayPage::Splash;
        assert_eq!(
            manager.take_splash_end(),
            Some(false),
            "the boot's own landing is news once"
        );
        long_press_the_button(&mut manager);
        assert_eq!(
            manager.take_splash_end(),
            None,
            "the splash is still up, so there is no landing to report"
        );

        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
        assert_eq!(
            manager.take_splash_end(),
            Some(true),
            "one answer: the page ended, and setup mode was asked for on it"
        );
        assert_eq!(
            manager.take_splash_end(),
            None,
            "and the landing is news once"
        );
    }

    #[test]
    fn a_hold_on_a_row_this_device_has_not_saved_asks_nothing() {
        let mut manager = on_network_page();
        assert_eq!(hold_band(&mut manager, 0), None);
        assert_eq!(manager.state().network_page.confirm, None);

        marked(&mut manager, &["neighbour"]);
        hold_band(&mut manager, 0);
        assert!(manager.state().network_page.confirm.is_some());
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Dismiss));
        marked(&mut manager, &[]);
        hold_band(&mut manager, 0);
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "a name the store no longer holds is not one to ask about"
        );
    }

    #[test]
    fn the_stores_marks_ride_the_snapshot_the_panel_draws_from() {
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour", "cafe"]);
        let snapshot = manager.state();
        let list = snapshot.network.aps.expect("the page has a list");
        assert!(list.is_saved(0), "the row the store holds is marked");
        assert!(list.is_saved(1), "and so is the other one");
        assert!(
            !list.is_saved(2),
            "a row the store does not hold is not marked, and neither is a row that is not there"
        );

        marked(&mut manager, &["cafe"]);
        let list = manager.state().network.aps.expect("the page has a list");
        assert!(!list.is_saved(0), "a mark the store dropped is gone");
        assert!(list.is_saved(1));
    }

    #[test]
    fn the_same_row_is_a_request_to_join_while_the_link_is_not_on_it() {
        let mut manager = on_network_page();
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ap: Some(ap(b"neighbour")),
            address: None,
            ..LinkStatus::default()
        })));

        let asked = tap_band(&mut manager, 0);
        assert_ne!(asked, Some(NetworkCommand::Leave));
        assert!(
            manager
                .state()
                .network_page
                .selected
                .is_some_and(|picked| picked.is_same_network(&ap(b"neighbour"))),
            "the row is a selection, and the passphrase can be fixed from there"
        );
    }

    #[test]
    fn the_row_the_link_is_on_is_neither_a_leave_nor_a_pick() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");

        assert_eq!(
            tap_band(&mut manager, 0),
            None,
            "a tap on the link's own row is not a leave"
        );
        assert!(
            !manager.state().network_page.keyboard,
            "and it is not a pick while the link is up"
        );
        assert_eq!(
            manager.state().network_page.selected,
            None,
            "nothing on the page moved"
        );
    }

    #[test]
    fn a_scan_that_shrank_pulls_the_list_back_to_its_own_end() {
        let mut manager = landed(DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            ..Default::default()
        }));
        let eight: Vec<AccessPoint> = (0..8).map(|n| ap(&[b'a' + n as u8])).collect();
        manager.apply_scan(ApList::from_ranked(&eight));
        manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::Scroll {
            rows: 1,
        }));
        assert_eq!(manager.state().network_page.list_top, 1);

        manager.apply_scan(ApList::from_ranked(&[ap(b"a"), ap(b"b")]));
        assert_eq!(
            manager.state().network_page.list_top,
            0,
            "a list shorter than its own window cannot be scrolled past its end"
        );
        tap_band(&mut manager, 1);
        assert_eq!(manager.state().network_page.selected, Some(ap(b"b")));
    }

    #[test]
    fn connect_hands_over_what_was_typed_for_the_row_that_was_picked() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        type_letter(&mut manager, b'd', 1_000);
        let handed = key(&mut manager, Key::Connect);

        let credential = handed_over(handed).expect("connect hands one over");
        assert_eq!(credential.ssid, Ssid::from_octets(b"neighbour"));
        assert_eq!(credential.password.as_bytes(), b"bd");
        assert!(
            !manager.state().network_page.keyboard,
            "the keyboard closes on connect: the radio says whether it worked"
        );
        assert_eq!(
            manager.state().network_page.entry.map(|entry| entry.len()),
            Some(2),
            "and the draft stays: a refusal comes back to the text it refused"
        );
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"neighbour")),
            "the page keeps showing which network is being tried"
        );
        assert_eq!(
            handed_over(key(&mut manager, Key::Connect)),
            None,
            "a credential is a command, not a latch: the keyboard is closed on the first"
        );
    }

    #[test]
    fn an_open_network_is_handed_over_without_a_keyboard() {
        let mut manager = on_network_page();
        manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
        let handed = tap_band(&mut manager, 0);
        assert!(
            !manager.state().network_page.keyboard,
            "no keyboard for a network that asks for nothing"
        );
        assert_eq!(
            manager.state().network_page.selected,
            Some(open(b"cafe")),
            "but the page still says which network is being tried"
        );
        let credential = handed_over(handed).expect("the pick hands one over");
        assert_eq!(credential.ssid, Ssid::from_octets(b"cafe"));
        assert!(
            credential.password.as_bytes().is_empty(),
            "no passphrase rather than a guess at one"
        );
    }

    #[test]
    fn a_tap_on_a_saved_row_asks_for_the_copy_the_device_keeps() {
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        assert_eq!(
            tap_band(&mut manager, 0),
            Some(NetworkCommand::Join(JoinRequest::Held(Ssid::from_octets(
                b"neighbour"
            )))),
            "the tap names the row for the store rather than for the keyboard"
        );
        assert!(
            !manager.state().network_page.keyboard,
            "and nothing is asked for"
        );
        assert_eq!(manager.state().network_page.entry, None);
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"neighbour")),
            "with the row still saying which network is being tried"
        );
    }

    #[test]
    fn a_saved_row_that_is_open_is_still_handed_over_without_a_keyboard() {
        let mut manager = on_network_page();
        manager.apply_scan(ApList::from_ranked(&[open(b"cafe")]));
        marked(&mut manager, &["cafe"]);
        let handed = tap_band(&mut manager, 0);
        assert!(!manager.state().network_page.keyboard);
        let credential = handed_over(handed).expect("the pick hands one over");
        assert!(credential.password.is_empty());
    }

    #[test]
    fn a_row_the_store_has_no_copy_of_puts_the_page_back_on_the_keyboard() {
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        tap_band(&mut manager, 0);
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::NoCopy(
            Ssid::from_octets(b"neighbour"),
        )));
        assert!(manager.state().network_page.keyboard, "the page has to ask");
        assert_eq!(
            manager.state().network_page.entry,
            Some(Entry::default()),
            "over an empty draft, which is what the keys are drawn from"
        );
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"neighbour")),
            "on the row the finger picked"
        );
    }

    #[test]
    fn a_copy_the_store_has_that_was_refused_comes_back_as_a_keyboard() {
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour"]);
        tap_band(&mut manager, 0);
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ap: Some(ap(b"neighbour")),
            address: None,
            ..LinkStatus::default()
        })));
        assert!(
            manager.state().network_page.keyboard,
            "the refusal asks for a fix"
        );
        assert_eq!(manager.state().network_page.entry, Some(Entry::default()));
        assert_eq!(
            manager.state().network_page.refused,
            Some(NetworkError::Credentials),
            "and the page says what went wrong"
        );
    }

    #[test]
    fn an_answer_about_another_row_is_not_this_rows_question() {
        let mut manager = on_network_page();
        marked(&mut manager, &["neighbour", "cafe"]);
        tap_band(&mut manager, 0);
        assert_eq!(
            tap_band(&mut manager, 1),
            Some(NetworkCommand::Join(JoinRequest::Held(Ssid::from_octets(
                b"cafe"
            )))),
            "the newer row is the one the page asks for"
        );
        manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::NoCopy(
            Ssid::from_octets(b"neighbour"),
        )));
        assert!(
            !manager.state().network_page.keyboard,
            "the answer is about a row that is not the picked one"
        );
        assert_eq!(manager.state().network_page.entry, None);
        assert_eq!(
            manager.state().network_page.selected,
            Some(ap(b"cafe")),
            "which is still the row the finger is on"
        );
    }

    #[test]
    fn a_second_tap_of_the_same_row_keeps_the_draft_through_a_rescan() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        key(&mut manager, Key::Connect);
        manager.apply_scan(ApList::from_ranked(&[
            ap_named(b"neighbour", -70, true),
            ap(b"cafe"),
        ]));
        tap_band(&mut manager, 0);
        assert!(
            manager.state().network_page.keyboard,
            "the same row opens the entry again over the draft"
        );
        assert_eq!(
            manager
                .state()
                .network_page
                .entry
                .map(|entry| entry.as_bytes().to_vec()),
            Some(b"b".to_vec()),
            "the draft belongs to the network, not to the reading of it"
        );
        key(&mut manager, Key::Cancel);
        tap_band(&mut manager, 1);
        assert_eq!(
            manager.state().network_page.entry.map(|entry| entry.len()),
            Some(0)
        );
        assert_eq!(manager.state().network_page.selected, Some(ap(b"cafe")));
    }

    #[test]
    fn the_row_a_link_was_left_from_opens_a_new_draft() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        key(&mut manager, Key::Connect);
        link_up(&mut manager, b"neighbour");
        assert!(
            manager.state().network_page.selected.is_some()
                && manager.state().network_page.entry.is_none(),
            "the link kept the row and dropped the draft"
        );

        tap_strip(&mut manager);
        tap_answer(&mut manager, keyboard::DialogChoice::Act);
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(
            LinkStatus::default(),
        )));
        assert!(
            !manager.state().network.link.is_leavable(),
            "the link is down"
        );

        tap_band(&mut manager, 0);
        assert!(
            manager.state().network_page.keyboard,
            "the row the link was on is a request to join it again"
        );
        assert_eq!(
            manager.state().network_page.entry.map(|entry| entry.len()),
            Some(0),
            "and the entry under the keys is a new one, not the draft the link dropped"
        );
    }

    #[test]
    fn a_refused_passphrase_comes_back_to_the_keyboard_over_the_draft() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        key(&mut manager, Key::Connect);
        assert!(
            !manager.state().network_page.keyboard,
            "the keyboard is down while the radio tries"
        );
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Credentials),
            ..LinkStatus::default()
        })));
        assert_eq!(
            manager.state().network_page.refused,
            Some(NetworkError::Credentials)
        );
        assert!(
            manager.state().network_page.keyboard,
            "the keyboard comes back to fix the passphrase"
        );
        assert_eq!(
            manager
                .state()
                .network_page
                .entry
                .map(|entry| entry.as_bytes().to_vec()),
            Some(b"b".to_vec()),
            "over the draft that was refused, not over an empty field"
        );
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Connected,
            ..LinkStatus::default()
        })));
        assert_eq!(manager.state().network_page.entry, None);
        assert!(
            !manager.state().network_page.keyboard,
            "and a link that came up has nothing left to type"
        );
    }

    #[test]
    fn the_reveal_key_shows_the_draft_without_touching_it() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        type_letter(&mut manager, b'b', 0);
        assert!(!manager.state().network_page.revealed);
        key(&mut manager, Key::Reveal);
        assert!(manager.state().network_page.revealed);
        assert_eq!(
            manager
                .state()
                .network_page
                .entry
                .map(|entry| entry.as_bytes().to_vec()),
            Some(b"b".to_vec()),
            "showing the draft is not typing on it"
        );
        key(&mut manager, Key::Cancel);
        assert!(
            !manager.state().network_page.revealed,
            "and leaving the entry puts the stars back"
        );
    }

    #[test]
    fn a_draft_spelling_a_word_reaches_the_radio_byte_for_byte() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        for (n, byte) in b"pass".iter().enumerate() {
            type_letter(&mut manager, *byte, 2_000 * n as u32);
        }
        let handed = key(&mut manager, Key::Connect);
        let credential = handed_over(handed).expect("connect hands one over");
        assert_eq!(credential.password.as_bytes(), b"pass");
    }

    #[test]
    fn cancel_throws_the_draft_away_without_asking_the_radio() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        key(&mut manager, Key::Char(b'2'));
        let handed = key(&mut manager, Key::Cancel);
        assert_eq!(manager.state().network_page.entry, None);
        assert_eq!(manager.state().network_page.selected, None);
        assert_eq!(
            handed_over(handed),
            None,
            "cancelling sends nothing to the radio"
        );
    }

    #[test]
    fn a_key_with_no_keyboard_up_types_nothing() {
        let mut manager = on_network_page();
        let before = manager.state();
        assert_eq!(key(&mut manager, Key::Char(b'2')), None);
        assert_eq!(key(&mut manager, Key::Connect), None);
        assert_eq!(manager.state(), before);
    }

    #[test]
    fn a_repeat_of_one_key_cycles_it_and_any_other_key_starts_a_character() {
        let mut entry = Entry::default();
        let two = Layer::Abc.face(1);
        entry.edit(Key::Char(two), 0);
        assert_eq!(
            entry.as_bytes(),
            b"a",
            "the first tap is the first candidate"
        );
        entry.edit(Key::Char(two), 100);
        entry.edit(Key::Char(two), 200);
        assert_eq!(
            entry.as_bytes(),
            b"c",
            "each tap inside the window moves the character on rather than adding one"
        );
        entry.edit(Key::Char(two), 300);
        assert_eq!(entry.as_bytes(), b"a", "and the run comes round again");
        entry.edit(Key::Char(Layer::Abc.face(2)), 400);
        assert_eq!(
            entry.as_bytes(),
            b"ad",
            "another key ends the run and starts a character of its own"
        );
        entry.edit(Key::Char(Layer::Abc.face(2)), 400 + MULTITAP_WINDOW_MS + 1);
        assert_eq!(
            entry.as_bytes(),
            b"add",
            "and so does the same key once the window has closed"
        );
    }

    #[test]
    fn backspace_ends_the_run_so_the_next_tap_starts_a_character() {
        let mut entry = Entry::default();
        let two = Layer::Abc.face(1);
        entry.edit(Key::Char(two), 0);
        entry.edit(Key::Char(two), 100);
        entry.edit(Key::Backspace, 200);
        entry.edit(Key::Char(two), 300);
        assert_eq!(entry.as_bytes(), b"a");
        assert_eq!(entry.cycling().map(|c| c.index), Some(0));
    }

    #[test]
    fn the_digit_and_symbol_layers_type_one_character_a_key() {
        let mut entry = Entry::default();
        entry.edit(Key::Layer, 0);
        assert_eq!(entry.layer(), Layer::Digits);
        let zero = Layer::Digits.face(9);
        entry.edit(Key::Char(zero), 0);
        entry.edit(Key::Char(zero), 100);
        assert_eq!(entry.as_bytes(), b"00");
        entry.edit(Key::Layer, 200);
        assert_eq!(entry.layer(), Layer::Symbols);
        let at = Layer::Symbols.face(8);
        entry.edit(Key::Char(at), 300);
        assert_eq!(entry.as_bytes(), b"00@");
    }

    #[test]
    fn shift_capitalises_one_letter_and_a_second_tap_locks_it() {
        let mut entry = Entry::default();
        let (two, three) = (Layer::Abc.face(1), Layer::Abc.face(2));
        entry.edit(Key::Shift, 0);
        entry.edit(Key::Char(two), 100);
        entry.edit(Key::Char(three), 200);
        assert_eq!(
            entry.as_bytes(),
            b"Ad",
            "one shot: the capital is the letter after the tap and nothing else"
        );
        entry.edit(Key::Shift, 300);
        entry.edit(Key::Shift, 400);
        entry.edit(Key::Char(two), 500);
        entry.edit(Key::Char(three), 600);
        assert_eq!(entry.as_bytes(), b"AdAD");
        entry.edit(Key::Shift, 700);
        entry.edit(Key::Char(two), 800);
        assert_eq!(entry.as_bytes(), b"AdADa");
        entry.edit(Key::Shift, 900);
        entry.edit(Key::Char(Layer::Abc.face(0)), 1_000);
        entry.edit(Key::Char(two), 1_100);
        assert_eq!(entry.as_bytes(), b"AdADa.A");
        entry.edit(Key::Shift, 3_000);
        let nine = Layer::Abc.face(8);
        for tap in 0..4 {
            entry.edit(Key::Char(nine), 3_100 + 100 * tap);
        }
        assert_eq!(entry.as_bytes(), b"AdADa.AZ");
        entry.edit(Key::Char(two), 4_000);
        assert_eq!(entry.as_bytes(), b"AdADa.AZa");
    }

    #[test]
    fn the_draft_stops_at_the_length_the_radio_takes() {
        let mut entry = Entry::default();
        let (two, three) = (Layer::Abc.face(1), Layer::Abc.face(2));
        for n in 0..(PASSWORD_BYTES + 8) {
            let face = if n % 2 == 0 { two } else { three };
            entry.edit(Key::Char(face), 100 * n as u32);
        }
        assert_eq!(entry.len(), PASSWORD_BYTES - 1);
        entry.edit(Key::Backspace, 0);
        entry.edit(Key::Char(two), 0);
        assert_eq!(entry.len(), PASSWORD_BYTES - 1);
        assert_eq!(entry.as_bytes().last(), Some(&b'a'));
        assert_eq!(entry.cycling().map(|c| c.index), Some(0));
    }

    #[test]
    fn the_panel_is_told_which_candidate_a_key_is_holding() {
        let mut entry = Entry::default();
        assert_eq!(entry.cycling(), None, "nothing is being cycled yet");
        let two = Layer::Abc.face(1);
        entry.edit(Key::Char(two), 0);
        let cycling = entry
            .cycling()
            .expect("the key just tapped is the one cycling");
        assert_eq!(cycling.face, two);
        assert_eq!(cycling.candidates, b"abc");
        assert_eq!(cycling.index, 0);
        entry.edit(Key::Char(two), 100);
        assert_eq!(entry.cycling().map(|c| c.index), Some(1));
        entry.edit(Key::Connect, 200);
        assert_eq!(entry.cycling(), None, "leaving the entry ends the run");
    }

    #[test]
    fn an_empty_passphrase_is_still_a_credential_the_radio_can_refuse() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        let handed = key(&mut manager, Key::Connect);
        let credential = handed_over(handed).expect("an empty one is still one");
        assert!(credential.password.is_empty());
    }

    #[test]
    fn a_refusal_is_kept_until_another_attempt_starts() {
        let mut manager = on_network_page();
        let failed = LinkStatus {
            phase: LinkPhase::Failed(NetworkError::Driver),
            ..LinkStatus::default()
        };
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(failed)));
        assert_eq!(
            manager.state().network_page.phase(LinkPhase::Off),
            LinkPhase::Failed(NetworkError::Driver),
            "the round's own phase does not bury the refusal"
        );
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Scanning,
            ..LinkStatus::default()
        })));
        assert_eq!(
            manager.state().network_page.phase(LinkPhase::Scanning),
            LinkPhase::Failed(NetworkError::Driver),
            "nor does the scan that follows it"
        );
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            phase: LinkPhase::Connecting,
            ..LinkStatus::default()
        })));
        assert_eq!(
            manager.state().network_page.phase(LinkPhase::Connecting),
            LinkPhase::Connecting
        );
    }

    #[test]
    fn the_page_draws_the_row_being_typed_for_before_the_link_is_on_it() {
        let mut manager = on_network_page();
        tap_band(&mut manager, 0);
        assert_eq!(
            manager.state().network.link.ap,
            None,
            "nothing is on the air yet"
        );
        assert_eq!(
            manager.state().network_page.selected.map(|ap| ap.ssid),
            Some(Ssid::from_octets(b"neighbour"))
        );
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

    #[test]
    fn the_strip_trades_a_link_for_setup_mode_and_closes_it_again() {
        let mut manager = on_network_page();
        link_up(&mut manager, b"neighbour");
        tap_strip(&mut manager);
        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Act),
            Some(NetworkCommand::Leave),
            "the question is asked, and answered"
        );

        setup_mode(&mut manager, true);
        assert_eq!(
            tap_strip_left(&mut manager),
            Some(NetworkCommand::LeaveSetup {
                from: SetupHost::AccessPoint
            }),
            "a device that keeps a network has a way back to it"
        );
        assert_eq!(
            tap_strip(&mut manager),
            Some(NetworkCommand::SwitchHost {
                from: SetupHost::AccessPoint
            })
        );
    }

    #[test]
    fn an_offline_page_keeps_its_ap_cell_but_a_hold_does_not_raise_one() {
        let mut manager = landed(DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            ..Default::default()
        }));
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            mode: NetworkMode::Browsing,
            ..LinkStatus::default()
        })));
        assert_eq!(
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)),
            Some(NetworkCommand::SwitchHost {
                from: SetupHost::Panel
            }),
            "the cell is still the way in"
        );

        let operation = op(InputEvent::Button(ButtonEvent::LongPress));
        let business = translate(&operation, &manager.state());
        assert_ne!(
            business,
            ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost),
            "a hold outside setup mode does not switch the host"
        );
    }

    #[test]
    fn a_board_with_no_display_has_no_front_to_switch_to() {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
            ..Default::default()
        });
        setup_mode(&mut manager, false);
        assert_eq!(
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)),
            None,
            "a lamp-only board has nowhere to switch to"
        );
    }

    #[test]
    fn a_front_switch_that_outlives_setup_mode_is_dropped() {
        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(
            LinkStatus::default(),
        )));
        assert_eq!(
            manager.apply_action(ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost)),
            None,
            "a switch cannot ask for another host once the mode is gone"
        );
    }

    #[test]
    fn a_device_with_nothing_saved_has_only_the_front_switch_on_the_access_points_front() {
        let mut manager = on_network_page();
        setup_mode(&mut manager, false);
        assert_eq!(
            tap_strip_left(&mut manager),
            Some(NetworkCommand::SwitchHost {
                from: SetupHost::AccessPoint
            }),
            "the strip's one action is the other host"
        );
    }

    #[test]
    fn the_panels_front_keeps_a_cell_for_looking_around_when_there_is_no_way_back() {
        let mut manager = on_network_page();
        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::Panel,
                cancellable: false,
            },
            ..LinkStatus::default()
        })));
        assert_eq!(
            tap_strip_left(&mut manager),
            Some(NetworkCommand::ScanNow),
            "a device with no way back still asks the radio to look around"
        );
        assert!(
            manager.state().network_page.rescan_wait,
            "and the strip says so"
        );

        assert_eq!(
            tap_strip(&mut manager),
            Some(NetworkCommand::SwitchHost {
                from: SetupHost::Panel
            }),
            "the right half is the switch"
        );
    }

    #[test]
    fn the_box_belongs_to_the_access_points_front_and_not_to_a_boot() {
        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        assert!(manager.state().network.link.shows_setup_box());

        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::Panel,
                cancellable: false,
            },
            ..LinkStatus::default()
        })));
        assert!(
            !manager.state().network.link.shows_setup_box(),
            "the list is what the panel's own host shows"
        );

        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(
            LinkStatus::default(),
        )));
        setup_mode(&mut manager, true);
        assert!(manager.state().network.link.shows_setup_box());
    }

    #[test]
    fn the_setup_box_s_other_answer_asks_for_the_access_point_to_be_open() {
        let mut manager = on_network_page();
        setup_mode(&mut manager, true);
        assert_eq!(
            tap_answer(&mut manager, keyboard::DialogChoice::Keep),
            Some(NetworkCommand::SetupOpen {
                from: SetupHost::AccessPoint,
                open: true
            })
        );

        manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::Panel,
                cancellable: false,
            },
            ..LinkStatus::default()
        })));
        assert_eq!(
            manager.apply_action(ActionIntent::NetworkPage(
                NetworkPageIntent::SetOpenAccessPoint(true)
            )),
            None,
            "the box belongs to the access point's front, not to the panel's"
        );
    }

    #[test]
    fn the_radio_s_three_moments_are_heard() {
        let mut manager = booted(Boot {
            backed: Backed {
                network: true,
                playback: true,
                ..Backed::NONE
            },
            ..Default::default()
        });

        setup_mode(&mut manager, true);
        assert_eq!(manager.playback_state().phase, PlaybackPhase::Playing);
        assert_eq!(
            manager.playback_state().sound,
            Sound::Rise,
            "entering setup mode"
        );

        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
        link_up(&mut manager, b"neighbour");
        assert_eq!(
            manager.playback_state().sound,
            Sound::Tap,
            "the network answered"
        );

        manager.apply_action(ActionIntent::Playback(PlaybackAction::Finished));
        link_at(
            &mut manager,
            LinkPhase::Failed(NetworkError::Credentials),
            b"neighbour",
        );
        assert_eq!(
            manager.playback_state().sound,
            Sound::Error,
            "the network refused"
        );
    }
}

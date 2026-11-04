//! The Network page's own state: the row a passphrase is being typed for, the draft itself,
//! the question the page is asking, and where the list is scrolled. The link is not here —
//! this record reads [`NetworkState`](super::NetworkState) and never writes it, so a report
//! from the worker cannot land in a page field and a page edit cannot change what the radio
//! said.

use crate::intent::NetworkPageIntent;
use iot_core::drivers::network::{
    AccessPoint, JoinRequest, LinkPhase, LinkStatus, NetworkError, NetworkMode, Ssid,
};
use iot_core::keyboard::{Key, LIST_ROWS};

use iot_core::drivers::network::NetworkCommand;

use super::entry::Entry;
use super::network::NetworkState;

pub(super) mod translate;

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
mod tests;

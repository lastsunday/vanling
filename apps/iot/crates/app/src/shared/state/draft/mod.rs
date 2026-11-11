//! The Network page's draft: the row a passphrase is being typed for, the draft itself, the
//! question the page is asking, and where the list is scrolled. The link is not here — this
//! record reads [`NetworkState`] and never writes it, so a report from the worker cannot land
//! in a page field and a page edit cannot change what the radio said.
//!
//! It lives beside [`DeviceState`](super::DeviceState) because the device owns it: the page
//! edits it, the diagnostics snapshot copies it, and the link never reads it.

use crate::shared::intent::NetworkPageIntent;
use crate::shared::keyboard::Key;
use iot_core::drivers::network::{
    AccessPoint, JoinRequest, LinkPhase, LinkStatus, NetworkError, NetworkMode, SetupHost, Ssid,
};

use iot_core::drivers::network::NetworkCommand;

use crate::shared::intent::Confirm;
use crate::shared::state::{Command, NetworkState};

/// The scan rows the draft keeps in its scroll window; the list draws this many.
pub const LIST_ROWS: usize = 3;

/// What the Network page is in the middle of: the row a passphrase is being typed for, the
/// draft itself, and the last refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetworkDraft {
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
    /// Whether a join the page handed over is still waiting for the radio's own word.
    pub join_wait: bool,
    /// The setup change the page asked for and has not seen reported yet.
    pub setup_wait: Option<SetupWait>,
}

/// What a setup tap is waiting on: the other host, or the access point's new passphrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupWait {
    Host(SetupHost),
    Open(bool),
}

impl NetworkDraft {
    /// Whether the page is waiting on an ask of its own, so the strip has nothing to offer.
    pub fn busy(&self) -> bool {
        self.rescan_wait || self.join_wait || self.setup_wait.is_some()
    }

    /// Whether the box the page is about to draw is the access point's.
    pub fn setup_box_pending(&self) -> bool {
        self.setup_wait == Some(SetupWait::Host(SetupHost::AccessPoint))
    }

    /// The phase the page should show.
    pub fn phase(&self, link: LinkPhase) -> LinkPhase {
        match (self.refused, link) {
            (Some(kind), LinkPhase::Off | LinkPhase::Scanning) => LinkPhase::Failed(kind),
            (_, link) => link,
        }
    }

    /// The page's intents: every ask the panel makes of the link, answered with the command
    /// the caller hands to the runtime. The link is read, never written.
    pub(crate) fn apply(
        &mut self,
        intent: NetworkPageIntent,
        link: &NetworkState,
        has_pages: bool,
    ) -> Option<Command> {
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
    pub(crate) fn on_link(&mut self, after: LinkStatus) {
        match after.phase {
            LinkPhase::Failed(kind) => {
                self.refused = Some(kind);
                self.reopen_entry();
            }
            LinkPhase::Connecting | LinkPhase::Connected => {
                self.refused = None;
                self.join_wait = false;
            }
            LinkPhase::Off | LinkPhase::Scanning => {}
        }
        if let Some(wait) = self.setup_wait {
            let done = match wait {
                SetupWait::Host(host) => after.mode.host() == Some(host),
                SetupWait::Open(open) => {
                    !after.shows_setup_box() || after.setup_password.is_some() != open
                }
            };
            if done || matches!(after.phase, LinkPhase::Failed(_)) || !after.mode.is_setup() {
                self.setup_wait = None;
            }
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
    pub(crate) fn clamp_top(&mut self, rows: usize) {
        self.list_top = self.list_top.min(Self::last_list_top(rows));
    }

    fn apply_key(&mut self, key: Key, at_ms: u32) -> Option<Command> {
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
                self.join_wait = true;
                Some(Command::Network(NetworkCommand::Join(JoinRequest::Typed(
                    entry.credentials(ap.ssid),
                ))))
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
        self.join_wait = false;
        if self.selected.is_some() {
            self.entry.get_or_insert_with(Entry::default);
        }
        self.keyboard = self.entry.is_some();
    }

    fn pick_row(&mut self, ap: AccessPoint, link: &NetworkState) -> Option<Command> {
        if self.join_wait || link.link.is_leavable() {
            return None;
        }
        let index = link.index_of(ap)?;
        if ap.secured && link.is_saved(index) {
            self.selected = Some(ap);
            self.entry = None;
            self.revealed = false;
            self.keyboard = false;
            self.join_wait = true;
            return Some(Command::Network(NetworkCommand::Join(JoinRequest::Held(
                ap.ssid,
            ))));
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
        self.join_wait = true;
        Some(Command::Network(NetworkCommand::Join(JoinRequest::Typed(
            entry.credentials(ap.ssid),
        ))))
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

    fn leave_link(&mut self, link: &NetworkState, has_pages: bool) -> Option<Command> {
        if !link.link.is_leavable() {
            return None;
        }
        if has_pages {
            self.confirm = Some(Confirm::Leave);
            None
        } else {
            Some(Command::Network(NetworkCommand::Leave))
        }
    }

    fn switch_host(&mut self, link: &NetworkState, has_pages: bool) -> Option<Command> {
        if !has_pages || self.join_wait || self.setup_wait.is_some() {
            return None;
        }
        let from = link.setup_host()?;
        let to = match from {
            SetupHost::Panel => SetupHost::AccessPoint,
            SetupHost::AccessPoint => SetupHost::Panel,
        };
        self.setup_wait = Some(SetupWait::Host(to));
        Some(Command::Network(NetworkCommand::SwitchHost { from }))
    }

    fn close_setup(&mut self, link: &NetworkState) -> Option<Command> {
        if self.setup_wait.is_some() {
            return None;
        }
        let NetworkMode::Setup {
            host,
            cancellable: true,
        } = link.link.mode
        else {
            return None;
        };
        Some(Command::Network(NetworkCommand::LeaveSetup { from: host }))
    }

    fn scan_now(&mut self, link: &NetworkState) -> Option<Command> {
        if self.busy() || link.link.is_leavable() {
            return None;
        }
        self.rescan_wait = true;
        Some(Command::Network(NetworkCommand::ScanNow))
    }

    fn set_open(&mut self, open: bool, link: &NetworkState) -> Option<Command> {
        if self.join_wait || self.setup_wait.is_some() || !link.link.shows_setup_box() {
            return None;
        }
        let from = link.setup_host()?;
        self.setup_wait = Some(SetupWait::Open(open));
        Some(Command::Network(NetworkCommand::SetupOpen { from, open }))
    }

    fn answer(&mut self, question: Confirm, link: &NetworkState) -> Option<Command> {
        if self.confirm != Some(question) {
            return None;
        }
        self.confirm = None;
        match question {
            Confirm::Leave if link.link.is_leavable() => {
                Some(Command::Network(NetworkCommand::Leave))
            }
            Confirm::Leave => None,
            Confirm::Forget(ap) => {
                let index = link.index_of(ap)?;
                link.is_saved(index)
                    .then_some(Command::Network(NetworkCommand::Forget(ap.ssid)))
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

mod entry;

pub use self::entry::Entry;

#[cfg(test)]
mod tests;

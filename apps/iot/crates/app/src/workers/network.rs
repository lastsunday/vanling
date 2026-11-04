//! The network worker's side: what the panel asks for, the page it is showing, and the scan
//! list coming back. The command vocabulary itself belongs to the state machine
//! (`iot_core::state::NetworkCommand`); this file only carries it.
//!
//! This is the one file under `workers` that names the radio's types, because the coordinator
//! and the radio cannot reach each other and this is the ground they share.

use core::future::Future;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use iot_core::drivers::network::ApList;
use iot_core::state::{DisplayPage, NetworkCommand};

use super::inbox::{Inbox, Retained};

/// How many commands may wait for the network worker: one frame's largest burst, because a pass
/// takes every intent the bus holds and each one can carry a command. That depth is what keeps
/// the normal path lossless — an entry gives way only when the worker is more than a frame
/// behind, and then it is the request the panel has already moved past.
pub(crate) const COMMAND_SLOTS: usize = 8;

pub type NetworkInbox = Inbox<NetworkCommand, COMMAND_SLOTS>;

/// The coordinator's handle on the network worker.
///
/// `Copy`: every field is a handle to a static, so either side can take it by value.
#[derive(Clone, Copy)]
pub struct Network {
    /// Coordinator → worker: what the panel asked the link to do.
    pub commands: &'static NetworkInbox,
    /// Coordinator → worker: the page the panel is showing, as a retained value.
    pub page: &'static PageWatch,
    /// Worker → coordinator: the newest scan list, and which round produced it.
    pub scan: &'static Scan,
}

static NETWORK_INBOX: NetworkInbox = NetworkInbox::new();

static PAGE: PageWatch = PageWatch::new();

static SCAN: Scan = Scan::new();

pub static NETWORK: Network = Network {
    commands: &NETWORK_INBOX,
    page: &PAGE,
    scan: &SCAN,
};

/// The scan worker's published snapshot: the newest list, plus a counter that advances once
/// per completed round even when the list did not change. The page asks `/rescan`, reads the
/// counter from that answer, and polls until it moves — a list alone cannot say whether the
/// round it asked for has run or is still the one from before.
pub struct Scan {
    list: Retained<ApList>,
    generation: AtomicU32,
}

impl Scan {
    pub(crate) const fn new() -> Self {
        Self {
            list: Retained::new(),
            generation: AtomicU32::new(0),
        }
    }

    pub fn take(&self) -> Option<ApList> {
        self.list.take()
    }

    pub fn peek(&self) -> Option<ApList> {
        self.list.peek()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.list.ready()
    }

    pub fn publish(&self, list: &ApList) {
        self.list.publish(list);
    }

    /// One successful scan round finished, whether or not its list differed from the last. The
    /// release pairs with [`Self::generation`], so a reader that sees round N reads a list from N
    /// or later.
    pub fn complete(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Acquire)
    }
}

/// The panel's page, for the network worker, which cannot reach device state. A retained value
/// the worker reads rather than a message it receives: the worker asks whether the list is on
/// screen, and the end of the splash reaches it as a command like everything else the panel
/// asks for.
pub struct PageWatch {
    pub(crate) page: AtomicU8,
    pub(crate) changed: Signal<CriticalSectionRawMutex, ()>,
}

impl PageWatch {
    pub(crate) const fn new() -> Self {
        Self {
            page: AtomicU8::new(DisplayPage::Ambient.code()),
            changed: Signal::new(),
        }
    }

    pub fn publish(&self, page: DisplayPage) {
        if self.page.swap(page.code(), Ordering::Relaxed) != page.code() {
            self.changed.signal(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::network::{AccessPoint, Ssid};

    fn round_list(saved: bool) -> ApList {
        let mut list = ApList::from_ranked(&[AccessPoint {
            ssid: Ssid::from_octets(b"target"),
            bssid: [7; 6],
            channel: 11,
            rssi: -50,
            secured: true,
        }]);
        list.mark_saved(u8::from(saved));
        list
    }

    #[test]
    fn a_round_advances_the_generation_even_when_the_list_is_the_same() {
        let scan = Scan::new();
        assert_eq!(scan.generation(), 0, "nothing has been scanned yet");

        scan.publish(&round_list(false));
        assert_eq!(
            scan.generation(),
            0,
            "publishing a list is not the end of a round"
        );

        scan.complete();
        scan.complete();
        assert_eq!(
            scan.generation(),
            2,
            "each finished round is a new generation, changed list or not"
        );
        assert_eq!(
            scan.peek().map(|list| list.len()),
            Some(1),
            "the list the page reads is still the published one"
        );
        assert!(
            scan.take().is_some(),
            "the panel takes the list once, as before"
        );
    }
}

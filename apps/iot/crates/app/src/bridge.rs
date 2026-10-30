//! The panel task and the network task cannot reach each other's state, so this is the whole
//! contract between them: the page the panel is on, the commands the panel asks for, and the
//! reports the link sends back.
//!
//! Compiled with and without `wifi`: the pipeline holds these handles either way, and a build
//! with no radio simply never answers them.

use core::cell::Cell;
use core::future::Future;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use iot_core::drivers::network::{ApList, Front, JoinRequest, LinkStatus, Ssid};
use iot_core::intent::{BusinessIntent, Intent};
use iot_core::state::DisplayPage;

use crate::input::INTENT_BUS;

pub struct Latch<T> {
    value: Signal<CriticalSectionRawMutex, T>,
}

impl<T: Send> Latch<T> {
    const fn new() -> Self {
        Self {
            value: Signal::new(),
        }
    }

    pub fn set(&self, value: T) {
        self.value.signal(value);
    }

    pub async fn ready(&self) -> T {
        self.value.wait().await
    }

    pub fn clear(&self) {
        self.value.reset();
    }
}

pub(crate) static LEAVE: Latch<()> = Latch::new();

pub static RESCAN: Latch<()> = Latch::new();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SetupAsk {
    Cancel { from: Front },
    Front { from: Front },
    Pass { from: Front, open: bool },
}

#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
impl SetupAsk {
    pub fn applies_to(self, front: Front) -> bool {
        let from = match self {
            Self::Cancel { from } | Self::Front { from } | Self::Pass { from, .. } => from,
        };
        from == front
    }
}

pub(crate) static SETUP_ASK: Latch<SetupAsk> = Latch::new();

pub static FORGET: Latch<Ssid> = Latch::new();

pub struct Handoff<T> {
    slot: Channel<CriticalSectionRawMutex, T, 1>,
}

impl<T: Send> Handoff<T> {
    const fn new() -> Self {
        Self {
            slot: Channel::new(),
        }
    }

    pub fn take(&self) -> Option<T> {
        self.slot.try_receive().ok()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.slot.ready_to_receive()
    }

    pub fn publish(&self, asked: T) {
        let _ = self.slot.try_send(asked);
    }
}

pub static JOIN: Handoff<JoinRequest> = Handoff::new();

pub struct Latest<T> {
    value: Signal<CriticalSectionRawMutex, T>,
    arrived: Signal<CriticalSectionRawMutex, ()>,
}

impl<T: Copy + Send> Latest<T> {
    const fn new() -> Self {
        Self {
            value: Signal::new(),
            arrived: Signal::new(),
        }
    }

    pub fn take(&self) -> Option<T> {
        self.value.try_take()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.arrived.wait()
    }

    /// By reference: the 344-byte list would otherwise be copied into the survey future's frame.
    pub fn publish(&self, value: &T) {
        self.value.signal(*value);
        self.arrived.signal(());
    }
}

pub static SCAN: Latest<ApList> = Latest::new();

/// The link the panel was last told about.
///
/// A reading that repeats is not a report: the panel's state only moves on a difference, and
/// the intent bus also carries every command the other way.
struct LinkWatch {
    last: Mutex<CriticalSectionRawMutex, Cell<Option<LinkStatus>>>,
}

#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
impl LinkWatch {
    const fn new() -> Self {
        Self {
            last: Mutex::new(Cell::new(None)),
        }
    }

    /// Delivers `status` with the store's bit folded in, if it differs from the last
    /// delivery. The reading is only remembered once the bus took it, so a full bus retries
    /// rather than swallowing the change.
    fn deliver(&self, status: LinkStatus, saved: bool) {
        let status = LinkStatus { saved, ..status };
        if self.last.lock(|last| last.get()) == Some(status) {
            return;
        }
        if INTENT_BUS
            .try_send(Intent::Business(BusinessIntent::LinkReport(status)))
            .is_ok()
        {
            self.last.lock(|last| last.set(Some(status)));
        }
    }
}

#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
static LINK: LinkWatch = LinkWatch::new();

/// Reports the link to the panel. The network task's whole view of the intent bus is this
/// call.
#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
pub(crate) fn deliver(status: LinkStatus, saved: bool) {
    LINK.deliver(status, saved);
}

/// The panel's page, for the network task, which cannot reach device state.
pub struct PageWatch {
    page: AtomicU8,
    splash: AtomicU8,
    changed: Signal<CriticalSectionRawMutex, ()>,
}

#[cfg_attr(not(feature = "wifi"), allow(dead_code))]
impl PageWatch {
    const fn new() -> Self {
        Self {
            page: AtomicU8::new(DisplayPage::Ambient as u8),
            splash: AtomicU8::new(0),
            changed: Signal::new(),
        }
    }

    pub fn shows(&self, page: DisplayPage) -> bool {
        self.page.load(Ordering::Relaxed) == page as u8
    }

    pub fn publish(&self, page: DisplayPage) {
        if self.page.swap(page as u8, Ordering::Relaxed) != page as u8 {
            self.changed.signal(());
        }
    }

    /// `setup` is whether the button asked for setup mode while the splash was up.
    pub fn end_splash(&self, setup: bool) {
        self.splash
            .store(if setup { 2 } else { 1 }, Ordering::Relaxed);
        self.changed.signal(());
    }

    /// `true` when setup was asked for while the splash was up.
    pub async fn wait_for_splash_end(&self) -> bool {
        loop {
            match self.splash.load(Ordering::Relaxed) {
                0 => self.changed.wait().await,
                ended => return ended == 2,
            }
        }
    }

    /// `true` when the panel had to wait for `page`.
    pub async fn wait_for(&self, page: DisplayPage) -> bool {
        let mut waited = false;
        while !self.shows(page) {
            waited = true;
            self.changed.wait().await;
        }
        waited
    }

    /// Edge, not level: a panel that never left `page` must not satisfy it.
    pub async fn wait_for_return(&self, page: DisplayPage) {
        let wanted = page as u8;
        let mut seen = self.page.load(Ordering::Relaxed);
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                continue;
            }
            seen = page;
            if page == wanted {
                return;
            }
        }
    }

    pub async fn wait_until_away(&self, page: DisplayPage) {
        let unwanted = page as u8;
        let mut seen = self.page.load(Ordering::Relaxed);
        if seen != unwanted {
            return;
        }
        loop {
            self.changed.wait().await;
            let page = self.page.load(Ordering::Relaxed);
            if page == seen {
                continue;
            }
            seen = page;
            if page != unwanted {
                return;
            }
        }
    }
}

pub static PAGE: PageWatch = PageWatch::new();

pub(crate) struct Outbound {
    pub joins: &'static Handoff<JoinRequest>,
    pub leave: &'static Latch<()>,
    pub rescan: &'static Latch<()>,
    pub setup_ask: &'static Latch<SetupAsk>,
    pub forget: &'static Latch<Ssid>,
}

pub(crate) static OUTBOUND: Outbound = Outbound {
    joins: &JOIN,
    leave: &LEAVE,
    rescan: &RESCAN,
    setup_ask: &SETUP_ASK,
    forget: &FORGET,
};

pub(crate) struct Inbound {
    pub scan: &'static Latest<ApList>,
    pub page: &'static PageWatch,
}

pub(crate) static INBOUND: Inbound = Inbound {
    scan: &SCAN,
    page: &PAGE,
};

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::{Context, Waker};
    use iot_core::drivers::network::AccessPoint;

    #[test]
    fn a_setup_request_only_answers_the_front_end_it_was_made_on() {
        for ask in [
            SetupAsk::Front { from: Front::Panel },
            SetupAsk::Cancel { from: Front::Panel },
            SetupAsk::Pass {
                from: Front::Panel,
                open: true,
            },
        ] {
            assert!(
                ask.applies_to(Front::Panel),
                "{ask:?} was asked on the panel"
            );
            assert!(
                !ask.applies_to(Front::AccessPoint),
                "{ask:?} is not an answer the access point's front end asked for"
            );
        }
        for ask in [
            SetupAsk::Front {
                from: Front::AccessPoint,
            },
            SetupAsk::Cancel {
                from: Front::AccessPoint,
            },
            SetupAsk::Pass {
                from: Front::AccessPoint,
                open: false,
            },
        ] {
            assert!(
                ask.applies_to(Front::AccessPoint),
                "{ask:?} was asked on the phone"
            );
            assert!(
                !ask.applies_to(Front::Panel),
                "{ask:?} is not an answer the panel's front end asked for"
            );
        }
    }

    fn cx() -> Context<'static> {
        Context::from_waker(Waker::noop())
    }

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
    fn a_list_published_before_the_last_one_is_taken_replaces_it_rather_than_being_refused() {
        let latest = Latest::new();
        latest.publish(&round_list(true));
        latest.publish(&round_list(false));

        let list = latest.take().expect("a list is waiting");
        assert!(
            !list.is_saved(0),
            "the newest list is the one the page is handed, and the mark is off it"
        );
        assert!(latest.take().is_none(), "and it is handed over once");
    }

    #[test]
    fn the_wake_a_park_waits_on_does_not_take_the_list_it_woke_for() {
        let latest = Latest::new();
        latest.publish(&round_list(true));
        let mut wake = core::pin::pin!(latest.ready());
        assert!(
            wake.as_mut().poll(&mut cx()).is_ready(),
            "the wake is up as soon as a list is waiting"
        );
        assert!(
            latest.take().is_some(),
            "the take at the top of the pass is the one that takes the list"
        );
    }

    #[test]
    fn a_rest_on_the_list_is_not_cut_short_by_the_list_being_up() {
        let watch = PageWatch::new();
        watch.publish(DisplayPage::Network);
        let mut level = core::pin::pin!(watch.wait_for(DisplayPage::Network));
        let mut back = core::pin::pin!(watch.wait_for_return(DisplayPage::Network));
        assert!(
            level.as_mut().poll(&mut cx()).is_ready(),
            "the level check answers \"is the list up?\", and it is"
        );
        assert!(
            back.as_mut().poll(&mut cx()).is_pending(),
            "a return cannot be satisfied by never having left"
        );

        watch.publish(DisplayPage::Ambient);
        assert!(back.as_mut().poll(&mut cx()).is_pending());
        watch.publish(DisplayPage::Network);
        assert!(
            back.as_mut().poll(&mut cx()).is_ready(),
            "away and back is the return the pause waits on"
        );
    }

    #[test]
    fn the_same_watch_answers_for_whichever_page_the_caller_names() {
        let watch = PageWatch::new();
        watch.publish(DisplayPage::Camera);

        assert!(watch.shows(DisplayPage::Camera));
        assert!(!watch.shows(DisplayPage::Network));
        let mut away = core::pin::pin!(watch.wait_until_away(DisplayPage::Network));
        assert!(
            away.as_mut().poll(&mut cx()).is_ready(),
            "the camera is already away from the network page"
        );
        let mut list = core::pin::pin!(watch.wait_for(DisplayPage::Network));
        assert!(
            list.as_mut().poll(&mut cx()).is_pending(),
            "and the camera is not the page a caller waiting for the list is after"
        );
    }
}

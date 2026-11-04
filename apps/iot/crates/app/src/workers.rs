//! The coordinator and its workers cannot reach each other's state, so this is the whole
//! vocabulary they share: one handle per worker, and the shapes those handles are made of.
//!
//! A worker is a task that owns something the coordinator must not block on: the drivers it
//! polls, the codec's I2C, the radio's stack. Each field of [`Workers`] is that worker's side
//! of the boundary; adding a worker adds a field, not a `TO_X`/`FROM_X` pair at the call site.
//!
//! The network command vocabulary is the state machine's ([`NetworkCommand`]); this module
//! only carries it. Reports go the other way as intents, so the only thing the network worker
//! publishes here is the list, which is a window rather than an event.
//!
//! Compiled with and without `wifi`: the coordinator holds these handles either way, and a
//! build with no radio simply never answers them.

use core::cell::Cell;
use core::future::Future;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use iot_core::demand::EffectiveDemand;
use iot_core::drivers::audio::{AudioSample, CaptureSink};
use iot_core::drivers::network::ApList;
use iot_core::state::{DisplayPage, NetworkCommand, PlaybackCommand};

/// The demand the input worker reads and the coordinator writes, under the same lock the
/// feed interrupt takes. A `Copy` record in a `Cell`, so the lock covers a compare and one
/// bounded copy rather than replacing a map handle.
pub type SharedDemand = Mutex<CriticalSectionRawMutex, Cell<EffectiveDemand>>;

/// The coordinator's handle on the input worker.
#[derive(Clone, Copy)]
pub struct Input<'a> {
    /// Coordinator → worker: what each source is asked to do this pass.
    pub to: &'a SharedDemand,
    /// Worker → coordinator: the newest microphone window, published by the driver's poll.
    pub from: &'static CaptureWindow,
}

impl Input<'_> {
    /// The coordinator's half of the demand slot: the cell holds a `Copy` record, and the feed
    /// interrupt takes the same lock, so the section is one compare and at most one bounded copy.
    pub fn publish(&self, demand: &EffectiveDemand) {
        let wanted = *demand;
        self.to.lock(|current| {
            if current.get() != wanted {
                current.set(wanted);
            }
        });
    }
}

/// Off the intent ring: at 1.7 KB the window would set every ring slot that wide.
pub struct CaptureWindow(Channel<CriticalSectionRawMutex, AudioSample, 1>);

impl CaptureWindow {
    pub fn take(&self) -> Option<AudioSample> {
        self.0.try_receive().ok()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.0.ready_to_receive()
    }

    /// The driver's half: the newest window replaces the one nobody took.
    pub(crate) fn push(&self, sample: AudioSample) {
        let _ = self.0.try_send(sample);
    }
}

impl CaptureSink for CaptureWindow {
    fn publish(&self, sample: AudioSample) {
        self.push(sample);
    }
}

pub(crate) static CAPTURE: CaptureWindow = CaptureWindow(Channel::new());

pub type PlaybackQueue = Channel<CriticalSectionRawMutex, PlaybackCommand, 4>;

static PLAYBACK_QUEUE: PlaybackQueue = PlaybackQueue::new();

/// The coordinator's handle on the playback worker: commands go out, and a sound's completion
/// comes back as an intent, so there is no `from` side here.
#[derive(Clone, Copy)]
pub struct Playback {
    pub to: &'static PlaybackQueue,
}

pub(crate) static PLAYBACK: Playback = Playback {
    to: &PLAYBACK_QUEUE,
};

/// How many commands may wait for the network worker. The coordinator keeps the one that
/// found no room and offers it again next pass, so this is slack, not a depth the panel can
/// fill.
pub(crate) const COMMAND_SLOTS: usize = 2;

pub type NetworkQueue = Channel<CriticalSectionRawMutex, NetworkCommand, COMMAND_SLOTS>;

/// The coordinator's handle on the network worker.
///
/// `Copy`: every field is a handle to a static, so either side can take it by value.
#[derive(Clone, Copy)]
pub struct Network {
    /// Coordinator → worker: what the panel asked the link to do.
    pub commands: &'static NetworkQueue,
    /// Coordinator → worker: the page the panel is showing, as a level.
    pub page: &'static PageWatch,
    /// Worker → coordinator: the newest scan list, and which round produced it.
    pub scan: &'static Scan,
}

static NETWORK_QUEUE: NetworkQueue = NetworkQueue::new();

static PAGE: PageWatch = PageWatch::new();

static SCAN: Scan = Scan::new();

pub static NETWORK: Network = Network {
    commands: &NETWORK_QUEUE,
    page: &PAGE,
    scan: &SCAN,
};

/// The scan worker's published snapshot: the newest list, plus a counter that advances once
/// per completed round even when the list did not change. The page asks `/rescan`, reads the
/// counter from that answer, and polls until it moves — a list alone cannot say whether the
/// round it asked for has run or is still the one from before.
pub struct Scan {
    list: Latest<ApList>,
    generation: AtomicU32,
}

impl Scan {
    pub(crate) const fn new() -> Self {
        Self {
            list: Latest::new(),
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

/// The newest value only: the panel draws one list, so a burst of scans is one list plus the
/// news that it changed.
pub struct Latest<T> {
    value: Mutex<CriticalSectionRawMutex, Cell<Option<T>>>,
    fresh: Mutex<CriticalSectionRawMutex, Cell<bool>>,
    arrived: Signal<CriticalSectionRawMutex, ()>,
}

impl<T: Copy + Send> Latest<T> {
    const fn new() -> Self {
        Self {
            value: Mutex::new(Cell::new(None)),
            fresh: Mutex::new(Cell::new(false)),
            arrived: Signal::new(),
        }
    }

    /// Takes the newest value once per publish, leaving it in place: the panel draws it and the
    /// phone's page reads the same list with [`Self::peek`], so a taken list is not a gone list.
    pub fn take(&self) -> Option<T> {
        if !self.fresh.lock(|slot| slot.replace(false)) {
            return None;
        }
        self.value.lock(|slot| slot.get())
    }

    /// Reads the newest value without taking it: setup mode's page and the panel draw the same
    /// list, and the phone's request is not a consumer.
    pub fn peek(&self) -> Option<T> {
        self.value.lock(|slot| slot.get())
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.arrived.wait()
    }

    /// By reference: the whole list would otherwise be copied into the scan future's frame.
    pub fn publish(&self, value: &T) {
        self.value.lock(|slot| slot.set(Some(*value)));
        self.fresh.lock(|slot| slot.set(true));
        self.arrived.signal(());
    }
}

/// The panel's page, for the network worker, which cannot reach device state. A level, not
/// an edge: the worker asks whether the list is on screen, and the end of the splash reaches
/// it as a command like everything else the panel asks for.
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

/// The coordinator's handles on every worker, in one value.
#[derive(Clone, Copy)]
pub struct Workers<'a> {
    pub input: Input<'a>,
    pub playback: Playback,
    pub network: Network,
}

impl<'a> Workers<'a> {
    /// The handles as the composition root builds them. One constructor, so a new worker
    /// cannot be added to the struct and wired in one place but not the other.
    pub fn new(to: &'a SharedDemand) -> Self {
        Self {
            input: Input { to, from: &CAPTURE },
            playback: PLAYBACK,
            network: NETWORK,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::{Context, Waker};
    use iot_core::drivers::network::{AccessPoint, Ssid};

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

        let list = latest.take().expect("the newest list is handed over");
        assert!(
            !list.is_saved(0),
            "the newest list is the one the page is handed, and the mark is off it"
        );
        assert!(
            latest.take().is_none(),
            "and it is handed over once per publish"
        );
        assert!(
            latest.peek().is_some(),
            "the copy stays for the phone's page, which is not a consumer"
        );
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

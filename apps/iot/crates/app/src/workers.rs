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

use core::cell::RefCell;
use core::future::Future;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use iot_core::demand::EffectiveDemand;
use iot_core::drivers::audio::AudioSample;
use iot_core::drivers::network::ApList;
use iot_core::state::{DisplayPage, NetworkCommand, PlaybackCommand};

/// The demand the input worker reads and the coordinator writes, under the same lock the
/// feed interrupt takes.
pub type SharedDemand = Mutex<CriticalSectionRawMutex, RefCell<EffectiveDemand>>;

/// The coordinator's handle on the input worker.
#[derive(Clone, Copy)]
pub struct Input<'a> {
    /// Coordinator → worker: what each source is asked to do this pass.
    pub to: &'a SharedDemand,
    /// Worker → coordinator: the newest microphone window, published by the driver's poll.
    pub from: &'static CaptureWindow,
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

pub(crate) static CAPTURE: CaptureWindow = CaptureWindow(Channel::new());

pub type SpeakerCommands = Channel<CriticalSectionRawMutex, PlaybackCommand, 4>;

static SPEAKER_COMMANDS: SpeakerCommands = SpeakerCommands::new();

/// The coordinator's handle on the speaker worker: commands go out, and a sound's completion
/// comes back as an intent, so there is no `from` side here.
#[derive(Clone, Copy)]
pub struct Speaker {
    pub to: &'static SpeakerCommands,
}

pub(crate) const SPEAKER: Speaker = Speaker {
    to: &SPEAKER_COMMANDS,
};

/// How many commands may wait for the network worker. The coordinator keeps the one that
/// found no room and offers it again next pass, so this is slack, not a depth the panel can
/// fill.
const COMMAND_SLOTS: usize = 2;

pub type NetCommands = Channel<CriticalSectionRawMutex, NetworkCommand, COMMAND_SLOTS>;

/// The coordinator's handle on the network worker.
///
/// `Copy`: every field is a handle to a static, so either side can take it by value.
#[derive(Clone, Copy)]
pub struct Net {
    /// Coordinator → worker: what the panel asked the link to do.
    pub commands: &'static NetCommands,
    /// Coordinator → worker: the page the panel is showing, as a level.
    pub page: &'static PageWatch,
    /// Worker → coordinator: the newest scan list.
    pub scan: &'static Latest<ApList>,
}

static NET_COMMANDS: NetCommands = NetCommands::new();

static PAGE: PageWatch = PageWatch::new();

static SCAN: Latest<ApList> = Latest::new();

pub const NET: Net = Net {
    commands: &NET_COMMANDS,
    page: &PAGE,
    scan: &SCAN,
};

/// Drops setup requests left over from a previous session; every other command goes back.
///
/// A cancel made while the last session was up must not close the next one, and a queue has
/// no `clear()` of its own. The array is as deep as the channel, so it holds everything that
/// is not stale.
#[cfg(any(feature = "wifi", test))]
pub(crate) fn discard_setup(commands: &NetCommands) {
    let mut kept: [Option<NetworkCommand>; COMMAND_SLOTS] = core::array::from_fn(|_| None);
    let mut held = 0;
    while let Ok(command) = commands.try_receive() {
        if command.is_setup() {
            continue;
        }
        if let Some(slot) = kept.get_mut(held) {
            *slot = Some(command);
            held += 1;
        }
    }
    for command in kept.into_iter().flatten() {
        let _ = commands.try_send(command);
    }
}

/// The newest value only: the panel draws one list, so a burst of scans is one list plus the
/// news that it changed.
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

    /// By reference: the whole list would otherwise be copied into the survey future's frame.
    pub fn publish(&self, value: &T) {
        self.value.signal(*value);
        self.arrived.signal(());
    }
}

/// The panel's page, for the network worker, which cannot reach device state.
pub struct PageWatch {
    page: AtomicU8,
    splash: AtomicU8,
    changed: Signal<CriticalSectionRawMutex, ()>,
}

impl PageWatch {
    const fn new() -> Self {
        Self {
            page: AtomicU8::new(DisplayPage::Ambient as u8),
            splash: AtomicU8::new(0),
            changed: Signal::new(),
        }
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
}

#[cfg(any(feature = "wifi", test))]
impl PageWatch {
    pub fn shows(&self, page: DisplayPage) -> bool {
        self.page.load(Ordering::Relaxed) == page as u8
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

    /// Waits until the panel is on `page`.
    pub async fn wait_for(&self, page: DisplayPage) {
        while !self.shows(page) {
            self.changed.wait().await;
        }
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

/// The coordinator's handles on every worker, in one value.
#[derive(Clone, Copy)]
pub struct Workers<'a> {
    pub input: Input<'a>,
    pub speaker: Speaker,
    pub net: Net,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::{Context, Waker};
    use iot_core::drivers::network::{AccessPoint, Front, JoinRequest, Ssid};

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
    fn a_cancel_left_over_from_the_last_session_does_not_survive_into_the_next() {
        let commands = NetCommands::new();
        assert!(
            commands
                .try_send(NetworkCommand::LeaveSetup {
                    from: Front::AccessPoint
                })
                .is_ok()
        );
        let join = NetworkCommand::Join(JoinRequest::Held(Ssid::from_octets(b"cafe")));
        assert!(commands.try_send(join.clone()).is_ok());

        discard_setup(&commands);

        assert_eq!(
            commands.try_receive(),
            Ok(join),
            "the join is not setup state and goes back untouched"
        );
        assert!(
            commands.try_receive().is_err(),
            "the previous session's cancel is gone"
        );
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

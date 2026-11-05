//! The input worker's side: the demand it reads, the capture window it publishes, and the
//! coordinator's copy of both.

use core::cell::Cell;
use core::future::Future;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use iot_core::demand::EffectiveDemand;
use iot_core::drivers::audio::{AudioSample, CaptureSink};

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

impl<'a> Input<'a> {
    pub fn new(to: &'a SharedDemand) -> Self {
        Self { to, from: &CAPTURE }
    }
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
        if self.0.try_send(sample).is_ok() {
            super::WAKE.signal(());
        }
    }
}

impl CaptureSink for CaptureWindow {
    fn publish(&self, sample: AudioSample) {
        self.push(sample);
    }
}

pub(crate) static CAPTURE: CaptureWindow = CaptureWindow(Channel::new());

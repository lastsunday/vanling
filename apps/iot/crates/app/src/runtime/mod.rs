//! The tasks beside the model: the coordinator, the workers it drives, and the I/O each worker
//! owns.
//!
//! A worker reports upward as an intent and is asked downward by a command; nothing here
//! defines a page or paints a pixel.

pub(crate) mod coordinator;
pub mod feature;
pub(crate) mod system;
pub mod workers;

use crate::runtime::feature::input::IntentBus;
use crate::runtime::workers::{CaptureWindow, FeedWake, Network, Playback};
use iot_core::boot::Readiness;

/// Every handle both halves of the composition touch, built once by the entry and passed down.
///
/// Held by the entry's own `static` and reached through a parameter, never through a module
/// path: a signature says which handles a function uses, and a test can build its own.
pub struct Runtime {
    /// Input and the workers report upward here; the coordinator drains it once per frame.
    pub bus: IntentBus,
    /// The newest microphone window: the audio driver's sink and the coordinator's pull.
    pub capture: CaptureWindow,
    /// The panel's commands, the page it shows, and the list coming back.
    pub network: Network,
    /// The speaker's commands and its feeder's wake.
    pub playback: Playback,
    pub feed_wake: FeedWake,
    /// What this boot has proved; the watchdog feeder reads it.
    pub readiness: Readiness,
}

impl Runtime {
    pub const fn new() -> Self {
        Self {
            bus: IntentBus::new(),
            capture: CaptureWindow::new(),
            network: Network::new(),
            playback: Playback::new(),
            feed_wake: FeedWake::new(),
            readiness: Readiness::new(),
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

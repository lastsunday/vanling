//! The coordinator and its workers cannot reach each other's state, so this is the whole
//! vocabulary they share: one file per worker, and the shapes those handles are made of.
//!
//! A worker is a task that owns something the coordinator must not block on: the drivers it
//! polls, the codec's I2C, the radio's stack. Each field of [`Workers`] is that worker's side
//! of the boundary; adding a worker adds a field and a file, not a `TO_X`/`FROM_X` pair at the
//! call site. [`inbox`] holds the two shapes the handles are made of and names no worker.
//!
//! Reports go the other way as intents, so the only thing a worker publishes here is state it
//! owns, such as the scan list. Compiled with and without `wifi`: the coordinator holds these
//! handles either way, and a build with no radio simply never answers them.

mod inbox;
mod input;
mod network;
mod playback;

pub use self::inbox::{Inbox, Retained};
pub(crate) use self::input::CAPTURE;
pub use self::input::{CaptureWindow, Input, SharedDemand};
pub(crate) use self::network::COMMAND_SLOTS;
pub use self::network::{NETWORK, Network, NetworkInbox, PageWatch, Scan};
pub use self::playback::{Playback, PlaybackQueue};

use self::playback::PLAYBACK;

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

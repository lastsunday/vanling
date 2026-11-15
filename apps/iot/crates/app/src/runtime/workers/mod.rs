//! The coordinator and its workers cannot reach each other's state, so this is the whole
//! vocabulary they share: one file per worker, and the shapes those handles are made of.
//!
//! A worker is a task that owns something the coordinator must not block on: the drivers it
//! polls, the codec's I2C, the radio's stack. Each handle is that worker's side of the
//! boundary; the system that drives it holds the handle, so adding a worker adds a file, one
//! `register` at the composition root, and one field in [`crate::runtime::Runtime`]. There is no
//! shared handle struct to edit, and nothing here names a worker the others may not use.
//! [`inbox`] holds the two shapes the handles are made of and names no worker.
//!
//! Reports go the other way as intents, so the only thing a worker publishes here is state it
//! owns, such as the scan list. Compiled with and without `wifi`: the coordinator holds these
//! handles either way, and a build with no radio simply never answers them.

mod inbox;
mod input;
mod network;
mod playback;

pub use self::inbox::{Inbox, Retained};
pub use self::input::{CaptureWindow, Input, SharedDemand};
pub(crate) use self::network::COMMAND_SLOTS;
pub use self::network::{Network, NetworkInbox, PageWatch, Scan};
pub use self::playback::{FeedWake, Playback, PlaybackQueue};

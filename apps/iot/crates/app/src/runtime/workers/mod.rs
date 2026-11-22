//! The vocabulary the coordinator and its workers share: one file per worker, plus the shapes
//! their handles are made of. A worker owns something the coordinator must not block on; adding
//! one adds a file, a `register` at the composition root, and a field in
//! [`crate::runtime::Runtime`]. Reports go back as intents; `wifi` only decides whether they
//! are ever answered.

mod inbox;
mod input;
mod network;
mod playback;

pub use self::inbox::{Inbox, Retained};
pub use self::input::{CaptureWindow, Input, SharedDemand};
pub(crate) use self::network::COMMAND_SLOTS;
pub use self::network::{Network, NetworkInbox, PageWatch, Scan};
pub use self::playback::{FeedWake, Playback, PlaybackQueue};

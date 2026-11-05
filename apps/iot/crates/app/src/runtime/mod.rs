//! The tasks beside the model: the frame loop, the workers it drives, and the I/O each worker
//! owns.
//!
//! A worker reports upward as an intent and is asked downward by a command; nothing here
//! defines a page or paints a pixel.

pub(crate) mod frame_loop;
pub mod input;
#[cfg(feature = "wifi")]
pub mod network;
pub(crate) mod playback;
pub(crate) mod replay;
pub mod workers;

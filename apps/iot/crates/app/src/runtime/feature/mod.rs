//! The workers themselves, one module per feature: each owns the I/O its feature drives and
//! reports upward as intents.
//!
//! The shared vocabulary they speak lives in [`crate::runtime::workers`]; a feature may reach
//! for it, never the other way around.

pub mod input;
#[cfg(feature = "wifi")]
pub mod network;
pub(crate) mod playback;

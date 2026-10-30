//! Bench probes: one measurement each, run by the matching `src/bin/*-probe.rs`.
//!
//! Compiled into every build on purpose: a probe nothing compiles rots in place.

pub mod audio;
pub mod echo;

/// The canceller against a real room, through the product's own pairing.
#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod echo_room;

/// Writes the clock registers directly, which only this part can do.
#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod clock;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod motion;

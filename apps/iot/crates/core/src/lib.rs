//! Chip-independent logic for the Vanling firmware: the driver models every board shares,
//! the per-plane demand model, and the page/state/render machinery.
//!
//! No chip or board dependency, so it compiles and tests on the host. What a board supplies
//! is injected through the capability traits here; what a chip decides (clocks, heap) stays
//! in `iot-chip-esp`.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod config;
pub mod demand;
pub mod diagnostics;
pub mod drivers;
pub mod horizon;
pub mod intent;
pub mod keyboard;
pub mod overlay;
pub mod page;
pub mod render;
pub mod stamp;
pub mod state;

//! Bench-only drivers: the probe bodies behind `bin/*-probe.rs`, and the panel replay the
//! `panel-bench` feature selects instead of the product frame loop.
//!
//! `probe` is referenced only by the probe binaries. `replay` registers one ordinary system and
//! drives the panel through the same intent bus a finger does; the product entry names it only
//! under that feature, and the library's frame loop never does. What a bench build should join is
//! decided in `state::env`.

use alloc::boxed::Box;

use iot_core::demand::ClockSink;
use iot_core::drivers::board::{HasAudio, HasCamera, HasInput, HasMotion, HasPlayback};

use crate::Wiring;

pub mod probe;
pub(crate) mod replay;

/// The panel replay's entry: the product frame loop with [`replay::Slot`] registered.
pub async fn run<B, C>(board: B, wiring: Wiring<C>) -> !
where
    B: HasInput + HasMotion + HasAudio + HasPlayback + HasCamera,
    C: ClockSink,
{
    crate::run_with(board, wiring, Some(Box::new(replay::Slot::new()))).await
}

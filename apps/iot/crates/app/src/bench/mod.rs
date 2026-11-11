//! Bench-only drivers: the bodies behind `bin/*-probe.rs` and the panel replay behind
//! `bin/panel-bench.rs`.
//!
//! `probe` is referenced only by the probe binaries. `replay` is reached only by [`run`], the
//! entry `bin/panel-bench.rs` calls: it registers one ordinary system in the frame loop and
//! drives the panel through the same intent bus a finger does. The product entry (`main`) and
//! the library's frame loop name neither, so the bench only ever depends downward. What a bench
//! build should join is decided in `state::boot`, next to the other build-time configuration.

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

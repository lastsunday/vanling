//! Bench-only drivers: the bodies behind `bin/*-probe.rs` and the panel replay the product loop
//! reaches under `IOT_PANEL_BENCH`.
//!
//! `probe` is referenced only by the probe binaries. `replay` is called by the coordinator and
//! the network worker, and folds away when the build is not a bench run.

pub mod probe;
pub(crate) mod replay;

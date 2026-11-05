//! Bench-only drivers: the bodies behind `bin/*-probe.rs` and the panel replay the product loop
//! reaches under `IOT_PANEL_BENCH`.
//!
//! `probe` is referenced only by the probe binaries. `replay` is registered by the composition
//! root under `IOT_PANEL_BENCH`, and the network worker asks [`replay::build_credentials`] what a
//! bench build should join. Both fold away when the build is not a bench run, and neither makes
//! the bench depend on the coordinator.

pub mod probe;
pub(crate) mod replay;

//! Build-time probes: the bodies behind `bin/*-probe.rs`.
//!
//! The panel replay a product build can reach is not here — it lives with the runtime that
//! drives it (`runtime::replay`).

pub mod probe;

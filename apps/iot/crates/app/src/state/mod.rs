//! The composition root: the aggregate state, the manager that routes intents into it, and
//! the two leaves a boot decides before any state exists.
//!
//! This is the only module allowed to know every feature: it composes the shared records
//! with the Network page's own draft and hands the runtime one state to read.

mod apply;

pub mod boot;

mod device;

mod manager;

pub(crate) mod translate;

mod wiring;

pub use self::device::DeviceState;

pub use self::manager::DeviceManager;

pub use self::wiring::Boot;

pub use crate::shared::route::{DisplayPage, Navigator};

pub use crate::shared::state::*;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;

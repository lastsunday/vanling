//! The behaviour over the shared state: the manager that routes intents into it, the dispatch
//! table that hands each input to the page that owns it, and the two leaves a boot decides
//! before any state exists.
//!
//! The records themselves live in [`crate::shared::state`]; this module is the only one
//! allowed to know every page, because it is the one that dispatches to them.

mod apply;

pub mod boot;

mod manager;

pub(crate) mod translate;

mod wiring;

pub use self::manager::DeviceManager;

pub use self::wiring::Boot;

pub use crate::shared::route::{Navigator, Page};

pub use crate::shared::state::*;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;

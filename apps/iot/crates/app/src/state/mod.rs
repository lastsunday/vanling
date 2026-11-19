//! The behaviour over the shared state: the owned value and its demand on the read side, and
//! every write on the write side.
//!
//! The records themselves live in [`crate::shared::model`]; that module is re-exported here as
//! the facade callers write (`state::PageId`, `state::Command`).

pub mod env;

mod manager;

pub use self::manager::{Boot, DeviceManager};

pub use crate::shared::route::{Navigator, PageId};

pub use crate::shared::model::*;

#[cfg(test)]
pub(crate) use self::tests::support as test_support;

#[cfg(test)]
mod tests;

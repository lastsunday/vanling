//! The behaviour over the shared state: the owned value and its demand on the read side, every
//! write on the write side, and the one dispatch table that knows all pages.
//!
//! The records themselves live in [`crate::shared::model`]; that module is re-exported here as
//! the facade callers write (`state::Page`, `state::Command`), and this module is the only one
//! allowed to know every page, because it is the one that dispatches to them.

pub mod env;

mod intent;

mod manager;

pub(crate) mod translate;

pub use self::manager::{Boot, DeviceManager};

pub use crate::shared::route::{Navigator, Page};

pub use crate::shared::model::*;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;

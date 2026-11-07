//! The product's own vocabulary: the pages it has, the state it is in, and what it can be
//! asked.
//!
//! Nothing here opens a file, drives a pin or paints a pixel, so every module compiles for a
//! host test with no board attached.

pub mod boot;
pub mod intent;
pub mod keyboard;
pub mod layout;
pub mod route;
pub mod state;

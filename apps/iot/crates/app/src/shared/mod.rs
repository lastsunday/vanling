//! Cross-page truth and vocabulary, one bucket per change reason: `model` (records), `route`
//! (destinations), `intent` (inbound messages), `gesture` (answers pages share), `kit` (pixel
//! primitives), `diagnostics` (the snapshot).
//!
//! A page's private state stays in the page. This is the leaf of the module graph: it may not
//! name a layer that reads it.

pub mod diagnostics;
pub mod gesture;
pub mod intent;
pub mod kit;
pub mod model;
pub mod route;

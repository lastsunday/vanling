//! Cross-page truth and vocabulary, one bucket per change reason: `model` (records), `route`
//! (destinations), `intent` (inbound messages), `kit` (pixel primitives), `snapshot` (frame and
//! page views). A page's private state stays in the page; this is the graph's leaf.

pub mod intent;
pub mod kit;
pub mod model;
pub mod route;
pub mod snapshot;

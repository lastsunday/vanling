//! Cross-page truth and vocabulary: the app's shared records, action vocabulary, route
//! table, keyboard layers, diagnostic snapshot, and the pixel primitives every page draws
//! with.
//!
//! Nothing here owns a page's private state; a feature does. What lives here is what more
//! than one page reads or writes.

pub mod diagnostics;
pub mod intent;
pub mod keyboard;
pub mod light;
pub mod route;
pub mod state;
pub mod view_kit;

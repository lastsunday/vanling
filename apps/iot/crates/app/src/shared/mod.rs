//! Cross-page truth and vocabulary, one bucket per change reason: `model` records, `route`
//! destinations, `intent` messages, `i18n` words, `ui` widgets, `page` layout, `surface` painting,
//! `snapshot` views. A page's own state stays in the page; this is the graph's leaf.

pub mod i18n;
pub mod intent;
pub mod model;
pub mod page;
pub mod route;
pub mod snapshot;
pub mod surface;
pub mod ui;

//! The presentation side: what a panel or a lamp should show, the pages that describe it, and
//! the wiring that gives each surface the handle it draws through.
//!
//! [`wiring::wired`] is the only place a board's surface becomes a renderer; `pages` answers
//! *what* to show and `render` answers *how* to drive the surface.

pub mod diagnostics;
pub mod pages;
pub mod render;
pub mod wiring;

pub use self::wiring::{Renderers, wired};

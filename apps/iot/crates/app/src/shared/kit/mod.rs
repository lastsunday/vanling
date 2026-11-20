//! The pixel primitives every page draws with: the panel, its shared chrome, and the text
//! vocabulary the product prints.

pub mod chrome;
pub mod panel;
pub mod scroll;
pub mod text;

pub use self::panel::Panel;

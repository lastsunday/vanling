//! The pixel primitives every page draws with: the panel, its shared chrome, and the text
//! vocabulary the product prints.

pub mod chrome;
pub mod i18n;
pub mod keyboard;
pub mod panel;
pub mod scroll;
pub mod slot;
pub mod state;
pub mod tag;
pub mod text;
pub mod theme;
pub mod widgets;

pub use self::panel::Panel;
pub use self::theme::{ButtonKind, Theme};

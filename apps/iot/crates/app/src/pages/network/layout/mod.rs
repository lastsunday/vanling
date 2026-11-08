//! The Network page's controls: the boxes a finger lands on, and the one geometry both the
//! drawing and the hit test read.
//!
//! Split by what a tap is aimed at — the key grid, the scan list, the action strip, the
//! confirmation box — so a page that draws one and the input that tests it cannot drift
//! apart.

pub mod dialog;
pub mod keyboard;
pub mod list;
pub mod toolbar;

pub use self::dialog::*;
pub use self::keyboard::*;
pub use self::list::*;
pub use self::toolbar::*;

/// The height of one touch target, in panel rows: what a finger can be asked to aim at.
pub const TOUCH_MIN: usize = 44;

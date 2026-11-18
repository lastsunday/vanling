//! The boxes a finger lands on, and the one geometry both the drawing and the hit test read.
//! One file per band's targets: the list or key grid, the strip, the confirmation box.

pub mod content;
pub mod dialog;
pub mod toolbar;

pub use self::content::*;
pub use self::dialog::*;
pub use self::toolbar::*;

/// The height of one touch target, in panel rows: what a finger can be asked to aim at.
pub const TOUCH_MIN: usize = 44;

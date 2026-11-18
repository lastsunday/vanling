//! The pixel primitives every page draws with: the painter, its widgets, and the bands that
//! more than one page draws.

pub mod bands;
pub mod overlay;
pub mod painter;
pub mod placement;
pub mod text;
pub mod widgets;

pub use self::painter::Painter;

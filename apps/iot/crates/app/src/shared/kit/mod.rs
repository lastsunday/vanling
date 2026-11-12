//! The pixel primitives every page draws with: the surface, its widgets, and the bands that
//! more than one page stamps.

pub mod bands;
pub mod overlay;
pub mod placement;
pub mod surface;
pub mod text;
pub mod widgets;

pub use self::surface::Surface;

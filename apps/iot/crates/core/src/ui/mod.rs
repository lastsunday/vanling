//! Pixels and ink for a panel-sized surface: a canvas, a font, and glyph arithmetic.
//!
//! Nothing here knows a product, a page, or a reading.

pub mod canvas;
mod font;
pub mod text;

pub use canvas::Canvas;

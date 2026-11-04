//! Pixels and ink for a panel-sized surface: a canvas, a font, and glyph arithmetic.
//!
//! Nothing here knows a product, a page, or a reading.

#![no_std]

#[cfg(test)]
extern crate alloc;

pub mod canvas;
mod font;
pub mod text;

pub use canvas::Canvas;

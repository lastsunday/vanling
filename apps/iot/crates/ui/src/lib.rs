//! Portable page rendering for a panel-sized surface.
//!
//! The crate is the boundary between what a page says and the peripheral that shows it: it owns
//! the glyph table, the reading-to-glyph arithmetic, the layout and the immediate-mode page
//! drawing; the board owns the frame buffer, the panel driver, the camera chain and the page
//! choice. Nothing here names a chip, a board or a sensor, so the pages build and test on the
//! development machine.
//!
//! The layering is `iot-core → iot-ui → iot-bsp-esp → iot-app`: semantic state and geometry are
//! in `iot-core`, the drawing is here, and `iot-bsp-esp` adapts the panel into the surface this
//! crate paints on.

#![no_std]

#[cfg(test)]
extern crate alloc;

pub mod canvas;
mod font;
mod layout;
pub mod pages;
pub mod surface;
pub mod text;

pub use canvas::Canvas;
pub use surface::{CameraReadout, Surface};

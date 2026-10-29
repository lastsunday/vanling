//! Portable page rendering for a panel-sized surface.

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

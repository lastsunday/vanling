//! Pixels, text, and the boxes a page declares: the panel's whole drawing surface.
//!
//! A page states what it wants while its `view` runs; [`Ui`] builds the tree, lays it out once,
//! then draws its text and custom marks into the canvas it was handed. The same declaration can
//! run without a canvas, which is how a hit test uses the picture's own rectangles.

pub mod canvas;
pub mod font;
pub mod layout;
pub mod panel;
pub mod text;
pub mod typeset;

mod declare;
mod geometry;
mod paint;
mod screen;

pub use canvas::Canvas;
pub use declare::Ui;
pub use geometry::Rect;
pub use layout::{
    Align, BoxConstraints, Flow, Justify, Length, NodeId, PX, ScrollId, ScrollInfo, Style,
    TextAlign,
};
pub use screen::Screen;

//! Pixels, text, and the boxes a page declares: the panel's whole drawing surface.
//!
//! A page states what it wants while its `view` runs; [`Ui`] builds the tree, lays it out once,
//! then draws its text and custom marks into the canvas it was handed. The same declaration can
//! run without a canvas, which is how a hit test uses the picture's own rectangles.

pub mod canvas;
pub mod color;
pub mod font;
pub mod layout;
pub mod panel;
pub mod text;
pub mod typeset;
pub mod widget;

mod declare;
mod geometry;
mod paint;
mod screen;

pub use canvas::{Canvas, GlyphCache};
pub use color::{Color, ColorId};
pub use declare::{TEXT_BYTES, Ui};
pub use geometry::Rect;
pub use layout::{
    Align, BoxConstraints, Decoration, DecorationStyle, Flow, FontFamily, FontWeight, Justify,
    Length, MAX_NODES, MAX_SCROLL_AREAS, NodeId, PX, RunStyle, ScrollId, ScrollInfo, Style,
    TextAlign,
};
pub use screen::Screen;
pub use widget::Widget;

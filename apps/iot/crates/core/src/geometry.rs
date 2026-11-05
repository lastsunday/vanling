//! A rectangle on the panel, in pixels: the one shape drawing and hit testing agree on.

/// A rectangle on the panel, in pixels: the box a control is drawn in and the box a tap is
/// tested against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

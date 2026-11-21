//! The arena one declaration lives in: nodes, their identity, and what layout hands back.

use super::constraints::{BoxConstraints, Point, Size, Subpx};
use super::style::{RunStyle, Style};
use crate::ui::Rect;

/// Boxes one declaration may hold. The fullest page is Text at 56 (one box per wrapped line),
/// and every node is a slice of the panel core's stack, so the arena is sized to the page with
/// a few boxes to grow into rather than to a round number.
pub const MAX_NODES: usize = 60;
pub const NONE: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Box,
    Text { start: u16, len: u16 },
    Paint { slot: u8 },
}

/// The constraints a measurement was taken at, four fields packed into one word so a node
/// keeps its stack budget. `u64::MAX` is a value no constraint produces, so a freshly
/// declared node never reads as measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Measured(u64);

impl Default for Measured {
    fn default() -> Self {
        Self(u64::MAX)
    }
}

impl Measured {
    pub(super) fn of(constraints: BoxConstraints) -> Self {
        let pack = |value: Subpx| value.clamp(0, i16::MAX as Subpx) as u64;
        Self(
            pack(constraints.min_w)
                | pack(constraints.max_w) << 16
                | pack(constraints.min_h) << 32
                | pack(constraints.max_h) << 48,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub style: Style,
    pub kind: NodeKind,
    pub parent: u16,
    pub first: u16,
    pub last: u16,
    pub next: u16,
    pub offset: Point,
    pub measured: Size,
    /// The constraints `measured` came from. A node is asked for its size many times in one
    /// declaration — flex measures, then places, then measures again — and every ask is a full
    /// pass of glyph lookups in flash unless the answer is kept.
    pub measured_at: Measured,
    pub alloc: Subpx,
    pub rect: Rect,
    pub clip: Rect,
    pub clipped: bool,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            style: Style::new(),
            kind: NodeKind::Box,
            parent: NONE,
            first: NONE,
            last: NONE,
            next: NONE,
            offset: Point::default(),
            measured: Size::default(),
            measured_at: Measured::default(),
            alloc: 0,
            rect: Rect::default(),
            clip: Rect::default(),
            clipped: false,
        }
    }
}

/// A paint override over part of one run, with offsets already made absolute in the
/// declaration's text buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub start: u16,
    pub end: u16,
    pub style: RunStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub kind: NodeKind,
    pub style: Style,
    pub rect: Rect,
    pub clip: Rect,
    pub clipped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u16);

impl NodeId {
    pub const NONE: Self = Self(NONE);

    pub fn is_none(self) -> bool {
        self.0 == NONE
    }
}

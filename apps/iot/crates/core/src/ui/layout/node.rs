//! The arena one declaration lives in: nodes, their identity, and what layout hands back.

use super::constraints::{BoxConstraints, Point, Size, Subpx};
use super::style::{RunStyle, Style};
use crate::ui::Rect;

/// Boxes one declaration may hold. The fullest page is the menu at 62 (63 with its scroll thumb;
/// its page buttons are two boxes each) and Text at 56, so the arena is sized to the page with a
/// box to grow into rather than to a round number.
pub const MAX_NODES: usize = 64;
pub const NONE: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Box,
    Text {
        start: u16,
        len: u16,
    },
    /// A leaf the page paints itself. `state` is the page's own fingerprint of what it draws;
    /// [`UNKNOWN_PAINT`] says the page does not describe its pixels.
    Paint {
        slot: u8,
        state: u32,
    },
}

/// The `state` a custom leaf carries when the page cannot say whether its pixels moved.
pub const UNKNOWN_PAINT: u32 = u32::MAX;

/// The constraints a measurement was taken at. A box keys on all four; a run keys on width alone
/// (a line breaks on width, not on the height it is offered) and keeps its natural height in the
/// high half, so a height change re-clamps without re-wrapping. `u64::MAX` marks a node unmeasured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Measured(u64);

impl Default for Measured {
    fn default() -> Self {
        Self(u64::MAX)
    }
}

impl Measured {
    pub(super) fn of(constraints: BoxConstraints) -> Self {
        Self(pack(constraints))
    }

    /// A run's key: the width it wrapped at, and the natural height it came out to.
    pub(super) fn of_text(constraints: BoxConstraints, height: Subpx) -> Self {
        Self(pack(constraints) & 0xFFFF_FFFF | u64::from(pack_height(height)) << 32)
    }

    /// Whether a run measured at `self` answers a question asked at `at`: width only.
    pub(super) fn answers_text(self, at: Self) -> bool {
        self.0 & 0xFFFF_FFFF == at.0 & 0xFFFF_FFFF
    }

    /// The unclamped height the run measured to.
    pub(super) fn natural(self) -> Subpx {
        (self.0 >> 32) as u32 as Subpx
    }
}

fn pack(constraints: BoxConstraints) -> u64 {
    let word = |value: Subpx| value.clamp(0, i16::MAX as Subpx) as u64;
    word(constraints.min_w)
        | word(constraints.max_w) << 16
        | word(constraints.min_h) << 32
        | word(constraints.max_h) << 48
}

fn pack_height(height: Subpx) -> u32 {
    height.max(0) as u32
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

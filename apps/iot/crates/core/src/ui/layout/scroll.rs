//! The scroll boxes a declaration can name, and what layout reports about them.

use crate::ui::Rect;

/// The name a page gives a scroll box so it can drive and read it.
///
/// `NONE` is the unnamed box: layout still clips it, but reports nothing. Names are unique
/// within one declaration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollId(pub u16);

impl ScrollId {
    pub const NONE: Self = Self(0);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn is_none(self) -> bool {
        self.0 == Self::NONE.0
    }
}

/// One named scroll box after layout: where it sits, what it shows, and how far it can go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollInfo {
    pub id: ScrollId,
    pub rect: Rect,
    /// The offset the declaration asked for, as declared: the clamp and the bounce are the
    /// page's, because only it knows how far past the edge it wants to go.
    pub offset: i16,
    /// Content extent along the scroll axis, in pixels.
    pub content: i16,
    /// Visible extent along the scroll axis, in pixels.
    pub viewport: i16,
}

impl ScrollInfo {
    /// How far the content can travel: zero when it fits.
    pub const fn max(self) -> i16 {
        let travel = self.content - self.viewport;
        if travel > 0 { travel } else { 0 }
    }
}

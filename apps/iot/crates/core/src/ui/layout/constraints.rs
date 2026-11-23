//! Subpixel lengths and the box constraints that bound them.
//! Fractions exist only between a container's edges and its children's; the rectangles a page
//! finally draws at are whole pixels.

use super::style::{Edges, Style};

/// Subpixels per pixel.
pub const PX: i32 = 64;

/// Subpixels per device-independent pixel. The product's glass is 2.0" at 320×240, so 200 dpi
/// against the 160 dpi a logical pixel is defined at: 1 dp = 5/4 px, exactly.
pub const DP: i32 = PX * 5 / 4;

/// Subpixels in one rem: the root text size, today the bitmap face's line box.
pub const REM: i32 = PX * crate::ui::panel::LINE as i32;

/// A finite stand-in for "no limit", far enough from overflow for a 320 px panel.
pub const INFINITY: i32 = 1 << 24;

pub type Subpx = i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxConstraints {
    pub min_w: Subpx,
    pub max_w: Subpx,
    pub min_h: Subpx,
    pub max_h: Subpx,
}

impl BoxConstraints {
    pub const fn loose(width: Subpx, height: Subpx) -> Self {
        Self {
            min_w: 0,
            max_w: width,
            min_h: 0,
            max_h: height,
        }
    }

    pub const fn tight(width: Subpx, height: Subpx) -> Self {
        Self {
            min_w: width,
            max_w: width,
            min_h: height,
            max_h: height,
        }
    }

    pub(super) fn deflate(self, padding: Edges) -> Self {
        let horizontal = padding.horizontal();
        let vertical = padding.vertical();
        Self {
            min_w: (self.min_w - horizontal).max(0),
            max_w: (self.max_w - horizontal).max(0),
            min_h: (self.min_h - vertical).max(0),
            max_h: (self.max_h - vertical).max(0),
        }
    }

    pub(super) fn enforce(self, style: &Style) -> Self {
        let mut constraints = self;
        if let Some(width) = style.width.resolve(self.max_w) {
            constraints.min_w = width.max(0);
            constraints.max_w = width.max(0);
        }
        if let Some(height) = style.height.resolve(self.max_h) {
            constraints.min_h = height.max(0);
            constraints.max_h = height.max(0);
        }
        if let Some(value) = style.min_width.resolve(self.max_w) {
            constraints.min_w = constraints.min_w.max(value);
        }
        if let Some(value) = style.min_height.resolve(self.max_h) {
            constraints.min_h = constraints.min_h.max(value);
        }
        if let Some(value) = style.max_width.resolve(self.max_w) {
            constraints.max_w = constraints.max_w.min(value);
        }
        if let Some(value) = style.max_height.resolve(self.max_h) {
            constraints.max_h = constraints.max_h.min(value);
        }
        constraints.min_w = constraints.min_w.clamp(0, constraints.max_w);
        constraints.min_h = constraints.min_h.clamp(0, constraints.max_h);
        constraints
    }

    pub(super) fn constrain(self, width: Subpx, height: Subpx) -> Size {
        Size {
            w: width.clamp(self.min_w, self.max_w),
            h: height.clamp(self.min_h, self.max_h),
        }
    }

    pub(super) fn main(self, horizontal: bool) -> (Subpx, Subpx) {
        if horizontal {
            (self.min_w, self.max_w)
        } else {
            (self.min_h, self.max_h)
        }
    }

    pub(super) fn cross(self, horizontal: bool) -> (Subpx, Subpx) {
        if horizontal {
            (self.min_h, self.max_h)
        } else {
            (self.min_w, self.max_w)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub w: Subpx,
    pub h: Subpx,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: Subpx,
    pub y: Subpx,
}

/// The one value a tight axis allows, so a child absolutely positioned against the edge can
/// find that edge before the parent's own size is clamped.
pub(super) fn tight(min: Subpx, max: Subpx) -> Option<Subpx> {
    (min == max && max < INFINITY).then_some(max)
}

pub(super) fn pixels(value: Subpx) -> i32 {
    (value + PX / 2).div_euclid(PX)
}

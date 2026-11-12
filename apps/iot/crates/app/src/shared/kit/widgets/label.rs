//! Placing text: how wide a run is, where it sits in a box, and the mark under a finger.

use iot_core::geometry::ControlRect;
use iot_core::overlay::{FONT_H, GLYPH_PITCH, OVERLAY_GAP};

use crate::shared::kit::surface::Surface;

/// The panel width of a run of glyphs: the gap after the last one is not ink.
pub(crate) fn text_width(text: &[u8]) -> usize {
    text.len() * GLYPH_PITCH - OVERLAY_GAP
}

/// The left edge that centres `text` inside `rect`.
pub(crate) fn text_left(rect: ControlRect, text: &[u8]) -> usize {
    rect.x + rect.w.saturating_sub(text_width(text)) / 2
}

impl Surface<'_> {
    /// Stamps `text` over a filled block: the run the finger is on.
    pub(crate) fn stamp_marked(&mut self, text: &[u8], left: usize, top: usize) {
        let (right, bottom) = (left + text_width(text), top + FONT_H);
        for y in top.saturating_sub(1)..=bottom {
            for x in left.saturating_sub(1)..=right {
                self.stamp_pixel(x as isize, y as isize);
            }
        }
        self.stamp_text(text, left, top);
    }
}

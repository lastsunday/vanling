//! Boxes: the outline a control is drawn in, and that outline with its word centred.

use iot_core::geometry::ControlRect;
use iot_core::overlay::FONT_H;

use super::label::text_left;
use crate::ui::pages::surface::Surface;

impl Surface<'_> {
    /// One pixel of ink around `rect`'s own edge, inside it.
    pub(crate) fn stamp_frame(&mut self, rect: ControlRect) {
        let (x, y) = (rect.x as isize, rect.y as isize);
        let (right, bottom) = (x + rect.w as isize - 1, y + rect.h as isize - 1);
        self.stamp_line(x, y, right, y);
        self.stamp_line(x, bottom, right, bottom);
        self.stamp_line(x, y, x, bottom);
        self.stamp_line(right, y, right, bottom);
    }

    /// A framed button: the outline, and `label` centred in the box it makes.
    pub(crate) fn stamp_cell(&mut self, rect: ControlRect, label: &[u8]) {
        self.stamp_frame(rect);
        self.stamp_text(
            label,
            text_left(rect, label),
            rect.y + rect.h.saturating_sub(FONT_H) / 2,
        );
    }
}

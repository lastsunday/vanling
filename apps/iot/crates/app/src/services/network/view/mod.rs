//! The Network page's bands, and the colours each one is drawn with.

pub(super) mod content;
pub(super) mod header;
pub(super) mod overlay;
pub(super) mod text;

use crate::services::network::controller as page_controls;
use crate::services::network::shape;
use crate::services::network::view::text::control_label;
use crate::shared::model::NetworkView;
use iot_core::drivers::network::LinkStatus;
use iot_core::geometry::ControlRect;
use iot_core::ui::Canvas;
use iot_core::ui::panel::LINE;

/// One pixel of colour around `rect`'s own edge, inside it.
pub(super) fn draw_frame(canvas: &mut Canvas<'_>, rect: ControlRect) {
    let (x, y) = (rect.x as isize, rect.y as isize);
    let (right, bottom) = (x + rect.w as isize - 1, y + rect.h as isize - 1);
    canvas.draw_line(x, y, right, y);
    canvas.draw_line(x, bottom, right, bottom);
    canvas.draw_line(x, y, x, bottom);
    canvas.draw_line(right, y, right, bottom);
}

/// A framed button: the outline, and `label` centred in the box it makes.
pub(super) fn draw_cell(canvas: &mut Canvas<'_>, rect: ControlRect, label: &[u8]) {
    draw_frame(canvas, rect);
    canvas.draw_text(
        label,
        text_left(canvas, rect, label),
        rect.y + rect.h.saturating_sub(LINE) / 2,
    );
}

/// Columns a run occupies: the face's own advance, with no gap after the last glyph.
pub(super) fn text_width(canvas: &Canvas<'_>, text: &[u8]) -> usize {
    canvas.measure(text)
}

/// The left edge that centres `text` inside `rect`.
pub(super) fn text_left(canvas: &Canvas<'_>, rect: ControlRect, text: &[u8]) -> usize {
    rect.x + rect.w.saturating_sub(text_width(canvas, text)) / 2
}

/// Draws `text` over a filled block: the run the finger is on.
pub(super) fn draw_marked(canvas: &mut Canvas<'_>, text: &[u8], left: usize, top: usize) {
    let (right, bottom) = (left + text_width(canvas, text), top + LINE);
    for y in top.saturating_sub(1)..=bottom {
        for x in left.saturating_sub(1)..=right {
            canvas.plot_pixel(x as isize, y as isize);
        }
    }
    canvas.draw_text(text, left, top);
}

/// The custom slots the page's declaration leaves, in the order it declares them.
pub(super) const HEADER: u8 = 0;
pub(super) const CONTENT: u8 = 1;
pub(super) const STRIP: u8 = 2;
pub(super) const OVERLAY: u8 = 3;

pub(super) fn draw_strip(canvas: &mut Canvas<'_>, draft: NetworkView, link: LinkStatus) {
    let strip = page_controls::strip(draft, link);
    if strip.is_split() {
        draw_cell(
            canvas,
            shape::toolbar_cell(shape::ToolbarCell::Left),
            control_label(strip.kind(shape::ToolbarCell::Left)),
        );
        draw_cell(
            canvas,
            shape::toolbar_cell(shape::ToolbarCell::Right),
            control_label(strip.kind(shape::ToolbarCell::Right)),
        );
    } else {
        draw_cell(
            canvas,
            shape::toolbar_box(),
            control_label(strip.kind(shape::ToolbarCell::Right)),
        );
    }
}

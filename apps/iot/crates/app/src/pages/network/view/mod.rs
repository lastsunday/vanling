//! The Network page's bands, and the strip under them.

mod content;
mod header;
mod overlay;
mod text;

use crate::pages::network::controller as page_controls;
use crate::pages::network::layout;
use crate::pages::network::view::text::control_label;
use crate::shared::kit::painter::Painter;
use crate::shared::model::NetworkView;
use iot_core::drivers::network::LinkStatus;

impl Painter<'_> {
    pub(super) fn draw_strip(&mut self, draft: NetworkView, link: LinkStatus) {
        let strip = page_controls::strip(draft, link);
        if strip.is_split() {
            self.draw_cell(
                layout::toolbar_cell(layout::ToolbarCell::Left),
                control_label(strip.kind(layout::ToolbarCell::Left)),
            );
            self.draw_cell(
                layout::toolbar_cell(layout::ToolbarCell::Right),
                control_label(strip.kind(layout::ToolbarCell::Right)),
            );
        } else {
            self.draw_cell(
                layout::toolbar_box(),
                control_label(strip.kind(layout::ToolbarCell::Right)),
            );
        }
    }
}

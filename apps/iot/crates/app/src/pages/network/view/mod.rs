//! The Network page's bands, and the strip under them.

mod content;
mod header;
mod overlay;
mod text;

use crate::pages::network::controller::{self as page_controls, Band, each_band};
use crate::pages::network::layout;
use crate::pages::network::view::text::control_label;
use crate::shared::kit::surface::Surface;
use crate::shared::model::NetworkView;
use iot_core::drivers::network::LinkStatus;

impl Surface<'_> {
    pub(crate) fn paint_network(&mut self) {
        let link = self.diagnostics.link;
        // Copied rather than borrowed: the borrow would be of `self.diagnostics`, and every
        // paint below takes `self` mutably. The audio sweep makes the same trade for the same
        // reason, and a list is 350 B against the envelope's kilobyte and a half.
        let aps = self.diagnostics.aps;
        let setup = self.diagnostics.pages.network;
        let shown_ap = if setup.keyboard {
            setup.selected
        } else {
            link.ap.or(setup.selected)
        };

        each_band(setup, link, |band| match band {
            Band::Header => self.draw_header(
                setup,
                link,
                shown_ap,
                self.diagnostics.mac,
                aps.filter(|list| !list.is_empty()).map(|list| list.len()),
            ),
            Band::Content => self.draw_content(setup, link, aps),
            Band::Controls => self.draw_strip(setup, link),
            Band::Overlay => self.draw_box(setup, link, shown_ap, self.diagnostics.mac),
        });
    }

    fn draw_strip(&mut self, draft: NetworkView, link: LinkStatus) {
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

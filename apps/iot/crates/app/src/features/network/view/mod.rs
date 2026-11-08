//! The Network page's setup UI: one file per band, and the page's own strip.

mod dialog;
mod dialog_box;
mod header;
mod keyboard;
mod list;
mod text;

use crate::features::network::controller::{self as page_controls, Band, each_band};
use crate::features::network::layout;
use crate::features::network::model::NetworkPage;
use iot_core::drivers::network::{ApList, LinkStatus};

use crate::features::network::view::text::control_label;
use crate::shared::view_kit::surface::Surface;

impl Surface<'_> {
    pub(crate) fn stamp_network(&mut self) {
        let link = self.diagnostics.link;
        // Copied rather than borrowed: the borrow would be of `self.diagnostics`, and every
        // stamp below takes `self` mutably. The audio sweep makes the same trade for the same
        // reason, and a list is 350 B against the envelope's kilobyte and a half.
        let aps = self.diagnostics.aps;
        let setup = self.diagnostics.draft;
        let shown_ap = if setup.keyboard {
            setup.selected
        } else {
            link.ap.or(setup.selected)
        };

        each_band(setup, link, |band| match band {
            Band::Header => self.stamp_header(
                setup,
                link,
                shown_ap,
                self.diagnostics.mac,
                aps.filter(|list| !list.is_empty()).map(|list| list.len()),
            ),
            Band::Content => self.stamp_content(setup, link, aps),
            Band::Controls => self.stamp_strip(setup, link),
            Band::Overlay => self.stamp_box(setup, link, shown_ap, self.diagnostics.mac),
        });
    }

    fn stamp_content(&mut self, draft: NetworkPage, link: LinkStatus, aps: Option<ApList>) {
        if draft.keyboard {
            if let Some(entry) = draft.entry {
                self.stamp_keyboard(entry, draft.revealed);
            }
            return;
        }
        self.stamp_list(draft, link, aps);
    }

    fn stamp_strip(&mut self, draft: NetworkPage, link: LinkStatus) {
        let strip = page_controls::strip(draft, link);
        if strip.is_split() {
            self.stamp_cell(
                layout::toolbar_cell(layout::ToolbarCell::Left),
                control_label(strip.kind(layout::ToolbarCell::Left)),
            );
            self.stamp_cell(
                layout::toolbar_cell(layout::ToolbarCell::Right),
                control_label(strip.kind(layout::ToolbarCell::Right)),
            );
        } else {
            self.stamp_cell(
                layout::toolbar_box(),
                control_label(strip.kind(layout::ToolbarCell::Right)),
            );
        }
    }
}

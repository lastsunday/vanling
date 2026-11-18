//! The Network page: the link's list and setup box, the edit session the user moves through
//! them, and the drawing that reads both.

pub mod controller;
pub mod layout;
mod view;

#[cfg(test)]
mod tests;

use iot_core::light::Rgb;

use crate::shared::intent::{BusinessIntent, InputIntent, LinkIntent};
use crate::shared::kit::painter::Painter;
use crate::shared::model::{Command, DeviceState, NetworkView};
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::snapshot::PageSnapshot;

/// The page owns the one edit session the link never writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkPage {
    draft: NetworkView,
}

impl super::Service for NetworkPage {
    fn apply(&mut self, intent: &BusinessIntent, state: &DeviceState) -> Option<Command> {
        match intent {
            BusinessIntent::Link(LinkIntent::Status(status)) => {
                self.draft.on_link(*status);
                None
            }
            BusinessIntent::NetworkPage(intent) => {
                self.draft
                    .apply(*intent, &state.network, state.has_display())
            }
            BusinessIntent::Scan => {
                self.draft.clamp_top(state.network.len());
                None
            }
            _ => None,
        }
    }
}

impl super::LocalPage for NetworkPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        controller::bind(op, state, self.draft)
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.network = self.draft;
    }

    fn paint(snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        let link = snapshot.link;
        let aps = snapshot.aps;
        let setup = snapshot.pages.network;
        let shown_ap = if setup.keyboard {
            setup.selected
        } else {
            link.ap.or(setup.selected)
        };

        controller::each_band(setup, link, |band| match band {
            controller::Band::Header => painter.draw_header(
                setup,
                link,
                shown_ap,
                snapshot.mac,
                aps.filter(|list| !list.is_empty()).map(|list| list.len()),
            ),
            controller::Band::Content => painter.draw_content(setup, link, aps),
            controller::Band::Controls => painter.draw_strip(setup, link),
            controller::Band::Overlay => painter.draw_box(setup, link, shown_ap, snapshot.mac),
        });
    }

    fn on_hide(&mut self) {
        self.draft.collapse();
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::paint)
            .with_ground(Some(Rgb(0, 0, 0)))
            .with_repaints(super::Repaint::When(network_moved))
    }
}

fn network_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.link != next.link
        || prev.aps != next.aps
        || prev.pages != next.pages
        || prev.mac != next.mac
}

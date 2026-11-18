//! The Network page: the link's list and setup box, the edit session the user moves through
//! them, and the drawing that reads both.

pub mod controller;
pub mod layout;
mod view;

#[cfg(test)]
mod tests;

use iot_core::light::Rgb;

use crate::shared::intent::{BusinessIntent, InputIntent, LinkIntent, NetworkIntent};
use crate::shared::kit::painter::Painter;
use crate::shared::model::{Command, DeviceState, Effects, NetworkView};
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::snapshot::PageSnapshot;
use iot_core::drivers::input::{ButtonEvent, InputEvent};

/// The page owns the one edit session the link never writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkPage {
    draft: NetworkView,
}

impl super::Service for NetworkPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        match intent {
            BusinessIntent::Link(intent) => {
                if let LinkIntent::Status(status) = intent {
                    self.draft.on_link(*status);
                }
                (None, state.network.apply(*intent))
            }
            BusinessIntent::Network(intent) => (
                self.draft
                    .apply(*intent, &state.network, state.has_display()),
                Effects::default(),
            ),
            _ => (None, Effects::default()),
        }
    }

    fn on_scan(&mut self, state: &mut DeviceState) {
        self.draft.clamp_top(state.network.len());
    }
}

impl NetworkPage {
    /// The setup hold is a link decision, not a page one: switching the host is offered from
    /// anywhere the panel is not a transient.
    pub(super) fn from_anywhere(op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        (state.network.link.mode.is_setup()
            && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)))
        .then_some(BusinessIntent::Network(NetworkIntent::SwitchHost))
    }
}

impl super::LocalPage for NetworkPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
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

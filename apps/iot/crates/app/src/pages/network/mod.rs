//! The Network page: the link's list and setup box, the edit session the user moves through
//! them, and the drawing that reads both.

pub mod controller;
pub mod layout;
mod view;

#[cfg(test)]
mod tests;

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent, LinkIntent};
use crate::shared::model::{Command, DeviceState, NetworkView, PageSnapshot};

/// The page owns the one edit session the link never writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkPage {
    draft: NetworkView,
}

pub(super) const GROUND: Option<Rgb> = Some(Rgb(0, 0, 0));
pub(super) const CAMERA_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;
pub(super) const REPAINTS: super::Repaint = super::Repaint::When(network_moved);

impl super::Page for NetworkPage {
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

    fn publish(&self, out: &mut PageSnapshot) {
        out.network = self.draft;
    }
}

impl super::LocalPage for NetworkPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        controller::bind(op, state, self.draft)
    }

    fn on_hide(&mut self) {
        self.draft.collapse();
    }
}

fn network_moved(prev: &Diagnostics, next: &Diagnostics) -> bool {
    prev.link != next.link
        || prev.aps != next.aps
        || prev.pages != next.pages
        || prev.mac != next.mac
}

//! The Ambient page: the debug overlay over an empty ground, and nothing on a release build.

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::painter::Painter;
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;

/// The Ambient page owns no state: the lamp is its whole answer and the overlay its whole paint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmbientPage;

impl super::Service for AmbientPage {}

impl super::LocalPage for AmbientPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        super::lamp(op, state)
    }

    fn paint(snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        if painter.debug {
            let live = painter.live(snapshot);
            painter.draw_overlay(snapshot, live);
        }
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::paint).with_repaints(super::Repaint::Overlay)
    }
}

//! The Ambient page: the debug overlay over an empty ground, and nothing on a release build.

use iot_core::light::Rgb;

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::painter::Painter;
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;

/// The Ambient page owns no state: the lamp is its whole answer and the overlay its whole paint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmbientPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const CAMERA_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;
pub(super) const REPAINTS: super::Repaint = super::Repaint::Overlay;

impl super::Page for AmbientPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        super::lamp(op, state)
    }

    fn paint(snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        if painter.debug {
            let live = painter.live(snapshot);
            painter.draw_overlay(snapshot, live);
        }
    }
}

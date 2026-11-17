//! The Ambient page: the debug overlay over an empty ground, and nothing on a release build.

use iot_core::paint::Rgb;

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::model::DeviceState;

/// The Ambient page owns no state: the lamp is its whole answer and the overlay its whole paint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmbientPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const CAMERA_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;
pub(super) const REPAINTS: super::Repaint = super::Repaint::Overlay;

impl super::Page for AmbientPage {}

impl super::LocalPage for AmbientPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        super::lamp(op, state)
    }
}

impl Surface<'_> {
    pub(crate) fn paint_ambient(&mut self) {
        if self.debug {
            let live = self.live();
            self.draw_diagnostics(live);
        }
    }
}

//! The Ambient page: the debug overlay over an empty ground, and nothing on a release build.

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::model::DeviceState;

/// The Ambient page owns no state: the lamp is its whole answer and the overlay its whole ink.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmbientPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const DRAWS_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::DIAG_REPAINT_STEP_MS;

impl super::Page for AmbientPage {}

impl super::LocalPage for AmbientPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        super::lamp(op, state)
    }
}

pub(super) fn stamp(surface: &mut Surface<'_>) {
    if surface.debug {
        surface.stamp_ambient();
    }
}

impl Surface<'_> {
    pub(crate) fn stamp_ambient(&mut self) {
        let live = self.live();
        self.stamp_diagnostics(live);
    }
}

pub(super) fn repaints_on(_prev: &Diagnostics, _next: &Diagnostics, overlay: bool) -> bool {
    overlay
}

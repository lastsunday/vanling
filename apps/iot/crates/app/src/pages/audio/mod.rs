//! The Audio page: capture control and the meter the capture's envelope is drawn as.

pub mod controller;
mod view;

#[cfg(test)]
mod tests;

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::model::DeviceState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const DRAWS_FRAMES: bool = false;

/// A capture poll closes two envelope columns every 20 ms; at the default step the sweep would
/// hold the executor 40% of the wall time on the S3.
pub(super) const REPAINT_MS: u64 = super::DIAG_REPAINT_STEP_MS * 2;

impl super::Page for AudioPage {}

impl super::LocalPage for AudioPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        controller::bind(op, state)
    }
}

pub(super) fn stamp(surface: &mut Surface<'_>) {
    surface.stamp_audio();
}

pub(super) fn repaints_on(prev: &Diagnostics, next: &Diagnostics, _overlay: bool) -> bool {
    prev.audio.phase != next.audio.phase
        || prev.audio.envelope != next.audio.envelope
        || prev.audio.restarts != next.audio.restarts
        || prev.audio.dba_lsb != next.audio.dba_lsb
}

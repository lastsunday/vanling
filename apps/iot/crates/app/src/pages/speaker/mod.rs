//! The Speaker page: what a tap plays, the mute latch, and the readout the output reports.

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
pub struct SpeakerPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const DRAWS_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::DIAG_REPAINT_STEP_MS;

impl super::Page for SpeakerPage {}

impl super::LocalPage for SpeakerPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        controller::bind(op, state)
    }
}

pub(super) fn stamp(surface: &mut Surface<'_>) {
    surface.stamp_speaker();
}

pub(super) fn repaints_on(prev: &Diagnostics, next: &Diagnostics, _overlay: bool) -> bool {
    prev.playback != next.playback
}

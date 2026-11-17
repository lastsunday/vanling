//! The Speaker page: what a tap plays, the mute latch, and the readout the output reports.

pub mod controller;
mod view;

#[cfg(test)]
mod tests;

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::model::DeviceState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeakerPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const CAMERA_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;
pub(super) const REPAINTS: super::Repaint = super::Repaint::When(playback_moved);

impl super::Page for SpeakerPage {}

impl super::LocalPage for SpeakerPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        controller::bind(op, state)
    }
}

fn playback_moved(prev: &Diagnostics, next: &Diagnostics) -> bool {
    prev.playback != next.playback
}

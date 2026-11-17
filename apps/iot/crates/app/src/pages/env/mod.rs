//! The Env page: one line of text for now, and no gesture of its own.

pub mod controller;
mod view;

use iot_core::paint::Rgb;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::model::DeviceState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const DRAWS_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::DIAG_REPAINT_STEP_MS;

impl super::Page for EnvPage {}

impl super::LocalPage for EnvPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        controller::bind(op, state)
    }
}

pub(super) fn stamp(surface: &mut Surface<'_>) {
    surface.stamp_env();
}

pub(super) fn repaints_on(_prev: &Diagnostics, _next: &Diagnostics, overlay: bool) -> bool {
    overlay
}

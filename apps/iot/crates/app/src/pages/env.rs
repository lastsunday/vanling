//! The Env page: one line of text for now, and no gesture of its own.

use iot_core::overlay::{Block, block_x, overlay_row_top};
use iot_core::paint::Rgb;

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::model::DeviceState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPage;

pub(super) const GROUND: Option<Rgb> = None;
pub(super) const CAMERA_FRAMES: bool = false;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;
pub(super) const REPAINTS: super::Repaint = super::Repaint::Overlay;

impl super::Page for EnvPage {}

impl super::LocalPage for EnvPage {
    fn bind(&self, _op: &InputIntent, _state: &DeviceState) -> BusinessIntent {
        BusinessIntent::Invalid
    }
}

impl Surface<'_> {
    pub(crate) fn paint_env(&mut self) {
        self.draw_text(b"Env", block_x(Block::First), overlay_row_top(0));
    }
}

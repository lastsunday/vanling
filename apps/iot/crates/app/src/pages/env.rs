//! The Env page: one line of text for now, and no gesture of its own.

use iot_core::overlay::{Block, block_x, overlay_row_top};

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::painter::Painter;
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPage;

impl super::Page for EnvPage {
    fn bind(&self, _op: &InputIntent, _state: &DeviceState) -> BusinessIntent {
        BusinessIntent::Invalid
    }
}

impl super::LocalPage for EnvPage {
    fn paint(_snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        painter.draw_text(b"Env", block_x(Block::First), overlay_row_top(0));
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::paint).repaints(super::Repaint::Overlay)
    }
}

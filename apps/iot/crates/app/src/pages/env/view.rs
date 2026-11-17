use crate::shared::kit::surface::Surface;
use iot_core::overlay::Block;
use iot_core::overlay::block_x;
use iot_core::overlay::overlay_row_top;

impl Surface<'_> {
    pub(crate) fn stamp_env(&mut self) {
        self.stamp_text(b"Env", block_x(Block::First), overlay_row_top(0));
    }
}

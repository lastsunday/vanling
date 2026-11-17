//! A reading on one row of a text block: the label in the block's own column, and the value
//! in the column the block gives it.

use iot_core::drivers::audio::spl;
use iot_core::overlay::{Block, block_value_x, block_x, level_columns, overlay_row_top};

use crate::shared::kit::surface::Surface;
use crate::shared::kit::text::format_i32;

impl Surface<'_> {
    /// A label and its value on one row of `block`.
    pub(crate) fn draw_left(&mut self, label: &[u8], value: &[u8], block: Block, row: usize) {
        let top = overlay_row_top(row);
        self.draw_text(label, block_x(block), top);
        self.draw_text(value, block_value_x(block), top);
    }

    /// A level on one row: the label, the decibels, and the sound pressure they mean.
    pub(crate) fn draw_level(&mut self, label: &[u8], decibels: i16, row: usize) {
        let top = overlay_row_top(row);
        let columns = level_columns();
        self.draw_text(label, block_x(Block::First), top);
        self.draw_text(
            format_i32(i32::from(decibels), &mut [0u8; 12]),
            columns.value_x,
            top,
        );
        self.draw_text(b"DBFS", columns.unit_x, top);
        let offset = self.spl_offset_db;
        let mut spl_buf = [0u8; 12];
        let spl_text = format_i32(i32::from(spl(decibels, offset)), &mut spl_buf);
        self.draw_text(spl_text, columns.spl_x, top);
        self.draw_text(b"SPL", columns.spl_unit_x, top);
    }
}

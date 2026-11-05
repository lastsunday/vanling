//! A reading on one row of a text block: the label in the block's own column, and the value
//! in the column the block gives it.

use iot_core::drivers::audio::spl;
use iot_core::overlay::{Block, block_value_x, block_x, level_columns, overlay_row_top};

use crate::ui::surface::Surface;
use crate::ui::text::format_i32;

impl Surface<'_> {
    /// A label and its value on one row of `block`.
    pub(crate) fn stamp_left(&mut self, label: &[u8], value: &[u8], block: Block, row: usize) {
        let top = overlay_row_top(row);
        self.stamp_text(label, block_x(block), top);
        self.stamp_text(value, block_value_x(block), top);
    }

    /// A level on one row: the label, the decibels, and the sound pressure they mean.
    pub(crate) fn stamp_level(&mut self, label: &[u8], decibels: i16, row: usize) {
        let top = overlay_row_top(row);
        let columns = level_columns();
        self.stamp_text(label, block_x(Block::First), top);
        self.stamp_text(
            format_i32(i32::from(decibels), &mut [0u8; 12]),
            columns.value_x,
            top,
        );
        self.stamp_text(b"DBFS", columns.unit_x, top);
        let offset = self.spl_offset_db;
        let mut spl_buf = [0u8; 12];
        let spl_text = format_i32(i32::from(spl(decibels, offset)), &mut spl_buf);
        self.stamp_text(spl_text, columns.spl_x, top);
        self.stamp_text(b"SPL", columns.spl_unit_x, top);
    }
}

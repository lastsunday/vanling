//! The audio band, its ruler and peak holds, and the attitude dial.
use iot_core::drivers::audio::{ENVELOPE_COLUMNS, scope_height, spl};
use iot_core::drivers::motion::MotionSample;
use iot_core::horizon::{SCALE, horizon};
use iot_core::overlay::{Block, FONT_H, block_value_x, block_x, level_columns, overlay_row_top};

use crate::layout::*;
use crate::pages::draw::{bar_rows, decibels_row, format_decibels, format_i32, format_span};
use crate::surface::Surface;

impl Surface<'_> {
    // The meter: a logarithmic band, each column's A-weighted sustained level
    // solid and its peak dithered outside it. The sweep uses the readout's own
    // filter, so a low-frequency codec floor draws quiet. Stamping inverts, so
    // two levels in one ink have to be *patterns*: a stripe inside the peak
    // bar would be invisible. Heights come from [`scope_height`], so the bars
    // land on the same decibel grid as the ruler.
    //
    // The columns and the clip age arrive as values rather than as a borrow of
    // the envelope: the envelope is borrowed from the surface's snapshot, so
    // holding it across the `&mut self` stamps would either copy 1.6 KB onto the
    // stack or fail to borrow. The two arrays are 400 B each and are the sweep's
    // working set either way.
    pub(crate) fn stamp_sweep(
        &mut self,
        peaks: &[u16; ENVELOPE_COLUMNS],
        levels: &[u16; ENVELOPE_COLUMNS],
        clip_age: Option<usize>,
    ) {
        self.probe_stack();
        self.stamp_ruler();
        for column in 0..SCAN_COLUMNS as usize {
            let x = SCAN_X + column as isize;
            let peak = bar_rows(peaks[column]);
            let level = bar_rows(levels[column]).min(peak);
            for row in (WAVE_CY - level)..=(WAVE_CY + level) {
                self.stamp_pixel(x, row);
            }
            let mut shoulder = 0;
            for row in (WAVE_CY - peak)..(WAVE_CY - level) {
                if shoulder % 2 == 0 {
                    self.stamp_pixel(x, row);
                }
                shoulder += 1;
            }
            shoulder = 0;
            for row in (WAVE_CY + level + 1)..=(WAVE_CY + peak) {
                if shoulder % 2 == 0 {
                    self.stamp_pixel(x, row);
                }
                shoulder += 1;
            }
        }
        self.stamp_centre_line();
        self.stamp_peak_hold(peaks);
        self.stamp_clip_mark(clip_age);
        self.stamp_text(format_span(&mut [0u8; 12]), SCAN_X as usize, SPAN_Y);
    }

    fn stamp_ruler(&mut self) {
        for decibels in (0..SCOPE_DECIBEL_FLOOR).step_by(GRID_DECIBELS as usize) {
            let row = decibels_row(decibels);
            self.stamp_rule(SCAN_X, row, SCAN_COLUMNS);
        }
        self.stamp_rule(SCAN_X, WAVE_CY, SCAN_COLUMNS);
        for decibels in [0, 24, 48, SCOPE_DECIBEL_FLOOR] {
            let row = decibels_row(decibels);
            let mut buf = [0u8; 12];
            let label = format_decibels(decibels, &mut buf);
            let top = (row - FONT_H as isize / 2).max(WAVE_TOP) as usize;
            self.stamp_text_right(label, RULER_RIGHT, top);
            self.stamp_rule(RULER_TICK_X, row, RULER_TICK_COLUMNS);
        }
    }

    fn stamp_rule(&mut self, x: isize, row: isize, columns: isize) {
        for column in 0..columns {
            if column % 2 == 0 {
                self.stamp_pixel(x + column, row);
            }
        }
    }

    fn stamp_centre_line(&mut self) {
        for column in 0..SCAN_COLUMNS {
            self.stamp_pixel(SCAN_X + column, WAVE_CY);
        }
    }

    fn stamp_peak_hold(&mut self, bars: &[u16; ENVELOPE_COLUMNS]) {
        let loudest = bars.iter().copied().max().unwrap_or(0);
        if scope_height(loudest) == 0 {
            return;
        }
        let column = bars
            .iter()
            .position(|&bar| bar == loudest)
            .expect("a maximum is in the window it came from");
        let row = WAVE_CY - bar_rows(loudest);
        for offset in 0..4 {
            self.stamp_pixel(SCAN_X + column as isize + offset as isize, row);
        }
    }

    fn stamp_clip_mark(&mut self, clip_age: Option<usize>) {
        let Some(age) = clip_age else {
            return;
        };
        let column = SCAN_COLUMNS - 1 - age as isize;
        for offset in 0..CLIP_MARK_WIDTH {
            for row in WAVE_TOP..(WAVE_TOP + 8) {
                self.stamp_pixel(SCAN_X + column + offset, row);
            }
        }
    }

    pub(crate) fn stamp_left(&mut self, label: &[u8], value: &[u8], block: Block, row: usize) {
        let top = overlay_row_top(row);
        self.stamp_text(label, block_x(block), top);
        self.stamp_text(value, block_value_x(block), top);
    }

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

    pub(crate) fn stamp_horizon(&mut self, sample: &MotionSample) {
        let geo = horizon(sample.tilt_deg_x10[0], sample.tilt_deg_x10[1]);
        let (cx, cy, r) = (HORIZON_CX, HORIZON_CY, HORIZON_R);

        self.stamp_circle(cx, cy, r);

        self.stamp_line(cx - 4, cy - r + 2, cx + 4, cy - r + 2);
        self.stamp_line(cx - 10, cy, cx + 10, cy);

        let half_x = geo.unit.0 as isize * r / SCALE as isize;
        let half_y = geo.unit.1 as isize * r / SCALE as isize;
        let offset = geo.offset as isize * r / SCALE as isize;
        self.stamp_line(
            cx - half_x,
            cy - half_y + offset,
            cx + half_x,
            cy + half_y + offset,
        );
    }
}

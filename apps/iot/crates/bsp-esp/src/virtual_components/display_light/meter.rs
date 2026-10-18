//! The audio band, its ruler and peak holds, and the attitude dial.
use super::draw::{
    SCOPE_DECIBEL_FLOOR, bar_rows, decibels_row, format_decibels, format_i32, format_span,
    format_u16, stamp_text,
};
use super::stack::note_stack_high_water;
use super::*;

impl DisplayLight {
    /// The meter: a logarithmic band, each column's A-weighted sustained level
    /// solid and its peak dithered outside it. The sweep uses the readout's own
    /// filter, so a low-frequency codec floor draws quiet. Stamping inverts, so
    /// two levels in one ink have to be *patterns*: a stripe inside the peak
    /// bar would be invisible. Heights come from [`scope_height`], so the bars
    /// land on the same decibel grid as the ruler.
    ///
    /// The columns and the clip age arrive as values rather than as a borrow of
    /// the envelope: the envelope lives in the panel's own snapshot, so holding
    /// it across the `&mut self` stamps would either copy 1.6 KB onto the stack
    /// or fail to borrow. The two arrays are 400 B each and are the sweep's
    /// working set either way.
    pub(super) fn stamp_sweep(
        &mut self,
        peaks: &[u16; ENVELOPE_COLUMNS],
        levels: &[u16; ENVELOPE_COLUMNS],
        clip_age: Option<usize>,
        width: usize,
        height: usize,
    ) {
        note_stack_high_water();
        self.stamp_ruler(width, height);
        for column in 0..SCAN_COLUMNS as usize {
            let x = SCAN_X + column as isize;
            let peak = bar_rows(peaks[column]);
            let level = bar_rows(levels[column]).min(peak);
            // Solid to the sustained level: the part a listener would call the
            // volume of the column.
            for row in (WAVE_CY - level)..=(WAVE_CY + level) {
                self.stamp_pixel(x, row, width, height);
            }
            // Dithered out to the peak: transient energy above the level, which
            // has to stay visible without being mistaken for the level itself.
            let mut shoulder = 0;
            for row in (WAVE_CY - peak)..(WAVE_CY - level) {
                if shoulder % 2 == 0 {
                    self.stamp_pixel(x, row, width, height);
                }
                shoulder += 1;
            }
            shoulder = 0;
            for row in (WAVE_CY + level + 1)..=(WAVE_CY + peak) {
                if shoulder % 2 == 0 {
                    self.stamp_pixel(x, row, width, height);
                }
                shoulder += 1;
            }
        }
        self.stamp_centre_line(width, height);
        self.stamp_peak_hold(peaks, width, height);
        self.stamp_clip_mark(clip_age, width, height);
        self.stamp_text(
            format_span(&mut [0u8; 12]),
            SCAN_X as usize,
            SPAN_Y,
            width,
            height,
        );
    }

    /// The scale the band is read against, and the labels that give it numbers.
    /// Only the upper half is ruled: the band is mirrored, so ruling both would
    /// double the ink to say the same thing twice.
    fn stamp_ruler(&mut self, width: usize, height: usize) {
        for decibels in (0..SCOPE_DECIBEL_FLOOR).step_by(GRID_DECIBELS as usize) {
            let row = decibels_row(decibels);
            self.stamp_rule(SCAN_X, row, SCAN_COLUMNS, width, height);
        }
        // The centre line is solid rather than dithered: it is the floor itself,
        // the value every bar falls back to, and a reference the user has to
        // squint at is not a reference.
        self.stamp_rule(SCAN_X, WAVE_CY, SCAN_COLUMNS, width, height);
        for decibels in [0, 24, 48, SCOPE_DECIBEL_FLOOR] {
            let row = decibels_row(decibels);
            let mut buf = [0u8; 12];
            let label = format_decibels(decibels, &mut buf);
            // Clamped into the band rather than the panel: the top label would
            // otherwise reach up into the level rows the sweep starts below.
            let top = (row - FONT_H as isize / 2).max(WAVE_TOP) as usize;
            self.stamp_text_right(label, RULER_RIGHT, top, width, height);
            self.stamp_rule(RULER_TICK_X, row, RULER_TICK_COLUMNS, width, height);
        }
    }

    /// Dotted rule every other column, so a line across the band reads as a
    /// reference behind the bars rather than as one of them.
    fn stamp_rule(&mut self, x: isize, row: isize, columns: isize, width: usize, height: usize) {
        for column in 0..columns {
            if column % 2 == 0 {
                self.stamp_pixel(x + column, row, width, height);
            }
        }
    }

    fn stamp_centre_line(&mut self, width: usize, height: usize) {
        for column in 0..SCAN_COLUMNS {
            self.stamp_pixel(SCAN_X + column, WAVE_CY, width, height);
        }
    }

    /// A dash at the loudest column's own height, so the peak is located and not
    /// merely levelled. The window *is* the hold: the oldest column still at that
    /// level says how long it has been held, with no state of its own.
    fn stamp_peak_hold(&mut self, bars: &[u16; ENVELOPE_COLUMNS], width: usize, height: usize) {
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
            self.stamp_pixel(
                SCAN_X + column as isize + offset as isize,
                row,
                width,
                height,
            );
        }
    }

    /// A block at the top of the band on the column that clipped, while that
    /// column is still in the window. The latch beside it says a clip happened;
    /// this says where, and goes with the column it names.
    fn stamp_clip_mark(&mut self, clip_age: Option<usize>, width: usize, height: usize) {
        let Some(age) = clip_age else {
            return;
        };
        let column = SCAN_COLUMNS - 1 - age as isize;
        for offset in 0..CLIP_MARK_WIDTH {
            for row in WAVE_TOP..(WAVE_TOP + 8) {
                self.stamp_pixel(SCAN_X + column + offset, row, width, height);
            }
        }
    }

    /// Overdraws the `FPS <rate>` badge in the panel's bottom-right corner,
    /// right-aligned so it stays flush as the rate grows digits.
    ///
    /// A free function taking the rate rather than a method reading `self`, because the camera
    /// page stamps its readout over a buffer it already borrowed from `self` and cannot hold
    /// that borrow and the field at once. Every page draws this badge: a page missing it reads
    /// as a page that is not refreshing, which on the camera page is exactly the question the
    /// page exists to answer.
    pub(super) fn stamp_fps(frame: &mut [u8], fps: u8, width: usize, height: usize) {
        let buf = &mut [0u8; 6];
        let digits = format_u16(u16::from(fps), buf);
        let value_left = (PANEL_RIGHT + OVERLAY_GAP as isize
            - digits.len() as isize * GLYPH_PITCH as isize)
            .max(0) as usize;
        stamp_text(
            frame,
            b"FPS",
            (value_left as isize - (3 * GLYPH_PITCH as isize + OVERLAY_GAP as isize)).max(0)
                as usize,
            FPS_Y,
            width,
            height,
        );
        stamp_text(frame, digits, value_left, FPS_Y, width, height);
    }

    /// Writes one row: label at the block's left edge, one blank cell, then the
    /// reading at the block's own reading column.
    pub(super) fn stamp_left(
        &mut self,
        label: &[u8],
        value: &[u8],
        block: Block,
        row: usize,
        width: usize,
        height: usize,
    ) {
        let top = overlay_row_top(row);
        self.stamp_text(label, block_x(block), top, width, height);
        self.stamp_text(value, block_value_x(block), top, width, height);
    }

    /// Writes one Audio level row: the label, the level as dBFS, that reading's
    /// unit, and the same instant of sound as a pressure level with its own unit.
    ///
    /// Both readings belong on one line because they measure the same thing from
    /// two zeros, and a −21 that no one can judge against is the objection this
    /// page exists to answer — dBFS says how much of the converter the signal
    /// uses, dB SPL says how loud the room is, and only the second is a number
    /// anybody compares with a noise complaint.
    ///
    /// Four columns is more than any block on this panel is wide, so this is the
    /// one row that runs the full width of the first block's neighbour: the sweep
    /// starts below it rather than beside it.
    pub(super) fn stamp_level(
        &mut self,
        label: &[u8],
        decibels: i16,
        row: usize,
        width: usize,
        height: usize,
    ) {
        let top = overlay_row_top(row);
        let columns = level_columns();
        self.stamp_text(label, block_x(Block::First), top, width, height);
        self.stamp_text(
            format_i32(i32::from(decibels), &mut [0u8; 12]),
            columns.value_x,
            top,
            width,
            height,
        );
        self.stamp_text(b"DBFS", columns.unit_x, top, width, height);
        self.stamp_text(
            format_i32(
                i32::from(spl(decibels, SPL_OFFSET_DECIBELS)),
                &mut [0u8; 12],
            ),
            columns.spl_x,
            top,
            width,
            height,
        );
        self.stamp_text(b"SPL", columns.spl_unit_x, top, width, height);
    }

    /// Overdraws the attitude dial — the inverted counterpart of `R0`–`R2` —
    /// with ring, fixed wing/bank references, and a `-roll`/`pitch` horizon.
    pub(super) fn stamp_horizon(&mut self, sample: &MotionSample, width: usize, height: usize) {
        let geo = horizon(sample.tilt_deg_x10[0], sample.tilt_deg_x10[1]);
        let (cx, cy, r) = (HORIZON_CX, HORIZON_CY, HORIZON_R);

        self.stamp_circle(cx, cy, r, width, height);

        self.stamp_line(cx - 4, cy - r + 2, cx + 4, cy - r + 2, width, height);
        self.stamp_line(cx - 10, cy, cx + 10, cy, width, height);

        // Horizon line tilted `-roll`, shifted by the pitch fraction of `r`.
        let half_x = geo.unit.0 as isize * r / SCALE as isize;
        let half_y = geo.unit.1 as isize * r / SCALE as isize;
        let offset = geo.offset as isize * r / SCALE as isize;
        self.stamp_line(
            cx - half_x,
            cy - half_y + offset,
            cx + half_x,
            cy + half_y + offset,
            width,
            height,
        );
    }
}

//! The audio band, its ruler and peak holds, and the attitude dial.
use iot_core::drivers::audio::{COLUMN_MS, ENVELOPE_COLUMNS, scope_height};
use iot_core::drivers::motion::MotionSample;
use iot_core::horizon::{SCALE, horizon};
use iot_core::overlay::{
    Block, FONT_H, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, PANEL_ROWS, RIGHT_EDGE_X, block_x,
    overlay_row_top,
};

use crate::display::ui::surface::Surface;
use crate::display::ui::text::format_i32;

const SCOPE_DECIBEL_FLOOR: i16 = iot_core::drivers::audio::SCOPE_FLOOR_DECIBELS as i16;

const HORIZON_CX: isize = 266;
const HORIZON_CY: isize = 120;
const HORIZON_R: isize = 48;

pub(crate) const WAVE_TOP: isize = 42;
const WAVE_CY: isize = 132;
const WAVE_HALF: isize = 90;

// Decibels per grid line, chosen so a line lands on a whole row and the
// ten-octave window divides into whole steps: [`WAVE_HALF`] rows per 90 dB is
// one row per decibel, so twelve decibels is exactly twelve rows. A scale whose
// lines land off-pixel reads as a scale that is wrong.
const GRID_DECIBELS: i16 = 12;

const SCAN_X: isize = 40;
const SCAN_COLUMNS: isize = 200;

const RULER_RIGHT: isize = 29;
const RULER_TICK_X: isize = 32;
const RULER_TICK_COLUMNS: isize = 6;

const SPAN_Y: usize = 224;

const AUDIO_LAST_FULL_WIDTH_ROW: usize = 2;

const CLIP_MARK_WIDTH: isize = 3;

const _: () = assert!(
    SCAN_COLUMNS as usize <= ENVELOPE_COLUMNS,
    "the sweep draws one panel column per envelope column, so a wider sweep reads past the window"
);
const _: () = assert!(
    SCAN_X >= RULER_TICK_X + RULER_TICK_COLUMNS,
    "the ruler's tick runs into the sweep"
);
const _: () = assert!(
    RULER_RIGHT as usize + OVERLAY_GAP >= 3 * GLYPH_PITCH,
    "the widest ruler label is three glyphs, so the labels would hang off the panel's left edge"
);
const _: () = assert!(
    SCAN_X + (SCAN_COLUMNS - 1) + (CLIP_MARK_WIDTH - 1) < RIGHT_EDGE_X as isize,
    "the clip mark runs under the right-edge readouts"
);
const _: () = assert!(
    SCAN_X + SCAN_COLUMNS <= RIGHT_EDGE_X as isize,
    "the sweep runs under the right-edge readouts"
);
const _: () = assert!(
    WAVE_CY - WAVE_HALF == WAVE_TOP,
    "the band's top edge is where the window's floor falls, which is what the ruler's top label points at"
);
const _: () = assert!(
    WAVE_CY + WAVE_HALF <= SPAN_Y as isize,
    "the band's bottom edge runs through the span label"
);
const _: () = assert!(
    WAVE_HALF * GRID_DECIBELS as isize % SCOPE_DECIBEL_FLOOR as isize == 0,
    "a grid line lands off a whole number of rows, which reads as a scale that is wrong"
);
const _: () = assert!(
    WAVE_TOP as usize >= overlay_row_top(AUDIO_LAST_FULL_WIDTH_ROW) + FONT_H,
    "the band is drawn through the Audio page's level rows"
);
const _: () = assert!(
    SPAN_Y + FONT_H <= PANEL_ROWS,
    "the sweep's span label runs off the bottom of the panel"
);
const _: () = assert!(
    HORIZON_CY - HORIZON_R >= 0 && (HORIZON_CY + HORIZON_R) as usize + FONT_H <= PANEL_ROWS,
    "the attitude dial runs off the top or bottom of the panel"
);
const _: () = assert!(
    HORIZON_CX - HORIZON_R >= block_x(Block::Third) as isize
        && (HORIZON_CX + HORIZON_R) as usize <= PANEL_COLUMNS,
    "the attitude dial is not inside the third text block it is anchored to"
);

fn bar_rows(peak_lsb: u16) -> isize {
    WAVE_HALF * isize::from(iot_core::drivers::audio::scope_height(peak_lsb)) / isize::from(u8::MAX)
}

fn decibels_row(decibels: i16) -> isize {
    WAVE_CY - WAVE_HALF * (SCOPE_DECIBEL_FLOOR - decibels) as isize / SCOPE_DECIBEL_FLOOR as isize
}

fn format_decibels(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    format_i32(i32::from(decibels), buf)
}

fn format_span(buf: &mut [u8; 12]) -> &[u8] {
    let tenths = ENVELOPE_COLUMNS as u32 * COLUMN_MS as u32 / 100;
    let mut whole = [0u8; 12];
    let seconds = format_i32((tenths / 10) as i32, &mut whole);
    buf[..seconds.len()].copy_from_slice(seconds);
    let mut len = seconds.len();
    for tail in [b'.', b'0' + (tenths % 10) as u8, b'S'] {
        buf[len] = tail;
        len += 1;
    }
    &buf[..len]
}

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

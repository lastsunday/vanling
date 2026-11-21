//! The capture's sweep: the band it draws, its decibel ruler, and the peak and clip marks.

use iot_core::drivers::audio::{COLUMN_MS, ENVELOPE_COLUMNS, SCOPE_FLOOR_DECIBELS, scope_height};
use iot_core::ui::panel::LINE;
use iot_core::ui::{Canvas, Rect};

use crate::shared::kit::text::format_i32;

const FLOOR: i32 = SCOPE_FLOOR_DECIBELS;

/// Decibels between two grid lines. The face's 12 px line and the ruler's whole-decibel
/// rows land the labels on pixels: a scale whose lines sit off-pixel reads as a wrong scale.
const GRID_DECIBELS: i32 = 12;

/// The ruler's own strip, left of the scan: labels right-aligned against the ticks.
const RULER_RIGHT: i32 = 28;
const TICK_X: i32 = 30;
const TICK_WIDTH: i32 = 4;
const SCAN_LEFT: i32 = 36;

const CLIP_MARK_WIDTH: i32 = 3;
const PEAK_HOLD_WIDTH: i32 = 4;

pub(crate) const fn span_tenths() -> u32 {
    ENVELOPE_COLUMNS as u32 * COLUMN_MS as u32 / 100
}

/// The sweep, drawn between the readout above and the span label below.
pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    peaks: &[u16; ENVELOPE_COLUMNS],
    levels: &[u16; ENVELOPE_COLUMNS],
    clip_age: Option<usize>,
    probe: Option<fn()>,
) {
    if let Some(probe) = probe {
        probe();
    }
    let top = rect.y;
    let bottom = rect.bottom() - LINE as i32;
    let (centre, half) = ((top + bottom) / 2, (bottom - top) / 2);
    if half <= 0 {
        return;
    }
    let scan_x = rect.x + SCAN_LEFT;
    let columns = ENVELOPE_COLUMNS as i32;
    ruler(canvas, rect, top, centre, half);
    for column in 0..columns {
        let x = scan_x + column;
        let peak = bar_rows(peaks[column as usize], half);
        let level = bar_rows(levels[column as usize], half).min(peak);
        for row in (centre - level)..=(centre + level) {
            canvas.plot_pixel(x as isize, row as isize);
        }
        let mut shoulder = 0;
        for row in (centre - peak)..(centre - level) {
            if shoulder % 2 == 0 {
                canvas.plot_pixel(x as isize, row as isize);
            }
            shoulder += 1;
        }
        shoulder = 0;
        for row in (centre + level + 1)..=(centre + peak) {
            if shoulder % 2 == 0 {
                canvas.plot_pixel(x as isize, row as isize);
            }
            shoulder += 1;
        }
    }
    peak_hold(canvas, scan_x, centre, half, peaks);
    clip_mark(canvas, scan_x, top, clip_age);
    let mut buf = [0u8; 12];
    canvas.draw_text(
        span(&mut buf),
        scan_x as usize,
        (rect.bottom() - LINE as i32) as usize,
    );
}

fn bar_rows(peak_lsb: u16, half: i32) -> i32 {
    half * i32::from(scope_height(peak_lsb)) / i32::from(u8::MAX)
}

fn decibels_row(decibels: i32, centre: i32, half: i32) -> i32 {
    centre + half * decibels / FLOOR
}

fn ruler(canvas: &mut Canvas<'_>, rect: Rect, top: i32, centre: i32, half: i32) {
    let mut decibels = 0;
    while decibels < FLOOR {
        let row = decibels_row(decibels, centre, half);
        rule(canvas, rect.x + SCAN_LEFT, row, columns());
        decibels += GRID_DECIBELS;
    }
    rule(canvas, rect.x + SCAN_LEFT, centre, columns());
    for decibels in [0, 24, 48, FLOOR] {
        let row = decibels_row(decibels, centre, half);
        let mut buf = [0u8; 12];
        let label = format_i32(decibels, &mut buf);
        let label_top = (row - LINE as i32 / 2).max(top) as usize;
        canvas.draw_text_right(label, (rect.x + RULER_RIGHT) as isize, label_top);
        rule(canvas, rect.x + TICK_X, row, TICK_WIDTH);
    }
}

fn columns() -> i32 {
    ENVELOPE_COLUMNS as i32
}

fn rule(canvas: &mut Canvas<'_>, x: i32, row: i32, width: i32) {
    for column in (0..width).step_by(2) {
        canvas.plot_pixel((x + column) as isize, row as isize);
    }
}

fn peak_hold(canvas: &mut Canvas<'_>, scan_x: i32, centre: i32, half: i32, peaks: &[u16]) {
    let loudest = peaks.iter().copied().max().unwrap_or(0);
    if scope_height(loudest) == 0 {
        return;
    }
    let column = peaks
        .iter()
        .position(|&peak| peak == loudest)
        .expect("a maximum is in the window it came from");
    let row = centre - bar_rows(loudest, half);
    for offset in 0..PEAK_HOLD_WIDTH {
        canvas.plot_pixel((scan_x + column as i32 + offset) as isize, row as isize);
    }
}

fn clip_mark(canvas: &mut Canvas<'_>, scan_x: i32, top: i32, clip_age: Option<usize>) {
    let Some(age) = clip_age else {
        return;
    };
    let column = columns() - 1 - age as i32;
    for offset in 0..CLIP_MARK_WIDTH {
        for row in top..(top + LINE as i32) {
            canvas.plot_pixel((scan_x + column + offset) as isize, row as isize);
        }
    }
}

/// The window's length, in seconds: what the whole sweep covers.
fn span(buf: &mut [u8; 12]) -> &[u8] {
    let tenths = span_tenths();
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

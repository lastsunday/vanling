use alloc::vec::Vec;
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::audio::{
    COLUMN_MS, ENVELOPE_COLUMNS, SCOPE_FLOOR_DECIBELS, dbfs, scope_height, spl,
};
use iot_core::drivers::input::{
    FINGER_DOUBLE_TAP, FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP, FINGER_TRIPLE_TAP,
};
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, rgb_hue, scale_brightness, vertical_brightness,
};
use iot_core::drivers::motion::{MotionCapabilities, MotionSample};
use iot_core::drivers::playback::Sound;
use iot_core::horizon::{SCALE, horizon};
use iot_core::overlay::{
    FONT_W, LEFT_VALUE_X, LEVEL_UNIT_X, OVERLAY_GAP, OVERLAY_X, level_columns,
};
use iot_core::render::{MODE_BREATH, MODE_SOLID, diag_repaint_due, windowed_rate};
use iot_core::state::{AudioPhase, DisplayPage, PlaybackPhase};

use crate::components::backlight::Backlight;
use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::components::st7789::St7789;
use core::sync::atomic::{AtomicU32, Ordering};
use esp_hal::time::{Duration, Instant};

/// Last full-frame repaint taken by the panel, in milliseconds since boot.
/// Shared across the diagnostics pushes so the repaint rate bound adds no width
/// to the renderer state the main-task async machine holds across awaits.
static DIAG_LAST_REPAINT_MS: AtomicU32 = AtomicU32::new(0);

/// Deepest main-stack use observed so far, in bytes below the stack ceiling.
/// A rendering path that can walk tens of kilobytes down the stack is the kind
/// of thing that overflows silently after an innocent refactor, so the peak is
/// measured rather than assumed; see [`note_stack_high_water`].
static STACK_HIGH_WATER: AtomicU32 = AtomicU32::new(0);

/// The main stack the rtos main task (and with it every cooperative task) runs
/// on: ceiling minus guarded floor. Logged at bring-up so a stack-guard trip's
/// cause — a paint that really reached the guard, versus a layout that shrank
/// the stack — reads off one boot line instead of a bisect.
fn main_stack_bytes() -> usize {
    unsafe extern "C" {
        static _stack_start: u8;
        static _stack_end: u8;
    }
    core::ptr::addr_of!(_stack_start) as usize - core::ptr::addr_of!(_stack_end) as usize
}

/// This frame's address on the main stack, standing in for the stack pointer.
/// A local sits at most a frame's own locals below `sp`, which is noise next to
/// the kilobytes this measures, and reading it keeps the crate on stable: the
/// honest `mov {0}, sp` needs `#![feature(asm_experimental_arch)]`, which this
/// crate cannot take because it also builds for the host.
fn frame_address() -> usize {
    let probe = 0_u8;
    core::ptr::addr_of!(probe) as usize
}

/// Records how far this frame sits below the stack ceiling, keeping the deepest
/// value seen. Call it at the bottom of the deepest call chain worth knowing
/// about: the peak is then the stack the chain needed, not the whole stack's
/// size, and headroom is the difference.
fn note_stack_high_water() {
    let used = (main_stack_bytes() - (frame_address() - main_stack_floor())) as u32;
    STACK_HIGH_WATER.fetch_max(used, Ordering::Relaxed);
}

/// The main stack's low address, the floor the ceiling is measured from.
fn main_stack_floor() -> usize {
    unsafe extern "C" {
        static _stack_end: u8;
    }
    core::ptr::addr_of!(_stack_end) as usize
}

/// Minimum per-channel color delta that warrants a full-frame repaint.
const REPAINT_STEP: u8 = 12;

/// On-panel repaint-rate sampling window.
const FPS_WINDOW_MS: u64 = 500;

/// Frame-cost sampling window. The two costs are reported apart because they have
/// opposite fixes: a slow render is CPU the shared executor does not get back,
/// while a slow write is GDMA arbitration the playback DMA has to win.
const COST_WINDOW_MS: u64 = 2_000;

/// Debug overlay toggle. A compile-time switch (not a Cargo feature): the
/// ambient diagnostics rows and last-touch coordinates are a field/development
/// aid, so production builds keep them dark by setting this to `false`. The
/// attitude page remains visible in either mode.
const DEBUG_DIAGNOSTICS: bool = true;

/// Top edge of the first diagnostics row, in panel rows.
const OVERLAY_Y: usize = 8;
/// Vertical gap between diagnostics rows.
const OVERLAY_ROW_GAP: usize = 6;
/// Rows of one glyph cell, matching the 5×7 [`FONT`] whose columns are
/// [`FONT_W`] in `overlay.rs`.
const FONT_H: usize = 7;
/// Value column of the mode column: its label right-aligns into the fixed
/// five-glyph slot that ends one space before it, so every value starts on
/// one vertical line. The widest entry (`LO HI 140 255`) still fits the
/// 240-column panel.
const RIGHT_VALUE_X: usize = 174;
/// Stand-in value for a semantic the board does not declare, matching the
/// `ERR`/`WAIT` sentinels the attitude page already uses for missing data.
const NOT_AVAILABLE: &[u8] = b"N/A";

/// Attitude dial center, in panel columns/rows. Anchored in the free
/// bottom-right corner: below the mode column's last row (`y ≈ 184`) and
/// right of the touch column's widest value (`x ≈ 145`), so a radius of
/// [`HORIZON_R`] stays clear of both columns on the 240×320 panel.
const HORIZON_CX: usize = 192;
const HORIZON_CY: usize = 252;
const HORIZON_R: usize = 40;

/// Sweep band on the Audio page, in panel rows: mirrored about [`WAVE_CY`], and
/// starting below the six readout rows (the last ends at 93) so a full-scale
/// column fills the band without being drawn through the readout.
const WAVE_TOP: isize = 100;
/// Centre line of the mirrored band, which is also the bottom of the scale: a
/// mirrored bar measures *outward* from silence here, so 0 dBFS is at the band's
/// two edges and the floor is the middle.
const WAVE_CY: isize = 196;
/// Rows the tallest column reaches up and down from [`WAVE_CY`]. Set to clear
/// [`WAVE_TOP`] and leave room for the span label, not chosen: a logarithmic
/// scale spends the rows it leaves free on under half a decibel each.
const WAVE_HALF: isize = 96;

/// Decibels per grid line, chosen so a line lands every 20 rows of the half band
/// and the ten-octave window divides into whole steps: 96 rows per 60 dB is
/// 1.6 rows per decibel, and twelve is 19.2 — close to a whole row either way,
/// and a scale whose lines land off-pixel reads as a scale that is wrong.
const GRID_DECIBELS: i16 = 12;

/// Left column of the sweep, and its width in panel columns. The width is
/// [`ENVELOPE_COLUMNS`] so one column is one pixel: a scale that resamples the
/// envelope has to drop peaks, and dropping peaks on a peak meter means the
/// meter misses the thing it exists to show.
const SCAN_X: isize = 40;
const SCAN_COLUMNS: isize = 200;

/// Right edge the decibel ruler's labels are aligned to, and the tick that
/// follows them. Together they fill the 30 columns left of the sweep, which is
/// what a label plus its tick has to fit in.
const RULER_RIGHT: isize = 29;
const RULER_TICK_X: isize = 32;
const RULER_TICK_COLUMNS: isize = 6;

/// Rows below the band the window's own length is written at, so a sweep is read
/// as "two seconds" rather than as an unbounded strip whose rate nobody can
/// infer from a moving bar.
const SPAN_Y: usize = 298;

/// Global FPS badge pinned to the panel's top-right edge on every page, the
/// only strip no content uses; the value right-aligns to the panel edge.
const FPS_Y: usize = 0;

/// Printable ASCII 5×7 glyphs (`0x20`–`0x7E`, 95 × 5 column bytes), column-major,
/// bit 0 the top row: the Adafruit GFX `glcdfont` layout, drawn the way the panel
/// warms up. Indexing: `FONT[(ch - 0x20) * 5 ..][..5]`.
///
/// Table transcribed from Adafruit GFX 1.x `glcdfont.c`, BSD-3-Clause:
/// <https://github.com/adafruit/Adafruit-GFX-Library/blob/master/glcdfont.c>
const FONT: [u8; 95 * 5] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5F, 0x00, 0x00, 0x00, 0x07, 0x00, 0x07, 0x00, 0x14,
    0x7F, 0x14, 0x7F, 0x14, 0x24, 0x2A, 0x7F, 0x2A, 0x12, 0x23, 0x13, 0x08, 0x64, 0x62, 0x36, 0x49,
    0x56, 0x20, 0x50, 0x00, 0x08, 0x07, 0x03, 0x00, 0x00, 0x1C, 0x22, 0x41, 0x00, 0x00, 0x41, 0x22,
    0x1C, 0x00, 0x2A, 0x1C, 0x7F, 0x1C, 0x2A, 0x08, 0x08, 0x3E, 0x08, 0x08, 0x00, 0x80, 0x70, 0x30,
    0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x00, 0x60, 0x60, 0x00, 0x20, 0x10, 0x08, 0x04, 0x02,
    0x3E, 0x51, 0x49, 0x45, 0x3E, 0x00, 0x42, 0x7F, 0x40, 0x00, 0x72, 0x49, 0x49, 0x49, 0x46, 0x21,
    0x41, 0x49, 0x4D, 0x33, 0x18, 0x14, 0x12, 0x7F, 0x10, 0x27, 0x45, 0x45, 0x45, 0x39, 0x3C, 0x4A,
    0x49, 0x49, 0x31, 0x41, 0x21, 0x11, 0x09, 0x07, 0x36, 0x49, 0x49, 0x49, 0x36, 0x46, 0x49, 0x49,
    0x29, 0x1E, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x40, 0x34, 0x00, 0x00, 0x00, 0x08, 0x14, 0x22,
    0x41, 0x14, 0x14, 0x14, 0x14, 0x14, 0x00, 0x41, 0x22, 0x14, 0x08, 0x02, 0x01, 0x59, 0x09, 0x06,
    0x3E, 0x41, 0x5D, 0x59, 0x4E, 0x7C, 0x12, 0x11, 0x12, 0x7C, 0x7F, 0x49, 0x49, 0x49, 0x36, 0x3E,
    0x41, 0x41, 0x41, 0x22, 0x7F, 0x41, 0x41, 0x41, 0x3E, 0x7F, 0x49, 0x49, 0x49, 0x41, 0x7F, 0x09,
    0x09, 0x09, 0x01, 0x3E, 0x41, 0x41, 0x51, 0x73, 0x7F, 0x08, 0x08, 0x08, 0x7F, 0x00, 0x41, 0x7F,
    0x41, 0x00, 0x20, 0x40, 0x41, 0x3F, 0x01, 0x7F, 0x08, 0x14, 0x22, 0x41, 0x7F, 0x40, 0x40, 0x40,
    0x40, 0x7F, 0x02, 0x1C, 0x02, 0x7F, 0x7F, 0x04, 0x08, 0x10, 0x7F, 0x3E, 0x41, 0x41, 0x41, 0x3E,
    0x7F, 0x09, 0x09, 0x09, 0x06, 0x3E, 0x41, 0x51, 0x21, 0x5E, 0x7F, 0x09, 0x19, 0x29, 0x46, 0x26,
    0x49, 0x49, 0x49, 0x32, 0x03, 0x01, 0x7F, 0x01, 0x03, 0x3F, 0x40, 0x40, 0x40, 0x3F, 0x1F, 0x20,
    0x40, 0x20, 0x1F, 0x3F, 0x40, 0x38, 0x40, 0x3F, 0x63, 0x14, 0x08, 0x14, 0x63, 0x03, 0x04, 0x78,
    0x04, 0x03, 0x61, 0x59, 0x49, 0x4D, 0x43, 0x00, 0x7F, 0x41, 0x41, 0x41, 0x02, 0x04, 0x08, 0x10,
    0x20, 0x00, 0x41, 0x41, 0x41, 0x7F, 0x04, 0x02, 0x01, 0x02, 0x04, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x00, 0x03, 0x07, 0x08, 0x00, 0x20, 0x54, 0x54, 0x78, 0x40, 0x7F, 0x28, 0x44, 0x44, 0x38, 0x38,
    0x44, 0x44, 0x44, 0x28, 0x38, 0x44, 0x44, 0x28, 0x7F, 0x38, 0x54, 0x54, 0x54, 0x18, 0x00, 0x08,
    0x7E, 0x09, 0x02, 0x18, 0xA4, 0xA4, 0x9C, 0x78, 0x7F, 0x08, 0x04, 0x04, 0x78, 0x00, 0x44, 0x7D,
    0x40, 0x00, 0x20, 0x40, 0x40, 0x3D, 0x00, 0x7F, 0x10, 0x28, 0x44, 0x00, 0x00, 0x41, 0x7F, 0x40,
    0x00, 0x7C, 0x04, 0x78, 0x04, 0x78, 0x7C, 0x08, 0x04, 0x04, 0x78, 0x38, 0x44, 0x44, 0x44, 0x38,
    0xFC, 0x18, 0x24, 0x24, 0x18, 0x18, 0x24, 0x24, 0x18, 0xFC, 0x7C, 0x08, 0x04, 0x04, 0x08, 0x48,
    0x54, 0x54, 0x54, 0x24, 0x04, 0x04, 0x3F, 0x44, 0x24, 0x3C, 0x40, 0x40, 0x20, 0x7C, 0x1C, 0x20,
    0x40, 0x20, 0x1C, 0x3C, 0x40, 0x30, 0x40, 0x3C, 0x44, 0x28, 0x10, 0x28, 0x44, 0x4C, 0x90, 0x90,
    0x90, 0x7C, 0x44, 0x64, 0x54, 0x4C, 0x44, 0x00, 0x08, 0x36, 0x41, 0x00, 0x00, 0x00, 0x77, 0x00,
    0x00, 0x00, 0x41, 0x36, 0x08, 0x00, 0x02, 0x01, 0x02, 0x04, 0x02,
];

/// ST7789 panel framed as an `RgbLight` surface.
pub struct DisplayLight {
    /// Which light surface this panel renders (wiring order); the diagnostics
    /// snapshot carries every surface, so the panel reads its own row.
    instance: u8,
    panel: St7789,
    frame: Vec<u8>,
    screen_color: Option<(Fill, Rgb)>,
    backlight: Backlight,
    /// Diagnostic overlay payload: touch counters, light mode, and motion (see
    /// [`Diagnostics`]). A bump repaints via [`Self::paint`], which the
    /// `screen_color` guard would otherwise skip.
    diagnostics: Diagnostics,
    /// Panel repaint rate over the last window, `0` while nothing repaints.
    fps: u8,
    fps_frames: u32,
    fps_anchor: Instant,
    /// Worst render and worst blocking-SPI cost seen in the current
    /// [`COST_WINDOW_MS`] window, with the repaint count that produced them. The
    /// count is the point as much as the peaks are: it is what says whether a
    /// page predicate is actually holding the blocking duty cycle down, or only
    /// appears to.
    cost_frames: u32,
    cost_anchor: Instant,
    worst_render: Duration,
    worst_write: Duration,
}

impl DisplayLight {
    /// Raise the backlight to full as part of bring-up; the LEDC PWM channel
    /// keeps the active-low pin pulled low (bright) until a renderer write.
    pub fn new(instance: u8, panel: St7789, mut backlight: Backlight) -> Self {
        backlight.set_level_pct(100);
        log::info!("[DISPLAY] backlight raised");
        log::info!("[DISPLAY] main stack {} B", main_stack_bytes());
        Self {
            instance,
            panel,
            frame: Vec::new(),
            screen_color: None,
            backlight,
            diagnostics: Diagnostics::default(),
            fps: 0,
            fps_frames: 0,
            fps_anchor: Instant::now(),
            cost_frames: 0,
            cost_anchor: Instant::now(),
            worst_render: Duration::ZERO,
            worst_write: Duration::ZERO,
        }
    }

    fn frame_bytes(&self) -> usize {
        usize::from(self.panel.width()) * usize::from(self.panel.height()) * 2
    }
}

impl RgbLight for DisplayLight {
    fn repaint_step(&self) -> u8 {
        REPAINT_STEP
    }

    fn set_backlight(&mut self, level_pct: u8) {
        self.backlight.set_level_pct(level_pct);
    }

    fn set_fill(&mut self, fill: Fill, color: Rgb) {
        if self.screen_color == Some((fill, color)) {
            return;
        }
        self.paint(fill, color);
    }
}

impl DiagnosticsSink for DisplayLight {
    fn consume(&mut self, diagnostics: &Diagnostics) {
        if self.diagnostics == *diagnostics {
            return;
        }
        let page_changed = self.diagnostics.page != diagnostics.page;
        // A page switch is drawn immediately; a diagnostics drift is only drawn
        // when the page redraws it and the rate gate has opened. The gate exists
        // because a repaint ships the whole 240×320 frame down one blocking SPI
        // transfer, and without it the live pages (Audio follows the capture,
        // Attitude the motion sample) hold the shared cooperative executor in a
        // ~15 ms block every 20 ms snapshot — starving the playback feed and the
        // capture poll, which then restart their DMAs (the `RST` counter climbs).
        let now_ms = Instant::now().duration_since_epoch().as_millis() as u32;
        let since_ms = now_ms.wrapping_sub(DIAG_LAST_REPAINT_MS.load(Ordering::Relaxed)) as u64;
        let repaint = page_changed
            || diag_repaint_due(
                diagnostics.page,
                &self.diagnostics,
                diagnostics,
                since_ms,
                DEBUG_DIAGNOSTICS,
            );
        // The previous snapshot is read through `self` right up to the decision
        // above rather than saved into a local. `Diagnostics` embeds a 1.6 KB
        // audio envelope, so one by-value copy of it here was a large slice of
        // the main stack this repaint path was already overflowing.
        self.diagnostics = *diagnostics;
        if !repaint {
            return;
        }
        if let Some((fill, color)) = self.screen_color {
            note_stack_high_water();
            self.paint(fill, color);
            DIAG_LAST_REPAINT_MS.store(now_ms, Ordering::Relaxed);
        }
    }
}

impl DisplayLight {
    /// Paints `fill`/`color`, stamps the diagnostics rows into the corner, and ships
    /// the frame. Always paints; callers guard for repaint skipping.
    fn paint(&mut self, fill: Fill, color: Rgb) {
        let render_start = Instant::now();
        self.screen_color = Some((fill, color));
        self.frame.resize(self.frame_bytes(), 0);

        let height = self.panel.height();
        let colors = |row: u16| match fill {
            Fill::Uniform => color,
            Fill::VerticalGradient => scale_brightness(color, vertical_brightness(row, height)),
        };
        let width = usize::from(self.panel.width());
        for (row, px) in self.frame.chunks_exact_mut(width * 2).enumerate() {
            let rgb565 = rgb565(colors(row as u16));
            let hi = (rgb565 >> 8) as u8;
            let lo = rgb565 as u8;
            for pixel in px.chunks_exact_mut(2) {
                pixel[0] = hi;
                pixel[1] = lo;
            }
        }

        // In breathing mode the brightness/hue rows track the color actually
        // being driven this frame, so the digits undulate with the animation;
        // other modes show the snapshot's configured values. The panel reads
        // its own surface out of the device snapshot.
        let mine = self.diagnostics.lights[usize::from(self.instance)];
        let live = if mine.mode == MODE_BREATH {
            (brightness_of(color), rgb_hue(color))
        } else {
            (mine.brightness, mine.hue)
        };
        if self.diagnostics.page == DisplayPage::Attitude {
            self.stamp_attitude(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Audio {
            self.stamp_audio(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Speaker {
            self.stamp_speaker(width, usize::from(height));
        } else if DEBUG_DIAGNOSTICS {
            self.stamp_diagnostics(width, usize::from(height), live);
        }
        if DEBUG_DIAGNOSTICS {
            self.stamp_fps(width, usize::from(height));
        }

        // The render and the transfer are timed apart rather than together: this
        // whole path runs on the one cooperative executor the playback feed and
        // the capture poll share, and "the frame cost 20 ms" does not say whether
        // the frame can be made cheaper or whether the SPI transfer has to yield
        // the bus. Those need different fixes, so the log has to tell them apart.
        let render = render_start.elapsed();
        let write_start = Instant::now();
        if let Err(e) = self.panel.write_frame(&self.frame) {
            log::error!("[DISPLAY] frame write failed: {e:?}");
        }
        self.sample_frame_cost(render, write_start.elapsed());
        self.sample_fps();
    }

    /// Folds one frame's two costs into the current window and reports the peaks
    /// once the window closes. The repaint count carries as much weight as the
    /// peaks do: a page whose predicate is holding the panel quiet makes this
    /// report rare, and a low count next to a high playback feed rate is the
    /// result the gate was added to reach. The playback task reports on its own
    /// wall clock, so the two logs bracket the same window from either side.
    fn sample_frame_cost(&mut self, render: Duration, write: Duration) {
        self.cost_frames += 1;
        if render > self.worst_render {
            self.worst_render = render;
        }
        if write > self.worst_write {
            self.worst_write = write;
        }
        let now = Instant::now();
        let window = now - self.cost_anchor;
        if window.as_millis() >= COST_WINDOW_MS {
            log::info!(
                "[DISPLAY] {} repaints in {} ms, worst render {} ms, worst write {} ms",
                self.cost_frames,
                window.as_millis(),
                self.worst_render.as_millis(),
                self.worst_write.as_millis(),
            );
            self.cost_frames = 0;
            self.cost_anchor = now;
            self.worst_render = Duration::ZERO;
            self.worst_write = Duration::ZERO;
        }
    }

    /// Reports the deepest main-stack use seen so far, once per repaint window.
    /// Sampled here rather than at bring-up because the peak belongs to whichever
    /// page drew deepest, and the Audio page's sweep is the frame that decides
    /// whether the stack still fits.
    fn report_stack_high_water(&self) {
        let used = STACK_HIGH_WATER.load(Ordering::Relaxed) as usize;
        log::info!("[DISPLAY] stack peak {used} B of {} B", main_stack_bytes());
    }

    /// Samples the panel repaint rate into `fps`, resetting each window so a
    /// silent panel decays toward `0`.
    fn sample_fps(&mut self) {
        self.fps_frames += 1;
        let now = Instant::now();
        let window = now - self.fps_anchor;
        if window.as_millis() >= FPS_WINDOW_MS {
            self.fps = windowed_rate(self.fps_frames, window.as_millis());
            self.fps_frames = 0;
            self.fps_anchor = now;
            self.report_stack_high_water();
        }
    }

    /// Overdraws the corner as two columns of inverted-pixel rows. Touch column:
    /// the gesture counters, `DIR`, per-finger `P0*`/`P1*`, `XY`/`XY2`, `CHP`, and
    /// `FRM` — an applied-frame heartbeat, so a frozen `FRM` under a held finger
    /// tells "no frames arrived" from "coordinates did not move". Mode column: the
    /// active mode's own rows, breathing rows tracking the live color.
    fn stamp_diagnostics(&mut self, width: usize, height: usize, live: (u8, u8)) {
        let touch = self.diagnostics.touch;
        let light = self.diagnostics.lights[usize::from(self.instance)];
        let mut buf = [0u8; 6];

        let counters: [(&[u8], u16); 8] = [
            (b"GST".as_slice(), u16::from(touch.ghost)),
            (b"2F".as_slice(), u16::from(touch.two_finger_runs)),
            (b"PRS".as_slice(), u16::from(touch.presses)),
            (b"TAP".as_slice(), u16::from(touch.taps)),
            (b"DBL".as_slice(), u16::from(touch.double_taps)),
            (b"3T".as_slice(), u16::from(touch.triple_taps)),
            (b"LNG".as_slice(), u16::from(touch.long_presses)),
            (b"SWP".as_slice(), u16::from(touch.swipes)),
        ];
        for (row, (label, value)) in counters.into_iter().enumerate() {
            self.stamp_left(label, format_u16(value, &mut buf), row, width, height);
        }

        let mut dir = [b' '; 8];
        dir[0] = direction_arrow(touch.last_swipe_dir);
        dir[1] = b' ';
        let nd = write_u16(touch.last_swipe_dist, &mut dir, 2);
        self.stamp_left(b"DIR", &dir[..nd], 8, width, height);

        let mut ref_buf = [0u8; 8];
        let origin = format_pair(touch.last_gesture_origin, &mut ref_buf);
        self.stamp_left(b"XY", origin, 9, width, height);

        let mut end_buf = [0u8; 8];
        let end = format_pair(touch.last_gesture_end, &mut end_buf);
        self.stamp_left(b"XY2", end, 10, width, height);

        /// Overlay row label quartet for one finger slot: coordinate, resolved
        /// gesture digit, its value, and the live movement arrow.
        type SlotRow = (&'static [u8], &'static [u8], &'static [u8], &'static [u8]);

        const SLOT_ROWS: [SlotRow; 2] = [
            (b"P0", b"P0G", b"P0D", b"P0V"),
            (b"P1", b"P1G", b"P1D", b"P1V"),
        ];
        for (slot, (xy_label, gesture_label, value_label, dir_label)) in
            SLOT_ROWS.iter().enumerate()
        {
            let base = 11 + 4 * slot;
            let mut xy = [0u8; 8];
            let n = match touch.points[slot] {
                Some((x, y)) => {
                    let n = write_u16(x, &mut xy, 0);
                    xy[n] = b' ';
                    write_u16(y, &mut xy, n + 1)
                }
                None => {
                    xy[0] = b'-';
                    1
                }
            };
            self.stamp_left(xy_label, &xy[..n], base, width, height);

            let last = touch.finger[slot];
            let mut value = [b'-'; 6];
            let value_len = if last.kind == 0 {
                1
            } else {
                let digits = format_u16(last.value, &mut buf);
                value[..digits.len()].copy_from_slice(digits);
                digits.len()
            };
            let gesture = [gesture_glyph(last.kind)];
            self.stamp_left(gesture_label, &gesture, base + 1, width, height);
            self.stamp_left(value_label, &value[..value_len], base + 2, width, height);
            self.stamp_left(
                dir_label,
                &[direction_arrow(touch.live_dir[slot])],
                base + 3,
                width,
                height,
            );
        }

        self.stamp_left(
            b"CHP",
            format_u16(u16::from(touch.chip_gesture_id), &mut buf),
            19,
            width,
            height,
        );

        self.stamp_left(
            b"FRM",
            format_u16(touch.frames, &mut buf),
            20,
            width,
            height,
        );

        let mut lo_hi = [0u8; 8];
        let mut n = write_u16(u16::from(light.breath.min_brightness), &mut lo_hi, 0);
        lo_hi[n] = b' ';
        n = write_u16(u16::from(light.breath.max_brightness), &mut lo_hi, n + 1);

        match light.mode {
            MODE_BREATH => {
                self.stamp_right(mode_word(light.mode), b"MOD", 0, width, height);
                self.stamp_right(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    1,
                    width,
                    height,
                );
                self.stamp_right(&lo_hi[..n], b"LO HI", 2, width, height);
                self.stamp_right(
                    format_u16(light.breath.period_ms, &mut buf),
                    b"PER",
                    3,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    4,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(light.breath.hue_period_ms, &mut buf),
                    b"HPR",
                    5,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(light.breath.hue_span), &mut buf),
                    b"SPN",
                    6,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(light.breath.group_len), &mut buf),
                    b"GRP",
                    7,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(light.breath.saturation), &mut buf),
                    b"SAT",
                    8,
                    width,
                    height,
                );
            }
            MODE_SOLID => {
                self.stamp_right(mode_word(light.mode), b"MOD", 0, width, height);
                self.stamp_right(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    1,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    2,
                    width,
                    height,
                );
            }
            _ => self.stamp_right(mode_word(light.mode), b"MOD", 0, width, height),
        }
    }

    /// Overdraws the corner as two columns of inverted-pixel rows: the motion
    /// readout (`ERR`/`WAIT`/`READ` when the source has produced nothing or a read
    /// failed) on the left, and one firing count per semantic on the right, `N/A`
    /// where the board declares no such capability. Labels, fields and thresholds:
    /// `docs/content/development/iot/motion.md`.
    fn stamp_attitude(&mut self, width: usize, height: usize) {
        let top = OVERLAY_Y;
        self.stamp_text(b"ATTITUDE", OVERLAY_X, top, width, height);
        let Some(sample) = self.diagnostics.motion else {
            self.stamp_left(b"ERR", b"WAIT", 2, width, height);
            return;
        };
        if !sample.valid {
            self.stamp_left(b"ERR", b"READ", 2, width, height);
            return;
        }
        self.stamp_horizon(&sample, width, height);

        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let raw_accel = format_i32(i32::from(sample.raw_accel[axis]), &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"A0",
                    1 => b"A1",
                    _ => b"A2",
                },
                raw_accel,
                axis + 1,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let accel_mg = format_i32(sample.accel_mg[axis], &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"M0",
                    1 => b"M1",
                    _ => b"M2",
                },
                accel_mg,
                axis + 4,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let raw_gyro = format_i32(i32::from(sample.raw_gyro[axis]), &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"G0",
                    1 => b"G1",
                    _ => b"G2",
                },
                raw_gyro,
                axis + 7,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let gyro_dps = format_tenths(sample.gyro_dps_x10[axis], &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"D0",
                    1 => b"D1",
                    _ => b"D2",
                },
                gyro_dps,
                axis + 10,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let tilt = format_tenths(i32::from(sample.tilt_deg_x10[axis]), &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"R0",
                    1 => b"R1",
                    _ => b"R2",
                },
                tilt,
                axis + 13,
                width,
                height,
            );
        }
        self.stamp_left(
            b"ST",
            format_u16(u16::from(sample.status), &mut [0; 6]),
            16,
            width,
            height,
        );
        // What the recognizer's estimators decided on, which the raw axes cannot
        // show: a settled reading is near zero by definition, so a threshold has
        // to be read off the device.
        self.stamp_left(
            b"LR",
            format_i32(sample.gravity_deviation_mg, &mut [0; 12]),
            17,
            width,
            height,
        );
        self.stamp_left(
            b"SR",
            format_i32(sample.shake_residual_mg, &mut [0; 12]),
            18,
            width,
            height,
        );
        self.stamp_left(
            b"TR",
            format_i32(sample.tap_residual_mg, &mut [0; 12]),
            19,
            width,
            height,
        );
        let counts = self.diagnostics.motion_counts;
        let caps = self.diagnostics.motion_caps;
        // Every row is always drawn, so a semantic this board never declares
        // shows `N/A`: "the stack cannot report it" has to stay distinct from
        // "its threshold never fires".
        let rows: [(&[u8], MotionCapabilities, u16); 14] = [
            (b"TAP".as_slice(), MotionCapabilities::TAP, counts.taps),
            (
                b"2T".as_slice(),
                MotionCapabilities::TAP,
                counts.double_taps,
            ),
            (
                b"3T".as_slice(),
                MotionCapabilities::TAP,
                counts.triple_taps,
            ),
            (b"STL".as_slice(), MotionCapabilities::STILL, counts.still),
            (b"MOV".as_slice(), MotionCapabilities::MOVING, counts.moving),
            (
                b"ACT".as_slice(),
                MotionCapabilities::ACTIVITY,
                counts.activity,
            ),
            (b"STP".as_slice(), MotionCapabilities::STEP, counts.steps),
            (
                b"TIN".as_slice(),
                MotionCapabilities::TILT,
                counts.tilt_enters,
            ),
            (
                b"TOX".as_slice(),
                MotionCapabilities::TILT,
                counts.tilt_exits,
            ),
            (b"SHK".as_slice(), MotionCapabilities::SHAKE, counts.shakes),
            (
                b"LFT".as_slice(),
                MotionCapabilities::LIFT_PLACE,
                counts.lifts,
            ),
            (
                b"PLC".as_slice(),
                MotionCapabilities::LIFT_PLACE,
                counts.places,
            ),
            (
                b"PRT".as_slice(),
                MotionCapabilities::POSTURE,
                counts.portraits,
            ),
            (
                b"LND".as_slice(),
                MotionCapabilities::POSTURE,
                counts.landscapes,
            ),
        ];
        let mut buf = [0u8; 6];
        for (row, (label, capability, count)) in rows.into_iter().enumerate() {
            let value: &[u8] = if caps.contains(capability) {
                format_u16(count, &mut buf)
            } else {
                NOT_AVAILABLE
            };
            self.stamp_right(value, label, row, width, height);
        }
    }

    /// Overdraws the Audio page: the capture phase and its wall time, then the
    /// envelope as a scope sweep, one mirrored bar per panel column. The phase
    /// alone decides what is drawn, because the state layer already resolved which
    /// envelope that is. Readouts, scale and their meanings:
    /// `docs/content/development/iot/audio.md`.
    fn stamp_audio(&mut self, width: usize, height: usize) {
        // The snapshot is read in place rather than copied into a local: the
        // embedded `AudioEnvelope` is 1.6 KB, which on this stack was a large
        // slice of what a repaint could afford. Reading through `self` in the
        // arguments below keeps that borrowing honest without the copy.
        self.stamp_text(b"AUDIO", OVERLAY_X, OVERLAY_Y, width, height);
        // The corner readout is the number this page shows a human: the
        // A-weighted sound level of the same capture, counted in the same
        // decibels the SPL column uses, with its unit spelled out so a glance
        // answers "is it loud?" without converting dBFS.
        self.stamp_text_right(
            format_dba(
                spl(dbfs(self.diagnostics.audio.dba_lsb), SPL_OFFSET_DECIBELS),
                &mut [0u8; 12],
            ),
            width as isize - 1,
            OVERLAY_Y,
            width,
            height,
        );
        self.stamp_left(
            b"ST",
            match self.diagnostics.audio.phase {
                AudioPhase::Idle => b"IDLE",
                AudioPhase::Recording => b"REC",
                AudioPhase::Stopped => b"STOP",
            },
            1,
            width,
            height,
        );
        self.stamp_left(
            b"MS",
            format_i32(
                i32::try_from(self.diagnostics.audio.elapsed_ms).unwrap_or(i32::MAX),
                &mut [0u8; 12],
            ),
            2,
            width,
            height,
        );
        // The two levels, in decibels, because a level in LSB is a number only
        // this code can read: is anything arriving, and is it a voice or a knock.
        self.stamp_level(
            b"PK",
            dbfs(self.diagnostics.audio.envelope.loudest()),
            3,
            width,
            height,
        );
        self.stamp_level(
            b"RMS",
            dbfs(self.diagnostics.audio.envelope.loudest_rms()),
            4,
            width,
            height,
        );
        // What the sweep cannot say about itself. `COL` climbing with a peak that
        // does not is a quiet room, and `COL` climbing under a −60 dBFS peak is
        // a capture path that is not delivering samples at all.
        self.stamp_left(
            b"COL",
            format_u16(
                u16::from(self.diagnostics.audio.envelope.committed()),
                &mut [0u8; 6],
            ),
            5,
            width,
            height,
        );
        // How many times the capture had to be re-armed: what separates a quiet
        // room from a capture that keeps breaking, both of which draw a still
        // line.
        self.stamp_left(
            b"RST",
            format_u16(self.diagnostics.audio.restarts, &mut [0u8; 6]),
            6,
            width,
            height,
        );
        // A clip is an absolute statement about the whole window, not a level
        // reading, so it says so in words and not only as a mark. It takes the
        // unit column `CLIP` sits in for the same reason: a latch that appears
        // and clears must not push anything it appears next to.
        if self.diagnostics.audio.envelope.clipped() {
            self.stamp_unit(b"CLIP", 5, width, height);
        }
        if self.diagnostics.audio.phase == AudioPhase::Idle {
            self.stamp_text(b"TAP TO REC", OVERLAY_X, WAVE_TOP as usize, width, height);
            return;
        }
        let envelope = &self.diagnostics.audio.envelope;
        let peaks = envelope.weighted_released_peaks();
        let levels = envelope.weighted_rms_columns();
        let clip_age = envelope.clipped_age();
        self.stamp_sweep(&peaks, &levels, clip_age, width, height);
    }

    /// Overdraws the Speaker page: the phase, the sound a tap would play, the
    /// mute latch and the two tap tallies. Unlike the Audio page there is
    /// nothing here to animate — a sound is a one-shot and the state layer owns
    /// the whole of it — so the page is a readout plus its two gestures, spelled
    /// out so neither has to be discovered.
    fn stamp_speaker(&mut self, width: usize, height: usize) {
        let playback = self.diagnostics.playback;
        self.stamp_text(b"SPKR", OVERLAY_X, OVERLAY_Y, width, height);
        self.stamp_left(
            b"ST",
            match playback.phase {
                PlaybackPhase::Idle => b"IDLE",
                PlaybackPhase::Playing => b"PLAY",
            },
            1,
            width,
            height,
        );
        // The sound is the page's one piece of state a tap changes, so it gets a
        // row of its own: it is what the user is choosing between, and the
        // catalogue it walks is a word, not a level.
        self.stamp_left(
            b"SRC",
            match playback.sound {
                Sound::Chime => b"CHIME",
                Sound::Asset => b"ASSET",
            },
            2,
            width,
            height,
        );
        // A latch, so it reads as a state rather than as an event: `MUT` is only
        // stamped when it is engaged, the same way `CLIP` appears.
        if playback.muted {
            self.stamp_unit(b"MUTE", 1, width, height);
        }
        // Plays and dropped taps as two numbers rather than one, because they
        // answer different questions — "did it make a sound" and "did it hear me"
        // — and a single combined tally could not tell a quiet speaker from a
        // busy finger.
        self.stamp_left(
            b"PLY",
            format_u16(playback.plays, &mut [0u8; 6]),
            3,
            width,
            height,
        );
        self.stamp_left(
            b"DRP",
            format_u16(playback.dropped, &mut [0u8; 6]),
            4,
            width,
            height,
        );
        self.stamp_text(b"TAP PLAY", OVERLAY_X, WAVE_TOP as usize, width, height);
        self.stamp_text(
            b"HOLD MUTE",
            OVERLAY_X,
            WAVE_TOP as usize + FONT_H + OVERLAY_ROW_GAP,
            width,
            height,
        );
    }

    /// The meter: a logarithmic band, each column's A-weighted sustained level
    /// solid and its peak dithered outside it. The sweep uses the corner's own
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
    fn stamp_sweep(
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
            let top = (row - FONT_H as isize / 2).max(0) as usize;
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

    /// The floor, solid.
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
        for offset in 0..3 {
            for row in WAVE_TOP..(WAVE_TOP + 8) {
                self.stamp_pixel(SCAN_X + column + offset, row, width, height);
            }
        }
    }

    /// Overdraws the `FPS <rate>` badge in the corner, right-aligned so it stays
    /// flush as the rate grows digits.
    fn stamp_fps(&mut self, width: usize, height: usize) {
        let buf = &mut [0u8; 6];
        let digits = format_u16(u16::from(self.fps), buf);
        let pitch = FONT_W + OVERLAY_GAP;
        let value_left = width - digits.len() * pitch + OVERLAY_GAP;
        self.stamp_text(
            b"FPS",
            value_left - (3 * FONT_W + 4 * OVERLAY_GAP),
            FPS_Y,
            width,
            height,
        );
        self.stamp_text(digits, value_left, FPS_Y, width, height);
    }

    /// Writes one touch-column row: label at [`OVERLAY_X`], one space glyph,
    /// then the value at [`LEFT_VALUE_X`].
    fn stamp_left(&mut self, label: &[u8], value: &[u8], row: usize, width: usize, height: usize) {
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        self.stamp_text(label, OVERLAY_X, top, width, height);
        self.stamp_text(value, LEFT_VALUE_X, top, width, height);
    }

    /// Writes one Audio level row: the label, the level as dBFS, that reading's
    /// unit, and the same instant of sound as a pressure level with its own unit.
    ///
    /// Both readings belong on one line because they measure the same thing from
    /// two zeros, and a −21 that no one can judge against is the objection this
    /// page exists to answer — dBFS says how much of the converter the signal
    /// uses, dB SPL says how loud the room is, and only the second is a number
    /// anybody compares with a noise complaint.
    fn stamp_level(
        &mut self,
        label: &[u8],
        decibels: i16,
        row: usize,
        width: usize,
        height: usize,
    ) {
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        let columns = level_columns();
        self.stamp_text(label, OVERLAY_X, top, width, height);
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

    /// Writes a diagnostics row's unit at [`LEVEL_UNIT_X`]. Pinned to one column
    /// rather than flushed right so a value that changes width — a level growing
    /// a digit, `CLIP` appearing and clearing — cannot walk its unit sideways
    /// out from under the row above it: the eye finds one vertical line for the
    /// unit and never has to look for a second.
    fn stamp_unit(&mut self, text: &[u8], row: usize, width: usize, height: usize) {
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        self.stamp_text(text, LEVEL_UNIT_X, top, width, height);
    }

    /// Overdraws the attitude dial — the inverted counterpart of `R0`–`R2` —
    /// with ring, fixed wing/bank references, and a `-roll`/`pitch` horizon.
    fn stamp_horizon(&mut self, sample: &MotionSample, width: usize, height: usize) {
        let geo = horizon(sample.tilt_deg_x10[0], sample.tilt_deg_x10[1]);
        let cx = HORIZON_CX as isize;
        let cy = HORIZON_CY as isize;
        let r = HORIZON_R as isize;

        self.stamp_circle(cx, cy, r, width, height);

        // Fixed airframe reference: top bank index and center wing bar.
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

    /// Writes one mode-column row: value at [`RIGHT_VALUE_X`], the label
    /// right-aligned into the fixed five-glyph slot ending one space before
    /// it — so every label (`MOD` up to `LO HI`) floats right next to its
    /// value and the numbers line up in one column.
    fn stamp_right(&mut self, value: &[u8], label: &[u8], row: usize, width: usize, height: usize) {
        let pitch = FONT_W + OVERLAY_GAP;
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        self.stamp_text(value, RIGHT_VALUE_X, top, width, height);
        self.stamp_text(
            label,
            RIGHT_VALUE_X - (label.len() + 1) * pitch,
            top,
            width,
            height,
        );
    }

    fn stamp_text(&mut self, text: &[u8], left: usize, top: usize, width: usize, height: usize) {
        stamp_text(&mut self.frame, text, left, top, width, height)
    }

    /// `stamp_text` measured from the text's last column rather than its first,
    /// so a value that grows a digit grows leftwards into empty space.
    fn stamp_text_right(
        &mut self,
        text: &[u8],
        right: isize,
        top: usize,
        width: usize,
        height: usize,
    ) {
        stamp_text_right(&mut self.frame, text, right, top, width, height)
    }

    /// Inverts one pixel: the shared ink for glyphs and the dial. The glyph and
    /// text wrappers go straight to the free functions, since they have no state
    /// of their own to fold the frame into.
    fn stamp_pixel(&mut self, x: isize, y: isize, width: usize, height: usize) {
        stamp_pixel(&mut self.frame, x, y, width, height)
    }

    /// Inverts the straight run from `(x0, y0)` to `(x1, y1)` (Bresenham).
    fn stamp_line(
        &mut self,
        x0: isize,
        y0: isize,
        x1: isize,
        y1: isize,
        width: usize,
        height: usize,
    ) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.stamp_pixel(x, y, width, height);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Inverts the perimeter of the circle at `(cx, cy)` with radius `r`
    /// (midpoint algorithm).
    fn stamp_circle(&mut self, cx: isize, cy: isize, r: isize, width: usize, height: usize) {
        let mut x = r;
        let mut y = 0;
        let mut err = 1 - r;
        while x >= y {
            for (dx, dy) in [
                (x, y),
                (-x, y),
                (x, -y),
                (-x, -y),
                (y, x),
                (-y, x),
                (y, -x),
                (-y, -x),
            ] {
                self.stamp_pixel(cx + dx, cy + dy, width, height);
            }
            y += 1;
            if err <= 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }
}

/// The window's floor in decibels below full scale, which is the bottom of the
/// Audio page's scale. Read back from the mapping rather than written here, so a
/// band and its own ruler cannot end up describing different windows.
const SCOPE_DECIBEL_FLOOR: i16 = SCOPE_FLOOR_DECIBELS as i16;

/// Rows a level sits at, mirrored about [`WAVE_CY`]: the row the bar's edge
/// lands on. The same [`scope_height`] the drawing is built from, at panel
/// scale, so the ruler's lines and the bars' edges stay on the same decibels.
fn bar_rows(peak_lsb: u16) -> isize {
    WAVE_HALF * isize::from(scope_height(peak_lsb)) / isize::from(u8::MAX)
}

/// The row `decibels` below full scale falls on, above the centre line. Measured
/// out from the *floor* rather than the rail, because the band is mirrored: the
/// floor is the middle and 0 dBFS is the edge.
fn decibels_row(decibels: i16) -> isize {
    WAVE_CY - WAVE_HALF * (SCOPE_DECIBEL_FLOOR - decibels) as isize / SCOPE_DECIBEL_FLOOR as isize
}

/// A ruler label: decibels under full scale, signed, with the zero written bare
/// because `-0` is not a level anyone says out loud.
fn format_decibels(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    format_i32(i32::from(decibels), buf)
}

/// The sweep's own time span, in tenths of a second and a unit — `2.0S` for the
/// window the envelope is drawn at. Computed rather than printed, because a
/// hardcoded label goes on claiming two seconds through any later window.
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

/// Inverts one pixel of `frame`, the shared ink for glyphs and the dial. A
/// function of the frame alone rather than a method, so that everything it
/// builds can be stamped into a bare buffer and read back as pixels — the
/// layout of a readout is otherwise only checkable on a panel.
fn stamp_pixel(frame: &mut [u8], x: isize, y: isize, width: usize, height: usize) {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
        return;
    }
    let offset = (y as usize * width + x as usize) * 2;
    let packed = u16::from_be_bytes([frame[offset], frame[offset + 1]]);
    frame[offset..offset + 2].copy_from_slice(&invert_rgb565(packed).to_be_bytes());
}

/// Overdraws one 5×7 `ch` at `left`/`top` with inverted pixels; bytes outside
/// the printable ASCII block draw nothing.
fn stamp_char(frame: &mut [u8], ch: u8, left: usize, top: usize, width: usize, height: usize) {
    if !(0x20..=0x7E).contains(&ch) {
        return;
    }
    let base = usize::from(ch - 0x20) * FONT_W;
    for (col, &bits) in FONT[base..base + FONT_W].iter().enumerate() {
        for row in 0..FONT_H {
            if bits & (1 << row) == 0 {
                continue;
            }
            stamp_pixel(
                frame,
                left as isize + col as isize,
                top as isize + row as isize,
                width,
                height,
            );
        }
    }
}

/// Draws `text` left to right from `left` on one glyph pitch.
fn stamp_text(frame: &mut [u8], text: &[u8], left: usize, top: usize, width: usize, height: usize) {
    for (glyph, &ch) in text.iter().enumerate() {
        stamp_char(
            frame,
            ch,
            left + glyph * (FONT_W + OVERLAY_GAP),
            top,
            width,
            height,
        );
    }
}

/// `stamp_text` measured from the text's last column rather than its first, so a
/// value that grows a digit grows leftwards into empty space.
fn stamp_text_right(
    frame: &mut [u8],
    text: &[u8],
    right: isize,
    top: usize,
    width: usize,
    height: usize,
) {
    let pitch = (FONT_W + OVERLAY_GAP) as isize;
    let left = right + OVERLAY_GAP as isize - text.len() as isize * pitch;
    stamp_text(frame, text, left.max(0) as usize, top, width, height);
}

fn format_i32(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let negative = value < 0;
    let magnitude = if negative {
        -(value as i64) as u64
    } else {
        value as u64
    };
    let mut n = 0;
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    n = write_u64(magnitude, buf, n);
    &buf[..n]
}

/// A pressure level in decibels with its unit, as the corner readout: unlike a
/// level row's `DBFS`/`SPL` halves, this one carries no sign — the A-weighted
/// readout is clamped to the floor the envelope can even see, and a "−42 dBA"
/// that can only be wrong is worse than a floor that says so indirectly.
fn format_dba(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(decibels.unsigned_abs()), buf, 0);
    buf[n] = b' ';
    buf[n + 1..n + 4].copy_from_slice(b"dBA");
    &buf[..n + 4]
}

fn format_tenths(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let negative = value < 0;
    let magnitude = if negative {
        -(value as i64) as u64
    } else {
        value as u64
    };
    let mut n = 0;
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    n = write_u64(magnitude / 10, buf, n);
    buf[n] = b'.';
    buf[n + 1] = b'0' + (magnitude % 10) as u8;
    &buf[..n + 2]
}

/// Decimal ASCII digits of `value` (no leading zeros) written into the start
/// of `buf`, returned as the filled prefix.
fn write_u64(mut value: u64, buf: &mut [u8], mut n: usize) -> usize {
    let mut tmp = [0u8; 20];
    let mut len = 0;
    loop {
        tmp[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in tmp[..len].iter().rev() {
        buf[n] = digit;
        n += 1;
    }
    n
}

fn format_u16(value: u16, buf: &mut [u8; 6]) -> &[u8] {
    let n = write_u16(value, buf, 0);
    &buf[..n]
}

/// Appends the decimal ASCII digits of `value` to `buf` starting at offset
/// `n`; returns the new length.
fn write_u16(mut value: u16, buf: &mut [u8], mut n: usize) -> usize {
    let mut tmp = [0u8; 5];
    let mut len = 0;
    loop {
        tmp[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in tmp[..len].iter().rev() {
        buf[n] = digit;
        n += 1;
    }
    n
}

/// Renders an optional coordinate pair as `x y` (single dash when unset) for
/// the `XY`/`XY2` overlay rows.
fn format_pair(pair: Option<(u16, u16)>, buf: &mut [u8; 8]) -> &[u8] {
    match pair {
        Some((x, y)) => {
            let n = write_u16(x, buf, 0);
            buf[n] = b' ';
            let end = write_u16(y, buf, n + 1);
            &buf[..end]
        }
        None => {
            buf[0] = b'-';
            &buf[..1]
        }
    }
}

/// The value channel `hsv_to_rgb` encoded a color with — its brightest
/// channel — back out of the driven frame, for the live brightness readout.
fn brightness_of(Rgb(r, g, b): Rgb) -> u8 {
    r.max(g).max(b)
}

/// Three-letter light mode word matching the core snapshot's codes: `0` off,
/// `1` breathing, `2` solid.
fn mode_word(mode: u8) -> &'static [u8] {
    match mode {
        1 => b"BRE",
        2 => b"SOL",
        _ => b"OFF",
    }
}

/// ASCII digit of a movement-direction code (see `SwipeDirection::code`'s
/// canonical numpad table; `5` is reserved): a direction's number is exactly
/// its ASCII digit, matching the overlay's "no glyphs but digits" contract; a
/// dash when nothing is moving (code 0).
fn direction_arrow(dir: u8) -> u8 {
    match dir {
        0 => b'-',
        d @ 1..=9 => b'0' + d,
        _ => b'-',
    }
}

/// Single-digit glyph of a finger's last resolved gesture, matching the
/// core `FINGER_*` codes (`1` tap, `2` double-tap, `3` triple-tap, `4`
/// long-press, `5` swipe) so the per-finger rows and the on-panel digit
/// gradient agree; a dash before the slot ever resolves one.
fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_DOUBLE_TAP => b'2',
        FINGER_TRIPLE_TAP => b'3',
        FINGER_LONG_PRESS => b'4',
        FINGER_SWIPE => b'5',
        _ => b'-',
    }
}

/// Pack an RGB color into ST7789 big-endian RGB565.
fn rgb565(color: Rgb) -> u16 {
    let r = color.0 as u16;
    let g = color.1 as u16;
    let b = color.2 as u16;
    ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
}

/// Per-channel bitwise inversion of an RGB565 word: a digit drawn this way is
/// visible on any fill, matching the "off" side of the contrast regardless of
/// base color.
fn invert_rgb565(packed: u16) -> u16 {
    let r = (packed >> 11) & 0x1F;
    let g = (packed >> 5) & 0x3F;
    let b = packed & 0x1F;
    ((0x1F - r) << 11) | ((0x3F - g) << 5) | (0x1F - b)
}

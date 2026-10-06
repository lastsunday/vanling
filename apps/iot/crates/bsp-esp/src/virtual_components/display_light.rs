use alloc::boxed::Box;
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::audio::{
    COLUMN_MS, ENVELOPE_COLUMNS, SCOPE_FLOOR_DECIBELS, dbfs, scope_height, spl,
};
#[cfg(feature = "camera")]
use iot_core::drivers::camera::{CameraCounters, FrameAdvance, WindowGeometry};
use iot_core::drivers::camera::{CameraTarget, FrameOwner, FrameSource};
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
    Block, FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, PANEL_ROWS, RIGHT_EDGE_X,
    ROW_PITCH, block_value_x, block_x, level_columns, overlay_row_top,
};
use iot_core::render::{MODE_BREATH, MODE_SOLID, diag_repaint_due, windowed_rate};
use iot_core::state::{AudioPhase, DisplayPage, PlaybackPhase};

use crate::camera_readout::format_pair;
#[cfg(feature = "camera")]
use crate::camera_readout::{format_u32, frame_fingerprint, write_ratio};
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

/// Stand-in value for a semantic the board does not declare, matching the
/// `ERR`/`WAIT` sentinels the attitude page already uses for missing data.
const NOT_AVAILABLE: &[u8] = b"N/A";

/// Attitude dial center, in panel columns/rows. Anchored in the third text
/// block, which the attitude page leaves empty for it: the dial is 96 rows across,
/// so the block's own width is what decides the radius and the rows below the
/// counts column (the last ends at 184) are what is left over.
const HORIZON_CX: isize = 266;
const HORIZON_CY: isize = 120;
const HORIZON_R: isize = 48;

/// Top edge of the sweep band, in panel rows. The Audio page's two level rows
/// are the only readouts wide enough not to fit the narrow block, so the band is
/// given every row from below them down: a portrait panel spent its top third on
/// readouts stacked above a band it had already shortened.
const WAVE_TOP: isize = 42;
/// Centre line of the mirrored band, which is also the bottom of the scale: a
/// mirrored bar measures *outward* from silence here, so 0 dBFS is at the band's
/// two edges and the floor is the middle.
const WAVE_CY: isize = 132;
/// Rows the tallest column reaches up and down from [`WAVE_CY`]. Set to clear
/// [`WAVE_TOP`] and leave room for the span label, not chosen: a logarithmic
/// scale spends the rows it leaves free on under half a decibel each. At 90 it
/// also spends one row on one decibel, which is what makes the grid land whole.
const WAVE_HALF: isize = 90;

/// Decibels per grid line, chosen so a line lands on a whole row and the
/// ten-octave window divides into whole steps: [`WAVE_HALF`] rows per 90 dB is
/// one row per decibel, so twelve decibels is exactly twelve rows. A scale whose
/// lines land off-pixel reads as a scale that is wrong.
const GRID_DECIBELS: i16 = 12;

/// Left column of the sweep, and its width in panel columns. The width is
/// [`ENVELOPE_COLUMNS`] so one column is one pixel: a scale that resamples the
/// envelope has to drop peaks, and dropping peaks on a peak meter means the
/// meter misses the thing it exists to show. Those columns and the ruler beside
/// them are what fix the right-edge block's left edge, so neither can move
/// without it.
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
const SPAN_Y: usize = 224;

/// Top edge of the global FPS badge, which every page shows. Below the span label
/// and clear of the sweep, so the one strip that belongs to no page sits where no
/// page's content is, and against the same panel edge the Audio readouts use.
const FPS_Y: usize = 232;
const PANEL_RIGHT: isize = PANEL_COLUMNS as isize - 1;

/// The Audio page's two level rows, the only readouts wide enough not to fit a
/// block, and the rows the band has to start below.
const AUDIO_LAST_FULL_WIDTH_ROW: usize = 2;

/// The attitude page's tall block, whose last row the dial is centred against.
const LAST_BLOCK_ROW: usize = 17;

/// Rows the attitude page's estimator readouts take at the head of its second
/// block, before the cumulative counts start.
const ESTIMATOR_ROWS: usize = 4;

/// Semantic counters the attitude page prints, and the last row of the attitude
/// page's first block.
const MOTION_SEMANTIC_ROWS: usize = 14;
const ATTITUDE_LAST_AXIS_ROW: usize = 15;

/// Width in columns of the block that marks the column which clipped, which is
/// drawn past the sweep's last column by its own width.
const CLIP_MARK_WIDTH: isize = 3;

// The layout is only checkable against the panel it was chosen for, and a stamp
// that falls off an edge is clipped rather than reported — so every relationship
// that decides whether something lands on the panel is settled at compile time,
// on the target build this crate is only ever compiled for.
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
    ESTIMATOR_ROWS + MOTION_SEMANTIC_ROWS <= LAST_BLOCK_ROW + 1,
    "the attitude page's second block runs off the bottom of the panel"
);
const _: () = assert!(
    ATTITUDE_LAST_AXIS_ROW <= LAST_BLOCK_ROW,
    "the attitude page's first block runs off the bottom of the panel"
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
    overlay_row_top(LAST_BLOCK_ROW) + FONT_H <= PANEL_ROWS,
    "the tallest text block runs off the bottom of the panel"
);
const _: () = assert!(
    SPAN_Y + FONT_H <= PANEL_ROWS,
    "the sweep's span label runs off the bottom of the panel"
);
const _: () = assert!(
    FPS_Y + FONT_H <= PANEL_ROWS,
    "the FPS badge runs off the bottom of the panel"
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
///
/// The frame buffer is the board's, not this type's: it is a static because it has to outlive
/// the DMA transfer the peripheral keeps running against it, and because its size is part of
/// the board's memory budget rather than of what one image happens to ask the heap for. A
/// camera wired to the same panel fills those same bytes, which is why they are handed in here
/// rather than allocated — one buffer, two engines, never at once.
pub struct DisplayLight {
    /// Which light surface this panel renders (wiring order); the diagnostics
    /// snapshot carries every surface, so the panel reads its own row.
    instance: u8,
    panel: St7789,
    frame: &'static mut [u8],
    /// The camera behind the Camera page, if this board mounted one. Absent means the page
    /// never appears; it is an `Option` rather than a required parameter so a board with no
    /// sensor needs no branch anywhere else.
    #[cfg(feature = "camera")]
    camera: Option<Box<dyn FrameSource>>,
    /// Which engine owns the frame buffer, and what the CPU last drew while it held it.
    /// Unconditional because the colour half is also how a panel with no camera decides
    /// whether a fill is worth shipping, so gating it would leave one half of the decision
    /// outside the value that holds it.
    owner: FrameOwner,
    backlight: Backlight,
    /// Diagnostic overlay payload: touch counters, light mode, and motion (see
    /// [`Diagnostics`]). A bump repaints via [`Self::paint`], which the
    /// [`FrameOwner::painted_color`] guard would otherwise skip.
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

/// A surface with no camera bound: the trait's two methods are how the app hands one over, and
/// a panel with nothing mounted answers `false` and drops it. Reached only when the app had no
/// camera to hand, which a board with no sensor is the ordinary case for.
impl CameraTarget for DisplayLight {
    fn paint_camera(&mut self, now_ms: u64) -> bool {
        #[cfg(feature = "camera")]
        {
            DisplayLight::paint_camera(self, now_ms)
        }
        #[cfg(not(feature = "camera"))]
        {
            let _ = now_ms;
            false
        }
    }

    fn attach_camera(&mut self, camera: Box<dyn FrameSource>) {
        #[cfg(feature = "camera")]
        {
            self.camera = Some(camera);
        }
        #[cfg(not(feature = "camera"))]
        {
            let _ = camera;
            log::info!("[CAM] this build has no camera path, dropping it");
        }
    }
}

impl DisplayLight {
    /// Raise the backlight to full as part of bring-up; the LEDC PWM channel
    /// keeps the active-low pin pulled low (bright) until a renderer write.
    pub fn new(
        instance: u8,
        panel: St7789,
        mut backlight: Backlight,
        frame: &'static mut [u8],
    ) -> Self {
        backlight.set_level_pct(100);
        log::info!("[DISPLAY] backlight raised");
        log::info!("[DISPLAY] main stack {} B", main_stack_bytes());
        Self {
            instance,
            panel,
            frame,
            #[cfg(feature = "camera")]
            camera: None,
            owner: FrameOwner::NeedsPaint,
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

    #[cfg(feature = "camera")]
    fn frame_bytes(&self) -> usize {
        usize::from(self.panel.width()) * usize::from(self.panel.height()) * 2
    }

    /// This surface's buffer, for a board wiring a camera onto the same bytes.
    ///
    /// A window rather than the whole buffer: the camera fills a frame of exactly this size,
    /// and handing over more would let it write past the panel's window.
    #[cfg(feature = "camera")]
    pub fn frame(&mut self) -> &mut [u8] {
        // The length is taken first: computing it inside the index would borrow `self` to read
        // the panel while the same borrow is handing out the buffer.
        let bytes = self.frame_bytes();
        &mut self.frame[..bytes]
    }

    /// Binds the camera that fills this panel's buffer.
    ///
    /// Takes it by value and erases it to the trait, because the surface is its only
    /// consumer — a frame has nowhere else to be seen — and so that neither this signature
    /// nor the renderer's names a sensor.
    #[cfg(feature = "camera")]
    pub fn with_camera(mut self, camera: Box<dyn FrameSource>) -> Self {
        self.camera = Some(camera);
        self
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
        if !self.owner.accepts_fill(fill, color) {
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
        // Leaving the camera's page is what hands the buffer back, and it is done here rather
        // than in `paint` for two reasons. A chain left running would overwrite every fill
        // after it a row at a time; and waiting for `paint` to release it deadlocked the
        // panel, because taking the buffer is what clears the colour `paint` reads to know
        // what to draw — so the call that had to release it was gated on there being a colour,
        // and the page that needed releasing was the one that had cleared it.
        #[cfg(feature = "camera")]
        if page_changed && self.owner.is_camera() {
            if let Some(camera) = &mut self.camera {
                camera.pause();
            }
            self.owner.released();
        }
        // A page switch is drawn immediately; a diagnostics drift is only drawn
        // when the page redraws it and the rate gate has opened. The gate exists
        // because a repaint ships the whole 320×240 frame down one blocking SPI
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
        // above rather than saved into a local: `Diagnostics` embeds a 1.6 KB
        // audio envelope, and one by-value copy of it is a large slice of a main
        // stack this path is already close to overflowing.
        self.diagnostics = *diagnostics;
        if !repaint {
            return;
        }
        if let Some((fill, color)) = self.owner.painted_color() {
            note_stack_high_water();
            self.paint(fill, color);
            DIAG_LAST_REPAINT_MS.store(now_ms, Ordering::Relaxed);
        }
    }
}

impl DisplayLight {
    /// Paints `fill`/`color`, stamps the page's rows over it, and ships the frame.
    /// Always paints; callers guard for repaint skipping.
    fn paint(&mut self, fill: Fill, color: Rgb) {
        let render_start = Instant::now();
        self.owner.painted(fill, color);

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
        } else if self.diagnostics.page == DisplayPage::Camera {
            self.stamp_camera_page(width, usize::from(height));
        } else if DEBUG_DIAGNOSTICS {
            self.stamp_diagnostics(width, usize::from(height), live);
        }
        if DEBUG_DIAGNOSTICS {
            Self::stamp_fps(&mut self.frame, self.fps, width, usize::from(height));
        }

        // The render and the transfer are timed apart rather than together: this
        // whole path runs on the one cooperative executor the playback feed and
        // the capture poll share, and "the frame cost 20 ms" does not say whether
        // the frame can be made cheaper or whether the SPI transfer has to yield
        // the bus. Those need different fixes, so the log has to tell them apart.
        let render = render_start.elapsed();
        let write_start = Instant::now();
        if let Err(e) = self.panel.write_frame(self.frame) {
            log::error!("[DISPLAY] frame write failed: {e:?}");
        }
        self.sample_frame_cost(render, write_start.elapsed());
        self.sample_fps();
    }

    /// Advances the camera and, if a whole frame arrived, stamps the readout over it and
    /// ships it. Returns whether this pass painted.
    ///
    /// The frame is not copied: the buffer the camera just filled is the one the panel reads,
    /// which is why they are the same bytes. The ordering is the load-bearing part — `advance`
    /// reports a frame and does not re-arm until the next call, so nothing writes the buffer
    /// between it being reported and the panel being given it, and the readout goes on top of
    /// a picture that is not being rewritten underneath.
    #[cfg(feature = "camera")]
    pub(crate) fn paint_camera(&mut self, now_ms: u64) -> bool {
        // Each field is borrowed where it is needed rather than copied out, and the borrows
        // are of disjoint fields so they hold at once. The snapshot is the reason: it embeds a
        // 1.6 KB audio envelope, and copying it here would put a kilobyte and a half on a stack
        // this path is already close to the floor of — see the note on `paint`.
        let bytes = self.frame_bytes();
        let diagnostics = &self.diagnostics;
        let fps = self.fps;
        let width = usize::from(self.panel.width());
        let height = usize::from(self.panel.height());
        let Some(camera) = &mut self.camera else {
            return false;
        };
        let frame = &mut self.frame[..bytes];

        // Parked by the last page change, so this is where it comes back — with the chain it
        // handed back, over a buffer whose contents were a colour fill until now.
        if !self.owner.is_camera() {
            camera.resume(frame);
            self.owner.taken();
        }
        let sample = camera.advance(frame, now_ms);
        if sample != FrameAdvance::Fresh {
            return false;
        }
        let counters = camera.counters();
        let geometry = camera.geometry();

        let render_start = Instant::now();
        // A free function rather than a method: the glyph writes need the buffer while the
        // snapshot and the panel are reached through `self`, and a method taking `&mut self`
        // could not hold those at once. Disjoint fields, so it can be handed all three. The
        // rate is read out first because it is a `self` field and `frame` borrows `self`.
        stamp_camera(
            frame,
            width,
            height,
            counters,
            geometry,
            diagnostics,
            fps,
            false,
        );
        let render = render_start.elapsed();

        let write_start = Instant::now();
        if let Err(error) = self.panel.write_frame(&self.frame[..bytes]) {
            log::error!("[DISPLAY] camera frame write failed: {error:?}");
        }
        self.sample_frame_cost(render, write_start.elapsed());
        self.sample_fps();
        // Sampled here because the camera page reaches the panel through this path and nowhere
        // else: without it the reading is `0 B` on this page for the whole run, which says
        // nothing about the stack. `consume` covers the light pages and `stamp_sweep` the audio
        // page's own working set.
        note_stack_high_water();
        true
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

    /// Overdraws the ambient overlay as three blocks of inverted-pixel rows,
    /// which is what the landscape panel's extra width is for. First block: the
    /// gesture counters, `DIR`, and the two gesture coordinate pairs. Second
    /// block: the per-finger readouts and the two rows that say whether the input
    /// path is alive. Third block: the active mode's own rows, breathing rows
    /// tracking the live color.
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
            self.stamp_left(
                label,
                format_u16(value, &mut buf),
                Block::First,
                row,
                width,
                height,
            );
        }

        let mut dir = [b' '; 8];
        dir[0] = direction_arrow(touch.last_swipe_dir);
        dir[1] = b' ';
        let nd = write_u16(touch.last_swipe_dist, &mut dir, 2);
        self.stamp_left(b"DIR", &dir[..nd], Block::First, 8, width, height);

        let mut ref_buf = [0u8; 12];
        let origin = format_pair(touch.last_gesture_origin, &mut ref_buf);
        self.stamp_left(b"XY", origin, Block::First, 9, width, height);

        let mut end_buf = [0u8; 12];
        let end = format_pair(touch.last_gesture_end, &mut end_buf);
        self.stamp_left(b"XY2", end, Block::First, 10, width, height);

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
            let base = 4 * slot;
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
            self.stamp_left(xy_label, &xy[..n], Block::Second, base, width, height);

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
            self.stamp_left(
                gesture_label,
                &gesture,
                Block::Second,
                base + 1,
                width,
                height,
            );
            self.stamp_left(
                value_label,
                &value[..value_len],
                Block::Second,
                base + 2,
                width,
                height,
            );
            self.stamp_left(
                dir_label,
                &[direction_arrow(touch.live_dir[slot])],
                Block::Second,
                base + 3,
                width,
                height,
            );
        }

        self.stamp_left(
            b"CHP",
            format_u16(u16::from(touch.chip_gesture_id), &mut buf),
            Block::Second,
            8,
            width,
            height,
        );

        self.stamp_left(
            b"FRM",
            format_u16(touch.frames, &mut buf),
            Block::Second,
            9,
            width,
            height,
        );

        let mut lo_hi = [0u8; 8];
        let mut n = write_u16(u16::from(light.breath.min_brightness), &mut lo_hi, 0);
        lo_hi[n] = b' ';
        n = write_u16(u16::from(light.breath.max_brightness), &mut lo_hi, n + 1);

        match light.mode {
            MODE_BREATH => {
                self.stamp_left(
                    mode_word(light.mode),
                    b"MOD",
                    Block::Third,
                    0,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    Block::Third,
                    1,
                    width,
                    height,
                );
                self.stamp_left(&lo_hi[..n], b"LO HI", Block::Third, 2, width, height);
                self.stamp_left(
                    format_u16(light.breath.period_ms, &mut buf),
                    b"PER",
                    Block::Third,
                    3,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    Block::Third,
                    4,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(light.breath.hue_period_ms, &mut buf),
                    b"HPR",
                    Block::Third,
                    5,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.hue_span), &mut buf),
                    b"SPN",
                    Block::Third,
                    6,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.group_len), &mut buf),
                    b"GRP",
                    Block::Third,
                    7,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.saturation), &mut buf),
                    b"SAT",
                    Block::Third,
                    8,
                    width,
                    height,
                );
            }
            MODE_SOLID => {
                self.stamp_left(
                    mode_word(light.mode),
                    b"MOD",
                    Block::Third,
                    0,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    Block::Third,
                    1,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    Block::Third,
                    2,
                    width,
                    height,
                );
            }
            _ => self.stamp_left(
                mode_word(light.mode),
                b"MOD",
                Block::Third,
                0,
                width,
                height,
            ),
        }
    }

    /// Overdraws the attitude page as two blocks of inverted-pixel rows and the
    /// dial in the third. First block: the motion readout (`ERR`/`WAIT`/`READ`
    /// when the source has produced nothing or a read failed), then one row per
    /// axis per channel — raw and scaled acceleration, raw gyro, yaw rate,
    /// roll/pitch. Second block: what the recognizer's estimators decided on,
    /// which the raw axes cannot show, then one firing count per semantic,
    /// `N/A` where the board declares no such capability. Third: the horizon
    /// dial. A block holds eighteen rows and the axis rows alone are fifteen, so
    /// the second block is where the estimators went rather than a fourth column.
    /// Labels, fields and thresholds: `docs/content/development/iot/motion.md`.
    fn stamp_attitude(&mut self, width: usize, height: usize) {
        self.stamp_text(
            b"ATTITUDE",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        let Some(sample) = self.diagnostics.motion else {
            self.stamp_left(b"ERR", b"WAIT", Block::First, 1, width, height);
            return;
        };
        if !sample.valid {
            self.stamp_left(b"ERR", b"READ", Block::First, 1, width, height);
            return;
        }
        self.stamp_horizon(&sample, width, height);

        // The label prefix carries the channel, the unit picks the formatter, and
        // `first_row` is where the block starts.
        const BLOCKS: [(u8, usize); 5] = [(b'A', 1), (b'M', 4), (b'G', 7), (b'D', 10), (b'R', 13)];
        for &(prefix, first_row) in &BLOCKS {
            for axis in 0..3 {
                let mut buf = [0u8; 12];
                let value = match prefix {
                    b'A' => format_i32(i32::from(sample.raw_accel[axis]), &mut buf),
                    b'M' => format_i32(sample.accel_mg[axis], &mut buf),
                    b'G' => format_i32(i32::from(sample.raw_gyro[axis]), &mut buf),
                    b'D' => format_tenths(sample.gyro_dps_x10[axis], &mut buf),
                    _ => format_tenths(i32::from(sample.tilt_deg_x10[axis]), &mut buf),
                };
                self.stamp_left(
                    &[prefix, b'0' + axis as u8],
                    value,
                    Block::First,
                    first_row + axis,
                    width,
                    height,
                );
            }
        }
        self.stamp_left(
            b"ST",
            format_u16(u16::from(sample.status), &mut [0; 6]),
            Block::Second,
            0,
            width,
            height,
        );
        // What the recognizer's estimators decided on, which the raw axes cannot
        // show: a settled reading is near zero by definition, so a threshold has
        // to be read off the device. `TR` publishes the tap peak threshold's
        // square root, the other two the residuals it is judged against.
        self.stamp_left(
            b"LR",
            format_i32(sample.gravity_deviation_mg, &mut [0; 12]),
            Block::Second,
            1,
            width,
            height,
        );
        self.stamp_left(
            b"SR",
            format_i32(sample.shake_residual_mg, &mut [0; 12]),
            Block::Second,
            2,
            width,
            height,
        );
        self.stamp_left(
            b"TR",
            format_i32(sample.tap_residual_mg, &mut [0; 12]),
            Block::Second,
            3,
            width,
            height,
        );
        let counts = self.diagnostics.motion_counts;
        let caps = self.diagnostics.motion_caps;
        // Every row is always drawn, so a semantic this board never declares
        // shows `N/A`: "the stack cannot report it" has to stay distinct from
        // "its threshold never fires".
        let rows: [(&[u8], MotionCapabilities, u16); MOTION_SEMANTIC_ROWS] = [
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
        for (index, (label, capability, count)) in rows.into_iter().enumerate() {
            let value: &[u8] = if caps.contains(capability) {
                format_u16(count, &mut buf)
            } else {
                NOT_AVAILABLE
            };
            self.stamp_left(
                label,
                value,
                Block::Second,
                ESTIMATOR_ROWS + index,
                width,
                height,
            );
        }
    }

    /// Overdraws the Audio page: the capture phase and its wall time beside the two
    /// level rows, then the envelope as a scope sweep taking the left of the
    /// panel and the narrow readouts the right of it. The phase alone decides
    /// what is drawn, because the state layer already resolved which envelope
    /// that is. Readouts, scale and their meanings:
    /// `docs/content/development/iot/audio.md`.
    fn stamp_audio(&mut self, width: usize, height: usize) {
        // The snapshot is read in place rather than copied into a local: the
        // embedded `AudioEnvelope` is 1.6 KB, which on this stack was a large
        // slice of what a repaint could afford. Reading through `self` in the
        // arguments below keeps that borrowing honest without the copy.
        //
        // The sweep owns the panel's left columns from `WAVE_TOP` down, so
        // everything this page reads out is either above that row or to the right
        // of it. The hint below stands in the ruler's top label's own rows, which
        // is safe only because the two are never drawn together: the hint is the
        // idle page's, and the idle page returns before the sweep.
        self.stamp_text(
            b"AUDIO",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        // The corner readout is the number this page shows a human: the
        // A-weighted sound level of the same capture, counted in the same
        // decibels the SPL column uses, with its unit spelled out so a glance
        // answers "is it loud?" without converting dBFS. It heads the readouts on
        // the right, which is the same position it held against the title.
        self.stamp_text_right(
            format_dba(
                spl(dbfs(self.diagnostics.audio.dba_lsb), SPL_OFFSET_DECIBELS),
                &mut [0u8; 12],
            ),
            PANEL_RIGHT,
            overlay_row_top(0),
            width,
            height,
        );
        self.stamp_level(
            b"PK",
            dbfs(self.diagnostics.audio.envelope.loudest()),
            1,
            width,
            height,
        );
        self.stamp_level(
            b"RMS",
            dbfs(self.diagnostics.audio.envelope.loudest_rms()),
            2,
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
            Block::RightEdge,
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
            Block::RightEdge,
            2,
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
            Block::RightEdge,
            3,
            width,
            height,
        );
        // How many times the capture had to be re-armed: what separates a quiet
        // room from a capture that keeps breaking, both of which draw a still
        // line.
        self.stamp_left(
            b"RST",
            format_u16(self.diagnostics.audio.restarts, &mut [0u8; 6]),
            Block::RightEdge,
            4,
            width,
            height,
        );
        // A clip is an absolute statement about the whole window, not a level reading,
        // so it says so in words and not only as a mark. It takes a row of its own
        // rather than a column beside `COL`, because the narrow block this page
        // reads in has no room for a fourth column — and a latch that appears and
        // clears must not push anything it appears next to.
        if self.diagnostics.audio.envelope.clipped() {
            self.stamp_text(
                b"CLIP",
                block_x(Block::RightEdge),
                overlay_row_top(5),
                width,
                height,
            );
        }
        if self.diagnostics.audio.phase == AudioPhase::Idle {
            self.stamp_text(
                b"TAP TO REC",
                block_x(Block::First),
                WAVE_TOP as usize,
                width,
                height,
            );
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
        self.stamp_text(
            b"SPKR",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        self.stamp_left(
            b"ST",
            match playback.phase {
                PlaybackPhase::Idle => b"IDLE",
                PlaybackPhase::Playing => b"PLAY",
            },
            Block::First,
            1,
            width,
            height,
        );
        self.stamp_left(
            b"SRC",
            match playback.sound {
                Sound::Chime => b"CHIME",
                Sound::Asset => b"ASSET",
            },
            Block::First,
            2,
            width,
            height,
        );
        // The latch is four glyphs, which is a glyph more than a block's label
        // slot, so it heads the second block rather than sitting beside the row
        // it latches: a latch that appears and clears must not push anything it
        // appears next to.
        if playback.muted {
            self.stamp_text(
                b"MUTE",
                block_x(Block::Second),
                overlay_row_top(1),
                width,
                height,
            );
        }
        self.stamp_left(
            b"PLY",
            format_u16(playback.plays, &mut [0u8; 6]),
            Block::First,
            3,
            width,
            height,
        );
        self.stamp_left(
            b"DRP",
            format_u16(playback.dropped, &mut [0u8; 6]),
            Block::First,
            4,
            width,
            height,
        );
        let hint = overlay_row_top(6);
        self.stamp_text(b"TAP PLAY", block_x(Block::First), hint, width, height);
        self.stamp_text(
            b"HOLD MUTE",
            block_x(Block::First),
            hint + ROW_PITCH,
            width,
            height,
        );
    }

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

    /// Draws the Camera page's own frame, for the tick where the page has just been entered
    /// and no sensor frame has arrived yet.
    ///
    /// The camera page has no fill — the sensor's picture is the page — but leaving the previous
    /// page's colour on the panel until a frame lands reads as a stall: at this sensor's rate
    /// that is about a fifth of a second of a frozen picture. This says `WAIT` instead, and
    /// leaves no fingerprint, because the bytes it would fold are the fill rather than a sensor
    /// picture.
    #[cfg(feature = "camera")]
    fn stamp_camera_page(&mut self, width: usize, height: usize) {
        stamp_camera(
            &mut self.frame,
            width,
            height,
            CameraCounters::ZERO,
            None,
            &self.diagnostics,
            self.fps,
            true,
        );
    }

    #[cfg(not(feature = "camera"))]
    fn stamp_camera_page(&mut self, _width: usize, _height: usize) {}

    /// Overdraws the `FPS <rate>` badge in the panel's bottom-right corner,
    /// right-aligned so it stays flush as the rate grows digits.
    ///
    /// A free function taking the rate rather than a method reading `self`, because the camera
    /// page stamps its readout over a buffer it already borrowed from `self` and cannot hold
    /// that borrow and the field at once. Every page draws this badge: a page missing it reads
    /// as a page that is not refreshing, which on the camera page is exactly the question the
    /// page exists to answer.
    fn stamp_fps(frame: &mut [u8], fps: u8, width: usize, height: usize) {
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
    fn stamp_left(
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
    fn stamp_level(
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
    fn stamp_horizon(&mut self, sample: &MotionSample, width: usize, height: usize) {
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

    /// Overdraws one run of text at a panel coordinate. The escape hatch the
    /// block rows above are deliberately not built from: a row placed by name is
    /// one whose position follows the layout, and a page that needs its words
    /// somewhere the blocks do not go says so here.
    fn stamp_text(&mut self, text: &[u8], left: usize, top: usize, width: usize, height: usize) {
        stamp_text(self.frame, text, left, top, width, height)
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
        stamp_text_right(self.frame, text, right, top, width, height)
    }

    /// Inverts one pixel: the shared ink for glyphs and the dial. The glyph and
    /// text wrappers go straight to the free functions, since they have no state
    /// of their own to fold the frame into.
    fn stamp_pixel(&mut self, x: isize, y: isize, width: usize, height: usize) {
        stamp_pixel(self.frame, x, y, width, height)
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

/// The Camera page's readout, stamped over the sensor's picture.
///
/// A free function of the buffer and its numbers, for the reason [`stamp_pixel`] is: a
/// camera readout's layout is otherwise only checkable by looking at a panel, and here it can
/// be stamped into a bare buffer and read back as pixels.
///
/// Two facts decide what is worth printing. The field of view is arithmetic — output width
/// over array width — so a decimation ratio that reads back wrong is a stretched picture of
/// the right shape, and the ratio and bins are what say so. And a pixel clock the sensor's PLL
/// will not lock to produces perfectly framed noise with every register reading back correct,
/// which the fingerprint is the only thing here that can see. Between them the geometry rows
/// answer "is this the picture I asked for" and the fingerprint answers "is there a picture".
///
/// The exposure line is the sensor's own and is AGC-driven, so it says whether a slow frame is
/// the part's doing rather than the transport's. It is read once at bring-up rather than
/// polled, because it is I²C and it does not change on its own.
#[cfg(feature = "camera")]
fn stamp_camera(
    frame: &mut [u8],
    width: usize,
    height: usize,
    counters: CameraCounters,
    geometry: Option<WindowGeometry>,
    diagnostics: &Diagnostics,
    fps: u8,
    // The page has just been entered and no frame has arrived yet, so the numbers would all
    // read zero and `NOBUS` would claim there is no sensor. Says it is starting instead.
    starting: bool,
) {
    stamp_text(
        frame,
        b"CAMERA",
        block_x(Block::First),
        overlay_row_top(0),
        width,
        height,
    );
    if starting {
        stamp_text(
            frame,
            b"WAIT",
            block_value_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
    }
    if let Some(geometry) = geometry {
        let ratio = &mut [0u8; 6];
        let ratio = write_ratio(ratio, geometry.subsample);
        stamp_text(
            frame,
            b"WIN",
            block_x(Block::First),
            overlay_row_top(1),
            width,
            height,
        );
        stamp_text(
            frame,
            ratio,
            block_value_x(Block::First),
            overlay_row_top(1),
            width,
            height,
        );
        let out = &mut [0u8; 12];
        let out = format_pair(Some((geometry.out_width, geometry.out_height)), out);
        stamp_text(
            frame,
            b"OUT",
            block_x(Block::First),
            overlay_row_top(2),
            width,
            height,
        );
        stamp_text(
            frame,
            out,
            block_value_x(Block::First),
            overlay_row_top(2),
            width,
            height,
        );
        let read = &mut [0u8; 12];
        let read = format_pair(Some((geometry.win_width, geometry.win_height)), read);
        stamp_text(
            frame,
            b"RD",
            block_x(Block::First),
            overlay_row_top(3),
            width,
            height,
        );
        stamp_text(
            frame,
            read,
            block_value_x(Block::First),
            overlay_row_top(3),
            width,
            height,
        );
    } else {
        stamp_text(
            frame,
            if starting { b"----" } else { b"NOBUS" },
            block_value_x(Block::First),
            overlay_row_top(1),
            width,
            height,
        );
    }

    // Frames, and the re-arms that cost: on a single-buffer ring every whole frame costs one
    // rebuild, so the two counters move together and a gap between them is what a stall looks
    // like.
    let frames = &mut [0u8; 12];
    let frames = format_u32(counters.frames, frames);
    stamp_text(
        frame,
        b"FRM",
        block_x(Block::Second),
        overlay_row_top(0),
        width,
        height,
    );
    stamp_text(
        frame,
        frames,
        block_value_x(Block::Second),
        overlay_row_top(0),
        width,
        height,
    );
    let restarts = &mut [0u8; 12];
    let restarts = format_u32(counters.restarts, restarts);
    stamp_text(
        frame,
        b"RM",
        block_x(Block::Second),
        overlay_row_top(1),
        width,
        height,
    );
    stamp_text(
        frame,
        restarts,
        block_value_x(Block::Second),
        overlay_row_top(1),
        width,
        height,
    );
    // The light's own row, because the Camera page does not draw the fill that row would
    // otherwise sit on — the state still moved, so the readout should still say where it is.
    let live = &diagnostics.lights[0];
    // Three glyphs, like every other label: the reading column sits a block label slot from
    // the block's left edge, and a four-glyph label fills that slot exactly, which leaves the
    // value butted against it. The value is the mode word either way, so the label says as
    // much as it needs to and no more.
    let mode = mode_word(live.mode);
    stamp_text(
        frame,
        b"MOD",
        block_x(Block::Second),
        overlay_row_top(2),
        width,
        height,
    );
    stamp_text(
        frame,
        mode,
        block_value_x(Block::Second),
        overlay_row_top(2),
        width,
        height,
    );

    // A fold over the frame's own bytes, over a prime stride so the walk lands on a spread of
    // rows rather than tracking one. Sparse on purpose: this runs on every painted frame.
    // Folded before any glyph goes down, so the number describes the sensor's picture and not
    // the readout sitting on it — otherwise the readout itself moves it every repaint.
    // Skipped while starting: the buffer holds the fill, not a sensor picture, so folding it
    // would report a fingerprint for a frame that does not exist.
    if !starting {
        let sampled = &mut [0u8; 12];
        let sampled = format_u32(frame_fingerprint(frame), sampled);
        stamp_text(
            frame,
            b"FP",
            block_x(Block::Third),
            overlay_row_top(0),
            width,
            height,
        );
        stamp_text(
            frame,
            sampled,
            block_value_x(Block::Third),
            overlay_row_top(0),
            width,
            height,
        );
    }

    if DEBUG_DIAGNOSTICS {
        DisplayLight::stamp_fps(frame, fps, width, height);
    }
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

fn stamp_text(frame: &mut [u8], text: &[u8], left: usize, top: usize, width: usize, height: usize) {
    for (glyph, &ch) in text.iter().enumerate() {
        stamp_char(frame, ch, left + glyph * GLYPH_PITCH, top, width, height);
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
    let left = right + OVERLAY_GAP as isize - text.len() as isize * GLYPH_PITCH as isize;
    stamp_text(frame, text, left.max(0) as usize, top, width, height);
}

fn format_i32(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_signed(value.unsigned_abs() as u64, value < 0, buf, 0);
    &buf[..n]
}

/// Writes a `-` sign when `negative`, then `magnitude`'s digits, starting at
/// offset `n`; returns the new length.
fn write_signed(magnitude: u64, negative: bool, buf: &mut [u8], mut n: usize) -> usize {
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    write_u64(magnitude, buf, n)
}

/// A pressure level in decibels with its unit, as the dB(A) readout heading the
/// Audio page's right-hand block: unlike a level row's `DBFS`/`SPL` halves,
/// this one carries no sign — the A-weighted readout is clamped to the floor
/// the envelope can even see, and a "−42 dBA" that can only be wrong is worse
/// than a floor that says so indirectly.
fn format_dba(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(decibels.unsigned_abs()), buf, 0);
    buf[n] = b' ';
    buf[n + 1..n + 4].copy_from_slice(b"dBA");
    &buf[..n + 4]
}

fn format_tenths(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let magnitude = value.unsigned_abs() as u64;
    let n = write_signed(magnitude / 10, value < 0, buf, 0);
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

fn write_u16(value: u16, buf: &mut [u8], n: usize) -> usize {
    write_u64(value.into(), buf, n)
}

/// Renders an optional coordinate pair as `x y` (single dash when unset) for
/// the `XY`/`XY2` overlay rows.
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

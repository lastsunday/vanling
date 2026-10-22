//! ST7789 panel framed as the product's `RgbLight` surface.
//!
//! The tree is split by what each part draws: `draw` is the ink and the glyph
//! work every page reads, `pages` the live readouts, `wifi` the network setup
//! UI, `meter` the audio band and the attitude dial, `camera` the sensor page,
//! and `stack` the main-stack high-water reading. The panel geometry they all
//! place against is here, next to the compile-time assertions that hold it.
use alloc::boxed::Box;
use iot_core::demand::{CAMERA_THUMBNAIL_PERIOD_MS, PlaneKey};
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::audio::{
    COLUMN_MS, ENVELOPE_COLUMNS, SCOPE_FLOOR_DECIBELS, dbfs, scope_height, spl,
};
use iot_core::drivers::camera::{
    CameraCounters, CameraTarget, FrameAdvance, FrameOwner, FrameSource, WindowGeometry,
};
use iot_core::drivers::input::{FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP};
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, rgb_hue, scale_brightness, vertical_brightness,
};
use iot_core::drivers::motion::{MotionCapabilities, MotionSample};
use iot_core::drivers::network::SSID_BYTES;
use iot_core::drivers::network::{AccessPoint, ApList, Front, Ipv4, LinkStatus};
use iot_core::horizon::{SCALE, horizon};
use iot_core::keyboard::{
    self, ControlRect, DIALOG_ANSWER_W, DIALOG_BODY_TOP, DIALOG_GLYPHS, DIALOG_H, DIALOG_INSET,
    DIALOG_TITLE_TOP, DIALOG_W, DIALOG_X, KEY_H, KEYBOARD_TOP, Key, LIST_ROW_H, LIST_ROWS,
    LIST_TOP, Layer, MAX_CANDIDATES, ROWS, band_lines_top,
};

use iot_core::overlay::{
    Block, CONTENT_BOTTOM, FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, PANEL_ROWS,
    RIGHT_EDGE_X, ROW_PITCH, block_value_x, block_x, level_columns, overlay_row_top,
};
use iot_core::page::{Part, walk};
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};
use iot_core::render::{
    MODE_BREATH, MODE_SOLID, diag_repaint_due, windowed_percent, windowed_rate,
};
use iot_core::stamp::{fill_surface, invert_rgb565, rgb565};
use iot_core::state::{AudioPhase, Confirm, DisplayPage, Entry, NetworkSetup, PlaybackPhase};

use crate::camera_readout::format_pair;
use crate::camera_readout::{
    NETWORK_NAME_GLYPHS, format_address, format_bars, format_body, format_joining_body,
    format_joining_title, format_network_name, format_phase, format_question, format_setup_name,
    format_setup_notice_body, format_setup_notice_title, format_setup_pass, format_verb,
};
use crate::camera_readout::{format_u32, frame_fingerprint, write_ratio};
use crate::components::backlight::Backlight;
use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::components::st7789::St7789;
use crate::font::FONT;
use crate::sounds::label_of;
use core::sync::atomic::{AtomicU32, Ordering};
use esp_hal::time::{Duration, Instant};

mod camera;
mod draw;
mod meter;
mod pages;
mod stack;
mod wifi;
use self::draw::{SCOPE_DECIBEL_FLOOR, brightness_of};
use self::stack::{main_stack_bytes, note_stack_high_water};

/// Last full-frame repaint taken by the panel, in milliseconds since boot.
/// Shared across the diagnostics pushes so the repaint rate bound adds no width
/// to the renderer state the main-task async machine holds across awaits.
static DIAG_LAST_REPAINT_MS: AtomicU32 = AtomicU32::new(0);

/// Deepest main-stack use observed so far, in bytes below the stack ceiling.
/// A rendering path that can walk tens of kilobytes down the stack is the kind
/// of thing that overflows silently after an innocent refactor, so the peak is
/// measured rather than assumed; see [`note_stack_high_water`].
static STACK_HIGH_WATER: AtomicU32 = AtomicU32::new(0);

/// Minimum per-channel color delta that warrants a full-frame repaint.
const REPAINT_STEP: u8 = 12;

/// On-panel repaint-rate sampling window.
const FPS_WINDOW_MS: u64 = 500;

/// Frame-cost sampling window. The two costs are reported apart because they have
/// opposite fixes: a slow render is CPU the shared executor does not get back,
/// while a slow write is GDMA arbitration the playback DMA has to win.
const COST_WINDOW_MS: u64 = 2_000;

/// How often a frame is written to the panel under a thumbnail request (a camera demand at
/// [`CAMERA_THUMBNAIL_PERIOD_MS`] or slower). The whole frame write is what the thumbnail
/// request exists to avoid.
const THUMBNAIL_INTERVAL_MS: u64 = CAMERA_THUMBNAIL_PERIOD_MS as u64;

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

/// Glyphs of the masked passphrase the row above the keyboard shows.
///
/// The whole row, from its reading column to the panel's right edge. It used to share the row with
/// the name it is being typed for and the phase word between them, and got what they left; the
/// keyboard now stops a row lower and the draft has the row to itself, so what it takes is the
/// width it can use. A draft can be 63 bytes and this is still shorter, which is why the row marks
/// the part of it that does not fit rather than pretending to show all of it.
const PSK_MASK_GLYPHS: usize =
    (PANEL_COLUMNS - block_value_x(Block::First) + OVERLAY_GAP) / GLYPH_PITCH;

/// Right edge of the word an open network's own list row carries, in panel columns.
///
/// Between the name and the bars rather than in either's place: which network a row is and how
/// strong it is are the two readings it exists for, and "no passphrase" is the one thing about a
/// row that the tap picking it acts on.
const OPEN_MARKER_RIGHT: isize = PANEL_RIGHT - 5 * GLYPH_PITCH as isize;

/// The word an open network's row carries, so the tap that connects it is not a surprise.
const OPEN_MARKER: &[u8] = b"OPEN";

/// Right edge of the mark a saved network's row carries, in panel columns.
///
/// Left of the open marker's own slot rather than in it, because the two are different facts about
/// a row and a network can be both: what the device would join again on its own, and what asks for
/// nothing to join now.
const SAVED_MARKER_RIGHT: isize =
    OPEN_MARKER_RIGHT - OPEN_MARKER.len() as isize * GLYPH_PITCH as isize - GLYPH_PITCH as isize;

/// The mark a saved network's row carries: the one glyph the font has for "this device keeps it".
///
/// A mark and not a word, unlike the open marker: the row has a name to show and a strength to
/// show, and this is a fact about the row rather than something a user acts on — the hold that acts
/// on it is what says what it means.
const SAVED_MARKER: &[u8] = b"*";

/// The action strip's words, which are what its cells can mean.
///
/// `LEAVE` is what a link can be traded for: the link goes down and setup mode opens in its place.
/// The word names what happens to the link rather than where the page lands, because the loss is
/// the part a person has to agree to; the box under it says the cost in full. It is not a constant
/// here: the cell draws [`camera_readout::format_verb`](crate::camera_readout::format_verb) for the
/// action it opens, so the strip and the box's own answer cannot drift apart. `CANCEL` is the
/// other direction, and it is only offered while there is a saved network to go back to.
///
/// `AP` and `PANEL` are the front switch, and they name the end they go to rather than the one
/// the page is showing: a switch is about the destination. `AP` is the access point a phone
/// joins; `PANEL` is this screen's own list.
///
/// `WAIT` is the strip's own state rather than a fifth action: a round of the survey is a fraction
/// of a second of radio time, and a strip that went on offering `SCAN` through it would be inviting
/// a second tap that asks for the same round again.
const CANCEL_LABEL: &[u8] = b"CANCEL";
const SCAN_LABEL: &[u8] = b"SCAN";
const WAIT_LABEL: &[u8] = b"WAIT";
const AP_LABEL: &[u8] = b"AP";
const PANEL_LABEL: &[u8] = b"PANEL";

/// The box's passphrase answer, either way round: what the tap does, named by what it makes.
const NO_PASS_LABEL: &[u8] = b"NO PASS";
const PASS_LABEL: &[u8] = b"PASS";

/// The strip's words have to fit the cells the tap geometry gives them: the narrowest cell is the
/// split strip's, and `CANCEL` is its longest word.
const _: () = assert!(
    CANCEL_LABEL.len() * GLYPH_PITCH - OVERLAY_GAP
        <= keyboard::toolbar_cell(keyboard::ToolbarCell::Left).w,
    "the way out of setup mode does not fit the cell the strip gives it"
);
const _: () = assert!(
    AP_LABEL.len() * GLYPH_PITCH - OVERLAY_GAP
        <= keyboard::toolbar_cell(keyboard::ToolbarCell::Right).w
        && PANEL_LABEL.len() * GLYPH_PITCH - OVERLAY_GAP
            <= keyboard::toolbar_cell(keyboard::ToolbarCell::Right).w,
    "the front switch does not fit the cell the strip gives it"
);
const _: () = assert!(
    NO_PASS_LABEL.len() * GLYPH_PITCH - OVERLAY_GAP <= DIALOG_ANSWER_W,
    "the box's own answer cannot hold the word of what it does"
);

/// Gap between the two lines of a key's own label: the face a phone keypad prints above the
/// characters the key cycles.
const KEY_LINE_GAP: usize = 4;

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

const _: () = assert!(
    block_value_x(Block::First) + PSK_MASK_GLYPHS * GLYPH_PITCH - OVERLAY_GAP <= PANEL_COLUMNS,
    "the draft a passphrase is typed into runs off the row's own right edge"
);
/// The widest link address the header's address row can put on the left: a dotted quad at its
/// longest (`255.255.255.255`).
const HEADER_ADDRESS_GLYPHS: usize = 15;

/// Glyphs the header's network row gives the name it is about, narrower than a list row's own slot:
/// the phase word shares that row.
///
/// A name that does not fit is folded to this width rather than allowed to run under the word beside
/// it, so where the phase starts does not move with the name printed before it.
const HEADER_NAME_GLYPHS: usize = 17;

/// Where the phase word starts on the header's network row: the cell after the name slot's last one.
const HEADER_PHASE_X: usize = block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH;

/// The phase word with the label that says what it is, at its widest: `ST` and one space, then the
/// longest word [`format_phase`] writes.
const HEADER_PHASE_GLYPHS: usize = 3 + 4;

/// The widest the signal bars are drawn on a header or a list row.
const BARS_GLYPHS: usize = 4;

const _: () = assert!(
    HEADER_PHASE_X + HEADER_PHASE_GLYPHS * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (BARS_GLYPHS * GLYPH_PITCH - OVERLAY_GAP),
    "the phase word runs into the signal bars on the header's network row"
);
const _: () = assert!(
    block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH - OVERLAY_GAP < HEADER_PHASE_X,
    "the name of the header's network runs into the phase word beside it"
);
/// The title the header's first row prints, which is what the list's own window starts after.
const LIST_TITLE_GLYPHS: usize = b"NETWORK".len();

/// The widest the list's window is drawn: two digits either side of the mark, and the count.
const LIST_WINDOW_GLYPHS: usize = 8;

const _: () = assert!(
    block_x(Block::First) + LIST_TITLE_GLYPHS * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (MAC_TEXT_BYTES * GLYPH_PITCH - OVERLAY_GAP),
    "the page's title runs into this board's own address on the header's title row"
);
const _: () = assert!(
    block_x(Block::First) + (LIST_TITLE_GLYPHS + 1 + LIST_WINDOW_GLYPHS) * GLYPH_PITCH
        - OVERLAY_GAP
        <= PANEL_COLUMNS - (MAC_TEXT_BYTES * GLYPH_PITCH - OVERLAY_GAP),
    "the list's window runs into this board's own address on the header's title row"
);
const _: () = assert!(
    block_x(Block::First) + (3 + HEADER_ADDRESS_GLYPHS) * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (MAC_TEXT_BYTES * GLYPH_PITCH - OVERLAY_GAP),
    "the link's address runs into the address of the network its own row names"
);
const _: () = assert!(
    block_x(Block::First) + NETWORK_NAME_GLYPHS * GLYPH_PITCH - OVERLAY_GAP + GLYPH_PITCH
        <= OPEN_MARKER_RIGHT as usize + 1 - (OPEN_MARKER.len() * GLYPH_PITCH - OVERLAY_GAP),
    "an open network's marker runs into the name of the row it is on"
);
const _: () = assert!(
    (OPEN_MARKER_RIGHT as usize) < PANEL_COLUMNS - BARS_GLYPHS * GLYPH_PITCH,
    "an open network's marker runs into the signal bars of the row it is on"
);
const _: () = assert!(
    LIST_TOP + LIST_ROWS * LIST_ROW_H <= CONTENT_BOTTOM,
    "the list's last band runs into the strip no page's content owns"
);
const _: () = assert!(
    KEYBOARD_TOP + ROWS * KEY_H <= CONTENT_BOTTOM,
    "the keyboard's last row runs into the strip no page's content owns"
);

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
    FPS_Y == CONTENT_BOTTOM,
    "the badge's strip is not the rows core sized the pages' controls against"
);
const _: () = assert!(
    KEYBOARD_TOP + keyboard::ROWS * KEY_H <= FPS_Y,
    "the keyboard's last row runs into the FPS badge, which is the strip no page's content owns"
);
const _: () = assert!(
    LIST_TOP + LIST_ROWS * LIST_ROW_H <= FPS_Y,
    "the Network page's list runs into the FPS badge, which is the strip no page's content owns"
);
const _: () = assert!(
    keyboard::TOOLBAR_TOP + keyboard::TOOLBAR_H <= FPS_Y,
    "the Network page's action strip runs into the FPS badge"
);
const _: () = assert!(
    SAVED_MARKER_RIGHT + OVERLAY_GAP as isize - GLYPH_PITCH as isize
        > (block_x(Block::First) + (NETWORK_NAME_GLYPHS - 1) * GLYPH_PITCH + FONT_W - 1) as isize,
    "the saved mark is drawn over the name slot the list gives a row"
);
const _: () = assert!(
    SAVED_MARKER_RIGHT < OPEN_MARKER_RIGHT,
    "the saved mark and the open marker are drawn in the same slot"
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
    camera: Option<Box<dyn FrameSource>>,
    /// Which engine owns the frame buffer, and what the CPU last drew while it held it.
    /// Unconditional because the colour half is also how a panel with no camera decides
    /// whether a fill is worth shipping, so gating it would leave one half of the decision
    /// outside the value that holds it.
    owner: FrameOwner,
    backlight: Backlight,
    /// Ceiling every `set_backlight` is clamped to, so a demand can hold the panel dim
    /// without the animation loop having to know it.
    backlight_ceiling: u8,
    /// The backlight level the renderer last asked for. Kept aside until the first
    /// frame reaches the panel: applying it earlier lights the previous run's GRAM.
    requested_backlight: u8,
    /// Whether a frame has actually been written since boot, which is what allows
    /// [`Self::requested_backlight`] to reach the LEDC channel.
    lit: bool,
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
    /// Every render-plus-write this window. The peaks above cannot be summed into it:
    /// 11 frames at 16 ms is not 16 ms, and a window reporting only a peak reads as cheaper
    /// than one spending twice as long on shorter frames.
    window_hold: Duration,
    /// When the panel last wrote a camera frame. `None` before the first one, which is what
    /// lets the first frame through without a warm-up.
    thumbnail_last_ms: Option<u64>,
}

/// How the app hands this surface a camera, and how a surface with none says so: with nothing
/// bound `paint_camera` answers `false`, which a board with no sensor is the ordinary case for.
impl CameraTarget for DisplayLight {
    fn paint_camera(&mut self, now_ms: u64) -> bool {
        DisplayLight::paint_camera(self, now_ms)
    }

    fn attach_camera(&mut self, camera: Box<dyn FrameSource>) {
        self.camera = Some(camera);
    }
}

impl DisplayLight {
    /// Takes the backlight dark and keeps it there until the first frame reaches
    /// the panel. The caller turns the LEDC channel off before the fallible part of
    /// bring-up; this type raises it only after a write succeeds.
    pub fn new(
        instance: u8,
        panel: St7789,
        backlight: Backlight,
        frame: &'static mut [u8],
    ) -> Self {
        log::info!("[DISPLAY] backlight held dark until the first frame");
        log::info!("[DISPLAY] main stack {} B", main_stack_bytes());
        Self {
            instance,
            panel,
            frame,
            camera: None,
            owner: FrameOwner::NeedsPaint,
            backlight,
            requested_backlight: 0,
            lit: false,
            backlight_ceiling: 100,
            diagnostics: Diagnostics::default(),
            fps: 0,
            fps_frames: 0,
            fps_anchor: Instant::now(),
            cost_frames: 0,
            cost_anchor: Instant::now(),
            worst_render: Duration::ZERO,
            worst_write: Duration::ZERO,
            thumbnail_last_ms: None,
            window_hold: Duration::ZERO,
        }
    }

    fn frame_bytes(&self) -> usize {
        usize::from(self.panel.width()) * usize::from(self.panel.height()) * 2
    }

    /// This surface's buffer, for a board wiring a camera onto the same bytes.
    ///
    /// A window rather than the whole buffer: the camera fills a frame of exactly this size,
    /// and handing over more would let it write past the panel's window.
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
        self.requested_backlight = level_pct.min(100);
        if self.lit {
            self.backlight
                .set_level_pct(self.requested_backlight.min(self.backlight_ceiling));
        }
    }

    fn set_backlight_ceiling(&mut self, level_pct: u8) {
        self.backlight_ceiling = level_pct.min(100);
        if self.lit {
            self.backlight
                .set_level_pct(self.requested_backlight.min(self.backlight_ceiling));
        }
    }

    fn set_fill(&mut self, fill: Fill, color: Rgb) {
        if !self.owner.accepts_fill(fill, color) {
            return;
        }
        self.paint(fill, color);
    }
}

impl DiagnosticsSink for DisplayLight {
    /// The panel is the surface the product's pages are drawn on: it is the one with the frame
    /// buffer, and `consume` below is what stamps the page's rows into it.
    fn draws_pages(&self) -> bool {
        true
    }

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
        // Gated on the demand rather than on the page: a demand granting no mode is
        // something an application other than a page switch could ask for.
        if !diagnostics
            .demand
            .plane(PlaneKey::CAMERA)
            .mode
            .is_collecting()
            && self.owner.is_camera()
        {
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
        self.diagnostics = diagnostics.clone();
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
        if self.diagnostics.page == DisplayPage::Splash {
            self.stamp_splash(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Attitude {
            self.stamp_attitude(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Audio {
            self.stamp_audio(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Speaker {
            self.stamp_speaker(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Camera {
            self.stamp_camera_page(width, usize::from(height));
        } else if self.diagnostics.page == DisplayPage::Network {
            self.stamp_network(width, usize::from(height));
        } else if DEBUG_DIAGNOSTICS {
            self.stamp_diagnostics(width, usize::from(height), live);
        }
        if DEBUG_DIAGNOSTICS {
            Self::stamp_fps(self.frame, self.fps, width, usize::from(height));
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
            // Stay dark and let the next fill retry: waking the panel now would
            // show the frame this write failed to replace.
            self.owner = FrameOwner::NeedsPaint;
        } else {
            self.note_frame_shipped();
        }
        self.sample_frame_cost(render, write_start.elapsed());
        self.sample_fps();
    }

    /// Raises the backlight once a frame has actually reached the panel.
    ///
    /// Until then the controller is still scanning whatever its own GRAM held
    /// before this boot, so lighting it early would present that picture as this
    /// run's first frame.
    fn note_frame_shipped(&mut self) {
        if !self.lit {
            self.lit = true;
            self.backlight
                .set_level_pct(self.requested_backlight.min(self.backlight_ceiling));
        }
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
        self.window_hold += render + write;
        let now = Instant::now();
        let window = now - self.cost_anchor;
        if window.as_millis() >= COST_WINDOW_MS {
            log::info!(
                "[DISPLAY] {:?} {} repaints in {} ms, held the executor {} ms ({}%), worst render {} ms, worst write {} ms; asked {}",
                self.diagnostics.page,
                self.cost_frames,
                window.as_millis(),
                self.window_hold.as_millis(),
                windowed_percent(self.window_hold.as_micros(), window.as_micros()),
                self.worst_render.as_millis(),
                self.worst_write.as_millis(),
                self.diagnostics.demand,
            );
            self.cost_frames = 0;
            self.cost_anchor = now;
            self.worst_render = Duration::ZERO;
            self.worst_write = Duration::ZERO;
            self.window_hold = Duration::ZERO;
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
}

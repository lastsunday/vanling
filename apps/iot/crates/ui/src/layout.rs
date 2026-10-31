//! Panel geometry the page renderer draws against.

use iot_core::controls::{AP_LABEL, CANCEL_LABEL, NO_PASS_LABEL, PANEL_LABEL};
use iot_core::drivers::audio::ENVELOPE_COLUMNS;
use iot_core::keyboard::{
    self, DIALOG_ANSWER_W, KEY_H, KEYBOARD_TOP, LIST_ROW_H, LIST_ROWS, LIST_TOP, ROWS,
};
use iot_core::overlay::{
    Block, CONTENT_BOTTOM, FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, PANEL_ROWS,
    RIGHT_EDGE_X, block_value_x, block_x, level_value_end, overlay_row_top,
};
use iot_core::provision::MAC_TEXT_BYTES;

use crate::text::NETWORK_NAME_GLYPHS;

pub(crate) const SCOPE_DECIBEL_FLOOR: i16 = iot_core::drivers::audio::SCOPE_FLOOR_DECIBELS as i16;

pub(crate) const NOT_AVAILABLE: &[u8] = b"N/A";

pub(crate) const HORIZON_CX: isize = 266;
pub(crate) const HORIZON_CY: isize = 120;
pub(crate) const HORIZON_R: isize = 48;

pub(crate) const WAVE_TOP: isize = 42;
pub(crate) const WAVE_CY: isize = 132;
pub(crate) const WAVE_HALF: isize = 90;

// Decibels per grid line, chosen so a line lands on a whole row and the
// ten-octave window divides into whole steps: [`WAVE_HALF`] rows per 90 dB is
// one row per decibel, so twelve decibels is exactly twelve rows. A scale whose
// lines land off-pixel reads as a scale that is wrong.
pub(crate) const GRID_DECIBELS: i16 = 12;

pub(crate) const SCAN_X: isize = 40;
pub(crate) const SCAN_COLUMNS: isize = 200;

pub(crate) const RULER_RIGHT: isize = 29;
pub(crate) const RULER_TICK_X: isize = 32;
pub(crate) const RULER_TICK_COLUMNS: isize = 6;

pub(crate) const SPAN_Y: usize = 224;

pub(crate) const FPS_Y: usize = 232;
pub(crate) const PANEL_RIGHT: isize = PANEL_COLUMNS as isize - 1;

// Glyphs of the masked passphrase on the row above the keyboard: the whole row, so a draft
// longer than it is marked rather than silently truncated.
pub(crate) const PSK_MASK_GLYPHS: usize =
    (PANEL_COLUMNS - block_value_x(Block::First) + OVERLAY_GAP) / GLYPH_PITCH;

pub(crate) const OPEN_MARKER_RIGHT: isize = PANEL_RIGHT - 5 * GLYPH_PITCH as isize;

pub(crate) const OPEN_MARKER: &[u8] = b"OPEN";

pub(crate) const SAVED_MARKER_RIGHT: isize =
    OPEN_MARKER_RIGHT - OPEN_MARKER.len() as isize * GLYPH_PITCH as isize - GLYPH_PITCH as isize;

pub(crate) const SAVED_MARKER: &[u8] = b"*";

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
    "the host switch does not fit the cell the strip gives it"
);
const _: () = assert!(
    NO_PASS_LABEL.len() * GLYPH_PITCH - OVERLAY_GAP <= DIALOG_ANSWER_W,
    "the box's own answer cannot hold the word of what it does"
);

pub(crate) const KEY_LINE_GAP: usize = 4;

pub(crate) const AUDIO_LAST_FULL_WIDTH_ROW: usize = 2;

pub(crate) const LAST_BLOCK_ROW: usize = 17;

pub(crate) const ESTIMATOR_ROWS: usize = 4;

pub(crate) const MOTION_SEMANTIC_ROWS: usize = 14;
pub(crate) const ATTITUDE_LAST_AXIS_ROW: usize = 15;

pub(crate) const CLIP_MARK_WIDTH: isize = 3;

const _: () = assert!(
    level_value_end(block_value_x(Block::First), PSK_MASK_GLYPHS) <= PANEL_COLUMNS,
    "the draft a passphrase is typed into runs off the row's own right edge"
);
pub(crate) const HEADER_ADDRESS_GLYPHS: usize = 15;

pub(crate) const HEADER_NAME_GLYPHS: usize = 17;

pub(crate) const HEADER_PHASE_X: usize =
    block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH;

pub(crate) const HEADER_PHASE_GLYPHS: usize = 3 + 4;

pub(crate) const BARS_GLYPHS: usize = 4;

const _: () = assert!(
    HEADER_PHASE_X + HEADER_PHASE_GLYPHS * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (BARS_GLYPHS * GLYPH_PITCH - OVERLAY_GAP),
    "the phase word runs into the signal bars on the header's network row"
);
const _: () = assert!(
    block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH - OVERLAY_GAP < HEADER_PHASE_X,
    "the name of the header's network runs into the phase word beside it"
);
pub(crate) const LIST_TITLE_GLYPHS: usize = b"NETWORK".len();

pub(crate) const LIST_WINDOW_GLYPHS: usize = 8;

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

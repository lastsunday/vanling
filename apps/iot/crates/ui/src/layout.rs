//! Panel geometry the page renderer draws against.
//!
//! A page can only be checked against the panel it was laid out for, and a stamp that falls off
//! an edge is clipped rather than reported. Every relationship that decides whether something
//! lands on the panel is therefore settled at compile time here, beside the numbers it holds.

use iot_core::controls::{AP_LABEL, CANCEL_LABEL, NO_PASS_LABEL, PANEL_LABEL};
use iot_core::drivers::audio::ENVELOPE_COLUMNS;
use iot_core::keyboard::{
    self, DIALOG_ANSWER_W, KEY_H, KEYBOARD_TOP, LIST_ROW_H, LIST_ROWS, LIST_TOP, ROWS,
};
use iot_core::overlay::{
    Block, CONTENT_BOTTOM, FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, PANEL_ROWS,
    RIGHT_EDGE_X, block_value_x, block_x, overlay_row_top,
};
use iot_core::provision::MAC_TEXT_BYTES;

use crate::text::NETWORK_NAME_GLYPHS;

/// The window's floor in decibels below full scale, which is the bottom of the
/// Audio page's scale. Read back from the mapping rather than written here, so a
/// band and its own ruler cannot end up describing different windows.
pub(crate) const SCOPE_DECIBEL_FLOOR: i16 = iot_core::drivers::audio::SCOPE_FLOOR_DECIBELS as i16;

/// Stand-in value for a semantic the board does not declare, matching the
/// `ERR`/`WAIT` sentinels the attitude page already uses for missing data.
pub(crate) const NOT_AVAILABLE: &[u8] = b"N/A";

/// Attitude dial center, in panel columns/rows. Anchored in the third text
/// block, which the attitude page leaves empty for it: the dial is 96 rows across,
/// so the block's own width is what decides the radius and the rows below the
/// counts column (the last ends at 184) are what is left over.
pub(crate) const HORIZON_CX: isize = 266;
pub(crate) const HORIZON_CY: isize = 120;
pub(crate) const HORIZON_R: isize = 48;

/// Top edge of the sweep band, in panel rows. The Audio page's two level rows
/// are the only readouts wide enough not to fit the narrow block, so the band is
/// given every row from below them down: a portrait panel spent its top third on
/// readouts stacked above a band it had already shortened.
pub(crate) const WAVE_TOP: isize = 42;
/// Centre line of the mirrored band, which is also the bottom of the scale: a
/// mirrored bar measures *outward* from silence here, so 0 dBFS is at the band's
/// two edges and the floor is the middle.
pub(crate) const WAVE_CY: isize = 132;
/// Rows the tallest column reaches up and down from [`WAVE_CY`]. Set to clear
/// [`WAVE_TOP`] and leave room for the span label, not chosen: a logarithmic
/// scale spends the rows it leaves free on under half a decibel each. At 90 it
/// also spends one row on one decibel, which is what makes the grid land whole.
pub(crate) const WAVE_HALF: isize = 90;

/// Decibels per grid line, chosen so a line lands on a whole row and the
/// ten-octave window divides into whole steps: [`WAVE_HALF`] rows per 90 dB is
/// one row per decibel, so twelve decibels is exactly twelve rows. A scale whose
/// lines land off-pixel reads as a scale that is wrong.
pub(crate) const GRID_DECIBELS: i16 = 12;

/// Left column of the sweep, and its width in panel columns. The width is
/// [`ENVELOPE_COLUMNS`] so one column is one pixel: a scale that resamples the
/// envelope has to drop peaks, and dropping peaks on a peak meter means the
/// meter misses the thing it exists to show. Those columns and the ruler beside
/// them are what fix the right-edge block's left edge, so neither can move
/// without it.
pub(crate) const SCAN_X: isize = 40;
pub(crate) const SCAN_COLUMNS: isize = 200;

/// Right edge the decibel ruler's labels are aligned to, and the tick that
/// follows them. Together they fill the 30 columns left of the sweep, which is
/// what a label plus its tick has to fit in.
pub(crate) const RULER_RIGHT: isize = 29;
pub(crate) const RULER_TICK_X: isize = 32;
pub(crate) const RULER_TICK_COLUMNS: isize = 6;

/// Rows below the band the window's own length is written at, so a sweep is read
/// as "two seconds" rather than as an unbounded strip whose rate nobody can
/// infer from a moving bar.
pub(crate) const SPAN_Y: usize = 224;

/// Top edge of the global FPS badge, which every page shows. Below the span label
/// and clear of the sweep, so the one strip that belongs to no page sits where no
/// page's content is, and against the same panel edge the Audio readouts use.
pub(crate) const FPS_Y: usize = 232;
pub(crate) const PANEL_RIGHT: isize = PANEL_COLUMNS as isize - 1;

/// Glyphs of the masked passphrase the row above the keyboard shows.
///
/// The whole row, from its reading column to the panel's right edge. It used to share the row with
/// the name it is being typed for and the phase word between them, and got what they left; the
/// keyboard now stops a row lower and the draft has the row to itself, so what it takes is the
/// width it can use. A draft can be 63 bytes and this is still shorter, which is why the row marks
/// the part of it that does not fit rather than pretending to show all of it.
pub(crate) const PSK_MASK_GLYPHS: usize =
    (PANEL_COLUMNS - block_value_x(Block::First) + OVERLAY_GAP) / GLYPH_PITCH;

/// Right edge of the word an open network's own list row carries, in panel columns.
///
/// Between the name and the bars rather than in either's place: which network a row is and how
/// strong it is are the two readings it exists for, and "no passphrase" is the one thing about a
/// row that the tap picking it acts on.
pub(crate) const OPEN_MARKER_RIGHT: isize = PANEL_RIGHT - 5 * GLYPH_PITCH as isize;

/// The word an open network's row carries, so the tap that connects it is not a surprise.
pub(crate) const OPEN_MARKER: &[u8] = b"OPEN";

/// Right edge of the mark a saved network's row carries, in panel columns.
///
/// Left of the open marker's own slot rather than in it, because the two are different facts about
/// a row and a network can be both: what the device would join again on its own, and what asks for
/// nothing to join now.
pub(crate) const SAVED_MARKER_RIGHT: isize =
    OPEN_MARKER_RIGHT - OPEN_MARKER.len() as isize * GLYPH_PITCH as isize - GLYPH_PITCH as isize;

/// The mark a saved network's row carries: the one glyph the font has for "this device keeps it".
///
/// A mark and not a word, unlike the open marker: the row has a name to show and a strength to
/// show, and this is a fact about the row rather than something a user acts on — the hold that acts
/// on it is what says what it means.
pub(crate) const SAVED_MARKER: &[u8] = b"*";

// The strip's words come from the same table the tap is resolved through, so a cell and the command
// it sends cannot drift apart. The assertions below are this panel's half: the words have to fit
// the cells and the box.
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
pub(crate) const KEY_LINE_GAP: usize = 4;

/// The Audio page's two level rows, the only readouts wide enough not to fit a
/// block, and the rows the band has to start below.
pub(crate) const AUDIO_LAST_FULL_WIDTH_ROW: usize = 2;

/// The attitude page's tall block, whose last row the dial is centred against.
pub(crate) const LAST_BLOCK_ROW: usize = 17;

/// Rows the attitude page's estimator readouts take at the head of its second
/// block, before the cumulative counts start.
pub(crate) const ESTIMATOR_ROWS: usize = 4;

/// Semantic counters the attitude page prints, and the last row of the attitude
/// page's first block.
pub(crate) const MOTION_SEMANTIC_ROWS: usize = 14;
pub(crate) const ATTITUDE_LAST_AXIS_ROW: usize = 15;

/// Width in columns of the block that marks the column which clipped, which is
/// drawn past the sweep's last column by its own width.
pub(crate) const CLIP_MARK_WIDTH: isize = 3;

const _: () = assert!(
    block_value_x(Block::First) + PSK_MASK_GLYPHS * GLYPH_PITCH - OVERLAY_GAP <= PANEL_COLUMNS,
    "the draft a passphrase is typed into runs off the row's own right edge"
);
/// The widest link address the header's address row can put on the left: a dotted quad at its
/// longest (`255.255.255.255`).
pub(crate) const HEADER_ADDRESS_GLYPHS: usize = 15;

/// Glyphs the header's network row gives the name it is about, narrower than a list row's own slot:
/// the phase word shares that row.
///
/// A name that does not fit is folded to this width rather than allowed to run under the word beside
/// it, so where the phase starts does not move with the name printed before it.
pub(crate) const HEADER_NAME_GLYPHS: usize = 17;

/// Where the phase word starts on the header's network row: the cell after the name slot's last one.
pub(crate) const HEADER_PHASE_X: usize =
    block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH;

/// The phase word with the label that says what it is, at its widest: `ST` and one space, then the
/// longest word [`format_phase`] writes.
pub(crate) const HEADER_PHASE_GLYPHS: usize = 3 + 4;

/// The widest the signal bars are drawn on a header or a list row.
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
/// The title the header's first row prints, which is what the list's own window starts after.
pub(crate) const LIST_TITLE_GLYPHS: usize = b"NETWORK".len();

/// The widest the list's window is drawn: two digits either side of the mark, and the count.
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

// The layout is only checkable against the panel it was chosen for, and a stamp
// that falls off an edge is clipped rather than reported — so every relationship
// that decides whether something lands on the panel is settled at compile time,
// against the same dimensions on the board and in a host test.
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

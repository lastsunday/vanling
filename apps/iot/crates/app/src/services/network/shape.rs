//! The Network page's boxes: the one geometry its drawing and its hit tests both read.
//!
//! The bands are the page's layers in the order they are drawn: the header's three rows, the
//! scan list or key grid, the action strip under them, and whatever box is over them.

use iot_core::drivers::network::{LinkPhase, LinkStatus};
use iot_core::geometry::ControlRect;
use iot_core::ui::panel::{CONTENT_BOTTOM, GLYPH, LINE, PANEL_COLUMNS, ROW_PITCH, TOUCH_MIN};

use crate::shared::intent::{Key, Layer};
use crate::shared::model::NetworkView;

pub use crate::shared::model::LIST_ROWS;

/// The row below the header's three, where the page's own content starts.
const HEADER_Y: usize = 8;
pub const LIST_TOP: usize = HEADER_Y + 3 * ROW_PITCH;

// ---- Bands ------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Header,
    Content,
    Controls,
    Overlay,
}

impl Band {
    pub const IN_ORDER: [Band; 4] = [Band::Header, Band::Content, Band::Controls, Band::Overlay];

    pub fn hidden(self, draft: NetworkView, link: LinkStatus) -> bool {
        let question = draft.confirm.is_some();
        let boxed = question || link.shows_setup_box() || draft.setup_box_pending();
        let joining = joining(draft, link);
        match self {
            Band::Controls => question || draft.keyboard,
            Band::Content => boxed || joining,
            Band::Overlay => !boxed && !joining,
            Band::Header => false,
        }
    }

    pub fn showing(self, draft: NetworkView, link: LinkStatus) -> bool {
        !self.hidden(draft, link)
    }
}

pub fn joining(draft: NetworkView, link: LinkStatus) -> bool {
    !draft.keyboard && (link.phase == LinkPhase::Connecting || draft.join_wait)
}

pub fn each_band<F: FnMut(Band)>(draft: NetworkView, link: LinkStatus, mut draw: F) {
    for band in Band::IN_ORDER {
        if band.showing(draft, link) {
            draw(band);
        }
    }
}

// ---- The action strip -------------------------------------------------------------------

/// The action strip under the list: the page's one control, one touch target tall.
pub const TOOLBAR_H: usize = TOUCH_MIN;
/// Top edge of the action strip, in panel rows.
pub const TOOLBAR_TOP: usize = CONTENT_BOTTOM - TOOLBAR_H;

/// The action strip's own box: the page's whole width, so the one control on it is as wide
/// as a thumb can want, and one touch target tall.
pub fn toolbar_box() -> ControlRect {
    ControlRect {
        x: 0,
        y: TOOLBAR_TOP,
        w: PANEL_COLUMNS,
        h: TOOLBAR_H,
    }
}

/// Which cell of the action strip a point is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolbarCell {
    #[default]
    Left,
    Right,
}

/// How many cells the strip has while it is split: the way back, and the host switch.
pub const TOOLBAR_CELLS: usize = 2;

/// The seam between two cells, taken out of the cells rather than out of the slots, so the
/// slots stay an exact division of the panel and only the pixels move.
pub const TOOLBAR_GAP: usize = 8;

const _: () = assert!(
    PANEL_COLUMNS.is_multiple_of(TOOLBAR_CELLS),
    "the strip's cells are an exact division of the panel"
);
const _: () = assert!(
    PANEL_COLUMNS / TOOLBAR_CELLS - TOOLBAR_GAP >= TOUCH_MIN,
    "a cell of the strip is smaller than a finger"
);

/// The box one cell of the split strip is drawn in.
pub const fn toolbar_cell(cell: ToolbarCell) -> ControlRect {
    let index = match cell {
        ToolbarCell::Left => 0,
        ToolbarCell::Right => 1,
    };
    let slot = PANEL_COLUMNS / TOOLBAR_CELLS;
    ControlRect {
        x: index * slot + TOOLBAR_GAP / 2,
        y: TOOLBAR_TOP,
        w: slot - TOOLBAR_GAP,
        h: TOOLBAR_H,
    }
}

/// Whether a tap at panel row `y` lands on the action strip.
pub fn toolbar_at(y: u16) -> bool {
    (TOOLBAR_TOP..CONTENT_BOTTOM).contains(&usize::from(y))
}

/// Which cell of the strip a point is on, if it is on the strip at all.
///
/// The seam counts as both halves' edge rather than as a dead gap: a finger aiming at two 152 px
/// cells does not have to miss an 8 px seam, and the two verbs the split strip carries are never
/// both destructive. A strip with one verb is still one box, and either cell means it.
pub fn toolbar_cell_at(x: u16, y: u16) -> Option<ToolbarCell> {
    if !toolbar_at(y) {
        return None;
    }
    Some(if usize::from(x) < PANEL_COLUMNS / TOOLBAR_CELLS {
        ToolbarCell::Left
    } else {
        ToolbarCell::Right
    })
}

// ---- The scan list ----------------------------------------------------------------------

/// The scan list's rows and their pitch.
pub const LIST_ROW_H: usize = (TOOLBAR_TOP - LIST_TOP) / LIST_ROWS;

const _: () = assert!(
    LIST_TOP + LIST_ROWS * LIST_ROW_H <= TOOLBAR_TOP,
    "the list's last row runs into the page's action strip"
);
const _: () = assert!(
    LIST_ROW_H >= TOUCH_MIN,
    "a control on this page is smaller than a finger"
);

/// Top edge of the first of the two lines a band carries: the network's name, and the
/// address its radio answers on below it.
pub const fn band_lines_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - ROW_PITCH - LINE) / 2
}

const _: () = assert!(
    (LIST_ROW_H - ROW_PITCH - LINE) / 2 + ROW_PITCH + LINE <= LIST_ROW_H,
    "a band's second line runs into the row under it"
);

/// The band of the scan list a point falls on, if it falls on the list at all.
pub fn list_band_at(y: u16) -> Option<usize> {
    let y = usize::from(y);
    (LIST_TOP..TOOLBAR_TOP)
        .contains(&y)
        .then(|| ((y - LIST_TOP) / LIST_ROW_H).min(LIST_ROWS - 1))
}

// ---- The key grid -----------------------------------------------------------------------

/// Keys across and keys down: the digits' own three columns and a column of controls beside
/// them, over a row that ends an entry.
pub const COLUMNS: usize = 4;
pub const ROWS: usize = 4;
/// Keys in all, which is every cell of the grid.
pub const KEYS: usize = COLUMNS * ROWS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Digit(usize),
    Control(Key),
}

const CELLS: [Cell; KEYS] = [
    Cell::Digit(0),
    Cell::Digit(1),
    Cell::Digit(2),
    Cell::Control(Key::Shift),
    Cell::Digit(3),
    Cell::Digit(4),
    Cell::Digit(5),
    Cell::Control(Key::Layer),
    Cell::Digit(6),
    Cell::Digit(7),
    Cell::Digit(8),
    Cell::Control(Key::Backspace),
    Cell::Control(Key::Cancel),
    Cell::Digit(9),
    Cell::Control(Key::Connect),
    Cell::Control(Key::Reveal),
];

/// One key cell, in panel pixels: four across the panel's whole width, so the widest key
/// the panel can hold is the one a thumb gets.
pub const KEY_W: usize = PANEL_COLUMNS / COLUMNS;
/// Four rows of these fill the page's own height down to the footer.
pub const KEY_H: usize = (CONTENT_BOTTOM - KEYBOARD_TOP) / ROWS;

/// Top edge of the keyboard, in panel rows: the row below the three the header keeps.
pub const KEYBOARD_TOP: usize = LIST_TOP;

const _: () = assert!(
    COLUMNS * KEY_W == PANEL_COLUMNS,
    "a key column is left over at the panel's right edge"
);
const _: () = assert!(
    KEYBOARD_TOP + ROWS * KEY_H <= CONTENT_BOTTOM,
    "the keyboard's last row runs into the strip no page's content owns"
);
const _: () = assert!(
    KEY_H >= TOUCH_MIN,
    "a control on this page is smaller than a finger"
);

/// The key in the grid's `cell`: a character key's own face or the control the layout table
/// puts there, and nothing past the grid.
pub fn key_of(layer: Layer, cell: usize) -> Option<Key> {
    Some(match *CELLS.get(cell)? {
        Cell::Digit(digit) => Key::Char(layer.face(digit)),
        Cell::Control(key) => key,
    })
}

/// The cell a named control sits in.
pub fn cell_of(key: Key) -> Option<usize> {
    (0..KEYS).find(|cell| matches!(CELLS[*cell], Cell::Control(own) if own == key))
}

/// The cell the `digit`-th character key sits in.
pub fn cell_for_digit(digit: usize) -> Option<usize> {
    (0..KEYS).find(|cell| CELLS[*cell] == Cell::Digit(digit))
}

/// The key a tap at `(x, y)` meant, in panel pixels.
pub fn key_at(layer: Layer, x: u16, y: u16) -> Option<Key> {
    key_of(layer, cell(usize::from(x), usize::from(y))?)
}

fn cell(x: usize, y: usize) -> Option<usize> {
    if x >= PANEL_COLUMNS || !(KEYBOARD_TOP..CONTENT_BOTTOM).contains(&y) {
        return None;
    }
    let row = (y - KEYBOARD_TOP) / KEY_H;
    (row < ROWS).then_some(row * COLUMNS + x / KEY_W)
}

/// The cell a key owns, in pixels: the one mapping both the drawing and the hit test read.
pub fn key_box(cell: usize) -> Option<ControlRect> {
    (cell < KEYS).then(|| ControlRect {
        x: (cell % COLUMNS) * KEY_W,
        y: KEYBOARD_TOP + (cell / COLUMNS) * KEY_H,
        w: KEY_W,
        h: KEY_H,
    })
}

// ---- The dialog -------------------------------------------------------------------------

/// Which of a confirmation's two answers a tap landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogChoice {
    Keep,
    Act,
}

/// The confirmation's box: a page-wide margin either side, centred on the rows the list
/// would have been showing.
pub const DIALOG_W: usize = 256;
pub const DIALOG_H: usize = 118;
pub const DIALOG_X: usize = (PANEL_COLUMNS - DIALOG_W) / 2;
pub const DIALOG_TOP: usize = LIST_TOP + (TOOLBAR_TOP - LIST_TOP - DIALOG_H) / 2;
/// Margin between the box's edge and what it holds, and between its two answers.
pub const DIALOG_INSET: usize = 24;
pub const DIALOG_GAP: usize = 8;

/// Glyphs one line of the box holds: what fits between its own margins.
pub const DIALOG_GLYPHS: usize = (DIALOG_W - 2 * DIALOG_INSET) / GLYPH;

/// The rows the box's two lines are drawn on: the question, and the consequence under it.
pub const DIALOG_TITLE_TOP: usize = DIALOG_TOP + DIALOG_INSET / 2;
pub const DIALOG_BODY_TOP: usize = DIALOG_TITLE_TOP + ROW_PITCH;

/// One answer's box: two of them, side by side along the box's own bottom edge.
pub const DIALOG_ANSWER_W: usize = (DIALOG_W - 2 * DIALOG_INSET - DIALOG_GAP) / 2;

/// The box one answer of a confirmation is drawn in and tested against.
pub const fn dialog_answer_box(choice: DialogChoice) -> ControlRect {
    let x = match choice {
        DialogChoice::Keep => DIALOG_X + DIALOG_INSET,
        DialogChoice::Act => DIALOG_X + DIALOG_INSET + DIALOG_ANSWER_W + DIALOG_GAP,
    };
    ControlRect {
        x,
        y: DIALOG_TOP + DIALOG_H - DIALOG_INSET - TOUCH_MIN,
        w: DIALOG_ANSWER_W,
        h: TOUCH_MIN,
    }
}

/// The answer a tap at `(x, y)` landed on, or `None` for a point that missed both.
pub fn dialog_choice_at(x: u16, y: u16) -> Option<DialogChoice> {
    [DialogChoice::Keep, DialogChoice::Act]
        .into_iter()
        .find(|choice| holds(dialog_answer_box(*choice), x, y))
}

fn holds(rect: ControlRect, x: u16, y: u16) -> bool {
    let (x, y) = (usize::from(x), usize::from(y));
    (rect.x..rect.x + rect.w).contains(&x) && (rect.y..rect.y + rect.h).contains(&y)
}

const _: () = assert!(
    DIALOG_TOP >= LIST_TOP && DIALOG_TOP + DIALOG_H <= TOOLBAR_TOP,
    "a confirmation is drawn over rows the list does not own"
);
const _: () = assert!(
    DIALOG_TITLE_TOP >= DIALOG_TOP
        && DIALOG_BODY_TOP >= DIALOG_TITLE_TOP + LINE
        && DIALOG_BODY_TOP + ROW_PITCH + LINE <= dialog_answer_box(DialogChoice::Keep).y
        && DIALOG_TITLE_TOP + LINE <= dialog_answer_box(DialogChoice::Act).y,
    "the box's own words run into its answers"
);
const _: () = assert!(
    dialog_answer_box(DialogChoice::Keep).x >= DIALOG_X
        && dialog_answer_box(DialogChoice::Act).x + DIALOG_ANSWER_W <= DIALOG_X + DIALOG_W,
    "an answer is drawn outside the box that holds it"
);
const _: () = assert!(
    dialog_answer_box(DialogChoice::Keep).w >= TOUCH_MIN
        && dialog_answer_box(DialogChoice::Keep).h >= TOUCH_MIN,
    "an answer to a confirmation is smaller than a finger"
);
const _: () = assert!(
    DIALOG_GLYPHS >= 16,
    "a line of a confirmation is too narrow to ask anything in"
);

#[cfg(test)]
mod tests;

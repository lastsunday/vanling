//! The Network page's own controls: which key a tap lands on and what it types, and which row
//! of the scan list it picks.
//!
//! Geometry rather than drawing, and here rather than in the panel's own module because two
//! things have to agree about where a control is: the panel, which draws it, and [`crate::intent`],
//! which decides which one a tap meant. Only one of those can be tested on the development
//! machine, so the numbers are here and the panel reads them — a box drawn from anything else
//! would be a target the panel does not actually have.
//!
//! The keyboard is the phone keypad's own arrangement: the digits `1`–`9` in their three-by-three
//! block, and the controls around them. That is the one keypad a thumb finds without looking, and
//! the characters a key cycles are printed on the key rather than left in a table the finger is
//! expected to know.
//!
//! What a key *types* is here too, and not in the panel: the second tap of a multi-tap key has to
//! find the candidate list the first one did, and a key drawn `ABC` that typed `b` would be a
//! keyboard whose labels lie.

use alloc::vec::Vec;

use crate::overlay::{
    CONTENT_BOTTOM, FONT_H, GLYPH_PITCH, NETWORK_LIST_ROW, OVERLAY_GAP, PANEL_COLUMNS, ROW_PITCH,
    overlay_row_top,
};

/// Keys across and keys down: the digits' own three columns and a column of controls beside them,
/// over a row that ends an entry.
pub const COLUMNS: usize = 4;
pub const ROWS: usize = 4;
/// The ten character keys: the nine the block holds and the zero beside the controls.
pub const CHARS: usize = 10;
/// Keys in all, which is every cell of the grid.
pub const KEYS: usize = COLUMNS * ROWS;

/// What one cell of the grid holds.
///
/// The layout is a table rather than arithmetic because the digits are not in cell order: the
/// three-by-three block every thumb knows leaves the cells at its right and below it to the
/// controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    /// A character key, by its index in the layer tables: `0`–`8` are the keys printed `1`–`9`,
    /// and `9` is the zero.
    Digit(usize),
    /// A control, which is the same key on every layer.
    Control(Key),
}

/// The grid, in reading order.
///
/// `1 2 3` over `4 5 6` over `7 8 9` is the arrangement a phone taught every thumb, and it is
/// worth the two cells the block leaves empty at the panel's right edge: a digit key found without
/// looking is worth more than a key column filled edge to edge. The controls keep the places that
/// keypad puts them — the layer key and shift at the top of the spare column, erase under them, and
/// the three that end an entry along the bottom, with the zero between cancel and connect where
/// the phone had it.
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

/// One key cell, in panel pixels: four across the panel's whole width, so the widest key the
/// panel can hold is the one a thumb gets.
pub const KEY_W: usize = PANEL_COLUMNS / COLUMNS;
/// Four rows of these fill the page's own height down to the footer. The top is a row of the
/// header's rhythm rather than the footer's, so what the keys do not claim is whole header rows —
/// which is what lets the keyboard give the rows above it the name and the draft while it is up.
pub const KEY_H: usize = (CONTENT_BOTTOM - KEYBOARD_TOP) / ROWS;

/// Top edge of the keyboard, in panel rows: the row below the three the header keeps.
///
/// The header's rows are the page's own — the link and its phase, the network in question and how
/// good it is, and the draft — and the keyboard is sized against them rather than against an
/// assumption about how much a finger needs: the keys take the rest of the page's height, which on
/// this panel is still wider and taller than a thumb's own contact.
pub const KEYBOARD_TOP: usize = overlay_row_top(NETWORK_LIST_ROW);

/// Top edge of the scan list, in panel rows: the row below the header's three.
pub const LIST_TOP: usize = overlay_row_top(NETWORK_LIST_ROW);

const _: () = assert!(
    COLUMNS * KEY_W == PANEL_COLUMNS,
    "a key column is left over at the panel's right edge"
);
const _: () = assert!(
    KEYBOARD_TOP + ROWS * KEY_H <= CONTENT_BOTTOM,
    "the keyboard's last row runs into the strip no page's content owns"
);

/// The height of one touch target, in panel rows: what a finger can be asked to aim at.
///
/// The number both platform guides land on, and the reason this page's controls are laid out
/// against each other rather than against what is left over: a control a thumb keeps missing is
/// one that is not there as far as the person standing at the panel is concerned.
pub const TOUCH_MIN: usize = 44;

/// The action strip under the list: the page's one control, one touch target tall.
///
/// It sits at the page's own bottom, and the list above it is what gives up the room — three rows
/// of a height the strip leaves rather than a height of its own. One control rather than two,
/// because there is one thing this page can do to the radio at a time: a link that is up is one to
/// leave, and a link that is not is one to look around for, and the two never both apply.
pub const TOOLBAR_H: usize = TOUCH_MIN;
/// Top edge of the action strip, in panel rows.
pub const TOOLBAR_TOP: usize = CONTENT_BOTTOM - TOOLBAR_H;

/// The scan list's rows and their pitch.
///
/// Three rows of what the page's own height has below its header and above the action strip, which
/// is what a scan's eight access points need three screens for. The pitch falls out of that rather
/// than being chosen, so the last row lands on the strip rather than in it — and the row a thumb
/// aims at is still taller than the strip it leaves room for.
pub const LIST_ROWS: usize = 3;
pub const LIST_ROW_H: usize = (TOOLBAR_TOP - LIST_TOP) / LIST_ROWS;

const _: () = assert!(
    LIST_TOP + LIST_ROWS * LIST_ROW_H <= TOOLBAR_TOP,
    "the list's last row runs into the page's action strip"
);

/// Top edge of the first of the two lines a band carries: the network's name, and the address its
/// radio answers on below it.
///
/// Two networks can share a name — a mesh node and its base, a repeater — and the address is the
/// only thing on the page that tells those rows apart. It gets a line of its own because the name
/// already spends the band's width; the pair is centred in the band, and a band is still one tap.
pub const fn band_lines_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - ROW_PITCH - FONT_H) / 2
}

const _: () = assert!(
    (LIST_ROW_H - ROW_PITCH - FONT_H) / 2 + ROW_PITCH + FONT_H <= LIST_ROW_H,
    "a band's second line runs into the row under it"
);
const _: () = assert!(
    LIST_ROW_H >= TOUCH_MIN && KEY_H >= TOUCH_MIN && TOOLBAR_H >= TOUCH_MIN,
    "a control on this page is smaller than a finger"
);
const _: () = assert!(
    KEYBOARD_TOP == LIST_TOP,
    "the keyboard and the list are the page's own rows, so a page that shows one shows it where the other was"
);

/// Whether a tap at panel row `y` lands on the action strip.
///
/// The strip is one band of the page whatever it is split into: a point in it is a point on the
/// strip, and which of its cells is [`toolbar_cell_at`]'s question.
pub fn toolbar_at(y: u16) -> bool {
    (TOOLBAR_TOP..CONTENT_BOTTOM).contains(&usize::from(y))
}
/// How long a key stays open for its next candidate, in milliseconds.
///
/// The window a phone keypad's multi-tap uses, and deliberately generous: it is the *same* key
/// that has to be tapped again to cycle, so a finger reading the candidates off the keys still
/// cycles, and a different key anywhere inside the window starts a new character instead.
pub const MULTITAP_WINDOW_MS: u32 = 800;

/// What the ten character keys type.
///
/// Three layers, the phone's own: `abc` cycles the letters printed on a key, and `123` and `#&?`
/// type one character a tap — a passphrase of a word, a couple of digits and a symbol is what
/// that covers, in the fewest keys a thumb can hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    #[default]
    Abc,
    Digits,
    Symbols,
}

/// The phone layer's ten keys, each a run of characters one tap apiece: the first tap types the
/// first, the next tap inside [`MULTITAP_WINDOW_MS`] the one after it, and so on round the run.
/// `1` is punctuation rather than a letter, which is the keypad's own arrangement, and `0` is the
/// space — a passphrase needs one and its own key would be a key spent on one character.
const ABC: [&[u8]; CHARS] = [
    b".?!,", // 1
    b"abc",  // 2
    b"def",  // 3
    b"ghi",  // 4
    b"jkl",  // 5
    b"mno",  // 6
    b"pqrs", // 7
    b"tuv",  // 8
    b"wxyz", // 9
    b" ",    // 0
];
/// The digit layer: one character a key, so a second tap of one is a second digit — which is
/// what a passphrase with `00` in it needs.
const DIGITS: [&[u8]; CHARS] = [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"0"];
/// The symbol layer: the punctuation a passphrase tends to want, one character a key.
const SYMBOLS: [&[u8]; CHARS] = [b".", b",", b":", b";", b"?", b"!", b"-", b"_", b"@", b"#"];

/// Most characters any one key types, which is the `pqrs` key's four.
///
/// The width a key's own label has to hold, and the length of the buffer a panel prints one in —
/// so a table that grew a fifth character would be a key whose last candidate is never drawn.
pub const MAX_CANDIDATES: usize = 4;

const _: () = {
    let mut cell = 0;
    while cell < CHARS {
        assert!(
            ABC[cell].len() <= MAX_CANDIDATES,
            "a phone-layer key types more characters than a key's label holds"
        );
        cell += 1;
    }
};

impl Layer {
    /// The `digit`-th key's face, in the order the grid lays them out: the digits that label the
    /// phone layer's keys, and the character each of the other layers' keys types.
    pub const fn face(self, digit: usize) -> u8 {
        match self {
            Layer::Abc => DIGITS[digit][0],
            Layer::Digits | Layer::Symbols => self.candidates(digit)[0],
        }
    }

    /// What the `digit`-th key types, in the order taps cycle it.
    pub const fn candidates(self, digit: usize) -> &'static [u8] {
        match self {
            Layer::Abc => ABC[digit],
            Layer::Digits => DIGITS[digit],
            Layer::Symbols => SYMBOLS[digit],
        }
    }

    /// What the key printed `face` types.
    ///
    /// Keyed on the face rather than on the cell because a tap is reported as the key it landed
    /// on and the panel prints faces: this one lookup is what keeps a key's label and the target
    /// that types it the same key. The faces of one layer are distinct, so it finds one.
    pub const fn candidates_of(self, face: u8) -> &'static [u8] {
        let mut digit = 0;
        while digit < CHARS {
            if self.face(digit) == face {
                return self.candidates(digit);
            }
            digit += 1;
        }
        b""
    }

    /// The layer the layer key steps to.
    pub const fn next(self) -> Self {
        match self {
            Layer::Abc => Layer::Digits,
            Layer::Digits => Layer::Symbols,
            Layer::Symbols => Layer::Abc,
        }
    }
}

/// One key of the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The character key printed `face`: one tap types the layer's first candidate for it, and
    /// each tap after that inside the window the next one.
    Char(u8),
    /// Upper case: one letter from a tap, a lock from a second tap inside the window.
    Shift,
    /// Erase the last character typed.
    Backspace,
    /// Step to the next layer.
    Layer,
    /// Hand what has been typed to the radio.
    Connect,
    /// Leave the entry without connecting.
    Cancel,
    /// Show the draft as it was typed rather than as stars, and hide it again on the next tap.
    ///
    /// A passphrase is entered blind on a keypad whose keys hold four characters each, so the one
    /// way to check what has landed is to look at it — in the room, which is why it is a tap rather
    /// than a setting: it lasts as long as the finger's own decision to look.
    Reveal,
}

/// The character key a repeat tap would cycle, and which of the characters printed on it the
/// panel is currently holding.
///
/// The keyboard's own feedback, built where the calculation is: it is the state of a key, not of
/// the draft, and the panel draws it on the key rather than anywhere a passphrase could be read
/// off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycling {
    /// The key, by its face.
    pub face: u8,
    /// What that key types, in the order taps cycle it.
    pub candidates: &'static [u8],
    /// Which of them is on the panel.
    pub index: u8,
}

/// A rectangle on the panel, in pixels: the box a control is drawn in and the box a tap is
/// tested against.
///
/// One type for every control on this page — a key, the action strip, an answer in a
/// confirmation — because the page's own rule is that the box drawn and the box hit are the same
/// box, and two types would be two places for that to come apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// The key in the grid's `cell`: a character key's own face or the control the layout table puts
/// there, and nothing past the grid.
pub fn key_of(layer: Layer, cell: usize) -> Option<Key> {
    Some(match *CELLS.get(cell)? {
        Cell::Digit(digit) => Key::Char(layer.face(digit)),
        Cell::Control(key) => key,
    })
}

/// The cell a named control sits in.
///
/// For the callers that name a control rather than walk the grid — a test spelling out which cell
/// it taps, or a panel drawing the shift key's own state. A character key is not looked up this
/// way: its face is what identifies it, and [`Layer::candidates_of`] is the lookup for that.
pub fn cell_of(key: Key) -> Option<usize> {
    (0..KEYS).find(|cell| matches!(CELLS[*cell], Cell::Control(own) if own == key))
}

/// The key a tap at `(x, y)` meant, in panel pixels.
///
/// `None` above the keyboard, past its last row, or off the panel: a tap that missed is not a
/// key, and the intent that comes out of it is the one that moves nothing.
pub fn key_at(layer: Layer, x: u16, y: u16) -> Option<Key> {
    key_of(layer, cell(usize::from(x), usize::from(y))?)
}

/// The cell a point falls in, or `None` where there is no cell.
fn cell(x: usize, y: usize) -> Option<usize> {
    if x >= PANEL_COLUMNS || !(KEYBOARD_TOP..CONTENT_BOTTOM).contains(&y) {
        return None;
    }
    let row = (y - KEYBOARD_TOP) / KEY_H;
    (row < ROWS).then_some(row * COLUMNS + x / KEY_W)
}

/// The cell a key owns, in pixels: the one mapping both the drawing and the hit test read.
///
/// Every key is exactly one cell — the grid is the layout table read as rows either way — so this
/// is `cell`'s own arithmetic read backwards and nothing else.
pub fn key_box(cell: usize) -> Option<ControlRect> {
    (cell < KEYS).then(|| ControlRect {
        x: (cell % COLUMNS) * KEY_W,
        y: KEYBOARD_TOP + (cell / COLUMNS) * KEY_H,
        w: KEY_W,
        h: KEY_H,
    })
}

/// The action strip's own box: the page's whole width, so the one control on it is as wide as a
/// thumb can want, and one touch target tall.
pub fn toolbar_box() -> ControlRect {
    ControlRect {
        x: 0,
        y: TOOLBAR_TOP,
        w: PANEL_COLUMNS,
        h: TOOLBAR_H,
    }
}

/// Which cell of the action strip a point is on.
///
/// The strip is one box while the page has one verb, and two while it has two: setup mode offers
/// the way back and the front switch, and a strip that offered one of them would be a mode with a
/// single door. Which verbs are in which cell is the state's business; this is only the geometry.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolbarCell {
    /// The left cell: the way out of the mode, drawn only while the device keeps a network.
    #[default]
    Left,
    /// The right cell: the front switch. The single cell a one-verb strip has.
    Right,
}

/// How many cells the strip has while it is split: the way back, and the front switch.
pub const TOOLBAR_CELLS: usize = 2;

/// The seam between two cells, taken out of the cells rather than out of the slots, so the slots
/// stay an exact division of the panel and only the ink moves.
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
///
/// Inset by half the seam at each edge, so two cells are the same width and the gap between them
/// belongs to neither.
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

/// The band of the scan list a point falls on, if it falls on the list at all.
///
/// The whole band counts, not just the glyphs: a touch target that left the gaps between rows
/// dead would be a list a thumb keeps missing. Whether the band has a network in it is the
/// state's question, not this one's — and so is the list's own offset, because which of the
/// scan's access points a band holds is a page of the list rather than a row of the panel.
///
/// Above the action strip is the list and below it is the strip: the two are adjacent bands of the
/// page's own height, and a point in one is never a point in the other.
pub fn list_band_at(y: u16) -> Option<usize> {
    let y = usize::from(y);
    (LIST_TOP..TOOLBAR_TOP)
        .contains(&y)
        .then(|| ((y - LIST_TOP) / LIST_ROW_H).min(LIST_ROWS - 1))
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

/// Which of a confirmation's two answers a tap landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogChoice {
    /// Leave the action undone, which is also what a tap anywhere else on the page means.
    Keep,
    /// Do what the question asks.
    Act,
}

/// The confirmation's box: a page-wide margin either side, centred on the rows the list would
/// have been showing.
///
/// Over the list rather than over the strip, because the list is what the question is about — the
/// row that would be left, or the row that would be forgotten — and a box that covered the strip
/// too would be a question asked over the answer's own button.
pub const DIALOG_W: usize = 256;
pub const DIALOG_H: usize = 118;
pub const DIALOG_X: usize = (PANEL_COLUMNS - DIALOG_W) / 2;
pub const DIALOG_TOP: usize = LIST_TOP + (TOOLBAR_TOP - LIST_TOP - DIALOG_H) / 2;
/// Margin between the box's edge and what it holds, and between its two answers.
pub const DIALOG_INSET: usize = 24;
pub const DIALOG_GAP: usize = 8;

/// Glyphs one line of the box holds: what fits between its own margins.
///
/// The limit the panel's own words are written to, and the reason a question names its network in
/// the header's full width and here only as far as it fits.
pub const DIALOG_GLYPHS: usize = (DIALOG_W - 2 * DIALOG_INSET + OVERLAY_GAP) / GLYPH_PITCH;

/// The rows the box's two lines are stamped on: the question, and the consequence under it.
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
///
/// The gap between the two answers is a miss like any other, which is what the caller wants:
/// everything on this page that is not the action's own button is the safe answer.
pub fn dialog_choice_at(x: u16, y: u16) -> Option<DialogChoice> {
    [DialogChoice::Keep, DialogChoice::Act]
        .into_iter()
        .find(|choice| holds(dialog_answer_box(*choice), x, y))
}

/// Whether a box holds a point. Half-open, so two boxes that touch do not both hold their seam.
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
        && DIALOG_BODY_TOP >= DIALOG_TITLE_TOP + FONT_H
        && DIALOG_BODY_TOP + ROW_PITCH + FONT_H <= dialog_answer_box(DialogChoice::Keep).y
        && DIALOG_TITLE_TOP + FONT_H <= dialog_answer_box(DialogChoice::Act).y,
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

/// One tap of a script: the cell a finger lands on, and when its lift is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tap {
    pub cell: usize,
    pub at_ms: u32,
}

/// How long one tap takes on a script's own clock, in milliseconds.
const SCRIPT_TAP_MS: u32 = 100;
/// The gap a script leaves between two characters.
///
/// Longer than [`MULTITAP_WINDOW_MS`], because a script has no fingers: it cannot leave a key
/// alone, so the only thing that ends a run of taps is the clock. The same gap covers the shift's
/// own window, which a second shift tap inside would lock rather than spend.
const SCRIPT_GAP_MS: u32 = MULTITAP_WINDOW_MS + SCRIPT_TAP_MS;

/// The taps that type `text` on this keyboard, or `None` for a byte no key carries.
///
/// What this exists for is a bench that has no finger: a build can play a script through the page's
/// own geometry and state machine — the same taps a person would make, at the cells the panel draws
/// them in — which is the only way "the keyboard types this passphrase" is checkable without
/// standing at the panel. The clock is the script's own rather than a wall clock, so a script costs
/// nothing but the taps it makes.
///
/// What a key *is* comes from [`key_of`], so a script cannot type a character the keyboard does not
/// have: an upper-case letter is the shift and then the letter, a digit is the layer key and then
/// the digit, and a byte with no key is `None` rather than a silent omission.
pub fn script_for(text: &[u8]) -> Option<Vec<Tap>> {
    let mut taps = Vec::new();
    let mut layer = Layer::Abc;
    let mut clock = SCRIPT_GAP_MS;
    for byte in text {
        // An upper-case letter is its lower-case one with the shift set — the tables hold the
        // letters a key carries, and the shift is what makes one of them a capital — so what is
        // looked up is `wanted`, and the shift is a tap of its own before it.
        let wanted = byte.to_ascii_lowercase();
        let mut found = find(layer, wanted);
        let mut switches = 0;
        while found.is_none() {
            switches += 1;
            if switches > 3 {
                return None;
            }
            taps.push(Tap {
                cell: cell_of(Key::Layer)?,
                at_ms: clock,
            });
            clock += SCRIPT_TAP_MS;
            layer = layer.next();
            found = find(layer, wanted);
        }
        let (digit, index) = found?;
        // The shift is one shot and the letter after it does not end it for the shift's own key, so
        // a script writes the tap for it on every upper-case letter and lets the clock between them
        // keep the second tap from becoming a lock.
        if *byte != wanted {
            taps.push(Tap {
                cell: cell_of(Key::Shift)?,
                at_ms: clock,
            });
            clock += SCRIPT_TAP_MS;
        }
        let cell = digit_cell(digit)?;
        for _ in 0..=index {
            taps.push(Tap { cell, at_ms: clock });
            clock += SCRIPT_TAP_MS;
        }
        clock += SCRIPT_GAP_MS;
    }
    Some(taps)
}

/// Where a byte sits in a layer: which of the ten keys carries it, and how many taps in.
fn find(layer: Layer, byte: u8) -> Option<(usize, usize)> {
    (0..CHARS).find_map(|digit| {
        layer
            .candidates(digit)
            .iter()
            .position(|candidate| *candidate == byte)
            .map(|index| (digit, index))
    })
}

/// The cell a character key sits in, by its index in the layer tables.
fn digit_cell(digit: usize) -> Option<usize> {
    (0..KEYS).find(|cell| CELLS[*cell] == Cell::Digit(digit))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYERS: [Layer; 3] = [Layer::Abc, Layer::Digits, Layer::Symbols];

    #[test]
    fn every_cell_lands_on_the_key_whose_box_holds_it() {
        for layer in LAYERS {
            for cell in 0..KEYS {
                let rect = key_box(cell).expect("a cell of the grid");
                // Every corner of the box, because a box off by one column is a key whose label
                // sits outside the cell that types it.
                for (x, y) in [
                    (rect.x, rect.y),
                    (rect.x + rect.w - 1, rect.y),
                    (rect.x, rect.y + rect.h - 1),
                    (rect.x + rect.w - 1, rect.y + rect.h - 1),
                    (rect.x + rect.w / 2, rect.y + rect.h / 2),
                ] {
                    assert_eq!(
                        key_at(layer, x as u16, y as u16),
                        key_of(layer, cell),
                        "{layer:?} cell {cell} at ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_keys_a_tap_cannot_reach_are_the_ones_outside_the_keyboard() {
        // Above the keyboard is the page's header.
        assert_eq!(key_at(Layer::Abc, 5, KEYBOARD_TOP as u16 - 1), None);
        // Below it is the strip no page's content owns.
        assert_eq!(key_at(Layer::Abc, 5, CONTENT_BOTTOM as u16), None);
        // Through the right edge of the panel.
        assert_eq!(
            key_at(Layer::Abc, PANEL_COLUMNS as u16, KEYBOARD_TOP as u16),
            None
        );
        // And there is no cell past the grid the keys fill.
        assert_eq!(key_box(KEYS), None);
        assert_eq!(key_of(Layer::Abc, KEYS), None);
    }

    #[test]
    fn the_digits_sit_in_the_block_a_phone_taught_the_thumb() {
        // Nine digits in three rows of three: the one arrangement a thumb finds without looking,
        // and the reason the layout is a table rather than the order of the cells. The zero is
        // beside the controls, which is where a phone keypad's fourth row put it.
        for digit in 0..9 {
            let cell = (digit / 3) * COLUMNS + digit % 3;
            assert_eq!(
                CELLS[cell],
                Cell::Digit(digit),
                "the key printed {} is not where the block puts it",
                digit + 1
            );
            assert_eq!(
                key_of(Layer::Abc, cell),
                Some(Key::Char(b'1' + digit as u8))
            );
        }
        let connect = cell_of(Key::Connect).expect("a control's own cell");
        assert_eq!(
            CELLS[connect - 1],
            Cell::Digit(9),
            "the zero beside connect"
        );
        assert_eq!(key_of(Layer::Abc, connect - 1), Some(Key::Char(b'0')));
    }

    #[test]
    fn the_controls_are_the_same_cells_on_every_layer() {
        // The layer key is the one control that means something different per layer, and even it
        // sits in the same cell: a control that moved when the layer changed would be a keyboard
        // whose thumb has to hunt for the key that leaves the entry.
        let controls = [
            Key::Layer,
            Key::Shift,
            Key::Backspace,
            Key::Connect,
            Key::Cancel,
            Key::Reveal,
        ];
        let mut cells = Vec::new();
        for key in controls {
            let cell = cell_of(key).expect("a control's own cell");
            for layer in LAYERS {
                assert_eq!(key_of(layer, cell), Some(key), "{key:?} on {layer:?}");
            }
            assert!(!cells.contains(&cell), "{key:?} shares a cell");
            cells.push(cell);
            assert_eq!(
                key_box(cell).expect("a cell of the grid").w,
                PANEL_COLUMNS / COLUMNS
            );
        }
        assert_eq!(
            cells.len(),
            KEYS - CHARS,
            "a cell the controls do not fill would be a key the grid shows but cannot name"
        );
    }

    #[test]
    fn one_layer_covers_every_letter_and_the_one_below_it_every_digit() {
        // What the three layers exist to cover: letters a word, digits a number, and the
        // punctuation a passphrase picks up from both.
        let typed = |layer: Layer| {
            let mut bytes = Vec::new();
            for digit in 0..CHARS {
                bytes.extend_from_slice(layer.candidates(digit));
            }
            bytes
        };
        let abc = typed(Layer::Abc);
        for c in b'a'..=b'z' {
            assert!(abc.contains(&c), "{c:?} is not on the alphabet layer");
        }
        assert_eq!(abc[abc.len() - 1], b' ', "the zero key types the space");
        for c in b'0'..=b'9' {
            assert!(typed(Layer::Digits).contains(&c), "{c:?} is not typed");
        }
        for c in b".:,;?!-_@#" {
            assert!(typed(Layer::Symbols).contains(c), "{c:?} is not typed");
        }
    }

    #[test]
    fn every_candidate_is_one_byte_and_a_layers_faces_are_distinct() {
        // One byte, because a cycling tap replaces the character under it rather than working
        // out how many bytes of it to take back. Distinct faces, because the face is how a key
        // is named — to the panel, to the intent, and to the draft's own multi-tap.
        for layer in LAYERS {
            let mut faces = Vec::new();
            for digit in 0..CHARS {
                let candidates = layer.candidates(digit);
                assert!(
                    !candidates.is_empty(),
                    "{layer:?} key {digit} types nothing"
                );
                assert!(
                    candidates.len() <= MAX_CANDIDATES,
                    "{layer:?} key {digit} types more characters than a key's label holds"
                );
                for byte in candidates {
                    assert!(
                        byte.is_ascii_graphic() || *byte == b' ',
                        "{byte:?} is not a character the panel can draw"
                    );
                }
                let face = layer.face(digit);
                assert_eq!(
                    layer.candidates_of(face),
                    candidates,
                    "{layer:?} key {digit} does not read back from its own face"
                );
                assert!(!faces.contains(&face), "{layer:?} prints {face:?} twice");
                faces.push(face);
            }
        }
    }

    #[test]
    fn a_script_only_taps_cells_the_grid_has() {
        // A script is played at the panel's own coordinates, so every cell it names has to be a
        // cell the grid has: a bench that tapped outside the keyboard would type nothing and report
        // a keyboard that does not work.
        let script = script_for(b"abZ9.# 0_").expect("the ASCII the keypad covers");
        assert!(!script.is_empty());
        for tap in &script {
            assert!(
                key_box(tap.cell).is_some(),
                "cell {} is off the grid",
                tap.cell
            );
        }
        assert!(
            script.windows(2).all(|pair| pair[1].at_ms > pair[0].at_ms),
            "the script's clock only goes forward"
        );
        assert_eq!(script_for(b"$"), None, "no key carries a dollar sign");
        assert_eq!(script_for(b"\x07"), None, "and none carries a bell");
    }

    #[test]
    fn the_layer_key_says_where_it_goes_and_comes_back() {
        // Every layer is reachable from every other, because a passphrase may want the digits
        // first — and a cycle that stopped one short would strand whoever started there.
        let mut layer = Layer::Abc;
        for _ in 0..LAYERS.len() {
            layer = layer.next();
        }
        assert_eq!(layer, Layer::Abc, "the cycle closes");
        for start in LAYERS {
            assert_ne!(start.next(), start);
        }
    }

    #[test]
    fn a_band_is_a_row_from_the_top_of_the_list_to_the_next_one() {
        // Every row of the list lands on some band — a list a thumb keeps missing is one with
        // dead strips in it — and the band is the row shown there rather than a scan index, so
        // the page's own offset is the state's to add.
        for band in 0..LIST_ROWS {
            for offset in 0..LIST_ROW_H {
                assert_eq!(
                    list_band_at((LIST_TOP + band * LIST_ROW_H + offset) as u16),
                    Some(band),
                    "band {band} at offset {offset}"
                );
            }
        }
        assert_eq!(
            list_band_at((TOOLBAR_TOP - 1) as u16),
            Some(LIST_ROWS - 1),
            "the list's own last row is the last band"
        );
        assert_eq!(list_band_at(LIST_TOP as u16 - 1), None);
        assert_eq!(
            list_band_at(TOOLBAR_TOP as u16),
            None,
            "the action strip is not a band of the list"
        );
        assert_eq!(list_band_at(CONTENT_BOTTOM as u16), None);
    }

    #[test]
    fn a_band_holds_a_name_and_the_address_below_it_without_reaching_the_next_band() {
        // The address line is the panel's answer to two networks with one name, so it has to be
        // inside the band a finger taps and clear of the band under it: a line drawn in the next
        // row would be read as that row's, and a tap there would pick the wrong network.
        for band in 0..LIST_ROWS {
            let top = band_lines_top(band);
            let band_top = LIST_TOP + band * LIST_ROW_H;
            assert!(top >= band_top, "band {band}'s name is above its own row");
            assert!(
                top + ROW_PITCH + FONT_H <= band_top + LIST_ROW_H,
                "band {band}'s address runs into the row under it"
            );
            // And the pair is centred rather than crowded against either edge: what a band gives
            // up to the second line is the gap between the lines, not the target's height. The
            // leftover rows are odd, so the two margins differ by the one row no centring splits.
            let leftover = LIST_ROW_H - (ROW_PITCH + FONT_H);
            assert_eq!(top - band_top, leftover / 2, "band {band}");
            assert_eq!(
                band_top + LIST_ROW_H - (top + ROW_PITCH + FONT_H),
                leftover - leftover / 2,
                "band {band}"
            );
        }
    }

    #[test]
    fn the_action_strip_is_one_target_under_the_list_and_the_page_ends_where_it_does() {
        // The whole strip answers, at every column: the page's one action is a band rather than a
        // button in a band, and a tap at either edge of it has to mean the same thing.
        let strip = toolbar_box();
        assert_eq!(strip.y, TOOLBAR_TOP);
        assert_eq!(strip.y + strip.h, CONTENT_BOTTOM);
        assert_eq!(strip.w, PANEL_COLUMNS);
        for y in TOOLBAR_TOP..CONTENT_BOTTOM {
            assert!(toolbar_at(y as u16), "row {y} is the strip's");
        }
        assert!(!toolbar_at(TOOLBAR_TOP as u16 - 1));
        assert!(!toolbar_at(CONTENT_BOTTOM as u16));
        // Its height and the list's are a finger's own, which the module's own assert holds at
        // compile time — a control that shrank would not build.
    }

    #[test]
    fn a_split_strip_is_two_cells_of_one_width_that_meet_at_the_middle() {
        // Two verbs, two cells: the way back on the left and the front switch on the right, the
        // same width and the seam between them taken out of both rather than left as a dead gap.
        let left = toolbar_cell(ToolbarCell::Left);
        let right = toolbar_cell(ToolbarCell::Right);
        assert_eq!(left.w, right.w, "two cells of one strip are one width");
        assert_eq!(left.y, right.y);
        assert_eq!(left.y, TOOLBAR_TOP);
        assert_eq!(left.h, TOOLBAR_H);
        assert_eq!(left.x, TOOLBAR_GAP / 2, "the seam is outside the left cell");
        assert!(right.x + right.w + TOOLBAR_GAP / 2 == PANEL_COLUMNS);
        assert!(
            left.x + left.w <= right.x,
            "the cells do not overlap, whatever the seam does"
        );
        assert!(left.w >= TOUCH_MIN, "each cell is still a finger's own");

        // Every point of the strip is one cell or the other, split at the panel's middle: an 8 px
        // seam is not a gap a thumb is expected to miss, and the two verbs the split strip carries
        // are never both destructive.
        for (x, cell) in [
            (0, ToolbarCell::Left),
            (PANEL_COLUMNS / 2 - 1, ToolbarCell::Left),
            (PANEL_COLUMNS / 2, ToolbarCell::Right),
            (PANEL_COLUMNS - 1, ToolbarCell::Right),
        ] {
            assert_eq!(
                toolbar_cell_at(x as u16, (TOOLBAR_TOP + TOOLBAR_H / 2) as u16),
                Some(cell),
                "column {x}"
            );
        }
        // Off the strip, either way.
        assert_eq!(toolbar_cell_at(0, TOOLBAR_TOP as u16 - 1), None);
        assert_eq!(toolbar_cell_at(0, CONTENT_BOTTOM as u16), None);
    }

    #[test]
    fn a_confirmation_answers_from_two_boxes_that_do_not_touch() {
        let keep = dialog_answer_box(DialogChoice::Keep);
        let act = dialog_answer_box(DialogChoice::Act);
        assert_eq!(keep.y, act.y);
        assert_eq!(
            act.x,
            keep.x + keep.w + DIALOG_GAP,
            "the two answers are one gap apart"
        );
        // Every point of either box lands on that answer and on no other, and the seam between
        // them lands on neither: a miss is the caller's safe answer rather than the action.
        for choice in [DialogChoice::Keep, DialogChoice::Act] {
            let rect = dialog_answer_box(choice);
            for x in rect.x..rect.x + rect.w {
                for y in rect.y..rect.y + rect.h {
                    assert_eq!(dialog_choice_at(x as u16, y as u16), Some(choice));
                }
            }
        }
        let seam = (keep.x + keep.w) as u16;
        assert_eq!(dialog_choice_at(seam, keep.y as u16), None);
        assert_eq!(dialog_choice_at(keep.x as u16, DIALOG_TOP as u16), None);
        assert_eq!(dialog_choice_at(0, 0), None);
    }
}

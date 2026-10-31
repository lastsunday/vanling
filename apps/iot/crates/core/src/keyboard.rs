//! The Network page's own controls: which key a tap lands on and what it types, and which
//! row of the scan list it picks.

use alloc::vec::Vec;

use crate::overlay::{
    CONTENT_BOTTOM, FONT_H, GLYPH_PITCH, NETWORK_LIST_ROW, OVERLAY_GAP, PANEL_COLUMNS, ROW_PITCH,
    overlay_row_top,
};

/// Keys across and keys down: the digits' own three columns and a column of controls beside
/// them, over a row that ends an entry.
pub const COLUMNS: usize = 4;
pub const ROWS: usize = 4;
/// The ten character keys: the nine the block holds and the zero beside the controls.
pub const CHARS: usize = 10;
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
pub const TOUCH_MIN: usize = 44;

/// The action strip under the list: the page's one control, one touch target tall.
pub const TOOLBAR_H: usize = TOUCH_MIN;
/// Top edge of the action strip, in panel rows.
pub const TOOLBAR_TOP: usize = CONTENT_BOTTOM - TOOLBAR_H;

/// The scan list's rows and their pitch.
pub const LIST_ROWS: usize = 3;
pub const LIST_ROW_H: usize = (TOOLBAR_TOP - LIST_TOP) / LIST_ROWS;

const _: () = assert!(
    LIST_TOP + LIST_ROWS * LIST_ROW_H <= TOOLBAR_TOP,
    "the list's last row runs into the page's action strip"
);

/// Top edge of the first of the two lines a band carries: the network's name, and the
/// address its radio answers on below it.
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
pub fn toolbar_at(y: u16) -> bool {
    (TOOLBAR_TOP..CONTENT_BOTTOM).contains(&usize::from(y))
}
/// How long a key stays open for its next candidate, in milliseconds.
pub const MULTITAP_WINDOW_MS: u32 = 800;

/// What the ten character keys type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    #[default]
    Abc,
    Digits,
    Symbols,
}

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
const DIGITS: [&[u8]; CHARS] = [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"0"];
const SYMBOLS: [&[u8]; CHARS] = [b".", b",", b":", b";", b"?", b"!", b"-", b"_", b"@", b"#"];

/// Most characters any one key types, which is the `pqrs` key's four.
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
    /// The `digit`-th key's face, in the order the grid lays them out: the digits that
    /// label the phone layer's keys, and the character each of the other layers' keys
    /// types.
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
    Char(u8),
    Shift,
    Backspace,
    Layer,
    Connect,
    Cancel,
    Reveal,
}

/// The character key a repeat tap would cycle, and which of the characters printed on it
/// the panel is currently holding.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

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
/// slots stay an exact division of the panel and only the ink moves.
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

/// The band of the scan list a point falls on, if it falls on the list at all.
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

const SCRIPT_TAP_MS: u32 = 100;
const SCRIPT_GAP_MS: u32 = MULTITAP_WINDOW_MS + SCRIPT_TAP_MS;

/// The taps that type `text` on this keyboard, or `None` for a byte no key carries.
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

fn find(layer: Layer, byte: u8) -> Option<(usize, usize)> {
    (0..CHARS).find_map(|digit| {
        layer
            .candidates(digit)
            .iter()
            .position(|candidate| *candidate == byte)
            .map(|index| (digit, index))
    })
}

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
        assert_eq!(key_at(Layer::Abc, 5, KEYBOARD_TOP as u16 - 1), None);
        assert_eq!(key_at(Layer::Abc, 5, CONTENT_BOTTOM as u16), None);
        assert_eq!(
            key_at(Layer::Abc, PANEL_COLUMNS as u16, KEYBOARD_TOP as u16),
            None
        );
        assert_eq!(key_box(KEYS), None);
        assert_eq!(key_of(Layer::Abc, KEYS), None);
    }

    #[test]
    fn the_digits_sit_in_the_block_a_phone_taught_the_thumb() {
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
        for band in 0..LIST_ROWS {
            let top = band_lines_top(band);
            let band_top = LIST_TOP + band * LIST_ROW_H;
            assert!(top >= band_top, "band {band}'s name is above its own row");
            assert!(
                top + ROW_PITCH + FONT_H <= band_top + LIST_ROW_H,
                "band {band}'s address runs into the row under it"
            );
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
        let strip = toolbar_box();
        assert_eq!(strip.y, TOOLBAR_TOP);
        assert_eq!(strip.y + strip.h, CONTENT_BOTTOM);
        assert_eq!(strip.w, PANEL_COLUMNS);
        for y in TOOLBAR_TOP..CONTENT_BOTTOM {
            assert!(toolbar_at(y as u16), "row {y} is the strip's");
        }
        assert!(!toolbar_at(TOOLBAR_TOP as u16 - 1));
        assert!(!toolbar_at(CONTENT_BOTTOM as u16));
    }

    #[test]
    fn a_split_strip_is_two_cells_of_one_width_that_meet_at_the_middle() {
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

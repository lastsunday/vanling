//! The key grid: which key a tap lands on and the box it is drawn in.
//!
//! The vocabulary — what a key is, what each layer types, how long a repeat stays open —
//! lives in `model::keyboard`; this file only maps those keys onto panel pixels.

use iot_core::geometry::ControlRect;
use iot_core::overlay::{CONTENT_BOTTOM, PANEL_COLUMNS};

use crate::shared::intent::{Key, Layer};

use super::TOUCH_MIN;
use super::list::LIST_TOP;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::intent::CHARS;
    use alloc::vec::Vec;

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
}

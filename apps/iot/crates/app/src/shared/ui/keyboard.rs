//! The panel's keyboard: the text a field holds, the key grid it is typed on, and the multi-tap
//! that fits a whole alphabet into ten keys.

use iot_core::ui::panel::GLYPH;
use iot_core::ui::{Canvas, Rect};

use crate::shared::intent::{Cycling, Key, Layer, MULTITAP_WINDOW_MS};
use crate::shared::ui::theme::Theme;

/// The longest run a field on this panel holds.
pub const FIELD_BYTES: usize = 24;

/// Keys across and down; the grid's shape is also its hit test.
pub const COLUMNS: usize = 4;
pub const ROWS: usize = 4;
pub const KEYS: usize = COLUMNS * ROWS;

/// One key's height: four rows plus a field fit the page body.
pub const KEY_H: i32 = 38;
pub const H: i16 = (KEY_H * ROWS as i32) as i16;

/// A text field's contents and the state the keys type with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text<const N: usize> {
    octets: [u8; N],
    len: u8,
    shift: Shift,
    layer: Layer,
    pending: Option<Pending>,
    shift_at: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shift {
    #[default]
    Off,
    OneShot,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    face: u8,
    index: u8,
    shifted: bool,
    at_ms: u32,
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self {
            octets: [0; N],
            len: 0,
            shift: Shift::Off,
            layer: Layer::Abc,
            pending: None,
            shift_at: None,
        }
    }
}

impl<const N: usize> Text<N> {
    /// Type one key.
    pub fn edit(&mut self, key: Key, at_ms: u32) {
        match key {
            Key::Char(face) => self.tap(face, at_ms),
            Key::Backspace => {
                self.end();
                self.len = self.len.saturating_sub(1);
            }
            Key::Layer => {
                self.end();
                self.layer = self.layer.next();
            }
            Key::Shift => self.shift_tap(at_ms),
            Key::Connect | Key::Cancel | Key::Reveal => self.end(),
        }
    }

    fn tap(&mut self, face: u8, at_ms: u32) {
        let candidates = self.layer.candidates_of(face);
        if candidates.is_empty() {
            // A key that prints a character rather than a phone-layer digit types itself.
            self.end();
            self.push(face);
            return;
        }
        let cycling = candidates.len() > 1
            && self.pending.is_some_and(|pending| {
                pending.face == face && at_ms.saturating_sub(pending.at_ms) <= MULTITAP_WINDOW_MS
            });
        let index = match (cycling, self.pending) {
            (true, Some(pending)) => (pending.index + 1) % candidates.len() as u8,
            _ => 0,
        };
        if cycling {
            self.len = self.len.saturating_sub(1);
        }
        let shifted = match (cycling, self.pending) {
            (true, Some(pending)) => pending.shifted,
            _ => self.spend_shift(candidates[usize::from(index)]),
        };
        let candidate = candidates[usize::from(index)];
        let byte = if shifted {
            candidate.to_ascii_uppercase()
        } else {
            candidate
        };
        if self.push(byte) {
            self.pending = Some(Pending {
                face,
                index,
                shifted,
                at_ms,
            });
        } else {
            self.pending = None;
        }
    }

    fn shift_tap(&mut self, at_ms: u32) {
        self.end();
        let repeated = self
            .shift_at
            .is_some_and(|at| at_ms.saturating_sub(at) <= MULTITAP_WINDOW_MS);
        self.shift = match (self.shift, repeated) {
            (Shift::Locked, _) => Shift::Off,
            (Shift::OneShot, true) => Shift::Locked,
            _ => Shift::OneShot,
        };
        self.shift_at = Some(at_ms);
    }

    fn end(&mut self) {
        self.pending = None;
    }

    fn spend_shift(&mut self, candidate: u8) -> bool {
        let letter = candidate.to_ascii_uppercase() != candidate;
        match self.shift {
            Shift::Off => false,
            Shift::OneShot if letter => {
                self.shift = Shift::Off;
                true
            }
            Shift::OneShot | Shift::Locked => letter,
        }
    }

    fn push(&mut self, byte: u8) -> bool {
        if usize::from(self.len) >= self.octets.len() {
            return false;
        }
        self.octets[usize::from(self.len)] = byte;
        self.len += 1;
        true
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.octets[..usize::from(self.len)]
    }

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.end();
    }

    /// Whether letters type upper case.
    pub fn shift(&self) -> bool {
        self.shift != Shift::Off
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// The key a repeat tap would cycle, and which of its characters is on the panel.
    pub fn cycling(&self) -> Option<Cycling> {
        let pending = self.pending?;
        Some(Cycling {
            face: pending.face,
            candidates: self.layer.candidates_of(pending.face),
            index: pending.index,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Digit(usize),
    Literal(u8),
    Control(Key),
    Empty,
}

const CELLS: [Cell; KEYS] = [
    Cell::Digit(0),
    Cell::Digit(1),
    Cell::Digit(2),
    Cell::Control(Key::Backspace),
    Cell::Digit(3),
    Cell::Digit(4),
    Cell::Digit(5),
    Cell::Control(Key::Layer),
    Cell::Digit(6),
    Cell::Digit(7),
    Cell::Digit(8),
    Cell::Control(Key::Shift),
    Cell::Literal(b' '),
    Cell::Digit(9),
    Cell::Control(Key::Cancel),
    Cell::Empty,
];

/// The key in the grid's `cell`, if the cell holds one.
pub fn key_of(layer: Layer, cell: usize) -> Option<Key> {
    Some(match *CELLS.get(cell)? {
        Cell::Digit(digit) => Key::Char(layer.face(digit)),
        Cell::Literal(byte) => Key::Char(byte),
        Cell::Control(key) => key,
        Cell::Empty => return None,
    })
}

/// The key a panel point lands on inside the keyboard's box, on the layer on show.
pub fn key_at(rect: Rect, x: i32, y: i32, layer: Layer) -> Option<Key> {
    key_of(layer, cell_at(rect, x, y)?)
}

fn cell_at(rect: Rect, x: i32, y: i32) -> Option<usize> {
    if !rect.contains(x, y) {
        return None;
    }
    let column = usize::try_from((x - rect.x) / key_width(rect)).ok()?;
    let row = usize::try_from((y - rect.y) / KEY_H).ok()?;
    (column < COLUMNS && row < ROWS).then_some(row * COLUMNS + column)
}

fn key_width(rect: Rect) -> i32 {
    (rect.w / COLUMNS as i32).max(1)
}

/// The middle of the grid's `cell`, for a test that has to tap one.
pub fn key_centre(rect: Rect, cell: usize) -> (u16, u16) {
    let column = cell % COLUMNS;
    let row = cell / COLUMNS;
    (
        u16::try_from(rect.x + column as i32 * key_width(rect) + key_width(rect) / 2).unwrap_or(0),
        u16::try_from(rect.y + row as i32 * KEY_H + KEY_H / 2).unwrap_or(0),
    )
}

/// Draws the grid the hit test reads. Every cell is one key; the empty one stays blank.
pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, text: Text<FIELD_BYTES>, theme: &Theme) {
    let width = key_width(rect);
    let cycling = text.cycling();
    for cell in 0..KEYS {
        let Some(key) = key_of(text.layer(), cell) else {
            continue;
        };
        let key_rect = Rect::new(
            rect.x + (cell % COLUMNS) as i32 * width,
            rect.y + (cell / COLUMNS) as i32 * KEY_H,
            width,
            KEY_H,
        );
        canvas.set_color(Some(theme.muted));
        canvas.stroke_rect(key_rect);

        let mut face_buf = [0u8; 2];
        let face = face_of(key, text.layer(), text.shift(), &mut face_buf);
        let top = key_rect.y as usize + 4;
        canvas.set_color(Some(theme.foreground));
        canvas.draw_text(face, centre(canvas, face, key_rect), top);

        let mut run_buf = [0u8; 4];
        let run = run_of(key, text.layer(), text.shift(), &mut run_buf);
        if run.is_empty() {
            continue;
        }
        let run_top = top + 12;
        canvas.set_color(Some(theme.muted));
        let left = centre(canvas, run, key_rect);
        canvas.draw_text(run, left, run_top);
        if let Some(cycling) = cycling.filter(|cycling| key == Key::Char(cycling.face)) {
            let index = usize::from(cycling.index);
            canvas.set_color(Some(theme.foreground));
            canvas.draw_text(&run[index..=index], left + index * GLYPH, run_top);
        }
    }
}

fn centre(canvas: &Canvas<'_>, run: &[u8], rect: Rect) -> usize {
    let width = canvas.measure(run) as i32;
    (rect.x + (rect.w - width).max(0) / 2) as usize
}

fn face_of(key: Key, layer: Layer, shift: bool, out: &mut [u8; 2]) -> &[u8] {
    match key {
        Key::Char(byte) => {
            out[0] = if shift {
                byte.to_ascii_uppercase()
            } else {
                byte
            };
            &out[..1]
        }
        Key::Backspace => b"<",
        Key::Shift => b"^",
        Key::Layer => match layer {
            Layer::Abc => b"12",
            Layer::Digits => b"#",
            Layer::Symbols => b"&",
            Layer::More => b"ab",
        },
        Key::Cancel => b"OK",
        Key::Connect | Key::Reveal => b"",
    }
}

/// The characters a key cycles, shifted; empty when the key types only one.
fn run_of(key: Key, layer: Layer, shift: bool, out: &mut [u8; 4]) -> &[u8] {
    let Key::Char(byte) = key else {
        return b"";
    };
    let candidates = layer.candidates_of(byte);
    if candidates.len() <= 1 {
        return b"";
    }
    for (slot, candidate) in out.iter_mut().zip(candidates) {
        *slot = if shift {
            candidate.to_ascii_uppercase()
        } else {
            *candidate
        };
    }
    &out[..candidates.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn the_grid_holds_every_character_key_and_the_controls() {
        let keys: Vec<Key> = (0..KEYS)
            .filter_map(|cell| key_of(Layer::Abc, cell))
            .collect();
        assert_eq!(keys.len(), KEYS - 1, "one cell is deliberately empty");
        for key in [Key::Backspace, Key::Shift, Key::Layer, Key::Cancel] {
            assert!(keys.contains(&key), "{key:?} is missing");
        }
    }

    #[test]
    fn a_tap_types_and_a_repeat_cycles() {
        let mut text = Text::<FIELD_BYTES>::default();
        let two = Layer::Abc.face(1);
        text.edit(Key::Char(two), 0);
        assert_eq!(text.as_bytes(), b"a");
        text.edit(Key::Char(two), 100);
        assert_eq!(text.as_bytes(), b"b");
        text.edit(Key::Char(two), 200);
        assert_eq!(text.as_bytes(), b"c");
        text.edit(Key::Char(two), 300);
        assert_eq!(text.as_bytes(), b"a");
    }

    #[test]
    fn a_gap_ends_the_cycle_and_backspace_removes_the_last() {
        let mut text = Text::<FIELD_BYTES>::default();
        let two = Layer::Abc.face(1);
        text.edit(Key::Char(two), 0);
        text.edit(Key::Char(two), MULTITAP_WINDOW_MS + 1);
        assert_eq!(text.as_bytes(), b"aa", "a new tap starts the cycle over");
        text.edit(Key::Backspace, 0);
        assert_eq!(text.as_bytes(), b"a");
    }

    #[test]
    fn the_field_stops_at_its_capacity() {
        let mut text = Text::<4>::default();
        for _ in 0..8 {
            text.edit(Key::Char(b' '), 0);
        }
        assert_eq!(text.as_bytes(), b"    ");
    }

    #[test]
    fn a_panel_point_reads_back_the_key_it_landed_on() {
        let rect = Rect::new(10, 20, 200, H as i32);
        for cell in 0..KEYS {
            let (x, y) = key_centre(rect, cell);
            assert_eq!(
                key_at(rect, i32::from(x), i32::from(y), Layer::Abc),
                key_of(Layer::Abc, cell),
                "cell {cell}"
            );
        }
        assert_eq!(key_at(rect, rect.x - 1, rect.y, Layer::Abc), None);
        assert_eq!(key_at(rect, rect.x, rect.bottom(), Layer::Abc), None);
    }
}

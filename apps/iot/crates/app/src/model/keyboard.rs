//! The key grid's vocabulary: what a key is, what each layer types, and how long a key stays
//! open for its next candidate.
//!
//! The boxes those keys are drawn in and the hit test that maps a finger to one live in
//! `panel`, which reads this vocabulary and never the other way round.

/// The ten character keys: the nine the digit block holds and the zero beside the controls.
pub const CHARS: usize = 10;

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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    const LAYERS: [Layer; 3] = [Layer::Abc, Layer::Digits, Layer::Symbols];

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
}

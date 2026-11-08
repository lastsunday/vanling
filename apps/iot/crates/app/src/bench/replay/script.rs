//! A script of taps that types a passphrase on the panel's own key grid.
//!
//! Only the bench replays one: the product is handed its credential, a bench build has to
//! press the keys a person would.

use alloc::vec::Vec;

use crate::pages::network::layout::{cell_for_digit, cell_of};
use crate::shared::keyboard::{CHARS, Key, Layer, MULTITAP_WINDOW_MS};

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
        let cell = cell_for_digit(digit)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::network::layout::{key_box, key_of};
    use crate::pages::network::model::Entry;

    #[test]
    fn a_script_types_its_own_text_through_the_keyboard() {
        for text in [b"pass".as_slice(), b"aZ9.# 0_", b"00", b"z9", b"AaA", b" "] {
            let mut entry = Entry::default();
            for tap in script_for(text).expect("a text the keyboard carries") {
                let key = key_of(entry.layer(), tap.cell).expect("a cell of the grid");
                entry.edit(key, tap.at_ms);
            }
            assert_eq!(entry.as_bytes(), text, "{text:?} did not come back");
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
}

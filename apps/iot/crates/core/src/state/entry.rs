//! A passphrase being typed: the draft's octets, the shift state, and the pending character
//! a repeat of the same key can still cycle.

use crate::drivers::network::{Credentials, PASSWORD_BYTES, Password, Ssid};
use crate::keyboard::{Cycling, Key, Layer, MULTITAP_WINDOW_MS};

/// A passphrase being typed on the Network page's keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    octets: [u8; PASSWORD_BYTES - 1],
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

impl Default for Entry {
    fn default() -> Self {
        Self {
            octets: [0; PASSWORD_BYTES - 1],
            len: 0,
            shift: Shift::Off,
            layer: Layer::Abc,
            pending: None,
            shift_at: None,
        }
    }
}

impl Entry {
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
        if self.len as usize >= self.octets.len() {
            return false;
        }
        self.octets[self.len as usize] = byte;
        self.len += 1;
        true
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.octets[..self.len as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether letters type upper case.
    pub fn shift(&self) -> bool {
        self.shift != Shift::Off
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// The character key a repeat tap would cycle, and which of the characters printed on
    /// it the panel is holding: the keyboard's own feedback, and never the passphrase
    /// itself.
    pub fn cycling(&self) -> Option<Cycling> {
        let pending = self.pending?;
        Some(Cycling {
            face: pending.face,
            candidates: self.layer.candidates_of(pending.face),
            index: pending.index,
        })
    }

    /// What this draft would hand the radio, for the name the user picked it for.
    pub fn credentials(&self, ssid: Ssid) -> Credentials {
        Credentials {
            ssid,
            password: Password::from_bytes(self.as_bytes()),
        }
    }
}

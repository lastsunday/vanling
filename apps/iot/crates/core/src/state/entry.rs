//! A passphrase being typed: the draft's octets, the shift state, and the pending
//! character a repeat of the same key can still cycle.
use super::*;

/// A passphrase being typed on the Network page's keyboard.
///
/// A draft rather than a [`Password`], because the two are different jobs: a `Password` is
/// what the radio stores and what has been checked against its limits, this is text a finger
/// is entering and can be wrong. The cap is the radio's own — one byte short of
/// [`PASSWORD_BYTES`], which is the length the driver refuses — so a keystroke past it is
/// dropped rather than producing a credential the radio will not take as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    octets: [u8; PASSWORD_BYTES - 1],
    len: u8,
    /// What the shift key is set to: off, the one letter a tap capitalises, or the lock a second
    /// tap inside the window sets.
    shift: Shift,
    layer: Layer,
    /// The character the last character key typed, while a repeat of that key can still cycle it.
    /// What the panel draws a key's candidates from: the key being cycled is the one that knows
    /// what the next tap of it would type.
    pending: Option<Pending>,
    /// When the shift was last tapped, while a second tap could still lock it.
    shift_at: Option<u32>,
}

/// The shift key's three states.
///
/// A one-shot rather than a latch, because that is what a thumb expects of a shift: a single
/// capital is the common case and a lock is one tap away from it, which is cheaper than a lock
/// every time and a mode to escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shift {
    #[default]
    Off,
    /// The next letter typed comes out upper case, and then this is off again.
    OneShot,
    /// Letters come out upper case until shift is tapped again.
    Locked,
}

/// A character key whose character a repeat tap inside [`MULTITAP_WINDOW_MS`] can still change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    /// The face of the key: that key tapped again cycles, and any other key starts a character.
    face: u8,
    /// Where in the key's candidates the character on the panel came from, so the next tap of
    /// that key replaces it with the one after.
    index: u8,
    /// Whether the run's character is a capital.
    ///
    /// The run's rather than the tap's: a letter three taps down a key is still that run's
    /// character, so a shift spent on the first tap of it would leave `S` reachable only by locking
    /// the shift for everything after it.
    shifted: bool,
    at_ms: u32,
}

impl Default for Entry {
    /// An empty draft on the alphabet layer, which is where naming a network starts.
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
    ///
    /// The two that end an entry — connect and cancel — are the state's decisions rather than
    /// the draft's, so they are not here: this type only knows what text a key adds.
    ///
    /// `at_ms` is the touch panel's own clock, and it is what decides whether a repeat of a key
    /// is the same character cycling or a new one — which is the whole of what a multi-tap
    /// keyboard needs a clock for.
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
            // The two that end an entry end the run of taps with it: neither is a character, and a
            // `pending` kept past one would let a later tap of that key cycle a character the user
            // has already moved on from. Showing the draft is the third: it changes what the panel
            // draws, not what the draft holds.
            Key::Connect | Key::Cancel | Key::Reveal => self.end(),
        }
    }

    /// One character key: the same key tapped again inside the window moves the character on,
    /// and anything else starts a new one.
    fn tap(&mut self, face: u8, at_ms: u32) {
        let candidates = self.layer.candidates_of(face);
        if candidates.is_empty() {
            return;
        }
        // A tap of the same key cycles while the window is open; every other key ends the run by
        // replacing what the last one typed, which is why nothing here has to remember more than
        // the one character. A layer whose keys type one character each has nothing to cycle, so
        // a repeat of one of them is the second character — a passphrase with `00` in it.
        let cycling = candidates.len() > 1
            && self.pending.is_some_and(|pending| {
                pending.face == face && at_ms.saturating_sub(pending.at_ms) <= MULTITAP_WINDOW_MS
            });
        let index = match (cycling, self.pending) {
            (true, Some(pending)) => (pending.index + 1) % candidates.len() as u8,
            _ => 0,
        };
        if cycling {
            // The character on the panel is replaced rather than followed: that is what makes a
            // phone keypad's second tap type `b` where the first typed `a`.
            self.len = self.len.saturating_sub(1);
        }
        // The case belongs to the run, not to the tap that opened it: `Z` is four taps of one key
        // with the shift set, and a shift spent on the first of them would leave the capital on a
        // character the fourth tap replaces.
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
            // A draft at the cap took nothing, and a `pending` kept here would make the next
            // tap of this key erase the character it could not replace.
            self.pending = None;
        }
    }

    /// A shift tap: one shot from off, a lock from a second tap inside the window, and off from
    /// the lock. The phone's own shift, on the window every other key already uses.
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

    /// Ends the run of character-key taps, so that a later tap of the same key starts a new
    /// character instead of cycling one that some other key has already moved on from.
    fn end(&mut self) {
        self.pending = None;
    }

    /// Whether `candidate` is the letter a one-shot shift is spent on.
    ///
    /// Punctuation has no case, so a shift tapped before one is still there for the letter after
    /// it: a finger that means to capitalise is not aiming at the key it landed on.
    fn spend_shift(&mut self, candidate: u8) -> bool {
        let letter = candidate.to_ascii_uppercase() != candidate;
        match self.shift {
            Shift::Off => false,
            Shift::OneShot if letter => {
                self.shift = Shift::Off;
                true
            }
            // A shift no letter has spent yet is still set for the one that comes, and a lock is
            // never spent.
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

    /// The character key a repeat tap would cycle, and which of the characters printed on it the
    /// panel is holding: the keyboard's own feedback, and never the passphrase itself.
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

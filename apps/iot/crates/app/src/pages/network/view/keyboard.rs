//! The key grid: each key's face, the characters it cycles, and the two latched controls.

use crate::pages::network::layout;
use crate::pages::network::model::Entry;
use crate::shared::keyboard::{Key, MAX_CANDIDATES};
use iot_core::geometry::ControlRect;
use iot_core::overlay::{FONT_H, GLYPH_PITCH};

use crate::pages::network::view::text::{HIDE_LABEL, key_label};
use crate::shared::view_kit::surface::Surface;
use crate::shared::view_kit::widgets::label::text_left;

/// The gap between a key's face and the row of characters under it.
const KEY_LINE_GAP: usize = 4;

/// Which run of a key's face the panel marks: none, the face itself, or one candidate in the
/// row under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Face,
    Candidate(usize),
}

impl Surface<'_> {
    pub(super) fn stamp_keyboard(&mut self, entry: Entry, revealed: bool) {
        let layer = entry.layer();
        let shift = entry.shift();
        let cycling = entry.cycling();
        let mut candidates = [0u8; MAX_CANDIDATES];
        for cell in 0..layout::KEYS {
            let (Some(rect), Some(key)) = (layout::key_box(cell), layout::key_of(layer, cell))
            else {
                continue;
            };
            let Key::Char(face) = key else {
                // The two controls whose state the page has to show say it on themselves: the shift
                // because a latch that showed up nowhere but in the letters would be a capital the
                // user has to guess at, and the reveal because a key that both shows and hides a
                // passphrase has to say which it would do.
                let mark = if key == Key::Shift && shift {
                    Mark::Face
                } else {
                    Mark::None
                };
                let label = if key == Key::Reveal && revealed {
                    HIDE_LABEL
                } else {
                    key_label(key, layer)
                };
                self.stamp_key(rect, label, mark, b"");
                continue;
            };
            let run = &mut candidates[..layer.candidates_of(face).len()];
            for (glyph, candidate) in run.iter_mut().zip(layer.candidates_of(face)) {
                *glyph = if shift {
                    candidate.to_ascii_uppercase()
                } else {
                    *candidate
                };
            }
            let mark = match cycling {
                Some(cycling) if cycling.face == face && run.len() > 1 => {
                    Mark::Candidate(usize::from(cycling.index))
                }
                _ => Mark::None,
            };
            self.stamp_key(rect, &[face], mark, run);
        }
    }

    fn stamp_key(&mut self, rect: ControlRect, face: &[u8], mark: Mark, run: &[u8]) {
        let run = if run == face { b"".as_slice() } else { run };
        self.stamp_frame(rect);

        let lines = usize::from(!run.is_empty());
        let block = (lines + 1) * FONT_H + lines * KEY_LINE_GAP;
        let face_top = rect.y + rect.h.saturating_sub(block) / 2;
        let face_left = text_left(rect, face);
        if mark == Mark::Face {
            self.stamp_marked(face, face_left, face_top);
        } else {
            self.stamp_text(face, face_left, face_top);
        }
        if run.is_empty() {
            return;
        }
        let run_top = face_top + FONT_H + KEY_LINE_GAP;
        let run_left = text_left(rect, run);
        self.stamp_text(run, run_left, run_top);
        if let Mark::Candidate(index) = mark {
            let at = run_left + index * GLYPH_PITCH;
            self.stamp_marked(&run[index..=index], at, run_top);
        }
    }
}

//! The Content band: the scan list, or the key grid while the page is typing.

use crate::pages::network::layout;
use crate::pages::network::layout::{LIST_ROW_H, LIST_ROWS, LIST_TOP, band_lines_top};
use crate::pages::network::view::header::BARS_GLYPHS;
use crate::pages::network::view::text::{
    HIDE_LABEL, NETWORK_NAME_GLYPHS, format_bars, format_network_name, key_label,
};
use crate::shared::intent::{Key, MAX_CANDIDATES};
use crate::shared::kit::painter::Painter;
use crate::shared::kit::placement::PANEL_RIGHT;
use crate::shared::kit::widgets::label::text_left;
use crate::shared::model::{Entry, NetworkView};
use iot_core::drivers::network::{ApList, LinkStatus, SSID_BYTES};
use iot_core::geometry::ControlRect;
use iot_core::overlay::{
    Block, FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, ROW_PITCH, block_x,
};
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};

const OPEN_MARKER_RIGHT: isize = PANEL_RIGHT - 5 * GLYPH_PITCH as isize;

const OPEN_MARKER: &[u8] = b"OPEN";

const SAVED_MARKER_RIGHT: isize =
    OPEN_MARKER_RIGHT - OPEN_MARKER.len() as isize * GLYPH_PITCH as isize - GLYPH_PITCH as isize;

const SAVED_MARKER: &[u8] = b"*";

const _: () = assert!(
    block_x(Block::First) + NETWORK_NAME_GLYPHS * GLYPH_PITCH - OVERLAY_GAP + GLYPH_PITCH
        <= OPEN_MARKER_RIGHT as usize + 1 - (OPEN_MARKER.len() * GLYPH_PITCH - OVERLAY_GAP),
    "an open network's marker runs into the name of the row it is on"
);
const _: () = assert!(
    (OPEN_MARKER_RIGHT as usize) < PANEL_COLUMNS - BARS_GLYPHS * GLYPH_PITCH,
    "an open network's marker runs into the signal bars of the row it is on"
);
const _: () = assert!(
    SAVED_MARKER_RIGHT + OVERLAY_GAP as isize - GLYPH_PITCH as isize
        > (block_x(Block::First) + (NETWORK_NAME_GLYPHS - 1) * GLYPH_PITCH + FONT_W - 1) as isize,
    "the saved mark is drawn over the name slot the list gives a row"
);
const _: () = assert!(
    SAVED_MARKER_RIGHT < OPEN_MARKER_RIGHT,
    "the saved mark and the open marker are drawn in the same slot"
);

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

impl Painter<'_> {
    pub(crate) fn draw_content(
        &mut self,
        draft: NetworkView,
        link: LinkStatus,
        aps: Option<ApList>,
    ) {
        if draft.keyboard {
            if let Some(entry) = draft.entry {
                self.draw_keyboard(entry, draft.revealed);
            }
            return;
        }
        self.draw_list(draft, link, aps);
    }

    fn draw_list(&mut self, draft: NetworkView, link: LinkStatus, aps: Option<ApList>) {
        match aps {
            None => self.draw_band(b"NOT SCANNED"),
            Some(list) if list.is_empty() => self.draw_band(b"NO NETWORKS"),
            Some(list) => {
                for band in 0..LIST_ROWS {
                    let index = usize::from(draft.list_top) + band;
                    let Some(ap) = list.get(index) else {
                        break;
                    };
                    let marked = draft
                        .confirm
                        .and_then(|confirm| confirm.network(link))
                        .or(draft.selected);
                    if marked.is_some_and(|marked| marked.is_same_network(&ap)) {
                        self.draw_band_fill(band);
                    }
                    let top = band_lines_top(band);
                    self.draw_text(
                        format_network_name(ap, NETWORK_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                        block_x(Block::First),
                        top,
                    );
                    if ap.ssid.as_display_text().is_some() {
                        self.draw_text(
                            mac_text(ap.bssid, &mut [0u8; MAC_TEXT_BYTES]),
                            block_x(Block::First),
                            top + ROW_PITCH,
                        );
                    }
                    if !ap.secured {
                        self.draw_text_right(OPEN_MARKER, OPEN_MARKER_RIGHT, top);
                    }
                    if list.is_saved(index) {
                        self.draw_text_right(SAVED_MARKER, SAVED_MARKER_RIGHT, top);
                    }
                    self.draw_text_right(format_bars(ap.bars(), &mut [0u8; 4]), PANEL_RIGHT, top);
                }
            }
        }
    }

    fn draw_band_fill(&mut self, band: usize) {
        self.fill(ControlRect {
            x: 0,
            y: LIST_TOP + band * LIST_ROW_H,
            w: PANEL_COLUMNS,
            h: LIST_ROW_H,
        });
    }

    fn draw_band(&mut self, text: &[u8]) {
        self.draw_text(text, block_x(Block::First), band_top(0));
    }

    fn draw_keyboard(&mut self, entry: Entry, revealed: bool) {
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
                self.draw_key(rect, label, mark, b"");
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
            self.draw_key(rect, &[face], mark, run);
        }
    }

    fn draw_key(&mut self, rect: ControlRect, face: &[u8], mark: Mark, run: &[u8]) {
        let run = if run == face { b"".as_slice() } else { run };
        self.draw_frame(rect);

        let lines = usize::from(!run.is_empty());
        let block = (lines + 1) * FONT_H + lines * KEY_LINE_GAP;
        let face_top = rect.y + rect.h.saturating_sub(block) / 2;
        let face_left = text_left(rect, face);
        if mark == Mark::Face {
            self.draw_marked(face, face_left, face_top);
        } else {
            self.draw_text(face, face_left, face_top);
        }
        if run.is_empty() {
            return;
        }
        let run_top = face_top + FONT_H + KEY_LINE_GAP;
        let run_left = text_left(rect, run);
        self.draw_text(run, run_left, run_top);
        if let Mark::Candidate(index) = mark {
            let at = run_left + index * GLYPH_PITCH;
            self.draw_marked(&run[index..=index], at, run_top);
        }
    }
}

/// The first of a band's two lines: where the text block starts, centred in the band.
fn band_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - FONT_H) / 2
}

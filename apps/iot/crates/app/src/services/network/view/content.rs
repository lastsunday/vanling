//! The Content band: the scan list, or the key grid while the page is typing.

use crate::services::network::shape;
use crate::services::network::shape::{LIST_ROW_H, LIST_ROWS, LIST_TOP, band_lines_top};
use crate::services::network::view::text::{
    HIDE_LABEL, NETWORK_NAME_GLYPHS, format_bars, format_network_name, key_label,
};
use crate::shared::intent::{Key, MAX_CANDIDATES};
use crate::shared::model::{Entry, NetworkView};
use iot_core::drivers::network::{ApList, LinkStatus, SSID_BYTES};
use iot_core::geometry::ControlRect;
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};
use iot_core::ui::Canvas;
use iot_core::ui::Rect;
use iot_core::ui::panel::{GLYPH, LINE, MARGIN, PANEL_COLUMNS, RIGHT, ROW_PITCH};

const OPEN_MARKER_RIGHT: isize = (RIGHT - 5 * GLYPH) as isize;
const SAVED_MARKER_RIGHT: isize = OPEN_MARKER_RIGHT - 5 * GLYPH as isize;

const OPEN_MARKER: &[u8] = b"OPEN";
const SAVED_MARKER: &[u8] = b"*";

const _: () = assert!(
    MARGIN + NETWORK_NAME_GLYPHS * GLYPH <= SAVED_MARKER_RIGHT as usize,
    "the saved mark is drawn over the name slot the list gives a row"
);
const _: () = assert!(
    SAVED_MARKER_RIGHT < OPEN_MARKER_RIGHT && OPEN_MARKER_RIGHT < (RIGHT - 4 * GLYPH) as isize,
    "a row's markers and its signal bars are drawn in the same slot"
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

pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    draft: NetworkView,
    link: LinkStatus,
    aps: Option<ApList>,
) {
    if draft.keyboard {
        if let Some(entry) = draft.entry {
            draw_keyboard(canvas, entry, draft.revealed);
        }
        return;
    }
    draw_list(canvas, draft, link, aps);
}

fn draw_list(canvas: &mut Canvas<'_>, draft: NetworkView, link: LinkStatus, aps: Option<ApList>) {
    match aps {
        None => draw_band(canvas, b"NOT SCANNED"),
        Some(list) if list.is_empty() => draw_band(canvas, b"NO NETWORKS"),
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
                    draw_band_fill(canvas, band);
                }
                let top = band_lines_top(band);
                canvas.draw_text(
                    format_network_name(ap, NETWORK_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                    MARGIN,
                    top,
                );
                if ap.ssid.as_display_text().is_some() {
                    canvas.draw_text(
                        mac_text(ap.bssid, &mut [0u8; MAC_TEXT_BYTES]),
                        MARGIN,
                        top + ROW_PITCH,
                    );
                }
                if !ap.secured {
                    canvas.draw_text_right(OPEN_MARKER, OPEN_MARKER_RIGHT, top);
                }
                if list.is_saved(index) {
                    canvas.draw_text_right(SAVED_MARKER, SAVED_MARKER_RIGHT, top);
                }
                canvas.draw_text_right(format_bars(ap.bars(), &mut [0u8; 4]), RIGHT as isize, top);
            }
        }
    }
}

fn draw_band_fill(canvas: &mut Canvas<'_>, band: usize) {
    canvas.fill_rect(Rect::new(
        0,
        (LIST_TOP + band * LIST_ROW_H) as i32,
        PANEL_COLUMNS as i32,
        LIST_ROW_H as i32,
    ));
}

fn draw_band(canvas: &mut Canvas<'_>, text: &[u8]) {
    canvas.draw_text(text, MARGIN, band_top(0));
}

fn draw_keyboard(canvas: &mut Canvas<'_>, entry: Entry, revealed: bool) {
    let layer = entry.layer();
    let shift = entry.shift();
    let cycling = entry.cycling();
    let mut candidates = [0u8; MAX_CANDIDATES];
    for cell in 0..shape::KEYS {
        let (Some(rect), Some(key)) = (shape::key_box(cell), shape::key_of(layer, cell)) else {
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
            draw_key(canvas, rect, label, mark, b"");
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
        draw_key(canvas, rect, &[face], mark, run);
    }
}

fn draw_key(canvas: &mut Canvas<'_>, rect: ControlRect, face: &[u8], mark: Mark, run: &[u8]) {
    let run = if run == face { b"".as_slice() } else { run };
    super::draw_frame(canvas, rect);

    let lines = usize::from(!run.is_empty());
    let block = (lines + 1) * LINE + lines * KEY_LINE_GAP;
    let face_top = rect.y + rect.h.saturating_sub(block) / 2;
    let face_left = super::text_left(canvas, rect, face);
    if mark == Mark::Face {
        super::draw_marked(canvas, face, face_left, face_top);
    } else {
        canvas.draw_text(face, face_left, face_top);
    }
    if run.is_empty() {
        return;
    }
    let run_top = face_top + LINE + KEY_LINE_GAP;
    let run_left = super::text_left(canvas, rect, run);
    canvas.draw_text(run, run_left, run_top);
    if let Mark::Candidate(index) = mark {
        let at = run_left + index * GLYPH;
        super::draw_marked(canvas, &run[index..=index], at, run_top);
    }
}

/// The first of a band's two lines: where the text block starts, centred in the band.
fn band_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - LINE) / 2
}

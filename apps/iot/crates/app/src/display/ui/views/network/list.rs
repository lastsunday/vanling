//! The scan list: three bands between the header and the action strip.

use crate::controls::{LIST_ROW_H, LIST_ROWS, LIST_TOP, band_lines_top};
use crate::model::state::NetworkPage;
use iot_core::drivers::network::{ApList, LinkStatus, SSID_BYTES};
use iot_core::geometry::ControlRect;
use iot_core::overlay::{
    Block, FONT_H, FONT_W, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, ROW_PITCH, block_x,
};
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};

use crate::display::ui::placement::PANEL_RIGHT;
use crate::display::ui::surface::Surface;
use crate::display::ui::views::network::header::BARS_GLYPHS;
use crate::display::ui::views::network::text::{
    NETWORK_NAME_GLYPHS, format_bars, format_network_name,
};

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

impl Surface<'_> {
    pub(super) fn stamp_list(&mut self, draft: NetworkPage, link: LinkStatus, aps: Option<ApList>) {
        match aps {
            None => self.stamp_band(b"NOT SCANNED"),
            Some(list) if list.is_empty() => self.stamp_band(b"NO NETWORKS"),
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
                        self.stamp_band_fill(band);
                    }
                    let top = band_lines_top(band);
                    self.stamp_text(
                        format_network_name(ap, NETWORK_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                        block_x(Block::First),
                        top,
                    );
                    if ap.ssid.as_display_text().is_some() {
                        self.stamp_text(
                            mac_text(ap.bssid, &mut [0u8; MAC_TEXT_BYTES]),
                            block_x(Block::First),
                            top + ROW_PITCH,
                        );
                    }
                    if !ap.secured {
                        self.stamp_text_right(OPEN_MARKER, OPEN_MARKER_RIGHT, top);
                    }
                    if list.is_saved(index) {
                        self.stamp_text_right(SAVED_MARKER, SAVED_MARKER_RIGHT, top);
                    }
                    self.stamp_text_right(format_bars(ap.bars(), &mut [0u8; 4]), PANEL_RIGHT, top);
                }
            }
        }
    }

    fn stamp_band_fill(&mut self, band: usize) {
        self.fill(ControlRect {
            x: 0,
            y: LIST_TOP + band * LIST_ROW_H,
            w: PANEL_COLUMNS,
            h: LIST_ROW_H,
        });
    }

    fn stamp_band(&mut self, text: &[u8]) {
        self.stamp_text(text, block_x(Block::First), band_top(0));
    }
}

/// The first of a band's two lines: where the text block starts, centred in the band.
fn band_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - FONT_H) / 2
}

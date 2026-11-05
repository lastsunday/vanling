//! The header's three rows: the page's title and window, the link, and the draft or address.

use crate::model::state::NetworkPage;
use iot_core::drivers::network::{AccessPoint, Ipv4, LinkStatus, SSID_BYTES};
use iot_core::overlay::{
    Block, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS, block_value_x, block_x, level_value_end,
    overlay_row_top,
};
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};

use crate::display::ui::placement::PANEL_RIGHT;
use crate::display::ui::surface::Surface;
use crate::display::ui::views::network::text::{
    format_address, format_bars, format_network_name, format_phase, format_psk, format_setup_name,
    format_window,
};

// Glyphs of the masked passphrase on the row above the keyboard: the whole row, so a draft
// longer than it is marked rather than silently truncated.
const PSK_MASK_GLYPHS: usize =
    (PANEL_COLUMNS - block_value_x(Block::First) + OVERLAY_GAP) / GLYPH_PITCH;

const HEADER_ADDRESS_GLYPHS: usize = 15;

const HEADER_NAME_GLYPHS: usize = 17;

const HEADER_PHASE_X: usize = block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH;

const HEADER_PHASE_GLYPHS: usize = 3 + 4;

pub(super) const BARS_GLYPHS: usize = 4;

const LIST_TITLE_GLYPHS: usize = b"NETWORK".len();

const LIST_WINDOW_GLYPHS: usize = 8;

const _: () = assert!(
    level_value_end(block_value_x(Block::First), PSK_MASK_GLYPHS) <= PANEL_COLUMNS,
    "the draft a passphrase is typed into runs off the row's own right edge"
);
const _: () = assert!(
    HEADER_PHASE_X + HEADER_PHASE_GLYPHS * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (BARS_GLYPHS * GLYPH_PITCH - OVERLAY_GAP),
    "the phase word runs into the signal bars on the header's network row"
);
const _: () = assert!(
    block_value_x(Block::First) + HEADER_NAME_GLYPHS * GLYPH_PITCH - OVERLAY_GAP < HEADER_PHASE_X,
    "the name of the header's network runs into the phase word beside it"
);
const _: () = assert!(
    block_x(Block::First) + LIST_TITLE_GLYPHS * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (MAC_TEXT_BYTES * GLYPH_PITCH - OVERLAY_GAP),
    "the page's title runs into this board's own address on the header's title row"
);
const _: () = assert!(
    block_x(Block::First) + (LIST_TITLE_GLYPHS + 1 + LIST_WINDOW_GLYPHS) * GLYPH_PITCH
        - OVERLAY_GAP
        <= PANEL_COLUMNS - (MAC_TEXT_BYTES * GLYPH_PITCH - OVERLAY_GAP),
    "the list's window runs into this board's own address on the header's title row"
);
const _: () = assert!(
    block_x(Block::First) + (3 + HEADER_ADDRESS_GLYPHS) * GLYPH_PITCH - OVERLAY_GAP
        <= PANEL_COLUMNS - (MAC_TEXT_BYTES * GLYPH_PITCH - OVERLAY_GAP),
    "the link's address runs into the address of the network its own row names"
);

impl Surface<'_> {
    pub(super) fn stamp_header(
        &mut self,
        draft: NetworkPage,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
        list_len: Option<usize>,
    ) {
        self.stamp_text(b"NETWORK", block_x(Block::First), overlay_row_top(0));
        if let Some(rows) = list_len {
            self.stamp_text(
                format_window(draft.list_top, rows, &mut [0u8; 12]),
                block_x(Block::First) + (LIST_TITLE_GLYPHS + 1) * GLYPH_PITCH,
                overlay_row_top(0),
            );
        }
        if let Some(mac) = mac {
            let mut text = [0u8; MAC_TEXT_BYTES];
            self.stamp_text_right(mac_text(mac, &mut text), PANEL_RIGHT, overlay_row_top(0));
        }
        if link.mode.is_setup() {
            if link.shows_setup_box()
                && let Some(mac) = mac
            {
                let mut name = [0u8; SSID_BYTES];
                let name = format_setup_name(Some(mac), &mut name);
                self.stamp_left(b"AP", name, Block::First, 1);
            }
        } else if let Some(ap) = shown_ap {
            self.stamp_left(
                b"AP",
                format_network_name(ap, HEADER_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                Block::First,
                1,
            );
        }
        let mut text = [0u8; 3 + 5];
        text[..3].copy_from_slice(b"ST ");
        let mut phase_buf = [0u8; 4];
        let word: &[u8] = if link.mode.is_setup() {
            b"SETUP"
        } else {
            format_phase(draft.phase(link.phase), &mut phase_buf)
        };
        text[3..3 + word.len()].copy_from_slice(word);
        self.stamp_text(&text[..3 + word.len()], HEADER_PHASE_X, overlay_row_top(1));
        if let Some(ap) = shown_ap
            && !link.mode.is_setup()
        {
            self.stamp_text_right(
                format_bars(ap.bars(), &mut [0u8; 4]),
                PANEL_RIGHT,
                overlay_row_top(1),
            );
        }
        if draft.keyboard {
            let Some(entry) = draft.entry else {
                return;
            };
            self.stamp_left(
                b"PSK",
                format_psk(&entry, draft.revealed, &mut [0u8; PSK_MASK_GLYPHS]),
                Block::First,
                2,
            );
        } else {
            let mut quad = [0u8; 16];
            let address = if link.mode.is_setup() {
                if link.shows_setup_box() {
                    Some(Ipv4::from_octets(iot_core::provision::ADDRESS))
                } else {
                    None
                }
            } else {
                link.address
            };
            if address.is_some() || !link.mode.is_setup() {
                let quad = format_address(address, &mut quad);
                let mut text = [0u8; 3 + 16];
                text[..3].copy_from_slice(b"IP ");
                text[3..3 + quad.len()].copy_from_slice(quad);
                self.stamp_text(
                    &text[..3 + quad.len()],
                    block_x(Block::First),
                    overlay_row_top(2),
                );
            }
            if let Some(ap) = shown_ap
                && !link.mode.is_setup()
            {
                let mut text = [0u8; MAC_TEXT_BYTES];
                self.stamp_text_right(
                    mac_text(ap.bssid, &mut text),
                    PANEL_RIGHT,
                    overlay_row_top(2),
                );
            }
        }
    }
}

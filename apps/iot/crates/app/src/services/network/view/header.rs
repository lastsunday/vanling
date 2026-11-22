//! The header's three rows: the page's title and window, the link, and the draft or address.

use crate::services::network::view::text::{
    NAME_BYTES, format_address, format_bars, format_network_name, format_phase, format_psk,
    format_setup_name, format_window,
};
use crate::shared::kit::i18n::{Lang, Msg};
use crate::shared::model::NetworkView;
use iot_core::drivers::network::{AccessPoint, Ipv4, LinkStatus};
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};
use iot_core::ui::Canvas;
use iot_core::ui::panel::{GLYPH, MARGIN, RIGHT, ROW_PITCH};

pub(super) const ROW0: usize = 8;
const ROW1: usize = ROW0 + ROW_PITCH;
const ROW2: usize = ROW1 + ROW_PITCH;

/// The widest title keeps the list's window at one place in every language.
const TITLE_GLYPHS: usize = b"NETWORK".len();

/// Where the list's window starts, one blank cell after the title.
const WINDOW_X: usize = MARGIN + (TITLE_GLYPHS + 1) * GLYPH;

const BARS_GLYPHS: usize = 4;

/// Right edge of the phase word: one blank cell left of the signal bars.
const PHASE_RIGHT: isize = (RIGHT - BARS_GLYPHS * GLYPH - GLYPH) as isize;

/// Pixels a network name is given on the header's network row.
const HEADER_NAME_W: usize = 24 * GLYPH;

/// Glyphs of the masked passphrase on the row above the keyboard: the whole row, so a draft
/// longer than it is marked rather than silently truncated.
const PSK_MASK_GLYPHS: usize = (RIGHT - (MARGIN + 4 * GLYPH)) / GLYPH;

const _: () = assert!(
    (WINDOW_X + 8 * GLYPH) <= RIGHT - MAC_TEXT_BYTES * GLYPH,
    "the list's window runs into this board's own address on the header's title row"
);
const _: () = assert!(
    MARGIN + 3 * GLYPH + HEADER_NAME_W < PHASE_RIGHT as usize,
    "the name of the header's network runs into the phase word beside it"
);

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    lang: Lang,
    draft: NetworkView,
    link: LinkStatus,
    shown_ap: Option<AccessPoint>,
    mac: Option<[u8; 6]>,
    list_len: Option<usize>,
) {
    canvas.draw_text(Msg::PageNetwork.bytes(lang), MARGIN, ROW0);
    if let Some(rows) = list_len {
        canvas.draw_text(
            format_window(draft.list_top, rows, &mut [0u8; 12]),
            WINDOW_X,
            ROW0,
        );
    }
    if let Some(mac) = mac {
        let mut text = [0u8; MAC_TEXT_BYTES];
        canvas.draw_text_right(mac_text(mac, &mut text), RIGHT as isize, ROW0);
    }
    if link.shows_setup_box() {
        if let Some(mac) = mac {
            let mut name = [0u8; NAME_BYTES];
            let name = format_setup_name(Some(mac), &mut name);
            labelled(canvas, b"AP", name, ROW1);
        }
    } else if let Some(ap) = shown_ap {
        labelled(
            canvas,
            b"AP",
            format_network_name(ap, HEADER_NAME_W, &mut [0u8; NAME_BYTES]),
            ROW1,
        );
    }
    // `3 + 8`: `ST ` plus room for the widest language's word.
    let mut text = [0u8; 3 + 8];
    text[..3].copy_from_slice(b"ST ");
    let mut phase_buf = [0u8; 4];
    let word: &[u8] = if link.mode.is_setup() {
        Msg::Setup.bytes(lang)
    } else {
        format_phase(draft.phase(link.phase), &mut phase_buf)
    };
    text[3..3 + word.len()].copy_from_slice(word);
    canvas.draw_text_right(&text[..3 + word.len()], PHASE_RIGHT, ROW1);
    if let Some(ap) = shown_ap
        && !link.shows_setup_box()
    {
        canvas.draw_text_right(format_bars(ap.bars(), &mut [0u8; 4]), RIGHT as isize, ROW1);
    }
    if draft.keyboard {
        if let Some(entry) = draft.entry {
            labelled(
                canvas,
                b"PSK",
                format_psk(&entry, draft.revealed, &mut [0u8; PSK_MASK_GLYPHS]),
                ROW2,
            );
        }
        return;
    }
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
        labelled(canvas, b"IP", format_address(address, &mut [0u8; 16]), ROW2);
    }
    if let Some(ap) = shown_ap
        && !link.mode.is_setup()
    {
        let mut text = [0u8; MAC_TEXT_BYTES];
        canvas.draw_text_right(mac_text(ap.bssid, &mut text), RIGHT as isize, ROW2);
    }
}

/// A label and its reading on one of the header's rows.
fn labelled(canvas: &mut Canvas<'_>, label: &[u8], value: &[u8], top: usize) {
    let mut buf = [0u8; 48];
    let mut at = label.len().min(buf.len());
    buf[..at].copy_from_slice(&label[..at]);
    if at < buf.len() {
        buf[at] = b' ';
        at += 1;
    }
    let take = value.len().min(buf.len() - at);
    buf[at..at + take].copy_from_slice(&value[..take]);
    canvas.draw_text(&buf[..at + take], MARGIN, top);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::model::Entry;
    use alloc::vec;
    use alloc::vec::Vec;
    use iot_core::drivers::network::{NetworkMode, SetupHost, Ssid};
    use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};

    fn ap(ssid: &[u8]) -> AccessPoint {
        AccessPoint {
            ssid: Ssid::from_octets(ssid),
            ..AccessPoint::default()
        }
    }

    fn header(draft: NetworkView, link: LinkStatus, shown_ap: Option<AccessPoint>) -> Vec<u8> {
        let mut frame = vec![0u8; PANEL_COLUMNS * CONTENT_BOTTOM * 2];
        {
            let mut canvas = Canvas::new(&mut frame, PANEL_COLUMNS, CONTENT_BOTTOM, None);
            draw(
                &mut canvas,
                Lang::En,
                draft,
                link,
                shown_ap,
                Some([0x02, 1, 2, 3, 4, 5]),
                Some(3),
            );
        }
        frame
    }

    /// The name's own cells of the link row: what the header says the page is about, without
    /// the phase word beside it or the draft under it.
    fn name_band(frame: &[u8]) -> Vec<u8> {
        let left = MARGIN * 2;
        let right = (MARGIN + HEADER_NAME_W) * 2;
        (ROW1..ROW2)
            .flat_map(|y| {
                let row = y * PANEL_COLUMNS * 2;
                frame[row + left..row + right].to_vec()
            })
            .collect()
    }

    fn typing(selected: AccessPoint) -> NetworkView {
        NetworkView {
            keyboard: true,
            entry: Some(Entry::default()),
            selected: Some(selected),
            ..NetworkView::default()
        }
    }

    fn panel_front() -> LinkStatus {
        LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::Panel,
                cancellable: false,
            },
            ..LinkStatus::default()
        }
    }

    #[test]
    fn the_row_the_page_picked_is_on_the_header_while_the_keys_are_up() {
        let picked = ap(b"neighbour");
        let draft = typing(picked);
        let in_setup = name_band(&header(draft, panel_front(), Some(picked)));
        let browsing = name_band(&header(draft, LinkStatus::default(), Some(picked)));
        assert_eq!(
            in_setup, browsing,
            "the panel front draws the picked row exactly as the list does"
        );
        assert_ne!(
            in_setup,
            name_band(&header(draft, panel_front(), Some(ap(b"cafe")))),
            "and the name comes from the row, not from a blank"
        );
    }

    #[test]
    fn the_access_points_box_names_the_access_point_not_the_row_behind_it() {
        let phone = LinkStatus {
            mode: NetworkMode::Setup {
                host: SetupHost::AccessPoint,
                cancellable: false,
            },
            ..LinkStatus::default()
        };
        let picked = ap(b"neighbour");
        assert_eq!(
            name_band(&header(typing(picked), phone, Some(picked))),
            name_band(&header(typing(picked), phone, None)),
            "the row under the box has no say in the header"
        );
    }
}

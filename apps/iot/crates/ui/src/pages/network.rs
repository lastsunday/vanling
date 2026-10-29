//! The Network page's setup UI: list, join, keyboard and dialogs.
use iot_core::controls;
use iot_core::drivers::network::{AccessPoint, ApList, Ipv4, LinkStatus, SSID_BYTES};
use iot_core::keyboard::{
    self, ControlRect, DIALOG_BODY_TOP, DIALOG_GLYPHS, DIALOG_H, DIALOG_INSET, DIALOG_TITLE_TOP,
    DIALOG_W, DIALOG_X, Key, LIST_ROW_H, LIST_ROWS, LIST_TOP, MAX_CANDIDATES, band_lines_top,
};
use iot_core::overlay::{
    Block, FONT_H, GLYPH_PITCH, PANEL_COLUMNS, ROW_PITCH, block_x, overlay_row_top,
};
use iot_core::provision::{MAC_TEXT_BYTES, mac_text};
use iot_core::state::{Entry, NetworkSetup};

use crate::layout::*;
use crate::pages::draw::{
    HIDE_LABEL, Mark, band_top, control_label, format_psk, format_window, text_left, text_width,
};
use crate::surface::Surface;
use crate::text::{
    NETWORK_NAME_GLYPHS, format_address, format_bars, format_body, format_joining_body,
    format_joining_title, format_network_name, format_phase, format_question, format_setup_name,
    format_setup_notice_body, format_setup_notice_title, format_setup_pass,
};

impl Surface<'_> {
    pub(crate) fn stamp_header(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
        list_len: Option<usize>,
    ) {
        self.stamp_text(b"NETWORK", block_x(Block::First), overlay_row_top(0));
        if let Some(rows) = list_len {
            self.stamp_text(
                format_window(setup.list_top, rows, &mut [0u8; 12]),
                block_x(Block::First) + (LIST_TITLE_GLYPHS + 1) * GLYPH_PITCH,
                overlay_row_top(0),
            );
        }
        if let Some(mac) = mac {
            let mut text = [0u8; MAC_TEXT_BYTES];
            self.stamp_text_right(mac_text(mac, &mut text), PANEL_RIGHT, overlay_row_top(0));
        }
        if link.setup {
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
        let word: &[u8] = if link.setup {
            b"SETUP"
        } else {
            format_phase(setup.phase(link.phase), &mut phase_buf)
        };
        text[3..3 + word.len()].copy_from_slice(word);
        self.stamp_text(&text[..3 + word.len()], HEADER_PHASE_X, overlay_row_top(1));
        if let Some(ap) = shown_ap
            && !link.setup
        {
            self.stamp_text_right(
                format_bars(ap.bars(), &mut [0u8; 4]),
                PANEL_RIGHT,
                overlay_row_top(1),
            );
        }
        if setup.keyboard {
            let Some(entry) = setup.entry else {
                return;
            };
            self.stamp_left(
                b"PSK",
                format_psk(&entry, setup.revealed, &mut [0u8; PSK_MASK_GLYPHS]),
                Block::First,
                2,
            );
        } else {
            let mut quad = [0u8; 16];
            let address = if link.setup {
                if link.shows_setup_box() {
                    Some(Ipv4::from_octets(iot_core::provision::ADDRESS))
                } else {
                    None
                }
            } else {
                link.address
            };
            if address.is_some() || !link.setup {
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
                && !link.setup
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

    pub(crate) fn stamp_content(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        aps: Option<ApList>,
    ) {
        if setup.keyboard {
            if let Some(entry) = setup.entry {
                self.stamp_keyboard(entry, setup.revealed);
            }
            return;
        }
        match aps {
            None => self.stamp_band(b"NOT SCANNED"),
            Some(list) if list.is_empty() => self.stamp_band(b"NO NETWORKS"),
            Some(list) => {
                for band in 0..LIST_ROWS {
                    let index = usize::from(setup.list_top) + band;
                    let Some(ap) = list.get(index) else {
                        break;
                    };
                    let marked = setup
                        .confirm
                        .and_then(|confirm| confirm.network(link))
                        .or(setup.selected);
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

    fn stamp_frame(&mut self, rect: ControlRect) {
        let (x, y) = (rect.x as isize, rect.y as isize);
        let (right, bottom) = (x + rect.w as isize - 1, y + rect.h as isize - 1);
        self.stamp_line(x, y, right, y);
        self.stamp_line(x, bottom, right, bottom);
        self.stamp_line(x, y, x, bottom);
        self.stamp_line(right, y, right, bottom);
    }

    pub(crate) fn stamp_strip(&mut self, setup: NetworkSetup, link: LinkStatus) {
        let strip = controls::strip(setup, link);
        if strip.is_split() {
            self.stamp_cell(
                keyboard::toolbar_cell(keyboard::ToolbarCell::Left),
                strip.label(keyboard::ToolbarCell::Left),
            );
            self.stamp_cell(
                keyboard::toolbar_cell(keyboard::ToolbarCell::Right),
                strip.label(keyboard::ToolbarCell::Right),
            );
        } else {
            self.stamp_cell(
                keyboard::toolbar_box(),
                strip.label(keyboard::ToolbarCell::Right),
            );
        }
    }

    fn stamp_cell(&mut self, rect: ControlRect, label: &[u8]) {
        self.stamp_frame(rect);
        self.stamp_text(
            label,
            text_left(rect, label),
            rect.y + rect.h.saturating_sub(FONT_H) / 2,
        );
    }

    pub(crate) fn stamp_box(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
    ) {
        if setup.confirm.is_some() {
            self.stamp_confirm(setup, link);
        } else if link.shows_setup_box() {
            self.stamp_notice(link, mac);
        } else {
            self.stamp_joining(shown_ap);
        }
    }

    fn stamp_notice(&mut self, link: LinkStatus, mac: Option<[u8; 6]>) {
        self.fill(ControlRect {
            x: DIALOG_X,
            y: keyboard::DIALOG_TOP,
            w: DIALOG_W,
            h: DIALOG_H,
        });
        let title_left = DIALOG_X + DIALOG_INSET;
        self.stamp_text(format_setup_notice_title(), title_left, DIALOG_TITLE_TOP);
        let mut body = [0u8; SSID_BYTES];
        self.stamp_text(
            format_setup_notice_body(mac, &mut body),
            title_left,
            DIALOG_BODY_TOP,
        );
        self.stamp_text(
            format_setup_pass(link.setup_password.as_ref(), &mut body),
            title_left,
            DIALOG_BODY_TOP + ROW_PITCH,
        );
        let answers = controls::answers(link);
        let swap = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
        self.stamp_frame(swap);
        self.stamp_text(
            answers.keep.label,
            text_left(swap, answers.keep.label),
            swap.y + swap.h.saturating_sub(FONT_H) / 2,
        );
        let close = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
        self.stamp_frame(close);
        self.stamp_text(
            answers.act.label,
            text_left(close, answers.act.label),
            close.y + close.h.saturating_sub(FONT_H) / 2,
        );
    }

    fn stamp_joining(&mut self, shown_ap: Option<AccessPoint>) {
        self.fill(ControlRect {
            x: DIALOG_X,
            y: keyboard::DIALOG_TOP,
            w: DIALOG_W,
            h: DIALOG_H,
        });
        let title_left = DIALOG_X + DIALOG_INSET;
        self.stamp_text(
            format_joining_title(shown_ap, &mut [0u8; DIALOG_GLYPHS]),
            title_left,
            DIALOG_TITLE_TOP,
        );
        self.stamp_text(format_joining_body(shown_ap), title_left, DIALOG_BODY_TOP);
    }

    fn stamp_confirm(&mut self, setup: NetworkSetup, link: LinkStatus) {
        let Some(confirm) = setup.confirm else {
            return;
        };
        self.fill(ControlRect {
            x: DIALOG_X,
            y: keyboard::DIALOG_TOP,
            w: DIALOG_W,
            h: DIALOG_H,
        });
        let mut question = [0u8; DIALOG_GLYPHS];
        let title = format_question(confirm, link, &mut question);
        let title_left = DIALOG_X + DIALOG_INSET;
        self.stamp_text(title, title_left, DIALOG_TITLE_TOP);
        self.stamp_text(format_body(confirm), title_left, DIALOG_BODY_TOP);
        let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
        let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
        for (rect, label) in [
            (keep, controls::KEEP_LABEL),
            (act, controls::confirm_verb(confirm)),
        ] {
            self.stamp_frame(rect);
            self.stamp_text(
                label,
                text_left(rect, label),
                rect.y + rect.h.saturating_sub(FONT_H) / 2,
            );
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

    fn stamp_keyboard(&mut self, entry: Entry, revealed: bool) {
        let layer = entry.layer();
        let shift = entry.shift();
        let cycling = entry.cycling();
        let mut candidates = [0u8; MAX_CANDIDATES];
        for cell in 0..keyboard::KEYS {
            let (Some(rect), Some(key)) = (keyboard::key_box(cell), keyboard::key_of(layer, cell))
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
                    control_label(key, layer)
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

    fn stamp_marked(&mut self, text: &[u8], left: usize, top: usize) {
        let (right, bottom) = (left + text_width(text), top + FONT_H);
        for y in top.saturating_sub(1)..=bottom {
            for x in left.saturating_sub(1)..=right {
                self.stamp_pixel(x as isize, y as isize);
            }
        }
        self.stamp_text(text, left, top);
    }
}

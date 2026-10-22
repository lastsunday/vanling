//! The Network page's setup UI: list, join, keyboard and dialogs.
use super::draw::{
    HIDE_LABEL, Mark, band_top, control_label, format_psk, format_window, text_left, text_width,
};
use super::*;

impl DisplayLight {
    /// The page's readings, one row per subject; the rows are the labels, so the title carries this
    /// board's address and the row under the network's name carries that network's.
    ///
    /// Three rows idle, four while typing: a thumb needs the digits' block and the row that ends
    /// the entry, and a key too small to hit is no keyboard. The draft takes the link address's
    /// row, so neither state moves; the phase word shares the network's row, its column fixed.
    pub(super) fn stamp_header(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
        list_len: Option<usize>,
        (width, height): (usize, usize),
    ) {
        self.stamp_text(
            b"NETWORK",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        // Which rows of the scan the list is showing, in the title row's gap: the strip is where
        // the page's action is, and a reading drawn beside the action reads as part of it. The
        // title's own row is the one row that is about the page rather than about the network, and
        // its middle is empty because the address is right-aligned against the panel.
        if let Some(rows) = list_len {
            self.stamp_text(
                format_window(setup.list_top, rows, &mut [0u8; 12]),
                block_x(Block::First) + (LIST_TITLE_GLYPHS + 1) * GLYPH_PITCH,
                overlay_row_top(0),
                width,
                height,
            );
        }
        // This board's own address on the title's row: it is the row no network ever takes over and
        // the row whose right half nothing else uses, so the one reading about the page itself sits
        // on the page's own row.
        if let Some(mac) = mac {
            let mut text = [0u8; MAC_TEXT_BYTES];
            self.stamp_text_right(
                mac_text(mac, &mut text),
                PANEL_RIGHT,
                overlay_row_top(0),
                width,
                height,
            );
        }
        if link.setup {
            // The access point the page is about while the device is being set up is this board's
            // own, named from its address: there is no scanned row to show, and a header that
            // named nothing would leave the phone's list and the panel's page with no shared word.
            //
            // Only while the access point's front is the one up: the panel's own front raises no access
            // point, and a name on that row would say a network is on the air when none is.
            if link.shows_setup_box()
                && let Some(mac) = mac
            {
                let mut name = [0u8; SSID_BYTES];
                let name = format_setup_name(Some(mac), &mut name);
                self.stamp_left(b"AP", name, Block::First, 1, width, height);
            }
        } else if let Some(ap) = shown_ap {
            self.stamp_left(
                b"AP",
                format_network_name(ap, HEADER_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                Block::First,
                1,
                width,
                height,
            );
        }
        // The phase word is drawn whether or not there is a network to put beside it, and it is the
        // page's own phase: not the link's raw one, because the page's says the last refusal until
        // something else is tried and the survey republishes the link every round. Setup mode is
        // the one mode that is not a phase of the radio, so it is said here rather than derived.
        let mut text = [0u8; 3 + 5];
        text[..3].copy_from_slice(b"ST ");
        let mut phase_buf = [0u8; 4];
        let word: &[u8] = if link.setup {
            b"SETUP"
        } else {
            format_phase(setup.phase(link.phase), &mut phase_buf)
        };
        text[3..3 + word.len()].copy_from_slice(word);
        self.stamp_text(
            &text[..3 + word.len()],
            HEADER_PHASE_X,
            overlay_row_top(1),
            width,
            height,
        );
        if let Some(ap) = shown_ap
            && !link.setup
        {
            self.stamp_text_right(
                format_bars(ap.bars(), &mut [0u8; 4]),
                PANEL_RIGHT,
                overlay_row_top(1),
                width,
                height,
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
                width,
                height,
            );
        } else {
            // `IP ` and the address as one run at the block's left edge, rather than a label and a
            // value in its own slot: that slot costs four glyphs, six pixels more than the widest
            // dotted quad leaves beside the address of the network the row names. This is the only
            // row that prints two readings at once, so it is the only row that pays for it.
            let mut quad = [0u8; 16];
            // In setup mode the address on this row is the access point's own, which is fixed: the
            // page is reached at it, so it is the one reading a person needs from the panel while
            // they are being told to open it. `link.address` is the station's and there is none.
            // The panel's own front has raised no access point, so it has no address to print and
            // says nothing rather than the label of a reading it does not have.
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
                    width,
                    height,
                );
            }
            // And the address of the network this row answers for, which is the row above's own
            // subject seen from the other end of the link: `IP ` is what this board was given, and
            // the right half is the address of the radio that gave it. Setup mode has no such
            // radio: its access point is this board.
            if let Some(ap) = shown_ap
                && !link.setup
            {
                let mut text = [0u8; MAC_TEXT_BYTES];
                self.stamp_text_right(
                    mac_text(ap.bssid, &mut text),
                    PANEL_RIGHT,
                    overlay_row_top(2),
                    width,
                    height,
                );
            }
        }
    }

    /// The page's content: the keyboard a picked row opened, or the scan's bands. A full-width name
    /// is what a user picks by, with the address on a second line because two rows can share a
    /// name. Three distinct states: not scanned, scanned empty, and the bands.
    pub(super) fn stamp_content(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        aps: Option<ApList>,
        width: usize,
        height: usize,
    ) {
        if setup.keyboard {
            if let Some(entry) = setup.entry {
                self.stamp_keyboard(entry, setup.revealed, width, height);
            }
            return;
        }
        match aps {
            None => self.stamp_band(b"NOT SCANNED", width, height),
            Some(list) if list.is_empty() => self.stamp_band(b"NO NETWORKS", width, height),
            Some(list) => {
                for band in 0..LIST_ROWS {
                    let index = usize::from(setup.list_top) + band;
                    let Some(ap) = list.get(index) else {
                        break;
                    };
                    // The row the page is trying an association with is filled, because the phase
                    // and the refusal are about *that* network: the list is where the finger comes
                    // back to when one is refused, and the interruption the panel owes it is which
                    // row it was. While a question is up it is the row the question is about
                    // instead, which is the same row for a leave and the held one for a forget.
                    let marked = setup
                        .confirm
                        .and_then(|confirm| confirm.network(link))
                        .or(setup.selected);
                    if marked.is_some_and(|marked| marked.is_same_network(&ap)) {
                        self.stamp_band_fill(band, width, height);
                    }
                    let top = band_lines_top(band);
                    self.stamp_text(
                        format_network_name(ap, NETWORK_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                        block_x(Block::First),
                        top,
                        width,
                        height,
                    );
                    // The address on a line of its own, under the name: the name already spends the
                    // band's width, and this is what tells two rows with one name apart — a mesh
                    // node and its base, a repeater. It is read against a phone's own list, so it
                    // is the whole address rather than its last octets. A name that could not be
                    // drawn *is* the address, so that row already carries it and gets no second
                    // line: one string printed twice two rows apart is one string too many.
                    if ap.ssid.as_display_text().is_some() {
                        self.stamp_text(
                            mac_text(ap.bssid, &mut [0u8; MAC_TEXT_BYTES]),
                            block_x(Block::First),
                            top + ROW_PITCH,
                            width,
                            height,
                        );
                    }
                    // And a network that asks for nothing says so, because the tap that picks it is
                    // the whole handover: a user who expected a keyboard would read the list coming
                    // back as a failure.
                    if !ap.secured {
                        self.stamp_text_right(OPEN_MARKER, OPEN_MARKER_RIGHT, top, width, height);
                    }
                    // And one this device would join again on its own is marked as saved: the hold
                    // that forgets it is a gesture nobody would try on a row that said nothing, and
                    // what the mark reports is the store rather than the attempt. The mark travels
                    // with the list, one bit per row, so the row in front of the finger is the row
                    // the mark is about.
                    if list.is_saved(index) {
                        self.stamp_text_right(SAVED_MARKER, SAVED_MARKER_RIGHT, top, width, height);
                    }
                    self.stamp_text_right(
                        format_bars(ap.bars(), &mut [0u8; 4]),
                        PANEL_RIGHT,
                        top,
                        width,
                        height,
                    );
                }
            }
        }
    }

    /// A control's outline: the four edges of the box it is drawn in.
    ///
    /// The same box the tap that acts on it is resolved through, which is the page's own rule and
    /// the reason every control here takes a [`ControlRect`] rather than four numbers.
    fn stamp_frame(&mut self, rect: ControlRect, width: usize, height: usize) {
        let (x, y) = (rect.x as isize, rect.y as isize);
        let (right, bottom) = (x + rect.w as isize - 1, y + rect.h as isize - 1);
        self.stamp_line(x, y, right, y, width, height);
        self.stamp_line(x, bottom, right, bottom, width, height);
        self.stamp_line(x, y, x, bottom, width, height);
        self.stamp_line(right, y, right, bottom, width, height);
    }

    /// Fills a box with the surface: the page's own ground turned over, which is the panel's one
    /// way of laying down a paper for ink to be drawn on.
    ///
    /// The writing itself is [`iot_core::stamp::fill_surface`]'s — a function of the buffer alone,
    /// so what a fill covers can be read back as pixels off the device. What is here is where the
    /// ground comes from: the colour the page was painted with, which `paint` recorded before any
    /// of this ran, and which a buffer the camera owns never reaches.
    fn stamp_fill(&mut self, rect: ControlRect, width: usize, height: usize) {
        let Some((fill, color)) = self.owner.painted_color() else {
            return;
        };
        fill_surface(self.frame, rect, fill, color, width, height);
    }

    /// The action strip: one box, or two while setup mode has two verbs. Hollow, like keys and
    /// question answers, because a surface means "this row" or "this question". Drawn from
    /// [`keyboard::toolbar_cell`], the same boxes the tap is resolved through.
    ///
    /// A link offers leave, setup mode close or phone, otherwise the list's round. `WAIT` is a
    /// state, not an action.
    pub(super) fn stamp_strip(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        width: usize,
        height: usize,
    ) {
        if link.setup {
            let (left, right) = setup_strip_labels(setup, link);
            match left {
                Some(label) => {
                    self.stamp_cell(
                        keyboard::toolbar_cell(keyboard::ToolbarCell::Left),
                        label,
                        width,
                        height,
                    );
                }
                // One verb, one box: a half-drawn box would read as a cell that is not offered.
                None => {
                    self.stamp_cell(keyboard::toolbar_box(), right, width, height);
                    return;
                }
            }
            self.stamp_cell(
                keyboard::toolbar_cell(keyboard::ToolbarCell::Right),
                right,
                width,
                height,
            );
            return;
        }
        self.stamp_cell(
            keyboard::toolbar_box(),
            strip_label(link, setup),
            width,
            height,
        );
    }

    /// One cell of the strip: its frame, and the word it carries centred in it.
    fn stamp_cell(&mut self, rect: ControlRect, label: &[u8], width: usize, height: usize) {
        self.stamp_frame(rect, width, height);
        self.stamp_text(
            label,
            text_left(rect, label),
            rect.y + rect.h.saturating_sub(FONT_H) / 2,
            width,
            height,
        );
    }

    /// What the box over the list holds, which is one of two things and never both.
    ///
    /// One entry point because the two are the same rectangle in the same place, and which of them
    /// is the page's is [`Part::hidden`]'s answer rather than a second one here: the box is drawn
    /// exactly when the page has something to say over the list, and a question is the finger's own
    /// screen where an attempt in flight is the radio's — so when the two are up at once the box
    /// holds the question.
    pub(super) fn stamp_box(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        mac: Option<[u8; 6]>,
        width: usize,
        height: usize,
    ) {
        if setup.confirm.is_some() {
            self.stamp_confirm(setup, link, width, height);
        } else if link.shows_setup_box() {
            self.stamp_notice(link, mac, width, height);
        } else {
            self.stamp_joining(shown_ap, width, height);
        }
    }

    /// The box a phone's setup is read in: the mode's name, the access point, and the passphrase or
    /// the fact that there is none. Two answers: close and return to the list, or open the access
    /// point for this visit. The box says which state it is in, because open means anyone in range
    /// can configure it. The address stays on the header's `IP` row.
    fn stamp_notice(
        &mut self,
        link: LinkStatus,
        mac: Option<[u8; 6]>,
        width: usize,
        height: usize,
    ) {
        self.stamp_fill(
            ControlRect {
                x: DIALOG_X,
                y: keyboard::DIALOG_TOP,
                w: DIALOG_W,
                h: DIALOG_H,
            },
            width,
            height,
        );
        let title_left = DIALOG_X + DIALOG_INSET;
        self.stamp_text(
            format_setup_notice_title(),
            title_left,
            DIALOG_TITLE_TOP,
            width,
            height,
        );
        let mut body = [0u8; SSID_BYTES];
        self.stamp_text(
            format_setup_notice_body(mac, &mut body),
            title_left,
            DIALOG_BODY_TOP,
            width,
            height,
        );
        self.stamp_text(
            format_setup_pass(link.setup_password.as_ref(), &mut body),
            title_left,
            DIALOG_BODY_TOP + ROW_PITCH,
            width,
            height,
        );
        // The two answers, in the same boxes a question's are: the passphrase's on the left, and
        // the way back on the right.
        let swap = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
        let swap_label: &[u8] = if link.setup_password.is_some() {
            NO_PASS_LABEL
        } else {
            PASS_LABEL
        };
        self.stamp_frame(swap, width, height);
        self.stamp_text(
            swap_label,
            text_left(swap, swap_label),
            swap.y + swap.h.saturating_sub(FONT_H) / 2,
            width,
            height,
        );
        let close = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
        self.stamp_frame(close, width, height);
        let label: &[u8] = b"CLOSE";
        self.stamp_text(
            label,
            text_left(close, label),
            close.y + close.h.saturating_sub(FONT_H) / 2,
            width,
            height,
        );
    }

    /// The box the radio's attempt is drawn in, on the same paper as a question and over the list.
    /// It asks nothing: the way out is the strip's own `LEAVE`. `shown_ap` keeps the box and header
    /// naming the same network, first the picked row, then the radio's report.
    fn stamp_joining(&mut self, shown_ap: Option<AccessPoint>, width: usize, height: usize) {
        self.stamp_fill(
            ControlRect {
                x: DIALOG_X,
                y: keyboard::DIALOG_TOP,
                w: DIALOG_W,
                h: DIALOG_H,
            },
            width,
            height,
        );
        let title_left = DIALOG_X + DIALOG_INSET;
        self.stamp_text(
            format_joining_title(shown_ap, &mut [0u8; DIALOG_GLYPHS]),
            title_left,
            DIALOG_TITLE_TOP,
            width,
            height,
        );
        self.stamp_text(
            format_joining_body(shown_ap),
            title_left,
            DIALOG_BODY_TOP,
            width,
            height,
        );
    }

    /// The question, drawn over the list. The box inverts the ground once and the glyphs invert it
    /// again, so they read as dark ink; the answers are hollow like keys. Nothing is drawn under
    /// it ([`Part::hidden`]), because its paper matches a marked row's band.
    fn stamp_confirm(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        width: usize,
        height: usize,
    ) {
        let Some(confirm) = setup.confirm else {
            return;
        };
        self.stamp_fill(
            ControlRect {
                x: DIALOG_X,
                y: keyboard::DIALOG_TOP,
                w: DIALOG_W,
                h: DIALOG_H,
            },
            width,
            height,
        );
        // The question names the network it is about, folded to what the line holds; the row behind
        // the box is still showing the whole name.
        let mut question = [0u8; DIALOG_GLYPHS];
        let title = format_question(confirm, link, &mut question);
        let title_left = DIALOG_X + DIALOG_INSET;
        self.stamp_text(title, title_left, DIALOG_TITLE_TOP, width, height);
        self.stamp_text(
            format_body(confirm),
            title_left,
            DIALOG_BODY_TOP,
            width,
            height,
        );
        // The two answers along the box's own bottom edge: `KEEP` and the word of the action. The
        // labels come from the geometry core resolved the taps through, so a button and the tap that
        // lands on it cannot come apart.
        let keep = keyboard::dialog_answer_box(keyboard::DialogChoice::Keep);
        let act = keyboard::dialog_answer_box(keyboard::DialogChoice::Act);
        for (rect, label) in [(keep, b"KEEP".as_slice()), (act, format_verb(confirm))] {
            self.stamp_frame(rect, width, height);
            self.stamp_text(
                label,
                text_left(rect, label),
                rect.y + rect.h.saturating_sub(FONT_H) / 2,
                width,
                height,
            );
        }
    }

    /// Fills one band of the scan list, which is how the page says which row it is trying.
    ///
    /// The band's own ground turned over, with the name and the bars stamped into it and turned
    /// back — the trade [`Self::stamp_marked`] makes for one word, over the whole row because the
    /// row is the target the finger aims at.
    fn stamp_band_fill(&mut self, band: usize, width: usize, height: usize) {
        self.stamp_fill(
            ControlRect {
                x: 0,
                y: LIST_TOP + band * LIST_ROW_H,
                w: PANEL_COLUMNS,
                h: LIST_ROW_H,
            },
            width,
            height,
        );
    }

    /// One line of text in the middle of the list's first band, which is where the page says the
    /// two things it says in place of a list.
    fn stamp_band(&mut self, text: &[u8], width: usize, height: usize) {
        self.stamp_text(text, block_x(Block::First), band_top(0), width, height);
    }

    /// Overdraw the keyboard one key at a time. Every cell comes from [`keyboard::key_box`], the
    /// same call a tap is resolved through. Keys are hollow because stamping inverts: a filled cell
    /// would hide its glyph. A key prints its cycle and boxes the next character; shift and reveal
    /// print their own state.
    fn stamp_keyboard(&mut self, entry: Entry, revealed: bool, width: usize, height: usize) {
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
                self.stamp_key(rect, label, mark, b"", width, height);
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
            // The key a repeat tap would cycle says which of its characters it is holding — and
            // only a key with more than one has anything to hold: a digit layer's key types
            // itself, so a repeat of it is a second digit and not a cycle.
            let mark = match cycling {
                Some(cycling) if cycling.face == face && run.len() > 1 => {
                    Mark::Candidate(usize::from(cycling.index))
                }
                _ => Mark::None,
            };
            self.stamp_key(rect, &[face], mark, run, width, height);
        }
    }

    /// One key: frame, face, and the characters it types under the face when there is more than
    /// one. A one-character key prints only its face, as a phone keypad does.
    fn stamp_key(
        &mut self,
        rect: ControlRect,
        face: &[u8],
        mark: Mark,
        run: &[u8],
        width: usize,
        height: usize,
    ) {
        // A one-character layer's "run" *is* the face — the key still has a candidate table, it
        // just holds one character — and a key that drew both would carry its own label twice.
        let run = if run == face { b"".as_slice() } else { run };
        self.stamp_frame(rect, width, height);

        // The face and the characters under it are one block, centred in the cell: a key that
        // placed both lines off the cell's own middle would sit its face high and its letters low,
        // which reads as one key's label spilled across two.
        let lines = usize::from(!run.is_empty());
        let block = (lines + 1) * FONT_H + lines * KEY_LINE_GAP;
        let face_top = rect.y + rect.h.saturating_sub(block) / 2;
        let face_left = text_left(rect, face);
        if mark == Mark::Face {
            self.stamp_marked(face, face_left, face_top, width, height);
        } else {
            self.stamp_text(face, face_left, face_top, width, height);
        }
        if run.is_empty() {
            return;
        }
        let run_top = face_top + FONT_H + KEY_LINE_GAP;
        let run_left = text_left(rect, run);
        self.stamp_text(run, run_left, run_top, width, height);
        if let Mark::Candidate(index) = mark {
            // That one character stamped again inside a patch: a box around the character rather
            // than around the key, because the key's other characters are still its own.
            let at = run_left + index * GLYPH_PITCH;
            self.stamp_marked(&run[index..=index], at, run_top, width, height);
        }
    }

    /// Fills the cell around `text` and stamps it back in. The patch inverts to a white ground
    /// and the glyph, inverted a second time, reads black on it — this panel has one paper, and
    /// the paper is the ground turned over, so a patch is what picks something out on it. One
    /// blank column and row of margin, because a glyph flush against the patch's edge reads as
    /// clipped.
    fn stamp_marked(&mut self, text: &[u8], left: usize, top: usize, width: usize, height: usize) {
        let (right, bottom) = (left + text_width(text), top + FONT_H);
        for y in top.saturating_sub(1)..=bottom {
            for x in left.saturating_sub(1)..=right {
                self.stamp_pixel(x as isize, y as isize, width, height);
            }
        }
        self.stamp_text(text, left, top, width, height);
    }
}

/// The strip's word outside setup mode: what a tap on it would do from here.
///
/// A link that can be left is one the tap leaves, and the cell is drawn with the word the box it
/// opens will answer with: a cell saying one thing and a button saying another is a finger pressing
/// an action it did not read. A round already on its way is `WAIT` rather than a second invitation
/// to ask for it; the rest of the time it is the round the list is waiting for. Free rather than
/// written inside the drawing so the choice has a name.
fn strip_label(link: LinkStatus, setup: NetworkSetup) -> &'static [u8] {
    if link.is_leavable() {
        format_verb(Confirm::Leave)
    } else if setup.rescan_wait {
        WAIT_LABEL
    } else {
        SCAN_LABEL
    }
}

/// What setup mode's strip says: the left half when the device offers one, and the right always.
///
/// The left half is the way out where the device has one, and the strip's own "look around" where
/// it does not — the panel's front has a live list under it, and a blank cell would be a control
/// that does nothing. The access point's front has no list to ask about: its left half is the way
/// out or nothing, and "nothing" is the one box the right verb fills.
fn setup_strip_labels(
    setup: NetworkSetup,
    link: LinkStatus,
) -> (Option<&'static [u8]>, &'static [u8]) {
    let right = match link.front {
        Front::Panel => AP_LABEL,
        Front::AccessPoint => PANEL_LABEL,
    };
    let left = match (link.front, link.setup_cancellable) {
        (_, true) => Some(CANCEL_LABEL),
        (Front::Panel, false) if setup.rescan_wait => Some(WAIT_LABEL),
        (Front::Panel, false) => Some(SCAN_LABEL),
        (Front::AccessPoint, false) => None,
    };
    (left, right)
}

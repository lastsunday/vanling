//! The Network page's setup UI: list, join, keyboard and dialogs.
use super::draw::{
    HIDE_LABEL, Mark, band_top, control_label, format_psk, format_window, text_left, text_width,
};
use super::*;

impl DisplayLight {
    /// The page's readings: the title, the phase, the network it is about, and either the address
    /// the link is on or the draft being typed.
    ///
    /// The header keeps three rows while nothing is being typed and four while something is,
    /// because the four rows a thumb on the keys needs are the digits' own block and the row that
    /// ends the entry: a key too small to hit is no keyboard at all, and the phase and the address
    /// are what the finger is not looking at. So the draft takes the row the list state gives the
    /// address, and neither state has to move for the other.
    ///
    /// The phase word is where a refusal stays: the alternative was a row of its own, and a key a
    /// thumb cannot reach is worse than a reason that shares a row with what it is about.
    pub(super) fn stamp_header(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        shown_ap: Option<AccessPoint>,
        width: usize,
        height: usize,
    ) {
        self.stamp_text(
            b"NETWORK",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        self.stamp_left(
            b"ST",
            // Not the link's raw phase: the page's own says the last refusal until something
            // else is tried, because the survey republishes the phase every round.
            format_phase(setup.phase(link.phase), &mut [0u8; 4]),
            Block::Second,
            0,
            width,
            height,
        );
        if let Some(ap) = shown_ap {
            self.stamp_left(
                b"AP",
                format_network_name(ap, NETWORK_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                Block::First,
                1,
                width,
                height,
            );
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
            self.stamp_left(
                b"IP",
                format_address(link.address, &mut [0u8; 16]),
                Block::First,
                2,
                width,
                height,
            );
        }
    }

    /// The page's content: the keyboard a picked row opened, or the scan's bands in its place.
    ///
    /// The bands are the page's content rather than a reading beside a label: name from the left
    /// edge and bars against the right, because a full-width name is the one thing a user picks a
    /// network by.
    ///
    /// Three states the list draws differently, and they are not the same state: nothing has
    /// scanned (a build that holds its own credentials and associates instead), a scan that
    /// found nothing, and the scan's own bands.
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
                // Which of the scan's networks these bands hold, because a page of a list that a
                // finger scrolled to is one the finger has to be told the position of.
                self.stamp_text(
                    format_window(setup.list_top, list.len(), &mut [0u8; 12]),
                    block_x(Block::Third),
                    overlay_row_top(0),
                    width,
                    height,
                );
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
                    let top = band_top(band);
                    self.stamp_text(
                        format_network_name(ap, NETWORK_NAME_GLYPHS, &mut [0u8; SSID_BYTES]),
                        block_x(Block::First),
                        top,
                        width,
                        height,
                    );
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

    /// The page's action strip: one control, and the word for what it would do.
    ///
    /// Hollow, like the keys and the two answers on a question, because a surface is what this page
    /// says "this row" and "this question" with: a control that filled its own box would read as one
    /// of those. Drawn from [`keyboard::toolbar_box`], the same box the tap that acts on it is
    /// resolved through.
    ///
    /// Which word is the link's answer rather than the page's: a link to leave is a link to leave,
    /// so the two never both apply and the strip never has to choose. `WAIT` is the third, and it is
    /// the strip's own state rather than a third action — a round that is already on its way.
    pub(super) fn stamp_strip(
        &mut self,
        setup: NetworkSetup,
        link: LinkStatus,
        width: usize,
        height: usize,
    ) {
        let rect = keyboard::toolbar_box();
        let label: &[u8] = if link.is_leavable() {
            LEAVE_LABEL
        } else if setup.rescan_wait {
            WAIT_LABEL
        } else {
            SCAN_LABEL
        };
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
        width: usize,
        height: usize,
    ) {
        if setup.confirm.is_some() {
            self.stamp_confirm(setup, link, width, height);
        } else {
            self.stamp_joining(shown_ap, width, height);
        }
    }

    /// The box the radio's own attempt is drawn in: what the page says between the panel's own tap
    /// and an address.
    ///
    /// The same paper as a question's, and for the same reason: it is over the list, so the list is
    /// not drawn under it. What it does not have is an answer — nothing is being asked, and the way
    /// off the attempt is the strip's own `LEAVE`, which stays where it is.
    ///
    /// The network is the page's own `shown_ap`, so the box and the header name the same one: while
    /// there is nothing on the air yet it is the row that was picked, and once the radio is on the
    /// network it is the radio's report of it.
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

    /// The question the page is asking, drawn over the list.
    ///
    /// A filled box with the words stamped into it, which is the panel's own way of drawing ink on
    /// paper: the fill is one inversion of the page's ground, so it reads as the light surface, and
    /// a glyph stamped into it is a second inversion — the ground's own colour, which reads as the
    /// dark ink. The two answers are drawn hollow inside the box, the way a key is, so the button a
    /// finger presses is the button it reads.
    ///
    /// Nothing is drawn under the box — [`Part::hidden`] — because the box's paper is the same
    /// paper as a marked row's band, so a band showing either side of it would read as part of it.
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

    /// Overdraws the keyboard, one key at a time.
    ///
    /// Every key's cell comes from [`keyboard::key_box`] — the same call the tap that types it
    /// is resolved through, so a key is exactly where it looks like it is. Drawn hollow rather
    /// than filled because stamping inverts: a filled cell with a glyph stamped into it would
    /// leave the glyph in the cell's own ink, which is to say invisible.
    ///
    /// The characters a key cycles are printed on it, and the one a repeat tap would type is
    /// boxed; the two keys that are *states* rather than characters — the shift and the reveal —
    /// say their own state on themselves. That is the whole of what a finger needs to spell a
    /// passphrase on a keypad.
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

    /// One key: its frame, then the face it is printed with, with the characters it types under
    /// the face where it types more than one.
    ///
    /// A key that types a single character prints only its face, because the face *is* what it
    /// types — which is the phone keypad's own arrangement, where the digit and its letters are
    /// one key and the face is the digit.
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

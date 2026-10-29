use iot_core::overlay::{Block, FONT_H, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS};
use iot_core::page::{Part, walk};
use iot_core::render::{MODE_BREATH, MODE_SOLID};

use crate::pages::draw::{direction_arrow, format_u16, gesture_glyph, mode_word, write_u16};
use crate::surface::Surface;
use crate::text::format_pair;

impl Surface<'_> {
    pub(crate) fn stamp_diagnostics(&mut self, live: (u8, u8)) {
        let touch = self.diagnostics.touch;
        let light = self.diagnostics.lights[usize::from(self.instance)];
        let mut buf = [0u8; 6];

        let counters: [(&[u8], u16); 6] = [
            (b"GST".as_slice(), u16::from(touch.ghost)),
            (b"2F".as_slice(), u16::from(touch.two_finger_runs)),
            (b"PRS".as_slice(), u16::from(touch.presses)),
            (b"TAP".as_slice(), u16::from(touch.taps)),
            (b"LNG".as_slice(), u16::from(touch.long_presses)),
            (b"SWP".as_slice(), u16::from(touch.swipes)),
        ];
        for (row, (label, value)) in counters.into_iter().enumerate() {
            self.stamp_left(label, format_u16(value, &mut buf), Block::First, row);
        }

        let mut dir = [b' '; 8];
        dir[0] = direction_arrow(touch.last_swipe_dir);
        dir[1] = b' ';
        let nd = write_u16(touch.last_swipe_dist, &mut dir, 2);
        self.stamp_left(b"DIR", &dir[..nd], Block::First, 8);

        let mut ref_buf = [0u8; 12];
        let origin = format_pair(touch.last_gesture_origin, &mut ref_buf);
        self.stamp_left(b"XY", origin, Block::First, 9);

        let mut end_buf = [0u8; 12];
        let end = format_pair(touch.last_gesture_end, &mut end_buf);
        self.stamp_left(b"XY2", end, Block::First, 10);

        type SlotRow = (&'static [u8], &'static [u8], &'static [u8], &'static [u8]);

        const SLOT_ROWS: [SlotRow; 2] = [
            (b"P0", b"P0G", b"P0D", b"P0V"),
            (b"P1", b"P1G", b"P1D", b"P1V"),
        ];
        for (slot, (xy_label, gesture_label, value_label, dir_label)) in
            SLOT_ROWS.iter().enumerate()
        {
            let base = 4 * slot;
            let mut xy = [0u8; 8];
            let n = match touch.points[slot] {
                Some((x, y)) => {
                    let n = write_u16(x, &mut xy, 0);
                    xy[n] = b' ';
                    write_u16(y, &mut xy, n + 1)
                }
                None => {
                    xy[0] = b'-';
                    1
                }
            };
            self.stamp_left(xy_label, &xy[..n], Block::Second, base);

            let last = touch.finger[slot];
            let mut value = [b'-'; 6];
            let value_len = if last.kind == 0 {
                1
            } else {
                let digits = format_u16(last.value, &mut buf);
                value[..digits.len()].copy_from_slice(digits);
                digits.len()
            };
            let gesture = [gesture_glyph(last.kind)];
            self.stamp_left(gesture_label, &gesture, Block::Second, base + 1);
            self.stamp_left(value_label, &value[..value_len], Block::Second, base + 2);
            self.stamp_left(
                dir_label,
                &[direction_arrow(touch.live_dir[slot])],
                Block::Second,
                base + 3,
            );
        }

        self.stamp_left(
            b"CHP",
            format_u16(u16::from(touch.chip_gesture_id), &mut buf),
            Block::Second,
            8,
        );

        self.stamp_left(b"FRM", format_u16(touch.frames, &mut buf), Block::Second, 9);

        let mut lo_hi = [0u8; 8];
        let mut n = write_u16(u16::from(light.breath.min_brightness), &mut lo_hi, 0);
        lo_hi[n] = b' ';
        n = write_u16(u16::from(light.breath.max_brightness), &mut lo_hi, n + 1);

        match light.mode {
            MODE_BREATH => {
                self.stamp_left(mode_word(light.mode), b"MOD", Block::Third, 0);
                self.stamp_left(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    Block::Third,
                    1,
                );
                self.stamp_left(&lo_hi[..n], b"LO HI", Block::Third, 2);
                self.stamp_left(
                    format_u16(light.breath.period_ms, &mut buf),
                    b"PER",
                    Block::Third,
                    3,
                );
                self.stamp_left(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    Block::Third,
                    4,
                );
                self.stamp_left(
                    format_u16(light.breath.hue_period_ms, &mut buf),
                    b"HPR",
                    Block::Third,
                    5,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.hue_span), &mut buf),
                    b"SPN",
                    Block::Third,
                    6,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.group_len), &mut buf),
                    b"GRP",
                    Block::Third,
                    7,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.saturation), &mut buf),
                    b"SAT",
                    Block::Third,
                    8,
                );
            }
            MODE_SOLID => {
                self.stamp_left(mode_word(light.mode), b"MOD", Block::Third, 0);
                self.stamp_left(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    Block::Third,
                    1,
                );
                self.stamp_left(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    Block::Third,
                    2,
                );
            }
            _ => self.stamp_left(mode_word(light.mode), b"MOD", Block::Third, 0),
        }
    }

    pub(crate) fn stamp_network(&mut self) {
        let link = self.diagnostics.link;
        // Copied rather than borrowed: the borrow would be of `self.diagnostics`, and every
        // stamp below takes `self` mutably. The audio sweep makes the same trade for the same
        // reason, and a list is 350 B against the envelope's kilobyte and a half.
        let aps = self.diagnostics.aps;
        let setup = self.diagnostics.setup;
        let shown_ap = if setup.keyboard {
            setup.selected
        } else {
            link.ap.or(setup.selected)
        };

        walk(setup, link, |part| match part {
            Part::Header => self.stamp_header(
                setup,
                link,
                shown_ap,
                self.diagnostics.mac,
                aps.filter(|list| !list.is_empty()).map(|list| list.len()),
            ),
            Part::Content => self.stamp_content(setup, link, aps),
            Part::Controls => self.stamp_strip(setup, link),
            Part::Overlay => self.stamp_box(setup, link, shown_ap, self.diagnostics.mac),
        });
    }

    pub(crate) fn stamp_splash(&mut self) {
        let lines: [&[u8]; SPLASH_LINES] = [b"VANLING", b"HOLD BOOT", b"SETUP"];
        let block = (SPLASH_LINES - 1) * SPLASH_PITCH + FONT_H;
        let top = self.height().saturating_sub(block) / 2;
        for (line, text) in lines.iter().enumerate() {
            let ink = text.len() * GLYPH_PITCH - OVERLAY_GAP;
            let left = self.width().saturating_sub(ink) / 2;
            self.stamp_text(text, left, top + line * SPLASH_PITCH);
        }
    }
}

const SPLASH_LINES: usize = 3;

const SPLASH_PITCH: usize = 3 * FONT_H;

const _: () = assert!(
    b"HOLD BOOT".len() * GLYPH_PITCH - OVERLAY_GAP <= PANEL_COLUMNS,
    "the widest splash line has to fit the panel"
);

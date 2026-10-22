//! The live pages: diagnostics, attitude, audio, speaker and network.
use super::draw::{
    direction_arrow, format_dba, format_i32, format_tenths, format_u16, gesture_glyph, mode_word,
    write_u16,
};
use super::*;

impl DisplayLight {
    /// Overdraws the ambient overlay as three blocks of inverted-pixel rows,
    /// which is what the landscape panel's extra width is for. First block: the
    /// gesture counters, `DIR`, and the two gesture coordinate pairs. Second
    /// block: the per-finger readouts and the two rows that say whether the input
    /// path is alive. Third block: the active mode's own rows, breathing rows
    /// tracking the live color.
    pub(super) fn stamp_diagnostics(&mut self, width: usize, height: usize, live: (u8, u8)) {
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
            self.stamp_left(
                label,
                format_u16(value, &mut buf),
                Block::First,
                row,
                width,
                height,
            );
        }

        let mut dir = [b' '; 8];
        dir[0] = direction_arrow(touch.last_swipe_dir);
        dir[1] = b' ';
        let nd = write_u16(touch.last_swipe_dist, &mut dir, 2);
        self.stamp_left(b"DIR", &dir[..nd], Block::First, 8, width, height);

        let mut ref_buf = [0u8; 12];
        let origin = format_pair(touch.last_gesture_origin, &mut ref_buf);
        self.stamp_left(b"XY", origin, Block::First, 9, width, height);

        let mut end_buf = [0u8; 12];
        let end = format_pair(touch.last_gesture_end, &mut end_buf);
        self.stamp_left(b"XY2", end, Block::First, 10, width, height);

        /// Overlay row label quartet for one finger slot: coordinate, resolved
        /// gesture digit, its value, and the live movement arrow.
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
            self.stamp_left(xy_label, &xy[..n], Block::Second, base, width, height);

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
            self.stamp_left(
                gesture_label,
                &gesture,
                Block::Second,
                base + 1,
                width,
                height,
            );
            self.stamp_left(
                value_label,
                &value[..value_len],
                Block::Second,
                base + 2,
                width,
                height,
            );
            self.stamp_left(
                dir_label,
                &[direction_arrow(touch.live_dir[slot])],
                Block::Second,
                base + 3,
                width,
                height,
            );
        }

        self.stamp_left(
            b"CHP",
            format_u16(u16::from(touch.chip_gesture_id), &mut buf),
            Block::Second,
            8,
            width,
            height,
        );

        self.stamp_left(
            b"FRM",
            format_u16(touch.frames, &mut buf),
            Block::Second,
            9,
            width,
            height,
        );

        let mut lo_hi = [0u8; 8];
        let mut n = write_u16(u16::from(light.breath.min_brightness), &mut lo_hi, 0);
        lo_hi[n] = b' ';
        n = write_u16(u16::from(light.breath.max_brightness), &mut lo_hi, n + 1);

        match light.mode {
            MODE_BREATH => {
                self.stamp_left(
                    mode_word(light.mode),
                    b"MOD",
                    Block::Third,
                    0,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    Block::Third,
                    1,
                    width,
                    height,
                );
                self.stamp_left(&lo_hi[..n], b"LO HI", Block::Third, 2, width, height);
                self.stamp_left(
                    format_u16(light.breath.period_ms, &mut buf),
                    b"PER",
                    Block::Third,
                    3,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    Block::Third,
                    4,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(light.breath.hue_period_ms, &mut buf),
                    b"HPR",
                    Block::Third,
                    5,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.hue_span), &mut buf),
                    b"SPN",
                    Block::Third,
                    6,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.group_len), &mut buf),
                    b"GRP",
                    Block::Third,
                    7,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(light.breath.saturation), &mut buf),
                    b"SAT",
                    Block::Third,
                    8,
                    width,
                    height,
                );
            }
            MODE_SOLID => {
                self.stamp_left(
                    mode_word(light.mode),
                    b"MOD",
                    Block::Third,
                    0,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    Block::Third,
                    1,
                    width,
                    height,
                );
                self.stamp_left(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    Block::Third,
                    2,
                    width,
                    height,
                );
            }
            _ => self.stamp_left(
                mode_word(light.mode),
                b"MOD",
                Block::Third,
                0,
                width,
                height,
            ),
        }
    }

    /// Overdraws the attitude page as two blocks of inverted-pixel rows and the
    /// dial in the third. First block: the motion readout (`ERR`/`WAIT`/`READ`
    /// when the source has produced nothing or a read failed), then one row per
    /// axis per channel — raw and scaled acceleration, raw gyro, yaw rate,
    /// roll/pitch. Second block: what the recognizer's estimators decided on,
    /// which the raw axes cannot show, then one firing count per semantic,
    /// `N/A` where the board declares no such capability. Third: the horizon
    /// dial. A block holds eighteen rows and the axis rows alone are fifteen, so
    /// the second block is where the estimators went rather than a fourth column.
    /// Labels, fields and thresholds: `docs/content/development/iot/motion.md`.
    pub(super) fn stamp_attitude(&mut self, width: usize, height: usize) {
        self.stamp_text(
            b"ATTITUDE",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        let Some(sample) = self.diagnostics.motion else {
            self.stamp_left(b"ERR", b"WAIT", Block::First, 1, width, height);
            return;
        };
        if !sample.valid {
            self.stamp_left(b"ERR", b"READ", Block::First, 1, width, height);
            return;
        }
        self.stamp_horizon(&sample, width, height);

        // The label prefix carries the channel, the unit picks the formatter, and
        // `first_row` is where the block starts.
        const BLOCKS: [(u8, usize); 5] = [(b'A', 1), (b'M', 4), (b'G', 7), (b'D', 10), (b'R', 13)];
        for &(prefix, first_row) in &BLOCKS {
            for axis in 0..3 {
                let mut buf = [0u8; 12];
                let value = match prefix {
                    b'A' => format_i32(i32::from(sample.raw_accel[axis]), &mut buf),
                    b'M' => format_i32(sample.accel_mg[axis], &mut buf),
                    b'G' => format_i32(i32::from(sample.raw_gyro[axis]), &mut buf),
                    b'D' => format_tenths(sample.gyro_dps_x10[axis], &mut buf),
                    _ => format_tenths(i32::from(sample.tilt_deg_x10[axis]), &mut buf),
                };
                self.stamp_left(
                    &[prefix, b'0' + axis as u8],
                    value,
                    Block::First,
                    first_row + axis,
                    width,
                    height,
                );
            }
        }
        self.stamp_left(
            b"ST",
            format_u16(u16::from(sample.status), &mut [0; 6]),
            Block::Second,
            0,
            width,
            height,
        );
        // What the recognizer's estimators decided on, which the raw axes cannot
        // show: a settled reading is near zero by definition, so a threshold has
        // to be read off the device. `TR` publishes the tap peak threshold's
        // square root, the other two the residuals it is judged against.
        self.stamp_left(
            b"LR",
            format_i32(sample.gravity_deviation_mg, &mut [0; 12]),
            Block::Second,
            1,
            width,
            height,
        );
        self.stamp_left(
            b"SR",
            format_i32(sample.shake_residual_mg, &mut [0; 12]),
            Block::Second,
            2,
            width,
            height,
        );
        self.stamp_left(
            b"TR",
            format_i32(sample.tap_residual_mg, &mut [0; 12]),
            Block::Second,
            3,
            width,
            height,
        );
        let counts = self.diagnostics.motion_counts;
        let caps = self.diagnostics.motion_caps;
        // Every row is always drawn, so a semantic this board never declares
        // shows `N/A`: "the stack cannot report it" has to stay distinct from
        // "its threshold never fires".
        let rows: [(&[u8], MotionCapabilities, u16); MOTION_SEMANTIC_ROWS] = [
            (b"TAP".as_slice(), MotionCapabilities::TAP, counts.taps),
            (
                b"2T".as_slice(),
                MotionCapabilities::TAP,
                counts.double_taps,
            ),
            (
                b"3T".as_slice(),
                MotionCapabilities::TAP,
                counts.triple_taps,
            ),
            (b"STL".as_slice(), MotionCapabilities::STILL, counts.still),
            (b"MOV".as_slice(), MotionCapabilities::MOVING, counts.moving),
            (
                b"ACT".as_slice(),
                MotionCapabilities::ACTIVITY,
                counts.activity,
            ),
            (b"STP".as_slice(), MotionCapabilities::STEP, counts.steps),
            (
                b"TIN".as_slice(),
                MotionCapabilities::TILT,
                counts.tilt_enters,
            ),
            (
                b"TOX".as_slice(),
                MotionCapabilities::TILT,
                counts.tilt_exits,
            ),
            (b"SHK".as_slice(), MotionCapabilities::SHAKE, counts.shakes),
            (
                b"LFT".as_slice(),
                MotionCapabilities::LIFT_PLACE,
                counts.lifts,
            ),
            (
                b"PLC".as_slice(),
                MotionCapabilities::LIFT_PLACE,
                counts.places,
            ),
            (
                b"PRT".as_slice(),
                MotionCapabilities::POSTURE,
                counts.portraits,
            ),
            (
                b"LND".as_slice(),
                MotionCapabilities::POSTURE,
                counts.landscapes,
            ),
        ];
        let mut buf = [0u8; 6];
        for (index, (label, capability, count)) in rows.into_iter().enumerate() {
            let value: &[u8] = if caps.contains(capability) {
                format_u16(count, &mut buf)
            } else {
                NOT_AVAILABLE
            };
            self.stamp_left(
                label,
                value,
                Block::Second,
                ESTIMATOR_ROWS + index,
                width,
                height,
            );
        }
    }

    /// Overdraws the Audio page: the capture phase and its wall time beside the two
    /// level rows, then the envelope as a scope sweep taking the left of the
    /// panel and the narrow readouts the right of it. The phase alone decides
    /// what is drawn, because the state layer already resolved which envelope
    /// that is. Readouts, scale and their meanings:
    /// `docs/content/development/iot/audio.md`.
    pub(super) fn stamp_audio(&mut self, width: usize, height: usize) {
        // The snapshot is read in place rather than copied into a local: the
        // embedded `AudioEnvelope` is 1.6 KB, which on this stack was a large
        // slice of what a repaint could afford. Reading through `self` in the
        // arguments below keeps that borrowing honest without the copy.
        //
        // The sweep owns the panel's left columns from `WAVE_TOP` down, so
        // everything this page reads out is either above that row or to the right
        // of it. The hint below stands in the ruler's top label's own rows, which
        // is safe only because the two are never drawn together: the hint is the
        // idle page's, and the idle page returns before the sweep.
        self.stamp_text(
            b"AUDIO",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        // The corner readout is the number this page shows a human: the
        // A-weighted sound level of the same capture, counted in the same
        // decibels the SPL column uses, with its unit spelled out so a glance
        // answers "is it loud?" without converting dBFS. It heads the readouts on
        // the right, which is the same position it held against the title.
        self.stamp_text_right(
            format_dba(
                spl(dbfs(self.diagnostics.audio.dba_lsb), SPL_OFFSET_DECIBELS),
                &mut [0u8; 12],
            ),
            PANEL_RIGHT,
            overlay_row_top(0),
            width,
            height,
        );
        self.stamp_level(
            b"PK",
            dbfs(self.diagnostics.audio.envelope.loudest()),
            1,
            width,
            height,
        );
        self.stamp_level(
            b"RMS",
            dbfs(self.diagnostics.audio.envelope.loudest_rms()),
            2,
            width,
            height,
        );
        self.stamp_left(
            b"ST",
            match self.diagnostics.audio.phase {
                AudioPhase::Idle => b"IDLE",
                AudioPhase::Recording => b"REC",
                AudioPhase::Stopped => b"STOP",
            },
            Block::RightEdge,
            1,
            width,
            height,
        );
        self.stamp_left(
            b"MS",
            format_i32(
                i32::try_from(self.diagnostics.audio.elapsed_ms).unwrap_or(i32::MAX),
                &mut [0u8; 12],
            ),
            Block::RightEdge,
            2,
            width,
            height,
        );
        // What the sweep cannot say about itself. `COL` climbing with a peak that
        // does not is a quiet room, and `COL` climbing under a −60 dBFS peak is
        // a capture path that is not delivering samples at all.
        self.stamp_left(
            b"COL",
            format_u16(
                u16::from(self.diagnostics.audio.envelope.committed()),
                &mut [0u8; 6],
            ),
            Block::RightEdge,
            3,
            width,
            height,
        );
        // How many times the capture had to be re-armed: what separates a quiet
        // room from a capture that keeps breaking, both of which draw a still
        // line.
        self.stamp_left(
            b"RST",
            format_u16(self.diagnostics.audio.restarts, &mut [0u8; 6]),
            Block::RightEdge,
            4,
            width,
            height,
        );
        // A clip is an absolute statement about the whole window, not a level reading,
        // so it says so in words and not only as a mark. It takes a row of its own
        // rather than a column beside `COL`, because the narrow block this page
        // reads in has no room for a fourth column — and a latch that appears and
        // clears must not push anything it appears next to.
        if self.diagnostics.audio.envelope.clipped() {
            self.stamp_text(
                b"CLIP",
                block_x(Block::RightEdge),
                overlay_row_top(5),
                width,
                height,
            );
        }
        if self.diagnostics.audio.phase == AudioPhase::Idle {
            self.stamp_text(
                b"TAP TO REC",
                block_x(Block::First),
                WAVE_TOP as usize,
                width,
                height,
            );
            return;
        }
        let envelope = &self.diagnostics.audio.envelope;
        let peaks = envelope.weighted_released_peaks();
        let levels = envelope.weighted_rms_columns();
        let clip_age = envelope.clipped_age();
        self.stamp_sweep(&peaks, &levels, clip_age, width, height);
    }

    /// Overdraws the Speaker page: the phase, the sound a tap would play, the
    /// mute latch and the two tap tallies. Unlike the Audio page there is
    /// nothing here to animate — a sound is a one-shot and the state layer owns
    /// the whole of it — so the page is a readout plus its two gestures, spelled
    /// out so neither has to be discovered.
    pub(super) fn stamp_speaker(&mut self, width: usize, height: usize) {
        let playback = self.diagnostics.playback;
        self.stamp_text(
            b"SPKR",
            block_x(Block::First),
            overlay_row_top(0),
            width,
            height,
        );
        self.stamp_left(
            b"ST",
            match playback.phase {
                PlaybackPhase::Idle => b"IDLE",
                PlaybackPhase::Playing => b"PLAY",
            },
            Block::First,
            1,
            width,
            height,
        );
        self.stamp_left(
            b"SRC",
            label_of(playback.sound),
            Block::First,
            2,
            width,
            height,
        );
        // The latch is four glyphs, which is a glyph more than a block's label
        // slot, so it heads the second block rather than sitting beside the row
        // it latches: a latch that appears and clears must not push anything it
        // appears next to.
        if playback.muted {
            self.stamp_text(
                b"MUTE",
                block_x(Block::Second),
                overlay_row_top(1),
                width,
                height,
            );
        }
        self.stamp_left(
            b"PLY",
            format_u16(playback.plays, &mut [0u8; 6]),
            Block::First,
            3,
            width,
            height,
        );
        self.stamp_left(
            b"DRP",
            format_u16(playback.dropped, &mut [0u8; 6]),
            Block::First,
            4,
            width,
            height,
        );
        let hint = overlay_row_top(6);
        self.stamp_text(b"TAP PLAY", block_x(Block::First), hint, width, height);
        self.stamp_text(
            b"HOLD MUTE",
            block_x(Block::First),
            hint + ROW_PITCH,
            width,
            height,
        );
    }

    /// Overdraws the Network page, one part at a time.
    ///
    /// Which parts there are and what order they are drawn in are [`walk`]'s, because which of them
    /// covers which is a fact about the page rather than about the panel — and the panel is the only
    /// place a wrong answer to it is invisible. All that is left here is what each part looks like.
    pub(super) fn stamp_network(&mut self, width: usize, height: usize) {
        let link = self.diagnostics.link;
        // Copied rather than borrowed: the borrow would be of `self.diagnostics`, and every
        // stamp below takes `self` mutably. The audio sweep makes the same trade for the same
        // reason, and a list is 350 B against the envelope's kilobyte and a half.
        let aps = self.diagnostics.aps;
        let setup = self.diagnostics.setup;
        // The network the page is about. While a draft is open that is the row it is for, because
        // the draft is what the page is being used for; otherwise it is the link's own wherever
        // there is one, because a link that is up is the truth about where the device is, and the
        // picked row while nothing is up, because that is all there is to show.
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
                (width, height),
            ),
            Part::Content => self.stamp_content(setup, link, aps, width, height),
            Part::Controls => self.stamp_strip(setup, link, width, height),
            Part::Overlay => {
                self.stamp_box(setup, link, shown_ap, self.diagnostics.mac, width, height)
            }
        });
    }

    /// The splash page: its three lines, centred, and nothing else.
    ///
    /// The page is the whole of the window in which the button means setup mode (see
    /// [`DisplayPage::Splash`]), so its content is that window's own rule in the order a person
    /// needs it — the device, the verb, and what the verb asks for. No readout, because a device
    /// that has just booted has nothing to read yet.
    pub(super) fn stamp_splash(&mut self, width: usize, height: usize) {
        // The three lines held on the stack rather than in a table: a table of fat pointers is a
        // relocatable object in DRAM, and the page is worth no static memory at all.
        let lines: [&[u8]; SPLASH_LINES] = [b"VANLING", b"HOLD BOOT", b"SETUP"];
        let block = (SPLASH_LINES - 1) * SPLASH_PITCH + FONT_H;
        let top = height.saturating_sub(block) / 2;
        for (line, text) in lines.iter().enumerate() {
            let ink = text.len() * GLYPH_PITCH - OVERLAY_GAP;
            let left = width.saturating_sub(ink) / 2;
            self.stamp_text(text, left, top + line * SPLASH_PITCH, width, height);
        }
    }
}

/// How many lines the splash draws, top to bottom: the device, the button's verb, and what that
/// verb asks for.
const SPLASH_LINES: usize = 3;

/// Distance between the tops of two splash lines: wider than the readout grid's row pitch, because
/// the page has the panel to itself and these three lines are its whole content.
const SPLASH_PITCH: usize = 3 * FONT_H;

const _: () = assert!(
    b"HOLD BOOT".len() * GLYPH_PITCH - OVERLAY_GAP <= PANEL_COLUMNS,
    "the widest splash line has to fit the panel"
);

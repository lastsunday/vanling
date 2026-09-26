use alloc::vec::Vec;
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::input::{
    FINGER_DOUBLE_TAP, FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP, FINGER_TRIPLE_TAP,
};
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, rgb_hue, scale_brightness, vertical_brightness,
};
use iot_core::drivers::motion::{MotionCapabilities, MotionSample};
use iot_core::horizon::{SCALE, horizon};
use iot_core::render::{MODE_BREATH, MODE_SOLID};
use iot_core::state::DisplayPage;

use crate::components::backlight::Backlight;
use crate::components::st7789::St7789;

/// Minimum per-channel color delta that warrants a full-frame repaint.
const REPAINT_STEP: u8 = 12;

/// Debug overlay toggle. A compile-time switch (not a Cargo feature): the
/// ambient diagnostics rows and last-touch coordinates are a field/development
/// aid, so production builds keep them dark by setting this to `false`. The
/// attitude page remains visible in either mode.
const DEBUG_DIAGNOSTICS: bool = true;

/// Left edge of the diagnostics rows, in panel columns.
const OVERLAY_X: usize = 10;
/// Top edge of the first diagnostics row, in panel rows.
const OVERLAY_Y: usize = 8;
/// Vertical gap between diagnostics rows.
const OVERLAY_ROW_GAP: usize = 6;
/// Horizontal gap between glyph columns.
const OVERLAY_GAP: usize = 4;
/// Glyph cell: rows, then columns, of the 5×7 [`FONT`].
const FONT_H: usize = 7;
const FONT_W: usize = 5;
/// Value column of the touch/gesture column: a 3-glyph label slot plus one
/// space glyph between label and value.
const LEFT_VALUE_X: usize = OVERLAY_X + 4 * (FONT_W + OVERLAY_GAP);
/// Value column of the mode column: its label right-aligns into the fixed
/// five-glyph slot that ends one space before it, so every value starts on
/// one vertical line. The widest entry (`LO HI 140 255`) still fits the
/// 240-column panel.
const RIGHT_VALUE_X: usize = 174;
/// Stand-in value for a semantic the board does not declare, matching the
/// `ERR`/`WAIT` sentinels the attitude page already uses for missing data.
const NOT_AVAILABLE: &[u8] = b"N/A";

/// Attitude dial center, in panel columns/rows. Anchored in the free
/// bottom-right corner: below the mode column's last row (`y ≈ 184`) and
/// right of the touch column's widest value (`x ≈ 145`), so a radius of
/// [`HORIZON_R`] stays clear of both columns on the 240×320 panel.
const HORIZON_CX: usize = 192;
const HORIZON_CY: usize = 252;
const HORIZON_R: usize = 40;

/// Printable ASCII 5×7 glyphs (`0x20`–`0x7E`, 95 × 5 column bytes) in
/// column-major order, bit 0 of each byte the top row — the classic
/// Adafruit GFX `glcdfont` layout, drawn the way the panel warms up (a byte
/// is a pixel column). Indexing: `FONT[(ch - 0x20) * 5 ..][..5]`.
///
/// Table transcribed from Adafruit GFX 1.x `glcdfont.c`, BSD-3-Clause:
/// <https://github.com/adafruit/Adafruit-GFX-Library/blob/master/glcdfont.c>
const FONT: [u8; 95 * 5] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5F, 0x00, 0x00, 0x00, 0x07, 0x00, 0x07, 0x00, 0x14,
    0x7F, 0x14, 0x7F, 0x14, 0x24, 0x2A, 0x7F, 0x2A, 0x12, 0x23, 0x13, 0x08, 0x64, 0x62, 0x36, 0x49,
    0x56, 0x20, 0x50, 0x00, 0x08, 0x07, 0x03, 0x00, 0x00, 0x1C, 0x22, 0x41, 0x00, 0x00, 0x41, 0x22,
    0x1C, 0x00, 0x2A, 0x1C, 0x7F, 0x1C, 0x2A, 0x08, 0x08, 0x3E, 0x08, 0x08, 0x00, 0x80, 0x70, 0x30,
    0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x00, 0x60, 0x60, 0x00, 0x20, 0x10, 0x08, 0x04, 0x02,
    0x3E, 0x51, 0x49, 0x45, 0x3E, 0x00, 0x42, 0x7F, 0x40, 0x00, 0x72, 0x49, 0x49, 0x49, 0x46, 0x21,
    0x41, 0x49, 0x4D, 0x33, 0x18, 0x14, 0x12, 0x7F, 0x10, 0x27, 0x45, 0x45, 0x45, 0x39, 0x3C, 0x4A,
    0x49, 0x49, 0x31, 0x41, 0x21, 0x11, 0x09, 0x07, 0x36, 0x49, 0x49, 0x49, 0x36, 0x46, 0x49, 0x49,
    0x29, 0x1E, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x40, 0x34, 0x00, 0x00, 0x00, 0x08, 0x14, 0x22,
    0x41, 0x14, 0x14, 0x14, 0x14, 0x14, 0x00, 0x41, 0x22, 0x14, 0x08, 0x02, 0x01, 0x59, 0x09, 0x06,
    0x3E, 0x41, 0x5D, 0x59, 0x4E, 0x7C, 0x12, 0x11, 0x12, 0x7C, 0x7F, 0x49, 0x49, 0x49, 0x36, 0x3E,
    0x41, 0x41, 0x41, 0x22, 0x7F, 0x41, 0x41, 0x41, 0x3E, 0x7F, 0x49, 0x49, 0x49, 0x41, 0x7F, 0x09,
    0x09, 0x09, 0x01, 0x3E, 0x41, 0x41, 0x51, 0x73, 0x7F, 0x08, 0x08, 0x08, 0x7F, 0x00, 0x41, 0x7F,
    0x41, 0x00, 0x20, 0x40, 0x41, 0x3F, 0x01, 0x7F, 0x08, 0x14, 0x22, 0x41, 0x7F, 0x40, 0x40, 0x40,
    0x40, 0x7F, 0x02, 0x1C, 0x02, 0x7F, 0x7F, 0x04, 0x08, 0x10, 0x7F, 0x3E, 0x41, 0x41, 0x41, 0x3E,
    0x7F, 0x09, 0x09, 0x09, 0x06, 0x3E, 0x41, 0x51, 0x21, 0x5E, 0x7F, 0x09, 0x19, 0x29, 0x46, 0x26,
    0x49, 0x49, 0x49, 0x32, 0x03, 0x01, 0x7F, 0x01, 0x03, 0x3F, 0x40, 0x40, 0x40, 0x3F, 0x1F, 0x20,
    0x40, 0x20, 0x1F, 0x3F, 0x40, 0x38, 0x40, 0x3F, 0x63, 0x14, 0x08, 0x14, 0x63, 0x03, 0x04, 0x78,
    0x04, 0x03, 0x61, 0x59, 0x49, 0x4D, 0x43, 0x00, 0x7F, 0x41, 0x41, 0x41, 0x02, 0x04, 0x08, 0x10,
    0x20, 0x00, 0x41, 0x41, 0x41, 0x7F, 0x04, 0x02, 0x01, 0x02, 0x04, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x00, 0x03, 0x07, 0x08, 0x00, 0x20, 0x54, 0x54, 0x78, 0x40, 0x7F, 0x28, 0x44, 0x44, 0x38, 0x38,
    0x44, 0x44, 0x44, 0x28, 0x38, 0x44, 0x44, 0x28, 0x7F, 0x38, 0x54, 0x54, 0x54, 0x18, 0x00, 0x08,
    0x7E, 0x09, 0x02, 0x18, 0xA4, 0xA4, 0x9C, 0x78, 0x7F, 0x08, 0x04, 0x04, 0x78, 0x00, 0x44, 0x7D,
    0x40, 0x00, 0x20, 0x40, 0x40, 0x3D, 0x00, 0x7F, 0x10, 0x28, 0x44, 0x00, 0x00, 0x41, 0x7F, 0x40,
    0x00, 0x7C, 0x04, 0x78, 0x04, 0x78, 0x7C, 0x08, 0x04, 0x04, 0x78, 0x38, 0x44, 0x44, 0x44, 0x38,
    0xFC, 0x18, 0x24, 0x24, 0x18, 0x18, 0x24, 0x24, 0x18, 0xFC, 0x7C, 0x08, 0x04, 0x04, 0x08, 0x48,
    0x54, 0x54, 0x54, 0x24, 0x04, 0x04, 0x3F, 0x44, 0x24, 0x3C, 0x40, 0x40, 0x20, 0x7C, 0x1C, 0x20,
    0x40, 0x20, 0x1C, 0x3C, 0x40, 0x30, 0x40, 0x3C, 0x44, 0x28, 0x10, 0x28, 0x44, 0x4C, 0x90, 0x90,
    0x90, 0x7C, 0x44, 0x64, 0x54, 0x4C, 0x44, 0x00, 0x08, 0x36, 0x41, 0x00, 0x00, 0x00, 0x77, 0x00,
    0x00, 0x00, 0x41, 0x36, 0x08, 0x00, 0x02, 0x01, 0x02, 0x04, 0x02,
];

/// ST7789 panel framed as an `RgbLight` surface.
pub struct DisplayLight {
    /// Which light surface this panel renders (wiring order); the diagnostics
    /// snapshot carries every surface, so the panel reads its own row.
    instance: u8,
    panel: St7789,
    frame: Vec<u8>,
    screen_color: Option<(Fill, Rgb)>,
    backlight: Backlight,
    /// Diagnostic overlay payload: touch counters, light mode, and motion (see
    /// [`Diagnostics`]). A bump repaints via [`Self::paint`], which the
    /// `screen_color` guard would otherwise skip.
    diagnostics: Diagnostics,
}

impl DisplayLight {
    /// Raise the backlight to full as part of bring-up; the LEDC PWM channel
    /// keeps the active-low pin pulled low (bright) until a renderer write.
    pub fn new(instance: u8, panel: St7789, mut backlight: Backlight) -> Self {
        backlight.set_level_pct(100);
        log::info!("[DISPLAY] backlight raised");
        Self {
            instance,
            panel,
            frame: Vec::new(),
            screen_color: None,
            backlight,
            diagnostics: Diagnostics::default(),
        }
    }

    fn frame_bytes(&self) -> usize {
        usize::from(self.panel.width()) * usize::from(self.panel.height()) * 2
    }
}

impl RgbLight for DisplayLight {
    fn repaint_step(&self) -> u8 {
        REPAINT_STEP
    }

    fn set_backlight(&mut self, level_pct: u8) {
        self.backlight.set_level_pct(level_pct);
    }

    fn set_fill(&mut self, fill: Fill, color: Rgb) {
        if self.screen_color == Some((fill, color)) {
            return;
        }
        self.paint(fill, color);
    }
}

impl DiagnosticsSink for DisplayLight {
    fn consume(&mut self, diagnostics: &Diagnostics) {
        if self.diagnostics == *diagnostics {
            return;
        }
        let page_changed = self.diagnostics.page != diagnostics.page;
        self.diagnostics = *diagnostics;
        if !DEBUG_DIAGNOSTICS && self.diagnostics.page != DisplayPage::Attitude && !page_changed {
            return;
        }
        if let Some((fill, color)) = self.screen_color {
            self.paint(fill, color);
        }
    }
}

impl DisplayLight {
    /// Paints `fill`/`color`, stamps the diagnostics rows into the corner, and ships
    /// the frame. Always paints; callers guard for repaint skipping.
    fn paint(&mut self, fill: Fill, color: Rgb) {
        self.screen_color = Some((fill, color));
        self.frame.resize(self.frame_bytes(), 0);

        let height = self.panel.height();
        let colors = |row: u16| match fill {
            Fill::Uniform => color,
            Fill::VerticalGradient => scale_brightness(color, vertical_brightness(row, height)),
        };
        let width = usize::from(self.panel.width());
        for (row, px) in self.frame.chunks_exact_mut(width * 2).enumerate() {
            let rgb565 = rgb565(colors(row as u16));
            let hi = (rgb565 >> 8) as u8;
            let lo = rgb565 as u8;
            for pixel in px.chunks_exact_mut(2) {
                pixel[0] = hi;
                pixel[1] = lo;
            }
        }

        // In breathing mode the brightness/hue rows track the color actually
        // being driven this frame, so the digits undulate with the animation;
        // other modes show the snapshot's configured values. The panel reads
        // its own surface out of the device snapshot.
        let mine = self.diagnostics.lights[usize::from(self.instance)];
        let live = if mine.mode == MODE_BREATH {
            (brightness_of(color), rgb_hue(color))
        } else {
            (mine.brightness, mine.hue)
        };
        if self.diagnostics.page == DisplayPage::Attitude {
            self.stamp_attitude(width, usize::from(height));
        } else if DEBUG_DIAGNOSTICS {
            self.stamp_diagnostics(width, usize::from(height), live);
        }

        if let Err(e) = self.panel.write_frame(&self.frame) {
            log::error!("[DISPLAY] frame write failed: {e:?}");
        }
    }

    /// Overdraws the corner as two columns of inverted-pixel rows, visible on
    /// any fill. Touch column: the `GST`/`2F`/`PRS`/`TAP`/`DBL`/`3T`/`LNG`/`SWP`
    /// counters, `DIR` (last swipe arrow+distance), per-finger `P0*`/`P1*`
    /// rows (live coords, gesture digit `1`–`5` with value, live arrow),
    /// `XY`/`XY2` (last origin/trailing point), `CHP` (raw `GESTURE_ID`) and
    /// `FRM` (applied-frame heartbeat — a frozen `FRM` under a held finger
    /// tells "no frames arrived" from "coordinates did not move"). Mode column:
    /// the active mode's own rows (`BRI`/`LO HI`/`PER`/`HUE`/`HPR`/`SPN`/
    /// `GRP`/`SAT` breathing, `MOD`/`BRI`/`HUE` solid, `MOD` off); in breathing
    /// `BRI`/`HUE` show the live instantaneous color, so the digits undulate.
    fn stamp_diagnostics(&mut self, width: usize, height: usize, live: (u8, u8)) {
        let touch = self.diagnostics.touch;
        let light = self.diagnostics.lights[usize::from(self.instance)];
        let mut buf = [0u8; 6];

        let counters: [(&[u8], u16); 8] = [
            (b"GST".as_slice(), u16::from(touch.ghost)),
            (b"2F".as_slice(), u16::from(touch.two_finger_runs)),
            (b"PRS".as_slice(), u16::from(touch.presses)),
            (b"TAP".as_slice(), u16::from(touch.taps)),
            (b"DBL".as_slice(), u16::from(touch.double_taps)),
            (b"3T".as_slice(), u16::from(touch.triple_taps)),
            (b"LNG".as_slice(), u16::from(touch.long_presses)),
            (b"SWP".as_slice(), u16::from(touch.swipes)),
        ];
        for (row, (label, value)) in counters.into_iter().enumerate() {
            self.stamp_left(label, format_u16(value, &mut buf), row, width, height);
        }

        let mut dir = [b' '; 8];
        dir[0] = direction_arrow(touch.last_swipe_dir);
        dir[1] = b' ';
        let nd = write_u16(touch.last_swipe_dist, &mut dir, 2);
        self.stamp_left(b"DIR", &dir[..nd], 8, width, height);

        let mut ref_buf = [0u8; 8];
        let origin = format_pair(touch.last_gesture_origin, &mut ref_buf);
        self.stamp_left(b"XY", origin, 9, width, height);

        let mut end_buf = [0u8; 8];
        let end = format_pair(touch.last_gesture_end, &mut end_buf);
        self.stamp_left(b"XY2", end, 10, width, height);

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
            let base = 11 + 4 * slot;
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
            self.stamp_left(xy_label, &xy[..n], base, width, height);

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
            self.stamp_left(gesture_label, &gesture, base + 1, width, height);
            self.stamp_left(value_label, &value[..value_len], base + 2, width, height);
            self.stamp_left(
                dir_label,
                &[direction_arrow(touch.live_dir[slot])],
                base + 3,
                width,
                height,
            );
        }

        self.stamp_left(
            b"CHP",
            format_u16(u16::from(touch.chip_gesture_id), &mut buf),
            19,
            width,
            height,
        );

        self.stamp_left(
            b"FRM",
            format_u16(touch.frames, &mut buf),
            20,
            width,
            height,
        );

        let mut lo_hi = [0u8; 8];
        let mut n = write_u16(u16::from(light.breath.min_brightness), &mut lo_hi, 0);
        lo_hi[n] = b' ';
        n = write_u16(u16::from(light.breath.max_brightness), &mut lo_hi, n + 1);

        match light.mode {
            MODE_BREATH => {
                self.stamp_right(mode_word(light.mode), b"MOD", 0, width, height);
                self.stamp_right(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    1,
                    width,
                    height,
                );
                self.stamp_right(&lo_hi[..n], b"LO HI", 2, width, height);
                self.stamp_right(
                    format_u16(light.breath.period_ms, &mut buf),
                    b"PER",
                    3,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    4,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(light.breath.hue_period_ms, &mut buf),
                    b"HPR",
                    5,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(light.breath.hue_span), &mut buf),
                    b"SPN",
                    6,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(light.breath.group_len), &mut buf),
                    b"GRP",
                    7,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(light.breath.saturation), &mut buf),
                    b"SAT",
                    8,
                    width,
                    height,
                );
            }
            MODE_SOLID => {
                self.stamp_right(mode_word(light.mode), b"MOD", 0, width, height);
                self.stamp_right(
                    format_u16(u16::from(live.0), &mut buf),
                    b"BRI",
                    1,
                    width,
                    height,
                );
                self.stamp_right(
                    format_u16(u16::from(live.1), &mut buf),
                    b"HUE",
                    2,
                    width,
                    height,
                );
            }
            _ => self.stamp_right(mode_word(light.mode), b"MOD", 0, width, height),
        }
    }

    /// Overdraws the corner as two columns of inverted-pixel rows. Left column:
    /// the raw and scaled motion readout — `A0`–`A2` raw accelerometer,
    /// `M0`–`M2` milli-g, `G0`–`G2` raw gyroscope, `D0`–`D2` deci-dps, `R0`–`R2`
    /// tilt, `ST` engine status — or `ERR`/`WAIT`/`READ` when the source has
    /// produced nothing or a read failed. Right column: one row per semantic
    /// holding how many times it has fired since boot, `N/A` where the board
    /// declares no such capability.
    fn stamp_attitude(&mut self, width: usize, height: usize) {
        let top = OVERLAY_Y;
        self.stamp_text(b"ATTITUDE", OVERLAY_X, top, width, height);
        let Some(sample) = self.diagnostics.motion else {
            self.stamp_left(b"ERR", b"WAIT", 2, width, height);
            return;
        };
        if !sample.valid {
            self.stamp_left(b"ERR", b"READ", 2, width, height);
            return;
        }
        self.stamp_horizon(&sample, width, height);

        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let raw_accel = format_i32(i32::from(sample.raw_accel[axis]), &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"A0",
                    1 => b"A1",
                    _ => b"A2",
                },
                raw_accel,
                axis + 1,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let accel_mg = format_i32(sample.accel_mg[axis], &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"M0",
                    1 => b"M1",
                    _ => b"M2",
                },
                accel_mg,
                axis + 4,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let raw_gyro = format_i32(i32::from(sample.raw_gyro[axis]), &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"G0",
                    1 => b"G1",
                    _ => b"G2",
                },
                raw_gyro,
                axis + 7,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let gyro_dps = format_tenths(sample.gyro_dps_x10[axis], &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"D0",
                    1 => b"D1",
                    _ => b"D2",
                },
                gyro_dps,
                axis + 10,
                width,
                height,
            );
        }
        for axis in 0..3 {
            let mut buf = [0u8; 12];
            let tilt = format_tenths(i32::from(sample.tilt_deg_x10[axis]), &mut buf);
            self.stamp_left(
                match axis {
                    0 => b"R0",
                    1 => b"R1",
                    _ => b"R2",
                },
                tilt,
                axis + 13,
                width,
                height,
            );
        }
        self.stamp_left(
            b"ST",
            format_u16(u16::from(sample.status), &mut [0; 6]),
            16,
            width,
            height,
        );
        // What the recognizer's own estimators had left over, which is what
        // the thresholds actually decide on. The raw axes above cannot show
        // that, because a settled reading is near zero by definition and a
        // threshold has to be read off the device rather than inferred from
        // another flash. `LR` is how far the measured magnitude sits from one
        // g, the still band the lift/place pair measures; `SR` is the shake
        // estimate's leftover, taken as a length; `TR` is the same for the tap
        // peak bar, the root of the squared gravity-removed residual.
        self.stamp_left(
            b"LR",
            format_i32(sample.gravity_deviation_mg, &mut [0; 12]),
            17,
            width,
            height,
        );
        self.stamp_left(
            b"SR",
            format_i32(sample.shake_residual_mg, &mut [0; 12]),
            18,
            width,
            height,
        );
        self.stamp_left(
            b"TR",
            format_i32(sample.tap_residual_mg, &mut [0; 12]),
            19,
            width,
            height,
        );
        let counts = self.diagnostics.motion_counts;
        let caps = self.diagnostics.motion_caps;
        // The right column is free on this page, so it carries a count per
        // semantic rather than the latest one. Every row is always drawn: a
        // semantic this board never declares shows `N/A`, which keeps "the
        // stack cannot report it" visually distinct from "its threshold never
        // fires" — the distinction a threshold sweep is reading for.
        let rows: [(&[u8], MotionCapabilities, u16); 14] = [
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
        for (row, (label, capability, count)) in rows.into_iter().enumerate() {
            let value: &[u8] = if caps.contains(capability) {
                format_u16(count, &mut buf)
            } else {
                NOT_AVAILABLE
            };
            self.stamp_right(value, label, row, width, height);
        }
    }

    /// Writes one touch-column row: label at [`OVERLAY_X`], one space glyph,
    /// then the value at [`LEFT_VALUE_X`].
    fn stamp_left(&mut self, label: &[u8], value: &[u8], row: usize, width: usize, height: usize) {
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        self.stamp_text(label, OVERLAY_X, top, width, height);
        self.stamp_text(value, LEFT_VALUE_X, top, width, height);
    }

    /// Overdraws the attitude dial — the inverted counterpart of `R0`–`R2` —
    /// with ring, fixed wing/bank references, and a `-roll`/`pitch` horizon.
    fn stamp_horizon(&mut self, sample: &MotionSample, width: usize, height: usize) {
        let geo = horizon(sample.tilt_deg_x10[0], sample.tilt_deg_x10[1]);
        let cx = HORIZON_CX as isize;
        let cy = HORIZON_CY as isize;
        let r = HORIZON_R as isize;

        self.stamp_circle(cx, cy, r, width, height);

        // Fixed airframe reference: top bank index and center wing bar.
        self.stamp_line(cx - 4, cy - r + 2, cx + 4, cy - r + 2, width, height);
        self.stamp_line(cx - 10, cy, cx + 10, cy, width, height);

        // Horizon line tilted `-roll`, shifted by the pitch fraction of `r`.
        let half_x = geo.unit.0 as isize * r / SCALE as isize;
        let half_y = geo.unit.1 as isize * r / SCALE as isize;
        let offset = geo.offset as isize * r / SCALE as isize;
        self.stamp_line(
            cx - half_x,
            cy - half_y + offset,
            cx + half_x,
            cy + half_y + offset,
            width,
            height,
        );
    }

    /// Writes one mode-column row: value at [`RIGHT_VALUE_X`], the label
    /// right-aligned into the fixed five-glyph slot ending one space before
    /// it — so every label (`MOD` up to `LO HI`) floats right next to its
    /// value and the numbers line up in one column.
    fn stamp_right(&mut self, value: &[u8], label: &[u8], row: usize, width: usize, height: usize) {
        let pitch = FONT_W + OVERLAY_GAP;
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        self.stamp_text(value, RIGHT_VALUE_X, top, width, height);
        self.stamp_text(
            label,
            RIGHT_VALUE_X - (label.len() + 1) * pitch,
            top,
            width,
            height,
        );
    }

    fn stamp_text(&mut self, text: &[u8], left: usize, top: usize, width: usize, height: usize) {
        for (glyph, &ch) in text.iter().enumerate() {
            self.stamp_char(
                ch,
                left + glyph * (FONT_W + OVERLAY_GAP),
                top,
                width,
                height,
            );
        }
    }

    /// Overdraws one 5×7 `ch` at `left`/`top` with inverted pixels; bytes
    /// outside the printable ASCII block draw nothing.
    fn stamp_char(&mut self, ch: u8, left: usize, top: usize, width: usize, height: usize) {
        if !(0x20..=0x7E).contains(&ch) {
            return;
        }
        let base = usize::from(ch - 0x20) * FONT_W;
        for (col, &bits) in FONT[base..base + FONT_W].iter().enumerate() {
            for row in 0..FONT_H {
                if bits & (1 << row) == 0 {
                    continue;
                }
                self.stamp_pixel(
                    left as isize + col as isize,
                    top as isize + row as isize,
                    width,
                    height,
                );
            }
        }
    }

    /// Inverts one pixel: the shared ink for glyphs and the dial.
    fn stamp_pixel(&mut self, x: isize, y: isize, width: usize, height: usize) {
        if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
            return;
        }
        let offset = (y as usize * width + x as usize) * 2;
        let packed = u16::from_be_bytes([self.frame[offset], self.frame[offset + 1]]);
        self.frame[offset..offset + 2].copy_from_slice(&invert_rgb565(packed).to_be_bytes());
    }

    /// Inverts the straight run from `(x0, y0)` to `(x1, y1)` (Bresenham).
    fn stamp_line(
        &mut self,
        x0: isize,
        y0: isize,
        x1: isize,
        y1: isize,
        width: usize,
        height: usize,
    ) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.stamp_pixel(x, y, width, height);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Inverts the perimeter of the circle at `(cx, cy)` with radius `r`
    /// (midpoint algorithm).
    fn stamp_circle(&mut self, cx: isize, cy: isize, r: isize, width: usize, height: usize) {
        let mut x = r;
        let mut y = 0;
        let mut err = 1 - r;
        while x >= y {
            for (dx, dy) in [
                (x, y),
                (-x, y),
                (x, -y),
                (-x, -y),
                (y, x),
                (-y, x),
                (y, -x),
                (-y, -x),
            ] {
                self.stamp_pixel(cx + dx, cy + dy, width, height);
            }
            y += 1;
            if err <= 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }
}

/// Decimal ASCII digits of `value` (no leading zeros) written into the start
/// of `buf`, returned as the filled prefix.
fn format_i32(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let negative = value < 0;
    let magnitude = if negative {
        -(value as i64) as u64
    } else {
        value as u64
    };
    let mut n = 0;
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    n = write_u64(magnitude, buf, n);
    &buf[..n]
}

fn format_tenths(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let negative = value < 0;
    let magnitude = if negative {
        -(value as i64) as u64
    } else {
        value as u64
    };
    let mut n = 0;
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    n = write_u64(magnitude / 10, buf, n);
    buf[n] = b'.';
    buf[n + 1] = b'0' + (magnitude % 10) as u8;
    &buf[..n + 2]
}

fn write_u64(mut value: u64, buf: &mut [u8], mut n: usize) -> usize {
    let mut tmp = [0u8; 20];
    let mut len = 0;
    loop {
        tmp[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in tmp[..len].iter().rev() {
        buf[n] = digit;
        n += 1;
    }
    n
}

fn format_u16(value: u16, buf: &mut [u8; 6]) -> &[u8] {
    let n = write_u16(value, buf, 0);
    &buf[..n]
}

/// Appends the decimal ASCII digits of `value` to `buf` starting at offset
/// `n`; returns the new length.
fn write_u16(mut value: u16, buf: &mut [u8], mut n: usize) -> usize {
    let mut tmp = [0u8; 5];
    let mut len = 0;
    loop {
        tmp[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in tmp[..len].iter().rev() {
        buf[n] = digit;
        n += 1;
    }
    n
}

/// Renders an optional coordinate pair as `x y` (single dash when unset) for
/// the `XY`/`XY2` overlay rows.
fn format_pair(pair: Option<(u16, u16)>, buf: &mut [u8; 8]) -> &[u8] {
    match pair {
        Some((x, y)) => {
            let n = write_u16(x, buf, 0);
            buf[n] = b' ';
            let end = write_u16(y, buf, n + 1);
            &buf[..end]
        }
        None => {
            buf[0] = b'-';
            &buf[..1]
        }
    }
}

/// The value channel `hsv_to_rgb` encoded a color with — its brightest
/// channel — back out of the driven frame, for the live brightness readout.
fn brightness_of(Rgb(r, g, b): Rgb) -> u8 {
    r.max(g).max(b)
}

/// Three-letter light mode word matching the core snapshot's codes: `0` off,
/// `1` breathing, `2` solid.
fn mode_word(mode: u8) -> &'static [u8] {
    match mode {
        1 => b"BRE",
        2 => b"SOL",
        _ => b"OFF",
    }
}

/// ASCII digit of a movement-direction code (see `SwipeDirection::code`'s
/// canonical numpad table; `5` is reserved): a direction's number is exactly
/// its ASCII digit, matching the overlay's "no glyphs but digits" contract; a
/// dash when nothing is moving (code 0).
fn direction_arrow(dir: u8) -> u8 {
    match dir {
        0 => b'-',
        d @ 1..=9 => b'0' + d,
        _ => b'-',
    }
}

/// Single-digit glyph of a finger's last resolved gesture, matching the
/// core `FINGER_*` codes (`1` tap, `2` double-tap, `3` triple-tap, `4`
/// long-press, `5` swipe) so the per-finger rows and the on-panel digit
/// gradient agree; a dash before the slot ever resolves one.
fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_DOUBLE_TAP => b'2',
        FINGER_TRIPLE_TAP => b'3',
        FINGER_LONG_PRESS => b'4',
        FINGER_SWIPE => b'5',
        _ => b'-',
    }
}

/// Pack an RGB color into ST7789 big-endian RGB565.
fn rgb565(color: Rgb) -> u16 {
    let r = color.0 as u16;
    let g = color.1 as u16;
    let b = color.2 as u16;
    ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
}

/// Per-channel bitwise inversion of an RGB565 word: a digit drawn this way is
/// visible on any fill, matching the "off" side of the contrast regardless of
/// base color.
fn invert_rgb565(packed: u16) -> u16 {
    let r = (packed >> 11) & 0x1F;
    let g = (packed >> 5) & 0x3F;
    let b = packed & 0x1F;
    ((0x1F - r) << 11) | ((0x3F - g) << 5) | (0x1F - b)
}

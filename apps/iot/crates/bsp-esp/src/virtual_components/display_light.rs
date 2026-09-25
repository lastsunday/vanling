use alloc::vec::Vec;
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
use iot_core::drivers::input::{FINGER_DOUBLE_TAP, FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP};
use iot_core::drivers::light::{
    Fill, Rgb, RgbLight, rgb_hue, scale_brightness, vertical_brightness,
};
use iot_core::render::{MODE_BREATH, MODE_SOLID};

use crate::components::backlight::Backlight;
use crate::components::st7789::St7789;

/// Minimum per-channel color delta that warrants a full-frame repaint.
const REPAINT_STEP: u8 = 12;

/// Debug overlay toggle. A compile-time switch (not a Cargo feature): the
/// diagnostics digit rows and last-touch coordinates are a field/development aid, so
/// production builds keep them dark by setting this to `false`. Counting in
/// core is cheap and unconditional; only the panel paint is gated.
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
    /// Diagnostic overlay payload: touch counters and readout (see [`Diagnostics`]),
    /// shown when [`DEBUG_DIAGNOSTICS`] is on. A bump repaints via [`Self::paint`],
    /// which the `screen_color` guard would otherwise skip.
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
        if !DEBUG_DIAGNOSTICS {
            return;
        }
        if self.diagnostics == *diagnostics {
            return;
        }
        self.diagnostics = *diagnostics;
        // The light color may not have moved (bump at rest), so bypass the
        // `screen_color` guard and repaint the current surface onto the frame.
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
        self.stamp_diagnostics(width, usize::from(height), live);

        if let Err(e) = self.panel.write_frame(&self.frame) {
            log::error!("[DISPLAY] frame write failed: {e:?}");
        }
    }

    /// Overdraws the corner as two columns of inverted-pixel rows, visible on
    /// any fill. Touch column: the `GST`/`2F`/`PRS`/`TAP`/`DBL`/`LNG`/`SWP`
    /// counters, `DIR` (last swipe arrow+distance), per-finger `P0*`/`P1*`
    /// rows (live coords, gesture digit `1`–`4` with value, live arrow),
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

        let counters: [(&[u8], u16); 7] = [
            (b"GST".as_slice(), u16::from(touch.ghost)),
            (b"2F".as_slice(), u16::from(touch.two_finger_runs)),
            (b"PRS".as_slice(), u16::from(touch.presses)),
            (b"TAP".as_slice(), u16::from(touch.taps)),
            (b"DBL".as_slice(), u16::from(touch.double_taps)),
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
        self.stamp_left(b"DIR", &dir[..nd], 7, width, height);

        let mut ref_buf = [0u8; 8];
        let origin = format_pair(touch.last_gesture_origin, &mut ref_buf);
        self.stamp_left(b"XY", origin, 8, width, height);

        let mut end_buf = [0u8; 8];
        let end = format_pair(touch.last_gesture_end, &mut end_buf);
        self.stamp_left(b"XY2", end, 9, width, height);

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
            let base = 10 + 4 * slot;
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
            18,
            width,
            height,
        );

        self.stamp_left(
            b"FRM",
            format_u16(touch.frames, &mut buf),
            19,
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

    /// Writes one touch-column row: label at [`OVERLAY_X`], one space glyph,
    /// then the value at [`LEFT_VALUE_X`].
    fn stamp_left(&mut self, label: &[u8], value: &[u8], row: usize, width: usize, height: usize) {
        let top = OVERLAY_Y + row * (FONT_H + OVERLAY_ROW_GAP);
        self.stamp_text(label, OVERLAY_X, top, width, height);
        self.stamp_text(value, LEFT_VALUE_X, top, width, height);
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
                let x = left + col;
                let y = top + row;
                if x >= width || y >= height {
                    continue;
                }
                let offset = (y * width + x) * 2;
                let packed = u16::from_be_bytes([self.frame[offset], self.frame[offset + 1]]);
                self.frame[offset..offset + 2]
                    .copy_from_slice(&invert_rgb565(packed).to_be_bytes());
            }
        }
    }
}

/// Decimal ASCII digits of `value` (no leading zeros) written into the start
/// of `buf`, returned as the filled prefix.
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
/// core `FINGER_*` codes (`1` tap, `2` double-tap, `3` long-press, `4`
/// swipe) so the per-finger rows and the on-panel digit gradient agree; a
/// dash before the slot ever resolves one.
fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_DOUBLE_TAP => b'2',
        FINGER_LONG_PRESS => b'3',
        FINGER_SWIPE => b'4',
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

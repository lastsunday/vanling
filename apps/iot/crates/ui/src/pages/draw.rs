//! The ink's arithmetic: text widths, block rows, labels and the formatters pages read.

use iot_core::drivers::audio::{COLUMN_MS, ENVELOPE_COLUMNS};
use iot_core::drivers::input::{FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP};
use iot_core::keyboard::{ControlRect, Key, LIST_ROW_H, LIST_ROWS, LIST_TOP, Layer};
use iot_core::overlay::{FONT_H, GLYPH_PITCH, OVERLAY_GAP};
use iot_core::paint::Rgb;
use iot_core::state::Entry;

use crate::layout::{PSK_MASK_GLYPHS, SCOPE_DECIBEL_FLOOR, WAVE_CY, WAVE_HALF};

pub(crate) fn bar_rows(peak_lsb: u16) -> isize {
    WAVE_HALF * isize::from(iot_core::drivers::audio::scope_height(peak_lsb)) / isize::from(u8::MAX)
}

pub(crate) fn decibels_row(decibels: i16) -> isize {
    WAVE_CY - WAVE_HALF * (SCOPE_DECIBEL_FLOOR - decibels) as isize / SCOPE_DECIBEL_FLOOR as isize
}

pub(crate) fn format_decibels(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    format_i32(i32::from(decibels), buf)
}

pub(crate) fn format_span(buf: &mut [u8; 12]) -> &[u8] {
    let tenths = ENVELOPE_COLUMNS as u32 * COLUMN_MS as u32 / 100;
    let mut whole = [0u8; 12];
    let seconds = format_i32((tenths / 10) as i32, &mut whole);
    buf[..seconds.len()].copy_from_slice(seconds);
    let mut len = seconds.len();
    for tail in [b'.', b'0' + (tenths % 10) as u8, b'S'] {
        buf[len] = tail;
        len += 1;
    }
    &buf[..len]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    None,
    Face,
    Candidate(usize),
}

pub(crate) fn control_label(key: Key, layer: Layer) -> &'static [u8] {
    match key {
        Key::Layer => match layer {
            Layer::Abc => b"123",
            Layer::Digits => b"#&?",
            Layer::Symbols => b"abc",
        },
        Key::Shift => b"SH",
        Key::Backspace => b"<-",
        Key::Connect => b"OK",
        Key::Cancel => b"X",
        Key::Reveal => b"SHOW",
        Key::Char(_) => b"",
    }
}

pub(crate) const HIDE_LABEL: &[u8] = b"HIDE";

pub(crate) fn band_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - FONT_H) / 2
}

pub(crate) fn text_width(text: &[u8]) -> usize {
    text.len() * GLYPH_PITCH - OVERLAY_GAP
}

pub(crate) fn text_left(rect: ControlRect, text: &[u8]) -> usize {
    rect.x + rect.w.saturating_sub(text_width(text)) / 2
}

// The draft as the row above the keyboard shows it: masked, or as typed while the reveal is on.
//
// The *tail* of it rather than the head, because the end is what the finger just typed and a
// character to fix is at the end of a passphrase about as often as anywhere. What does not fit is
// marked with a leading `+` rather than silently dropped, and the mark is one of the row's own
// glyphs: the row's width is what the columns above leave it, so a draft a glyph wider than that
// would run into the phase word beside it.
pub(crate) fn format_psk<'a>(
    entry: &Entry,
    revealed: bool,
    buf: &'a mut [u8; PSK_MASK_GLYPHS],
) -> &'a [u8] {
    let shown = entry.len().min(PSK_MASK_GLYPHS);
    let more = entry.len() > shown;
    let tail = if more { shown - 1 } else { shown };
    let mut n = 0;
    if more {
        buf[0] = b'+';
        n = 1;
    }
    let bytes = entry.as_bytes();
    for (slot, byte) in buf[n..n + tail]
        .iter_mut()
        .zip(&bytes[bytes.len() - tail..])
    {
        *slot = match (revealed, byte.is_ascii_graphic()) {
            (true, true) => *byte,
            (true, false) => b'?',
            (false, _) => b'*',
        };
    }
    &buf[..n + tail]
}

pub(crate) fn format_window(top: u8, rows: usize, buf: &mut [u8; 12]) -> &[u8] {
    let first = usize::from(top) + 1;
    let last = (usize::from(top) + LIST_ROWS).min(rows);
    let mut n = write_u64(first as u64, buf, 0);
    buf[n] = b'-';
    n = write_u64(last as u64, buf, n + 1);
    buf[n] = b'/';
    n = write_u64(rows as u64, buf, n + 1);
    &buf[..n]
}

pub(crate) fn format_i32(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_signed(value.unsigned_abs() as u64, value < 0, buf, 0);
    &buf[..n]
}

fn write_signed(magnitude: u64, negative: bool, buf: &mut [u8], mut n: usize) -> usize {
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    write_u64(magnitude, buf, n)
}

pub(crate) fn format_dba(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(decibels.unsigned_abs()), buf, 0);
    buf[n] = b' ';
    buf[n + 1..n + 4].copy_from_slice(b"dBA");
    &buf[..n + 4]
}

pub(crate) fn format_tenths(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let magnitude = value.unsigned_abs() as u64;
    let n = write_signed(magnitude / 10, value < 0, buf, 0);
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

pub(crate) fn format_u16(value: u16, buf: &mut [u8; 6]) -> &[u8] {
    let n = write_u16(value, buf, 0);
    &buf[..n]
}

pub(crate) fn write_u16(value: u16, buf: &mut [u8], n: usize) -> usize {
    write_u64(value.into(), buf, n)
}

pub(crate) fn brightness_of(Rgb(r, g, b): Rgb) -> u8 {
    r.max(g).max(b)
}

pub(crate) fn mode_word(mode: u8) -> &'static [u8] {
    match mode {
        1 => b"BRE",
        2 => b"SOL",
        _ => b"OFF",
    }
}

pub(crate) fn direction_arrow(dir: u8) -> u8 {
    match dir {
        0 => b'-',
        d @ 1..=9 => b'0' + d,
        _ => b'-',
    }
}

pub(crate) fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_LONG_PRESS => b'2',
        FINGER_SWIPE => b'3',
        _ => b'-',
    }
}

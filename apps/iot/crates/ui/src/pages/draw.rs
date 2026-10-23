//! The ink's arithmetic: text widths, block rows, labels and the formatters pages read.
//!
//! Nothing here owns a frame. A page asks [`Canvas`](crate::canvas::Canvas) for the pixels and
//! this module for the characters; keeping the two apart is what lets the arithmetic be tested
//! on a host without a panel.

use iot_core::drivers::audio::{COLUMN_MS, ENVELOPE_COLUMNS};
use iot_core::drivers::input::{FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP};
use iot_core::drivers::light::Rgb;
use iot_core::keyboard::{ControlRect, Key, LIST_ROW_H, LIST_ROWS, LIST_TOP, Layer};
use iot_core::overlay::{FONT_H, GLYPH_PITCH, OVERLAY_GAP};
use iot_core::state::Entry;

use crate::layout::{PSK_MASK_GLYPHS, SCOPE_DECIBEL_FLOOR, WAVE_CY, WAVE_HALF};

/// Rows a level sits at, mirrored about [`WAVE_CY`]: the row the bar's edge lands on. The same
/// [`scope_height`](iot_core::drivers::audio::scope_height) the drawing is built from, at panel
/// scale, so the ruler's lines and the bars' edges stay on the same decibels.
pub(crate) fn bar_rows(peak_lsb: u16) -> isize {
    WAVE_HALF * isize::from(iot_core::drivers::audio::scope_height(peak_lsb)) / isize::from(u8::MAX)
}

/// The row `decibels` below full scale falls on, above the centre line. Measured out from the
/// *floor* rather than the rail, because the band is mirrored: the floor is the middle and
/// 0 dBFS is the edge.
pub(crate) fn decibels_row(decibels: i16) -> isize {
    WAVE_CY - WAVE_HALF * (SCOPE_DECIBEL_FLOOR - decibels) as isize / SCOPE_DECIBEL_FLOOR as isize
}

/// A ruler label: decibels under full scale, signed, with the zero written bare because `-0` is
/// not a level anyone says out loud.
pub(crate) fn format_decibels(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    format_i32(i32::from(decibels), buf)
}

/// The sweep's own time span, in tenths of a second and a unit — `2.0S` for the window the
/// envelope is drawn at. Computed rather than printed, because a hardcoded label goes on claiming
/// two seconds through any later window.
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

/// What a key draws inverted: the panel's way of saying "this one", since stamping inverts and a
/// filled cell with a plain glyph in it would be a cell with nothing in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// Nothing on this key.
    None,
    /// The whole face, for the one control whose *state* the page has to show — a shift that is
    /// on, which is a mode the user cannot read off the letters until one is typed.
    Face,
    /// The `n`-th character of the key's own run: the key a repeat tap would cycle, which says
    /// what counting presses on it is counting.
    Candidate(usize),
}

/// The word one of the six controls is printed with.
///
/// The layer key says where it goes rather than where it is: `123` on the alphabet layer, `#&?` on
/// the digits one, and `abc` on the symbols one, so the key always reads as the layer it would
/// switch to.
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

/// What the reveal key says while the draft is showing, which is the other half of
/// [`control_label`]'s own word: the key is a latch, and a latch that said `SHOW` while the
/// passphrase was on the panel would be a key whose word is not what it does.
pub(crate) const HIDE_LABEL: &[u8] = b"HIDE";

/// The top edge of the list's `band`-th row, on the glyphs' own line: the middle of the band,
/// because a band is a target a thumb aims at rather than a row of text.
pub(crate) fn band_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - FONT_H) / 2
}

/// The width `text` occupies: every glyph but the last, which carries no trailing gap.
pub(crate) fn text_width(text: &[u8]) -> usize {
    text.len() * GLYPH_PITCH - OVERLAY_GAP
}

/// Where `text` starts inside `rect`, measured on the glyphs' own width rather than on a cell
/// count: the last glyph carries no trailing gap, so counting cells would sit every label right of
/// centre by part of one.
pub(crate) fn text_left(rect: ControlRect, text: &[u8]) -> usize {
    rect.x + rect.w.saturating_sub(text_width(text)) / 2
}

/// The draft as the row above the keyboard shows it: masked, or as typed while the reveal is on.
///
/// The *tail* of it rather than the head, because the end is what the finger just typed and a
/// character to fix is at the end of a passphrase about as often as anywhere. What does not fit is
/// marked with a leading `+` rather than silently dropped, and the mark is one of the row's own
/// glyphs: the row's width is what the columns above leave it, so a draft a glyph wider than that
/// would run into the phase word beside it.
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
            // A passphrase can hold a space, which the font draws as a gap: an honest blank reads
            // as one, where a `_` would be a character the password does not have.
            (true, false) => b'?',
            (false, _) => b'*',
        };
    }
    &buf[..n + tail]
}

/// The rows of the scan list the page is showing, as `first-last/total`.
///
/// The one thing about the list that the list itself cannot say: a finger that scrolled it has to
/// be told where it scrolled to, and a list that ends at the panel's bottom edge looks the same as
/// one that has run out.
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

/// Writes a `-` sign when `negative`, then `magnitude`'s digits, starting at offset `n`; returns
/// the new length.
fn write_signed(magnitude: u64, negative: bool, buf: &mut [u8], mut n: usize) -> usize {
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    write_u64(magnitude, buf, n)
}

/// A pressure level in decibels with its unit, as the dB(A) readout heading the Audio page's
/// right-hand block: unlike a level row's `DBFS`/`SPL` halves, this one carries no sign — the
/// A-weighted readout is clamped to the floor the envelope can even see, and a "−42 dBA" that can
/// only be wrong is worse than a floor that says so indirectly.
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

/// Decimal ASCII digits of `value` (no leading zeros) written into the start of `buf`, returned
/// as the filled prefix.
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

/// The brightness of an RGB color: the channel the panel's own light reports.
pub(crate) fn brightness_of(Rgb(r, g, b): Rgb) -> u8 {
    r.max(g).max(b)
}

/// Three-letter light mode word matching the core snapshot's codes: `0` off, `1` breathing,
/// `2` solid.
pub(crate) fn mode_word(mode: u8) -> &'static [u8] {
    match mode {
        1 => b"BRE",
        2 => b"SOL",
        _ => b"OFF",
    }
}

/// ASCII digit of a movement-direction code (see `SwipeDirection::code`'s canonical numpad table;
/// `5` is reserved): a direction's number is exactly its ASCII digit, matching the overlay's "no
/// glyphs but digits" contract; a dash when nothing is moving (code 0).
pub(crate) fn direction_arrow(dir: u8) -> u8 {
    match dir {
        0 => b'-',
        d @ 1..=9 => b'0' + d,
        _ => b'-',
    }
}

/// Single-digit glyph of a finger's last resolved gesture, matching the core `FINGER_*` codes
/// (`1` tap, `2` long-press, `3` swipe) so the per-finger rows and the on-panel digit gradient
/// agree; a dash before the slot ever resolves one.
pub(crate) fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_LONG_PRESS => b'2',
        FINGER_SWIPE => b'3',
        _ => b'-',
    }
}

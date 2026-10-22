//! The ink: text, glyphs, lines and the formatters every page draws with.
use super::*;

/// The window's floor in decibels below full scale, which is the bottom of the
/// Audio page's scale. Read back from the mapping rather than written here, so a
/// band and its own ruler cannot end up describing different windows.
pub(super) const SCOPE_DECIBEL_FLOOR: i16 = SCOPE_FLOOR_DECIBELS as i16;

/// Rows a level sits at, mirrored about [`WAVE_CY`]: the row the bar's edge
/// lands on. The same [`scope_height`] the drawing is built from, at panel
/// scale, so the ruler's lines and the bars' edges stay on the same decibels.
pub(super) fn bar_rows(peak_lsb: u16) -> isize {
    WAVE_HALF * isize::from(scope_height(peak_lsb)) / isize::from(u8::MAX)
}

/// The row `decibels` below full scale falls on, above the centre line. Measured
/// out from the *floor* rather than the rail, because the band is mirrored: the
/// floor is the middle and 0 dBFS is the edge.
pub(super) fn decibels_row(decibels: i16) -> isize {
    WAVE_CY - WAVE_HALF * (SCOPE_DECIBEL_FLOOR - decibels) as isize / SCOPE_DECIBEL_FLOOR as isize
}

/// A ruler label: decibels under full scale, signed, with the zero written bare
/// because `-0` is not a level anyone says out loud.
pub(super) fn format_decibels(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    format_i32(i32::from(decibels), buf)
}

/// The sweep's own time span, in tenths of a second and a unit — `2.0S` for the
/// window the envelope is drawn at. Computed rather than printed, because a
/// hardcoded label goes on claiming two seconds through any later window.
pub(super) fn format_span(buf: &mut [u8; 12]) -> &[u8] {
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
pub(super) enum Mark {
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
/// The words are the panel's, not the driver's — the radio has no opinion about a keypad — so they
/// are written once, here, where the drawing reads them.
///
/// The layer key says where it goes rather than where it is: `123` on the alphabet layer, `#&?` on
/// the digits one, and `abc` on the symbols one, so the key always reads as the layer it would
/// switch to.
pub(super) fn control_label(key: Key, layer: Layer) -> &'static [u8] {
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
pub(super) const HIDE_LABEL: &[u8] = b"HIDE";

/// The top edge of the list's `band`-th row, on the glyphs' own line: the middle of the band,
/// because a band is a target a thumb aims at rather than a row of text.
pub(super) fn band_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - FONT_H) / 2
}

/// The width `text` occupies: every glyph but the last, which carries no trailing gap.
pub(super) fn text_width(text: &[u8]) -> usize {
    text.len() * GLYPH_PITCH - OVERLAY_GAP
}

/// Where `text` starts inside `rect`, measured on the glyphs' own width rather than on a cell
/// count: the last glyph carries no trailing gap, so counting cells would sit every label right of
/// centre by part of one.
pub(super) fn text_left(rect: ControlRect, text: &[u8]) -> usize {
    rect.x + rect.w.saturating_sub(text_width(text)) / 2
}

/// The draft as the row above the keyboard shows it: masked, or as typed while the reveal is on.
///
/// The *tail* of it rather than the head, because the end is what the finger just typed and a
/// character to fix is at the end of a passphrase about as often as anywhere. What does not fit is
/// marked with a leading `+` rather than silently dropped, and the mark is one of the row's own
/// glyphs: the row's width is what the columns above leave it, so a draft a glyph wider than that
/// would run into the phase word beside it.
pub(super) fn format_psk<'a>(
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
pub(super) fn format_window(top: u8, rows: usize, buf: &mut [u8; 12]) -> &[u8] {
    let first = usize::from(top) + 1;
    let last = (usize::from(top) + LIST_ROWS).min(rows);
    let mut n = write_u64(first as u64, buf, 0);
    buf[n] = b'-';
    n = write_u64(last as u64, buf, n + 1);
    buf[n] = b'/';
    n = write_u64(rows as u64, buf, n + 1);
    &buf[..n]
}

/// Inverts one pixel of `frame`, the shared ink for glyphs and the dial. A
/// function of the frame alone rather than a method, so that everything it
/// builds can be stamped into a bare buffer and read back as pixels — the
/// layout of a readout is otherwise only checkable on a panel.
pub(super) fn stamp_pixel(frame: &mut [u8], x: isize, y: isize, width: usize, height: usize) {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
        return;
    }
    let offset = (y as usize * width + x as usize) * 2;
    let packed = u16::from_be_bytes([frame[offset], frame[offset + 1]]);
    frame[offset..offset + 2].copy_from_slice(&invert_rgb565(packed).to_be_bytes());
}

/// Overdraws one cell of `ch` at `left`/`top` with inverted pixels; bytes outside
/// the printable ASCII block draw nothing.
fn stamp_char(frame: &mut [u8], ch: u8, left: usize, top: usize, width: usize, height: usize) {
    if !(0x20..=0x7E).contains(&ch) {
        return;
    }
    let base = usize::from(ch - 0x20) * FONT_W;
    for (col, &bits) in FONT[base..base + FONT_W].iter().enumerate() {
        for row in 0..FONT_H {
            if bits & (1 << row) == 0 {
                continue;
            }
            stamp_pixel(
                frame,
                left as isize + col as isize,
                top as isize + row as isize,
                width,
                height,
            );
        }
    }
}

pub(super) fn stamp_text(
    frame: &mut [u8],
    text: &[u8],
    left: usize,
    top: usize,
    width: usize,
    height: usize,
) {
    for (glyph, &ch) in text.iter().enumerate() {
        stamp_char(frame, ch, left + glyph * GLYPH_PITCH, top, width, height);
    }
}

/// `stamp_text` measured from the text's last column rather than its first, so a
/// value that grows a digit grows leftwards into empty space.
pub(super) fn stamp_text_right(
    frame: &mut [u8],
    text: &[u8],
    right: isize,
    top: usize,
    width: usize,
    height: usize,
) {
    let left = right + OVERLAY_GAP as isize - text.len() as isize * GLYPH_PITCH as isize;
    stamp_text(frame, text, left.max(0) as usize, top, width, height);
}

pub(super) fn format_i32(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_signed(value.unsigned_abs() as u64, value < 0, buf, 0);
    &buf[..n]
}

/// Writes a `-` sign when `negative`, then `magnitude`'s digits, starting at
/// offset `n`; returns the new length.
fn write_signed(magnitude: u64, negative: bool, buf: &mut [u8], mut n: usize) -> usize {
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    write_u64(magnitude, buf, n)
}

/// A pressure level in decibels with its unit, as the dB(A) readout heading the
/// Audio page's right-hand block: unlike a level row's `DBFS`/`SPL` halves,
/// this one carries no sign — the A-weighted readout is clamped to the floor
/// the envelope can even see, and a "−42 dBA" that can only be wrong is worse
/// than a floor that says so indirectly.
pub(super) fn format_dba(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(decibels.unsigned_abs()), buf, 0);
    buf[n] = b' ';
    buf[n + 1..n + 4].copy_from_slice(b"dBA");
    &buf[..n + 4]
}

pub(super) fn format_tenths(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let magnitude = value.unsigned_abs() as u64;
    let n = write_signed(magnitude / 10, value < 0, buf, 0);
    buf[n] = b'.';
    buf[n + 1] = b'0' + (magnitude % 10) as u8;
    &buf[..n + 2]
}

/// Decimal ASCII digits of `value` (no leading zeros) written into the start
/// of `buf`, returned as the filled prefix.
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

pub(super) fn format_u16(value: u16, buf: &mut [u8; 6]) -> &[u8] {
    let n = write_u16(value, buf, 0);
    &buf[..n]
}

pub(super) fn write_u16(value: u16, buf: &mut [u8], n: usize) -> usize {
    write_u64(value.into(), buf, n)
}

/// Renders an optional coordinate pair as `x y` (single dash when unset) for
/// the `XY`/`XY2` overlay rows.
pub(super) fn brightness_of(Rgb(r, g, b): Rgb) -> u8 {
    r.max(g).max(b)
}

/// Three-letter light mode word matching the core snapshot's codes: `0` off,
/// `1` breathing, `2` solid.
pub(super) fn mode_word(mode: u8) -> &'static [u8] {
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
pub(super) fn direction_arrow(dir: u8) -> u8 {
    match dir {
        0 => b'-',
        d @ 1..=9 => b'0' + d,
        _ => b'-',
    }
}

/// Single-digit glyph of a finger's last resolved gesture, matching the
/// core `FINGER_*` codes (`1` tap, `2` long-press, `3` swipe) so the
/// per-finger rows and the on-panel digit gradient agree; a dash before the
/// slot ever resolves one.
pub(super) fn gesture_glyph(kind: u8) -> u8 {
    match kind {
        FINGER_TAP => b'1',
        FINGER_LONG_PRESS => b'2',
        FINGER_SWIPE => b'3',
        _ => b'-',
    }
}

impl DisplayLight {
    /// Overdraws one run of text at a panel coordinate. The escape hatch the
    /// block rows are deliberately not built from: a row placed by name is
    /// one whose position follows the layout, and a page that needs its words
    /// somewhere the blocks do not go says so here.
    pub(super) fn stamp_text(
        &mut self,
        text: &[u8],
        left: usize,
        top: usize,
        width: usize,
        height: usize,
    ) {
        stamp_text(self.frame, text, left, top, width, height)
    }

    /// `stamp_text` measured from the text's last column rather than its first,
    /// so a value that grows a digit grows leftwards into empty space.
    pub(super) fn stamp_text_right(
        &mut self,
        text: &[u8],
        right: isize,
        top: usize,
        width: usize,
        height: usize,
    ) {
        stamp_text_right(self.frame, text, right, top, width, height)
    }

    /// Inverts one pixel: the shared ink for glyphs and the dial. The glyph and
    /// text wrappers go straight to the free functions, since they have no state
    /// of their own to fold the frame into.
    pub(super) fn stamp_pixel(&mut self, x: isize, y: isize, width: usize, height: usize) {
        stamp_pixel(self.frame, x, y, width, height)
    }

    /// Inverts the straight run from `(x0, y0)` to `(x1, y1)` (Bresenham).
    pub(super) fn stamp_line(
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
    pub(super) fn stamp_circle(
        &mut self,
        cx: isize,
        cy: isize,
        r: isize,
        width: usize,
        height: usize,
    ) {
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

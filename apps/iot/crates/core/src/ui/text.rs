//! Turning readings into glyphs: the arithmetic every page agrees on.

mod number;

pub use self::number::*;

use super::font::Face;

/// `text` cut to at most `width` pixels on a character boundary, marked with `~` when cut.
///
/// A byte buffer cannot stand in for a glyph count once names are not ASCII: a hanzi is three
/// bytes and two cells. The mark earns its own cell before the text does, so a name that only
/// narrowly overflows still says it was cut.
pub fn fit<'a>(text: &str, width: usize, buf: &'a mut [u8]) -> &'a [u8] {
    let face = Face::new();
    let bytes = text.as_bytes();
    if face.measure(bytes) <= width {
        let taken = floor_boundary(text, buf.len());
        buf[..taken].copy_from_slice(&bytes[..taken]);
        return &buf[..taken];
    }
    let mark = face.advance('~');
    let (budget, marked) = if width >= mark {
        (width - mark, true)
    } else {
        (width, false)
    };
    let mut used = 0;
    let mut taken = 0;
    for (at, ch) in text.char_indices() {
        let advance = face.advance(ch);
        if used + advance > budget || at + ch.len_utf8() > buf.len() {
            break;
        }
        used += advance;
        taken = at + ch.len_utf8();
    }
    buf[..taken].copy_from_slice(&bytes[..taken]);
    if marked && taken < buf.len() {
        buf[taken] = b'~';
        taken += 1;
    }
    &buf[..taken]
}

/// The largest prefix of `text` that fits `room` bytes without splitting a character.
fn floor_boundary(text: &str, room: usize) -> usize {
    if text.len() <= room {
        return text.len();
    }
    text.char_indices()
        .map(|(at, ch)| at + ch.len_utf8())
        .take_while(|end| *end <= room)
        .last()
        .unwrap_or(0)
}

/// Bytes between fingerprint samples.
pub const FINGERPRINT_STRIDE: usize = 997;

/// An FNV fold over a sparse sample of the frame, kept to sixteen bits.
pub fn frame_fingerprint(frame: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in frame.iter().step_by(FINGERPRINT_STRIDE) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash >> 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_fits_is_copied_whole() {
        let mut buf = [0u8; 32];
        assert_eq!(fit("cafe", 24, &mut buf), b"cafe");
        assert_eq!(fit("网络", 24, &mut buf), "网络".as_bytes());
    }

    #[test]
    fn a_name_that_overflows_is_cut_on_a_character_boundary_and_marked() {
        let mut buf = [0u8; 32];
        let cut = fit("无线路由器", 12 * 4 + 6, &mut buf);
        assert_eq!(cut, "无线路由~".as_bytes(), "four hanzi plus the mark");
        assert_eq!(Face::new().measure(cut), 12 * 4 + 6);
    }

    #[test]
    fn a_width_narrower_than_the_mark_keeps_only_text() {
        let mut buf = [0u8; 32];
        assert_eq!(fit("网络", 11, &mut buf), b"~");
        assert_eq!(fit("网络", 5, &mut buf), b"");
    }
}

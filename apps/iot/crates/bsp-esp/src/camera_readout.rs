//! Turning readings into the glyphs a panel draws.
//!
//! Separated from [`crate::virtual_components::display_light`] so it can be built and tested
//! on the development machine: this is arithmetic over glyph buffers and frame bytes, with no
//! peripheral in it, and the arithmetic is where the bugs were. A reading that formats wider
//! than its buffer indexes past the end, and that only shows up on hardware — twice, in two
//! different pages, before this had a test.
//!
//! Not camera-specific despite the module name: the panel's own coordinate formatting is here
//! too, because it is the same arithmetic and the panel is the only surface that draws it. The
//! camera's additions are marked, and the module is unconditional — a gate would leave
//! `display-light` naming a function only some features define.
//!
//! The layout — which block each reading sits in and at what row — belongs to the panel, which
//! is the only thing that knows where the columns are. What is here is how a number becomes
//! the characters that will go in one.

/// A coordinate pair written `x y`.
///
/// Sized for the widest pair a panel-space reading can take — five digits either side of a
/// space — rather than for the five-glyph block it is usually written in. A buffer sized to
/// the block indexes past its end on exactly the readings that are widest, and those are the
/// ones worth printing: a window that reads `319 239` fits anywhere, and one that overflows is
/// wrong only at the interesting value.
pub fn format_pair(pair: Option<(u16, u16)>, buf: &mut [u8; 12]) -> &[u8] {
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

/// `u32` in a glyph buffer, for counters that outgrow a `u16`.
///
/// Over [`write_u64`] rather than writing digits here, because a second implementation of the
/// same arithmetic is how a caller ends up with one format filling a buffer from the right and
/// every reader taking it from the left — which reads a label one glyph into the value beside
/// it, and overflows the moment the buffer is the narrower of the two.
pub fn format_u32(value: u32, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(value), buf, 0);
    &buf[..n]
}

/// Decimal ASCII digits of `value` (no leading zeros) written into the start of `buf`,
/// returned as the filled prefix.
pub fn write_u16(value: u16, buf: &mut [u8], n: usize) -> usize {
    write_u64(u64::from(value), buf, n)
}

/// Decimal ASCII digits of `value` (no leading zeros) written into `buf` from `n`, returned as
/// the index one past the last digit.
///
/// Widened over the `u16` the other formatters take, because a counter outgrows `u16` — frames
/// do after about eight hours at this frame rate, and the test reaches it at once.
pub fn write_u64(mut value: u64, buf: &mut [u8], mut n: usize) -> usize {
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

/// The decimation ratio as `r:c`, from the register's row and column nibbles.
///
/// Two nibbles rather than one number because they are set independently and a ratio written
/// as a single figure hides which axis is wrong.
pub fn write_ratio(buf: &mut [u8; 6], subsample: u8) -> &[u8] {
    buf[0] = b'0' + (subsample >> 4);
    buf[1] = b':';
    buf[2] = b'0' + (subsample & 0x0f);
    &buf[..3]
}

/// Bytes between fingerprint samples. Prime and over the frame's length, so the walk visits a
/// spread of rows instead of one stride's worth of them.
pub const FINGERPRINT_STRIDE: usize = 997;

/// An FNV fold over a sparse sample of the frame, kept to sixteen bits.
///
/// The one number that says whether there is a picture. A sensor whose PLL has not locked
/// streams correctly framed noise with every register reading back correct, so the geometry
/// readings cannot distinguish it from a real picture — only the pixels can. Sparse because a
/// frame is 150 KB and this runs on every painted frame.
pub fn frame_fingerprint(frame: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in frame.iter().step_by(FINGERPRINT_STRIDE) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash >> 16
}

#[cfg(test)]
mod tests {
    use super::{format_pair, format_u32, frame_fingerprint, write_ratio, write_u64};
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn a_pair_holds_the_widest_reading_a_sensor_reports() {
        // Five digits either side of a space is nine characters. This is the reading that
        // overflowed an eight-byte buffer on hardware — at a panel corner and on the camera's
        // own readout window, which is wider than the panel it is drawn on.
        let mut buf = [0u8; 12];
        assert_eq!(
            format_pair(Some((319, 239)), &mut buf),
            b"319 239",
            "a panel corner"
        );
        assert_eq!(
            format_pair(Some((1616, 1208)), &mut buf),
            b"1616 1208",
            "the sensor's readout window, which is wider than the panel"
        );
        assert_eq!(
            format_pair(Some((65535, 65535)), &mut buf),
            b"65535 65535",
            "the widest a u16 pair can be"
        );
    }

    #[test]
    fn a_pair_with_no_reading_says_so_in_one_glyph() {
        let mut buf = [0u8; 12];
        assert_eq!(format_pair(None, &mut buf), b"-");
    }

    #[test]
    fn a_counter_reads_left_to_right_from_zero() {
        // Digits written from the right and returned as the left-aligned prefix every reader
        // takes would walk a label into its own value — which is what a right-aligned writer
        // does, and the two directions must never meet in one module.
        let mut buf = [0u8; 12];
        assert_eq!(format_u32(0, &mut buf), b"0");
        assert_eq!(format_u32(7, &mut buf), b"7");
        assert_eq!(format_u32(1234, &mut buf), b"1234");
        assert_eq!(format_u32(99999, &mut buf), b"99999");
    }

    #[test]
    fn a_counter_outgrowing_u16_keeps_counting() {
        // Frames pass 65535 after about four hours at this frame rate, and a test reaches it
        // at once. A formatter that narrowed first would report a running camera as stalled —
        // the one reading that must never come out of a counter.
        let mut buf = [0u8; 12];
        assert_eq!(format_u32(u32::from(u16::MAX), &mut buf), b"65535");
        assert_eq!(format_u32(u32::from(u16::MAX) + 1, &mut buf), b"65536");
        assert_eq!(
            format_u32(u32::MAX, &mut buf),
            b"4294967295",
            "the widest a u32 can be, and the buffer is exactly wide enough"
        );
    }

    #[test]
    fn write_u64_fills_exactly_the_digits_it_was_given() {
        // The primitive both formatters are built on: no leading zeros, no trailing garbage.
        let mut buf = [0u8; 12];
        buf.fill(b'x');
        let n = write_u64(7, &mut buf, 0);
        assert_eq!(&buf[..n], b"7");
        assert_eq!(buf[1], b'x', "nothing written past the prefix");
    }

    #[test]
    fn the_decimation_ratio_reads_as_two_nibbles() {
        let mut buf = [0u8; 6];
        assert_eq!(
            write_ratio(&mut buf, 0x55),
            b"5:5",
            "ratio 5 is full field of view"
        );
        assert_eq!(write_ratio(&mut buf, 0x33), b"3:3");
        assert_eq!(write_ratio(&mut buf, 0x00), b"0:0");
    }

    #[test]
    fn the_fingerprint_separates_a_blank_frame_from_a_picture() {
        let blank: Vec<u8> = vec![0u8; 320 * 240 * 2];
        let mut filled = blank.clone();
        for (index, byte) in filled.iter_mut().enumerate() {
            // A gradient across the frame, so consecutive samples differ as a real picture's do.
            *byte = (index % 251) as u8;
        }
        assert_ne!(
            frame_fingerprint(&blank),
            frame_fingerprint(&filled),
            "a flat frame and a gradient cannot fold to the same number"
        );
        assert_eq!(
            frame_fingerprint(&blank),
            frame_fingerprint(&blank.clone()),
            "the same frame twice folds the same, which is what makes a repeat detectable"
        );
    }
}

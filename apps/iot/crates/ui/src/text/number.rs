//! Numeric glyph formatting, in one place so a value and its reader agree on where it starts.

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

/// A `u16` in a six-glyph buffer, returned as the filled prefix.
pub fn format_u16(value: u16, buf: &mut [u8; 6]) -> &[u8] {
    let n = write_u16(value, buf, 0);
    &buf[..n]
}

/// Decimal ASCII digits of `value` (no leading zeros) written into `buf` from `n`, returned
/// as the index one past the last digit.
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
pub fn write_ratio(buf: &mut [u8; 6], subsample: u8) -> &[u8] {
    buf[0] = b'0' + (subsample >> 4);
    buf[1] = b':';
    buf[2] = b'0' + (subsample & 0x0f);
    &buf[..3]
}

/// A signed `i32`, magnitude first so `i32::MIN` has no special case.
pub fn format_i32(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_signed(value.unsigned_abs() as u64, value < 0, buf, 0);
    &buf[..n]
}

/// The sign, when there is one, then [`write_u64`]'s digits.
pub fn write_signed(magnitude: u64, negative: bool, buf: &mut [u8], mut n: usize) -> usize {
    if negative {
        buf[n] = b'-';
        n += 1;
    }
    write_u64(magnitude, buf, n)
}

/// A sound level as `<digits> dBA`.
pub fn format_dba(decibels: i16, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(decibels.unsigned_abs()), buf, 0);
    buf[n] = b' ';
    buf[n + 1..n + 4].copy_from_slice(b"dBA");
    &buf[..n + 4]
}

/// Tenths as `<whole>.<digit>`.
pub fn format_tenths(value: i32, buf: &mut [u8; 12]) -> &[u8] {
    let magnitude = value.unsigned_abs() as u64;
    let n = write_signed(magnitude / 10, value < 0, buf, 0);
    buf[n] = b'.';
    buf[n + 1] = b'0' + (magnitude % 10) as u8;
    &buf[..n + 2]
}

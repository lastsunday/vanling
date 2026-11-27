//! The two fixed readings a page prints in a value cell: a date and a clock time. Both write
//! into the caller's buffer, which is sized for the widest reading either can produce.

/// `YYYY-MM-DD`, at most eleven bytes.
pub fn date(year: u16, month: u8, day: u8, buf: &mut [u8; 12]) -> &[u8] {
    let mut digits = [0u8; 6];
    let text = iot_core::ui::text::format_u16(year, &mut digits);
    buf[..text.len()].copy_from_slice(text);
    buf[text.len()] = b'-';
    let at = two(buf, text.len() + 1, month);
    buf[at] = b'-';
    let at = two(buf, at + 1, day);
    &buf[..at]
}

/// `HH:MM:SS`, always eight bytes.
pub fn time(hour: u8, minute: u8, second: u8, buf: &mut [u8; 10]) -> &[u8] {
    let at = two(buf, 0, hour);
    buf[at] = b':';
    let at = two(buf, at + 1, minute);
    buf[at] = b':';
    let at = two(buf, at + 1, second);
    &buf[..at]
}

fn two(buf: &mut [u8], at: usize, value: u8) -> usize {
    buf[at] = b'0' + value / 10;
    buf[at + 1] = b'0' + value % 10;
    at + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_and_a_clock_keep_two_digits() {
        let mut date_buf = [0u8; 12];
        assert_eq!(date(2026, 1, 9, &mut date_buf), b"2026-01-09");
        let mut time_buf = [0u8; 10];
        assert_eq!(time(9, 5, 30, &mut time_buf), b"09:05:30");
    }

    #[test]
    fn the_widest_year_still_fits_the_buffer() {
        let mut buf = [0u8; 12];
        assert_eq!(date(u16::MAX, 12, 31, &mut buf), b"65535-12-31");
    }
}

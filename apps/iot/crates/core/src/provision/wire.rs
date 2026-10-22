//! Bytes written without a formatter.
//!
//! The two answers that are built rather than copied — a response head, and one row of the scan
//! list — have to name their length before any of it is written, and this crate carries no
//! formatting crate for either. A sink is what lets one piece of code answer both, so that a
//! length and the bytes it names cannot come out of two implementations that disagree.

/// Where an answer's bytes go.
pub(crate) trait Sink {
    /// Take `bytes`, or refuse them: `false` means no room was left.
    fn write(&mut self, bytes: &[u8]) -> bool;
}

/// A fixed buffer, which refuses what does not fit.
pub(crate) struct Bounded<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl<'a> Bounded<'a> {
    pub(crate) fn new(out: &'a mut [u8]) -> Self {
        Self { out, len: 0 }
    }

    /// What was taken — less than what was offered only when something was refused, which is
    /// left out rather than truncated.
    pub(crate) fn written(&self) -> usize {
        self.len
    }
}

impl Sink for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some(end) = self.len.checked_add(bytes.len()) else {
            return false;
        };
        if end > self.out.len() {
            return false;
        }
        self.out[self.len..end].copy_from_slice(bytes);
        self.len = end;
        true
    }
}

/// The same bytes, counted instead of kept.
#[derive(Default)]
pub(crate) struct Counting {
    len: usize,
}

impl Counting {
    pub(crate) fn written(&self) -> usize {
        self.len
    }
}

impl Sink for Counting {
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.len += bytes.len();
        true
    }
}

/// Decimal digits, written a digit at a time: this crate carries no formatting crate, and both
/// the response headers and the page's own JSON need a number.
pub(crate) fn push_usize(out: &mut impl Sink, mut value: usize) -> bool {
    let mut digits = [0u8; 20];
    let mut index = digits.len();
    loop {
        index -= 1;
        digits[index] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    out.write(&digits[index..])
}

/// The same, signed: a signal reading is the only negative number here.
pub(crate) fn push_i64(out: &mut impl Sink, value: i64) -> bool {
    (value >= 0 || out.write(b"-")) && push_usize(out, value.unsigned_abs() as usize)
}

//! Bytes written without a formatter.

pub(crate) trait Sink {
    fn write(&mut self, bytes: &[u8]) -> bool;
}

pub(crate) struct Bounded<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl<'a> Bounded<'a> {
    pub(crate) fn new(out: &'a mut [u8]) -> Self {
        Self { out, len: 0 }
    }

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

pub(crate) fn push_i64(out: &mut impl Sink, value: i64) -> bool {
    (value >= 0 || out.write(b"-")) && push_usize(out, value.unsigned_abs() as usize)
}

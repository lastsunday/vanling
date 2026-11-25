//! The panel's face: a 12 px bitmap table cut at build time by `iot-tool font`, covering
//! GB2312 plus Big5. A larger size is a whole-number magnification of it.

/// Where the table lives.
mod table {
    pub const BYTES: &[u8] = include_bytes!("../../assets/font/ui12.bin");
}

const MAGIC: &[u8; 4] = b"VLF1";
const HEADER: usize = 32;

/// The face the panel draws with.
#[derive(Clone, Copy, Debug, Default)]
pub struct Face;

impl Face {
    pub const fn new() -> Self {
        Self
    }

    /// Rows one line occupies.
    pub const fn height(&self) -> usize {
        table::BYTES[6] as usize
    }

    /// Rows above the baseline.
    pub const fn ascent(&self) -> usize {
        table::BYTES[7] as usize
    }

    /// Rows below the baseline.
    pub const fn descent(&self) -> usize {
        table::BYTES[8] as usize
    }

    /// The glyph for `ch`, or the fallback mark when the subset has none.
    pub fn glyph(&self, ch: char) -> Glyph<'static> {
        self.find(ch)
            .or_else(|| self.find(MISSING))
            .expect("the subset carries the fallback mark")
    }

    /// Whether the face itself carries `ch`, without the fallback.
    pub fn has(&self, ch: char) -> bool {
        self.find(ch).is_some()
    }

    /// Columns `ch` occupies, fallback included.
    pub fn advance(&self, ch: char) -> usize {
        self.glyph(ch).advance
    }

    /// Columns a run occupies, with no letter spacing.
    pub fn measure(&self, text: &[u8]) -> usize {
        self.glyphs(text).map(|glyph| glyph.advance).sum()
    }

    pub fn glyphs<'a>(&self, text: &'a [u8]) -> Glyphs<'a> {
        Glyphs {
            face: *self,
            chars: Chars::new(text),
        }
    }

    fn find(&self, ch: char) -> Option<Glyph<'static>> {
        let code = u16::try_from(u32::from(ch)).ok()?;
        self.index(code).map(|index| self.at(index))
    }

    fn index(&self, code: u16) -> Option<usize> {
        let count = self.count();
        let (mut lo, mut hi) = (0usize, count);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.code(mid).cmp(&code) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    fn count(&self) -> usize {
        usize::from(u16::from_le_bytes([table::BYTES[4], table::BYTES[5]]))
    }

    fn section(&self, at: usize) -> usize {
        u32::from_le_bytes([
            table::BYTES[at],
            table::BYTES[at + 1],
            table::BYTES[at + 2],
            table::BYTES[at + 3],
        ]) as usize
    }

    fn code(&self, index: usize) -> u16 {
        let at = self.section(12) + index * 2;
        u16::from_le_bytes([table::BYTES[at], table::BYTES[at + 1]])
    }

    fn at(&self, index: usize) -> Glyph<'static> {
        let meta = self.section(16) + index * 4;
        let packed = self.section(meta);
        let offset = self.section(self.section(20) + index * 4);
        let (width, height) = (packed >> 11 & 0x1f, packed >> 6 & 0x1f);
        let stride = width.div_ceil(8);
        let start = self.section(24) + offset;
        Glyph {
            advance: packed & 0x3f,
            width,
            height,
            left: signed((packed >> 16 & 0x1f) as u32),
            top: signed((packed >> 21 & 0x1f) as u32),
            bits: &table::BYTES[start..start + stride * height],
            stride,
        }
    }
}

/// A five-bit two's complement field.
fn signed(value: u32) -> isize {
    ((value << 27) as i32 >> 27) as isize
}

impl Face {
    /// Debug check that the table on disk is the one this reader understands.
    pub fn table_is_readable(&self) -> bool {
        table::BYTES.starts_with(MAGIC) && table::BYTES.len() >= HEADER && self.count() > 0
    }
}

/// What a code point the subset does not carry is drawn as.
pub const MISSING: char = '\u{25A1}';

/// A face at one size: the bitmap table magnified by whole steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    size: u16,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::at(Self::ROOT as u16)
    }
}

impl Metrics {
    /// The size the bitmap table is cut at.
    pub const ROOT: u8 = 12;

    pub const fn at(size: u16) -> Self {
        Self {
            size: if size == 0 { Self::ROOT as u16 } else { size },
        }
    }

    pub const fn size(&self) -> u16 {
        self.size
    }

    /// Whole magnification of the bitmap table.
    pub const fn scale(&self) -> usize {
        (self.size as usize).div_ceil(Self::ROOT as usize)
    }

    /// Rows one line's glyphs occupy.
    pub fn height(&self) -> usize {
        Face::new().height() * self.scale()
    }

    /// Rows above the baseline.
    pub fn ascent(&self) -> usize {
        Face::new().ascent() * self.scale()
    }

    /// Rows below the baseline.
    pub fn descent(&self) -> usize {
        Face::new().descent() * self.scale()
    }

    /// Columns `ch` occupies.
    pub fn advance(&self, ch: char) -> usize {
        Face::new().advance(ch) * self.scale()
    }

    /// Columns a run occupies.
    pub fn measure(&self, text: &[u8]) -> usize {
        Face::new().measure(text) * self.scale()
    }
}

/// One glyph: an advance, a box placed against the baseline, and rows of bits with the leftmost
/// pixel in the most significant bit.
#[derive(Clone, Copy, Debug)]
pub struct Glyph<'a> {
    pub advance: usize,
    pub width: usize,
    pub height: usize,
    /// Columns right of the pen the box starts at.
    pub left: isize,
    /// Rows above the baseline the bottom edge sits at.
    pub top: isize,
    pub bits: &'a [u8],
    stride: usize,
}

impl Glyph<'_> {
    /// Columns one row of bits occupies.
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// The bits of `row`, leftmost pixel in the most significant bit.
    pub fn row(&self, row: usize) -> &[u8] {
        &self.bits[row * self.stride..(row + 1) * self.stride]
    }

    pub fn coverage(&self, row: usize, column: usize) -> bool {
        self.row(row)[column / 8] & (0x80 >> (column % 8)) != 0
    }
}

/// The glyphs of a UTF-8 run, with invalid bytes and missing code points drawn as [`MISSING`].
pub struct Glyphs<'a> {
    face: Face,
    chars: Chars<'a>,
}

impl Iterator for Glyphs<'_> {
    type Item = Glyph<'static>;

    fn next(&mut self) -> Option<Self::Item> {
        let ch = self.chars.next()?;
        Some(self.face.glyph(ch))
    }
}

/// The code points of a UTF-8 run, with invalid bytes reported as the replacement character
/// so a corrupt run still advances.
#[derive(Clone, Copy, Debug)]
pub struct Chars<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Chars<'a> {
    pub const fn new(text: &'a [u8]) -> Self {
        Self { text, at: 0 }
    }
}

impl Iterator for Chars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.text.get(self.at..)?;
        if rest.is_empty() {
            return None;
        }
        match core::str::from_utf8(rest) {
            Ok(text) => {
                let ch = text.chars().next()?;
                self.at += ch.len_utf8();
                Some(ch)
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    let text = core::str::from_utf8(&rest[..valid]).expect("valid prefix");
                    let ch = text.chars().next()?;
                    self.at += ch.len_utf8();
                    return Some(ch);
                }
                self.at += error.error_len().unwrap_or(rest.len().max(1));
                Some(core::char::REPLACEMENT_CHARACTER)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_one_this_reader_understands() {
        let face = Face::new();
        assert!(face.table_is_readable(), "the table's header is not VLF1");
        assert_eq!((face.height(), face.ascent(), face.descent()), (12, 10, 2));
    }

    #[test]
    fn an_ascii_run_advances_by_its_glyphs_and_a_hanzi_by_a_whole_cell() {
        let face = Face::new();
        assert_eq!(face.advance('A'), 6);
        assert_eq!(face.advance('中'), 12);
        assert_eq!(face.measure(b"NETWORK"), 7 * 6);
        assert_eq!(face.measure("中文".as_bytes()), 24);
    }

    #[test]
    fn a_missing_glyph_is_drawn_as_the_fallback_mark_not_dropped() {
        let face = Face::new();
        let missing = face.glyph('\u{10FFFF}');
        assert_eq!(missing.advance, face.glyph(MISSING).advance);
        assert!(missing.bits == face.glyph(MISSING).bits);
    }

    #[test]
    fn invalid_utf8_advances_instead_of_stalling() {
        let face = Face::new();
        assert_eq!(face.measure(b"a\xffb"), 6 + 12 + 6);
    }

    #[test]
    fn every_printable_ascii_is_one_cell() {
        let face = Face::new();
        for code in 0x20..=0x7E {
            let ch = char::from_u32(code).expect("ascii");
            assert!(face.has(ch), "ASCII {code:#04x} is missing");
            assert_eq!(face.advance(ch), 6, "ASCII {code:#04x} is not one cell");
        }
    }

    #[test]
    fn simplified_and_traditional_hanzi_are_two_cells() {
        let face = Face::new();
        for ch in "中文网络網路設定测试測試繁體简体臺灣".chars() {
            assert!(face.has(ch), "{ch} is missing");
            assert_eq!(face.advance(ch), 12, "{ch} is not a whole cell");
        }
    }

    #[test]
    fn the_pages_sample_of_traditional_text_is_all_present() {
        let face = Face::new();
        let sample =
            "繁體中文網路設定測試無線路由器臺灣中華電信我的最愛分享熱點訊號強度密碼連線成功失敗";
        let missing: alloc::string::String = sample.chars().filter(|ch| !face.has(*ch)).collect();
        assert!(missing.is_empty(), "missing {missing}");
    }

    #[test]
    fn the_grid_face_keeps_its_cell() {
        assert_eq!(Metrics::at(12).height(), 12);
        assert_eq!(crate::ui::typeset::line_height(Metrics::at(12)), 12);
    }

    #[test]
    fn a_bigger_size_is_a_whole_magnification() {
        let metrics = Metrics::at(24);
        assert_eq!(
            (metrics.ascent(), metrics.descent(), metrics.height()),
            (20, 4, 24)
        );
        assert_eq!(metrics.advance('中'), 24);
        assert_eq!(metrics.advance('A'), 12);
        assert_eq!(metrics.measure("中A".as_bytes()), 36);
        assert_eq!(Metrics::at(36).advance('A'), 18);
    }

    #[test]
    fn a_hanzi_keeps_the_bdf_box() {
        let face = Face::new();
        let glyph = face.glyph('中');
        assert_eq!(
            (glyph.width, glyph.height, glyph.left, glyph.top),
            (11, 11, 0, -2)
        );
        assert!(
            glyph.coverage(2, 0) || glyph.coverage(2, 10),
            "the box's frame is covered"
        );
    }
}

//! The panel's faces: 12 px glyphs, and the wrap region's outlines.
//!
//! The bitmap table comes from the subset BDF (`cargo run -p iot-tool -- font`): GB2312 plus
//! Big5, 15,466 code points, of which 866 Big5 points have no upstream glyph and draw as
//! [`MISSING`]. The vector table is cut by `iot-tool outline` from the release Noto Sans Mono
//! CJK SC for exactly `vector-sample.txt`; coordinates are 1/64 em, y up from the baseline.

/// Where the tables and their sources live.
mod table {
    pub const BYTES: &[u8] = include_bytes!("../../assets/font/ui12.bin");
}

mod vector {
    pub const BYTES: &[u8] = include_bytes!("../../assets/font/vector.bin");
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

/// Which face a run is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontFamily {
    /// The 12 px table, magnified by whole steps.
    #[default]
    Bitmap,
    /// The outlines, at the exact size asked for.
    Vector,
}

/// A face at one size. The bitmap table is cut at 12 px, so a larger size is a whole-number
/// magnification of it; the vector face takes the exact size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    size: u16,
    family: FontFamily,
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
            family: FontFamily::Bitmap,
        }
    }

    pub const fn vector(size: u16) -> Self {
        Self::new(size, FontFamily::Vector)
    }

    pub const fn new(size: u16, family: FontFamily) -> Self {
        Self {
            size: if size == 0 { Self::ROOT as u16 } else { size },
            family,
        }
    }

    pub const fn family(&self) -> FontFamily {
        self.family
    }

    pub const fn size(&self) -> u16 {
        self.size
    }

    /// Whole magnification of the bitmap table.
    pub const fn scale(&self) -> usize {
        (self.size as usize).div_ceil(Self::ROOT as usize)
    }

    /// Rows one line's ink occupies.
    pub fn height(&self) -> usize {
        self.ascent() + self.descent()
    }

    /// Rows of air the face keeps under its ink, so two lines do not touch. The grid face is
    /// cut to its cell and needs none.
    pub fn leading(&self) -> usize {
        match self.family {
            FontFamily::Bitmap => 0,
            FontFamily::Vector => usize::from(self.size) / 8,
        }
    }

    /// Rows above the baseline.
    pub fn ascent(&self) -> usize {
        match self.family {
            FontFamily::Bitmap => Face::new().ascent() * self.scale(),
            FontFamily::Vector => scaled(VectorFace::new().ascent_units(), self.size),
        }
    }

    /// Rows below the baseline.
    pub fn descent(&self) -> usize {
        match self.family {
            FontFamily::Bitmap => Face::new().descent() * self.scale(),
            FontFamily::Vector => scaled(VectorFace::new().descent_units(), self.size),
        }
    }

    /// Columns `ch` occupies.
    pub fn advance(&self, ch: char) -> usize {
        match self.family {
            FontFamily::Bitmap => Face::new().advance(ch) * self.scale(),
            FontFamily::Vector => VectorFace::new().advance_units(ch).map_or_else(
                || Face::new().advance(ch) * self.scale(),
                |units| scaled(units, self.size),
            ),
        }
    }

    /// Columns a run occupies.
    pub fn measure(&self, text: &[u8]) -> usize {
        match self.family {
            FontFamily::Bitmap => Face::new().measure(text) * self.scale(),
            FontFamily::Vector => Chars::new(text).map(|ch| self.advance(ch)).sum(),
        }
    }

    /// This face for `run`, falling back to the bitmap table when the vector one lacks a
    /// glyph: a line is never half one face and half the other.
    pub fn covering(self, run: &[u8]) -> Self {
        if self.family == FontFamily::Vector && !VectorFace::new().covers(run) {
            Self::at(self.size)
        } else {
            self
        }
    }
}

/// One em-denominated metric in pixels, rounded to the nearest.
fn scaled(units: u16, size: u16) -> usize {
    ((u32::from(units) * u32::from(size) + 32) / 64) as usize
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

    pub fn ink(&self, row: usize, column: usize) -> bool {
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

/// The outline face: a table of flattened contours the canvas fills.
#[derive(Clone, Copy, Debug, Default)]
pub struct VectorFace;

const VMAGIC: &[u8; 4] = b"VLV1";
const VHEADER: usize = 32;
/// The table's units per em. A unit is 3/8 px at 24 px.
const EM: u32 = 64;
/// One glyph's meta: advance, point range, contour range, and the box the ink occupies.
const VMETA: usize = 18;

impl VectorFace {
    pub const fn new() -> Self {
        Self
    }

    /// Whether the face carries `ch` as a glyph with ink or as a blank cell.
    pub fn has(&self, ch: char) -> bool {
        self.find(ch).is_some()
    }

    /// Whether every drawn code point of `run` is in the table. A line break is not a glyph.
    pub fn covers(&self, run: &[u8]) -> bool {
        Chars::new(run).all(|ch| ch.is_control() || self.has(ch))
    }

    pub fn outline(&self, ch: char) -> Option<Outline<'_>> {
        let index = self.find(ch)?;
        let meta = self.section(16) as usize + index * VMETA;
        let advance = self.u16_at(meta);
        let first = usize::from(self.u16_at(meta + 2));
        let last = usize::from(self.u16_at(meta + 4));
        Some(Outline {
            table: vector::BYTES,
            points: self.section(20) as usize,
            contours: self.section(24) as usize,
            first,
            last,
            contour_first: usize::from(self.u16_at(meta + 6)),
            contour_last: usize::from(self.u16_at(meta + 8)),
            advance,
            xmin: self.i16_at(meta + 10) as isize,
            ymin: self.i16_at(meta + 12) as isize,
            xmax: self.i16_at(meta + 14) as isize,
            ymax: self.i16_at(meta + 16) as isize,
        })
    }

    pub fn advance_units(&self, ch: char) -> Option<u16> {
        let index = self.find(ch)?;
        Some(self.u16_at(self.section(16) as usize + index * VMETA))
    }

    /// Every code point the table carries, in table order.
    pub fn code_points(&self) -> impl Iterator<Item = char> + '_ {
        (0..self.code_count()).filter_map(move |index| {
            let code = self.u16_at(self.section(12) as usize + index * 4);
            char::from_u32(u32::from(code))
        })
    }

    pub fn ascent_units(&self) -> u16 {
        self.u16_at(28)
    }

    pub fn descent_units(&self) -> u16 {
        self.u16_at(30)
    }

    /// Debug check that the table on disk is the one this reader understands.
    pub fn table_is_readable(&self) -> bool {
        vector::BYTES.starts_with(VMAGIC)
            && vector::BYTES.len() >= VHEADER
            && self.u16_at(8) == EM as u16
            && self.count() > 0
    }

    fn count(&self) -> usize {
        usize::from(self.u16_at(4))
    }

    fn code_count(&self) -> usize {
        usize::from(self.u16_at(6))
    }

    fn find(&self, ch: char) -> Option<usize> {
        let code = u16::try_from(u32::from(ch)).ok()?;
        let codes = self.section(12) as usize;
        let (mut lo, mut hi) = (0usize, self.code_count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = codes + mid * 4;
            match self.u16_at(at).cmp(&code) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(usize::from(self.u16_at(at + 2))),
            }
        }
        None
    }

    fn section(&self, at: usize) -> u32 {
        u32::from_le_bytes([
            vector::BYTES[at],
            vector::BYTES[at + 1],
            vector::BYTES[at + 2],
            vector::BYTES[at + 3],
        ])
    }

    fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes([vector::BYTES[at], vector::BYTES[at + 1]])
    }

    fn i16_at(&self, at: usize) -> i16 {
        self.u16_at(at) as i16
    }
}

/// One glyph's flattened contours, read from the table without copying them into RAM.
#[derive(Clone, Copy, Debug)]
pub struct Outline<'a> {
    table: &'a [u8],
    points: usize,
    contours: usize,
    first: usize,
    last: usize,
    contour_first: usize,
    contour_last: usize,
    pub advance: u16,
    pub xmin: isize,
    pub ymin: isize,
    pub xmax: isize,
    pub ymax: isize,
}

impl Outline<'_> {
    /// Whether the glyph has any ink at all; a space has none.
    pub fn is_blank(&self) -> bool {
        self.last <= self.first
    }

    pub fn points(&self) -> core::ops::Range<usize> {
        self.first..self.last
    }

    /// One point in 1/64 em, y up from the baseline.
    pub fn point(&self, index: usize) -> (i32, i32) {
        let at = self.points + index * 4;
        (
            i32::from(u16::from_le_bytes([self.table[at], self.table[at + 1]]) as i16),
            i32::from(u16::from_le_bytes([self.table[at + 2], self.table[at + 3]]) as i16),
        )
    }

    pub fn contours(&self) -> core::ops::Range<usize> {
        self.contour_first..self.contour_last
    }

    /// The exclusive end of contour `index` in [`Outline::points`].
    pub fn contour_end(&self, index: usize) -> usize {
        let at = self.contours + index * 2;
        usize::from(u16::from_le_bytes([self.table[at], self.table[at + 1]]))
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
    fn no_outline_is_wider_than_the_canvas_scratch() {
        let face = VectorFace::new();
        let codes = face.section(12) as usize;
        for index in 0..face.code_count() {
            let code = face.u16_at(codes + index * 4);
            let ch = char::from_u32(u32::from(code)).expect("a table code point");
            let outline = face
                .outline(ch)
                .expect("a listed code point has an outline");
            assert!(
                outline.points().len() <= crate::ui::canvas::OUTLINE_POINTS,
                "{ch} holds {} points",
                outline.points().len()
            );
        }
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
    fn the_vector_table_is_the_one_this_reader_understands() {
        let face = VectorFace::new();
        assert!(face.table_is_readable(), "the table's header is not VLV1");
        assert_eq!((face.ascent_units(), face.descent_units()), (56, 15));
        for ch in "中A…□".chars() {
            assert!(face.has(ch), "{ch} is missing");
        }
        assert!(!face.has('\u{1F004}'), "outside the table's code space");
    }

    /// The line box is what every page's rhythm is built on: if one glyph reached past it,
    /// that glyph's ink would be shaved by whatever box the caller drew it in.
    #[test]
    fn the_tables_line_box_encloses_every_glyph() {
        let face = VectorFace::new();
        let (ascent, descent) = (
            isize::from(face.ascent_units() as i16),
            isize::from(face.descent_units() as i16),
        );
        for ch in face.code_points() {
            let outline = face
                .outline(ch)
                .expect("a listed code point has an outline");
            assert!(
                outline.ymax <= ascent,
                "{ch} reaches {} above",
                outline.ymax
            );
            assert!(
                -outline.ymin <= descent,
                "{ch} reaches {} below",
                -outline.ymin
            );
        }
    }

    #[test]
    fn a_vector_line_is_the_ink_box_plus_leading() {
        let metrics = Metrics::vector(24);
        assert_eq!(
            (metrics.ascent(), metrics.descent(), metrics.height()),
            (21, 6, 27)
        );
        assert_eq!(metrics.leading(), 3);
        assert_eq!(crate::ui::typeset::line_height(metrics), 30);
        assert_eq!(metrics.advance('中'), 24);
        assert_eq!(metrics.advance('A'), 12);
        assert_eq!(metrics.measure("中A".as_bytes()), 36);
        assert_eq!(Metrics::vector(48).advance('A'), 24, "exact, not magnified");
    }

    #[test]
    fn the_grid_face_keeps_its_cell_and_takes_no_leading() {
        assert_eq!(Metrics::at(12).leading(), 0);
        assert_eq!(crate::ui::typeset::line_height(Metrics::at(12)), 12);
    }

    #[test]
    fn a_run_the_vector_face_cannot_cover_falls_back_whole() {
        let metrics = Metrics::vector(24);
        assert_eq!(metrics.covering("中A".as_bytes()), metrics);
        let fallen = metrics.covering("\u{1F004}".as_bytes());
        assert_eq!(fallen.family(), FontFamily::Bitmap);
        assert_eq!(fallen.size(), 24);
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
            glyph.ink(2, 0) || glyph.ink(2, 10),
            "the box's frame is inked"
        );
    }
}

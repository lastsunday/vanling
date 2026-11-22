//! The vector face: the release OTF's curves flattened into the polylines the panel fills.
//!
//! Cuts the wrap region's code points out of `NotoSansMonoCJKsc-Regular.otf` and writes
//! `vector.bin` for `iot-core/src/ui/font.rs`. Coordinates are 1/64 em, y up from the
//! baseline; 1/4 em rounds a 24 px glyph to 6 px steps, hence the finer table.
//!
//! File layout (little-endian):
//!
//! ```text
//! 0..4   "VLV1"
//! 4..6   glyph count
//! 6..8   code count
//! 8..10  units per em (64)
//! 12..16 codes offset      [u16 code, u16 glyph]
//! 16..20 meta offset       [u16 advance, u16 first, u16 last, u16 contour_first,
//!                           u16 contour_last, i16 xmin, i16 ymin, i16 xmax, i16 ymax]
//! 20..24 points offset     [i16 x, i16 y]
//! 24..28 contours offset   [u16 end, an absolute index into points]
//! 28..30 ascent
//! 30..32 descent
//! ```
//!
//! Ascent/descent is the line box, not the typographic pair: each is widened to enclose
//! every cut glyph's ink.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::font::fonts_dir;

const MAGIC: &[u8; 4] = b"VLV1";
/// Units per em the table stores. A unit is 3/8 px at 24 px.
const EM: f64 = 64.0;
/// How far a flattened segment may stray from its curve, in stored units.
const TOLERANCE: f64 = 0.3;
const MAX_DEPTH: u8 = 5;

/// The paragraph the wrap region draws; the page includes the same bytes.
pub fn sample_path() -> PathBuf {
    crate_dir().join("../core/assets/font/vector-sample.txt")
}

/// The table the device includes.
pub fn table_path() -> PathBuf {
    crate_dir().join("../core/assets/font/vector.bin")
}

fn source_path() -> PathBuf {
    fonts_dir().join("NotoSansMonoCJKsc-Regular.otf")
}

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Cut the sample's glyphs and write the table.
pub fn write() -> Result<(), String> {
    let sample = std::fs::read(sample_path()).map_err(|e| e.to_string())?;
    let text =
        core::str::from_utf8(&sample).map_err(|e| format!("the sample is not UTF-8: {e}"))?;
    let mut wanted: Vec<char> = text.chars().filter(|ch| !ch.is_control()).collect();
    // The page draws these for a truncated paragraph and for an uncovered code point.
    wanted.extend(['…', '□']);
    wanted.sort_unstable();
    wanted.dedup();

    let data = std::fs::read(source_path()).map_err(|e| e.to_string())?;
    let face = Face::parse(&data, 0).map_err(|e| format!("{}: {e}", source_path().display()))?;
    let units = f64::from(face.units_per_em());

    let mut by_glyph: BTreeMap<GlyphId, usize> = BTreeMap::new();
    let mut glyphs = Vec::new();
    let mut codes: Vec<(u16, u16)> = Vec::new();
    for ch in wanted {
        let Some(code) = u16::try_from(u32::from(ch)).ok() else {
            return Err(format!("{ch} is outside the table's 16-bit code space"));
        };
        let Some(id) = face.glyph_index(ch) else {
            return Err(format!("U+{code:04X} has no glyph in the release font"));
        };
        let index = match by_glyph.get(&id) {
            Some(index) => *index,
            None => {
                let mut outliner = Outliner::new(units);
                // A blank glyph — the space — carries no contour and is still a cell.
                if face.outline_glyph(id, &mut outliner).is_none()
                    && outliner.points.is_empty()
                    && ch != ' '
                {
                    return Err(format!("U+{code:04X} has no outline"));
                }
                let glyph = outliner.finish(face.glyph_hor_advance(id).unwrap_or(0), units);
                glyphs.push(glyph);
                let index = glyphs.len() - 1;
                by_glyph.insert(id, index);
                index
            }
        };
        codes.push((code, index as u16));
    }
    if glyphs.len() > u16::MAX as usize {
        return Err("more glyphs than a 16-bit index holds".into());
    }
    codes.sort_unstable_by_key(|(code, _)| *code);
    for pair in codes.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(format!("U+{:04X} is listed twice", pair[0].0));
        }
    }

    // The typographic pair is the one line height is meant to come from; hhea on this face
    // reserves CJK ink headroom and would set every line a third taller than the text. The
    // pair is widened below to the ink this table actually carries: `g` reaches 15 units
    // under the baseline while the typographic descender reserves 8, so the pair alone would
    // shave every descender.
    let (ascender, descender) = (
        face.typographic_ascender()
            .unwrap_or_else(|| face.ascender()),
        face.typographic_descender()
            .unwrap_or_else(|| face.descender()),
    );
    let (ink_top, ink_bottom) = glyphs.iter().fold((0i16, 0i16), |(top, bottom), glyph| {
        (top.max(glyph.ymax), bottom.min(glyph.ymin))
    });
    let ascent = u16::try_from((f64::from(ascender) * EM / units).round() as i32)
        .map_err(|_| "ascent out of range")?
        .max(u16::try_from(ink_top.max(0)).map_err(|_| "ink ascent out of range")?);
    let descent = u16::try_from((f64::from(-descender) * EM / units).round() as i32)
        .map_err(|_| "descent out of range")?
        .max(u16::try_from(-i32::from(ink_bottom.min(0))).map_err(|_| "ink descent out of range")?);
    let table = encode(&glyphs, &codes, ascent, descent)?;
    let path = table_path();
    std::fs::create_dir_all(path.parent().expect("the table has a directory"))
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{} glyphs for {} code points, {} points, {} bytes, {}",
        glyphs.len(),
        codes.len(),
        glyphs.iter().map(|glyph| glyph.points.len()).sum::<usize>(),
        table.len(),
        path.display()
    );
    Ok(())
}

struct Glyph {
    advance: u16,
    points: Vec<(i16, i16)>,
    /// Exclusive end index of each contour in `points`.
    contours: Vec<u16>,
    xmin: i16,
    ymin: i16,
    xmax: i16,
    ymax: i16,
}

/// Collects a glyph's outline, flattening every curve into points against the baseline.
struct Outliner {
    units: f64,
    points: Vec<(i16, i16)>,
    contours: Vec<u16>,
    start: Option<usize>,
    last: (f64, f64),
}

impl Outliner {
    fn new(units: f64) -> Self {
        Self {
            units,
            points: Vec::new(),
            contours: Vec::new(),
            start: None,
            last: (0.0, 0.0),
        }
    }

    fn push(&mut self, x: f64, y: f64) {
        let scale = EM / self.units;
        let point = ((x * scale).round() as i16, (y * scale).round() as i16);
        if self.points.last() != Some(&point) {
            self.points.push(point);
        }
        self.last = (x, y);
    }

    /// Ends the contour the pen is on, dropping the closing point when it repeats the first:
    /// the filler closes every contour itself.
    fn end_contour(&mut self) {
        let Some(start) = self.start.take() else {
            return;
        };
        if self.points.len() > start && self.points.last() == self.points.get(start) {
            self.points.pop();
        }
        self.contours.push(self.points.len() as u16);
    }

    fn curve(&mut self, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), depth: u8) {
        if depth >= MAX_DEPTH || flat(p0, p1, p2, p3) {
            self.push(p3.0, p3.1);
            return;
        }
        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (a, b, c) = (mid(p0, p1), mid(p1, p2), mid(p2, p3));
        let (ab, bc) = (mid(a, b), mid(b, c));
        let centre = mid(ab, bc);
        self.curve(p0, a, ab, centre, depth + 1);
        self.curve(centre, bc, c, p3, depth + 1);
    }

    fn finish(mut self, advance: u16, units: f64) -> Glyph {
        self.end_contour();
        let bounds = self.points.iter().fold(
            (i16::MAX, i16::MAX, i16::MIN, i16::MIN),
            |(x0, y0, x1, y1), (x, y)| (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)),
        );
        let bounds = if self.points.is_empty() {
            (0, 0, 0, 0)
        } else {
            bounds
        };
        Glyph {
            advance: (f64::from(advance) * EM / units).round() as u16,
            points: self.points,
            contours: self.contours,
            xmin: bounds.0,
            ymin: bounds.1,
            xmax: bounds.2,
            ymax: bounds.3,
        }
    }
}

impl OutlineBuilder for Outliner {
    fn move_to(&mut self, x: f32, y: f32) {
        self.end_contour();
        let (x, y) = (f64::from(x), f64::from(y));
        self.start = Some(self.points.len());
        self.push(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(f64::from(x), f64::from(y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last;
        let p1 = (f64::from(x1), f64::from(y1));
        let p2 = (f64::from(x), f64::from(y));
        // The cubic equivalent of a quadratic keeps the curve's shape.
        let c1 = (
            p0.0 + 2.0 / 3.0 * (p1.0 - p0.0),
            p0.1 + 2.0 / 3.0 * (p1.1 - p0.1),
        );
        let c2 = (
            p2.0 + 2.0 / 3.0 * (p1.0 - p2.0),
            p2.1 + 2.0 / 3.0 * (p1.1 - p2.1),
        );
        self.curve(p0, c1, c2, p2, 0);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.last;
        let p1 = (f64::from(x1), f64::from(y1));
        let p2 = (f64::from(x2), f64::from(y2));
        let p3 = (f64::from(x), f64::from(y));
        self.curve(p0, p1, p2, p3, 0);
    }

    fn close(&mut self) {
        self.end_contour();
    }
}

/// Whether the cubic is close enough to its chord to stand as a straight line.
fn flat(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) -> bool {
    let dx = p3.0 - p0.0;
    let dy = p3.1 - p0.1;
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0.0 {
        return distance(p0, p1) <= TOLERANCE && distance(p0, p2) <= TOLERANCE;
    }
    let side = |p: (f64, f64)| ((p.0 - p0.0) * dy - (p.1 - p0.1) * dx).abs() / length;
    side(p1) <= TOLERANCE && side(p2) <= TOLERANCE
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn encode(
    glyphs: &[Glyph],
    codes: &[(u16, u16)],
    ascent: u16,
    descent: u16,
) -> Result<Vec<u8>, String> {
    const HEADER: usize = 32;
    const META: usize = 18;
    let codes_at = HEADER;
    let meta_at = codes_at + codes.len() * 4;
    let points_at = meta_at + glyphs.len() * META;
    let points_total: usize = glyphs.iter().map(|glyph| glyph.points.len()).sum();
    let contour_total: usize = glyphs.iter().map(|glyph| glyph.contours.len()).sum();
    let contours_at = points_at + points_total * 4;
    let mut table = vec![0u8; contours_at + contour_total * 2];

    table[0..4].copy_from_slice(MAGIC);
    put16(&mut table, 4, glyphs.len() as u16);
    put16(&mut table, 6, codes.len() as u16);
    put16(&mut table, 8, EM as u16);
    put32(&mut table, 12, codes_at as u32);
    put32(&mut table, 16, meta_at as u32);
    put32(&mut table, 20, points_at as u32);
    put32(&mut table, 24, contours_at as u32);
    put16(&mut table, 28, ascent);
    put16(&mut table, 30, descent);

    for (slot, (code, glyph)) in codes.iter().enumerate() {
        put16(&mut table, codes_at + slot * 4, *code);
        put16(&mut table, codes_at + slot * 4 + 2, *glyph);
    }

    let mut point_at = 0usize;
    let mut contour_at = 0usize;
    for (index, glyph) in glyphs.iter().enumerate() {
        if point_at > u16::MAX as usize || contour_at > u16::MAX as usize {
            return Err("more than 65,535 points or contours".into());
        }
        let meta = meta_at + index * META;
        put16(&mut table, meta, glyph.advance);
        put16(&mut table, meta + 2, point_at as u16);
        put16(&mut table, meta + 4, (point_at + glyph.points.len()) as u16);
        put16(&mut table, meta + 6, contour_at as u16);
        put16(
            &mut table,
            meta + 8,
            (contour_at + glyph.contours.len()) as u16,
        );
        put16(&mut table, meta + 10, glyph.xmin as u16);
        put16(&mut table, meta + 12, glyph.ymin as u16);
        put16(&mut table, meta + 14, glyph.xmax as u16);
        put16(&mut table, meta + 16, glyph.ymax as u16);
        for (slot, (x, y)) in glyph.points.iter().enumerate() {
            put16(&mut table, points_at + (point_at + slot) * 4, *x as u16);
            put16(&mut table, points_at + (point_at + slot) * 4 + 2, *y as u16);
        }
        for (slot, end) in glyph.contours.iter().enumerate() {
            // Contours name absolute point indices, so a reader walks the shared point array.
            let end = *end as usize + point_at;
            if end > u16::MAX as usize {
                return Err("more than 65,535 points".into());
            }
            put16(
                &mut table,
                contours_at + (contour_at + slot) * 2,
                end as u16,
            );
        }
        point_at += glyph.points.len();
        contour_at += glyph.contours.len();
    }
    debug_assert_eq!((point_at, contour_at), (points_total, contour_total));
    Ok(table)
}

fn put16(table: &mut [u8], at: usize, value: u16) {
    table[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(table: &mut [u8], at: usize, value: u32) {
    table[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_round_trips_through_its_header() {
        let glyphs = vec![Glyph {
            advance: 64,
            points: vec![(0, 0), (48, 0), (48, 56), (0, 0)],
            contours: vec![3],
            xmin: 0,
            ymin: 0,
            xmax: 48,
            ymax: 56,
        }];
        let table = encode(&glyphs, &[(0x4E2D, 0)], 56, 8).unwrap();
        assert_eq!(&table[0..4], MAGIC);
        assert_eq!(get16(&table, 4), 1);
        assert_eq!(get16(&table, 8), 64);
        let meta = get32(&table, 16) as usize;
        assert_eq!(get16(&table, meta), 64, "advance");
        assert_eq!(
            (get16(&table, meta + 14), get16(&table, meta + 16)),
            (48, 56)
        );
    }

    #[test]
    fn the_sample_has_the_glyphs_the_page_needs() {
        let sample = std::fs::read_to_string(sample_path()).unwrap();
        assert!(sample.contains('\n') && sample.contains("\r\n"));
        assert!(
            sample.contains('中') && sample.contains("English") && sample.contains('α'),
            "the page mixes scripts and the table is cut from this one file"
        );
    }

    fn get16(table: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([table[at], table[at + 1]])
    }

    fn get32(table: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([table[at], table[at + 1], table[at + 2], table[at + 3]])
    }
}

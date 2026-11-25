//! The panel's glyphs: the subset BDF as the source, the binary table as the device's copy.
//!
//! `font` rebuilds the table from the committed BDF; `font --from <release bdf>` re-cuts the
//! subset first, keeping the coverage the committed one has; `font --merge <bdf> --chars <list>`
//! adds the listed code points from another release. The table format is read by
//! `iot-core/src/ui/font.rs`.

use std::path::{Path, PathBuf};

/// Where the source BDF, its licences and the generated table live.
pub fn fonts_dir() -> PathBuf {
    crate_dir().join("assets/fonts")
}

/// The table the device includes; it lives in `iot-core` because it is compiled into firmware.
pub fn table_path() -> PathBuf {
    crate_dir().join("../core/assets/font/ui12.bin")
}

fn source_path() -> PathBuf {
    fonts_dir().join("ui12.bdf")
}

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

const MAGIC: &[u8; 4] = b"VLF1";
const HEADER: usize = 32;
const PIXELS: u8 = 12;
const ASCENT: u8 = 10;
const DESCENT: u8 = 2;

/// One glyph as the BDF states it: an advance, a box placed against the baseline, and rows of
/// bits with the leftmost pixel in the most significant bit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub code: u16,
    pub advance: u8,
    pub width: u8,
    pub height: u8,
    /// Left edge of the box, in pixels right of the pen.
    pub left: i8,
    /// Bottom edge of the box, in pixels above the baseline.
    pub top: i8,
    pub bits: Vec<u8>,
}

impl Glyph {
    /// Bytes one row of this glyph occupies.
    pub fn stride(&self) -> usize {
        usize::from(self.width).div_ceil(8)
    }
}

pub fn write() -> Result<(), String> {
    let glyphs = read_source()?;
    let table = encode(&glyphs)?;
    let path = table_path();
    std::fs::create_dir_all(path.parent().expect("the table has a directory"))
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{} glyphs, {} bytes, {}",
        glyphs.len(),
        table.len(),
        path.display()
    );
    Ok(())
}

fn read_source() -> Result<Vec<Glyph>, String> {
    let path = source_path();
    let bdf = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&bdf)
}

/// Re-cut the subset from a full release BDF, keeping the coverage the committed one has.
pub fn recut(full: &Path) -> Result<(), String> {
    let keep: Vec<u16> = read_source()?.iter().map(|glyph| glyph.code).collect();
    let text = std::fs::read_to_string(full).map_err(|e| format!("{}: {e}", full.display()))?;
    let release = parse(&text)?;
    let mut glyphs = Vec::with_capacity(keep.len());
    for code in &keep {
        let glyph = release
            .binary_search_by_key(code, |glyph| glyph.code)
            .map(|index| release[index].clone())
            .map_err(|_| format!("U+{code:04X} is not in {}", full.display()))?;
        glyphs.push(glyph);
    }
    write_bdf(&text, &glyphs)
}

/// Add every code point in `chars` from `full`, keeping the glyphs the subset already has.
pub fn merge(full: &Path, chars: &Path, allow_missing: bool) -> Result<(), String> {
    let mut glyphs = read_source()?;
    let wanted = read_chars(chars)?;
    let text = std::fs::read_to_string(full).map_err(|e| format!("{}: {e}", full.display()))?;
    let release = parse(&text)?;
    let (mut added, mut already, mut moved) = (0usize, 0usize, 0usize);
    let mut absent = Vec::new();
    for code in &wanted {
        let found = release.binary_search_by_key(code, |glyph| glyph.code);
        match (glyphs.binary_search_by_key(code, |glyph| glyph.code), found) {
            (Ok(index), Ok(at)) => {
                let (ours, theirs) = (&glyphs[index], &release[at]);
                // The advance is the cell the page's layout depends on; a regional variant may
                // legitimately place the same glyph inside a different box, so only that is held.
                if ours.advance != theirs.advance {
                    return Err(format!(
                        "U+{code:04X} advances {} here and {} in {}",
                        ours.advance,
                        theirs.advance,
                        full.display()
                    ));
                }
                if box_of(ours) != box_of(theirs) {
                    moved += 1;
                }
                already += 1;
            }
            (Err(at), Ok(index)) => {
                glyphs.insert(at, release[index].clone());
                added += 1;
            }
            (_, Err(_)) => absent.push(*code),
        }
    }
    if !absent.is_empty() && !allow_missing {
        return Err(format!(
            "{} listed code points are not in {}: {:04X?} (pass --allow-missing to merge the rest)",
            absent.len(),
            full.display(),
            &absent[..absent.len().min(8)]
        ));
    }
    if !absent.is_empty() {
        println!(
            "{} listed code points are not in {}: {:04X?}",
            absent.len(),
            full.display(),
            &absent[..absent.len().min(8)]
        );
    }
    write_bdf(&text, &glyphs)?;
    println!(
        "{added} added, {already} already ({moved} with a variant box), {} total",
        glyphs.len()
    );
    write()
}

fn box_of(glyph: &Glyph) -> (u8, u8, i8, i8) {
    (glyph.width, glyph.height, glyph.left, glyph.top)
}

/// The listed hexadecimal code points, sorted and de-duplicated.
fn read_chars(path: &Path) -> Result<Vec<u16>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut codes = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let code = u16::from_str_radix(line, 16).map_err(|_| {
            format!(
                "{}:{}: {line:?} is not a code point",
                path.display(),
                number + 1
            )
        })?;
        codes.push(code);
    }
    codes.sort_unstable();
    codes.dedup();
    Ok(codes)
}

/// Write the subset BDF with `CHARS` set to what it actually holds.
fn write_bdf(release: &str, glyphs: &[Glyph]) -> Result<(), String> {
    let mut out = head_of(release, glyphs.len());
    for glyph in glyphs {
        out.push_str(&block(glyph));
    }
    let path = source_path();
    std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// The BDF's own preamble, with `CHARS` set to what the subset keeps.
fn head_of(text: &str, chars: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("STARTCHAR") {
            break;
        }
        if line.starts_with("CHARS ") {
            writeln!(out, "CHARS {chars}").expect("writing to a String");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn block(glyph: &Glyph) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    writeln!(out, "STARTCHAR U+{:04X}", glyph.code).expect("writing to a String");
    writeln!(out, "ENCODING {}", glyph.code).expect("writing to a String");
    writeln!(
        out,
        "SWIDTH {} 0",
        u32::from(glyph.advance) * 1000 / u32::from(PIXELS)
    )
    .expect("string");
    writeln!(out, "DWIDTH {} 0", glyph.advance).expect("writing to a String");
    writeln!(
        out,
        "BBX {} {} {} {}",
        glyph.width, glyph.height, glyph.left, glyph.top
    )
    .expect("string");
    out.push_str("BITMAP\n");
    let stride = glyph.stride();
    for row in glyph.bits.chunks(stride.max(1)) {
        for byte in row {
            write!(out, "{byte:02X}").expect("writing to a String");
        }
        out.push('\n');
    }
    out.push_str("ENDCHAR\n");
    out
}

/// Every glyph the BDF states, sorted by code point.
pub fn parse(bdf: &str) -> Result<Vec<Glyph>, String> {
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut pending: Option<Glyph> = None;
    let mut rows: Vec<u8> = Vec::new();
    let mut in_bitmap = false;
    for line in bdf.lines() {
        let line = line.trim_end();
        if line.starts_with("STARTCHAR") {
            pending = None;
            in_bitmap = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("ENCODING ") {
            let code: i32 = rest
                .trim()
                .parse()
                .map_err(|_| format!("encoding {rest:?} is not a number"))?;
            pending = u16::try_from(code)
                .ok()
                .filter(|_| code >= 0x20)
                .map(|code| {
                    rows.clear();
                    Glyph {
                        code,
                        advance: 0,
                        width: 0,
                        height: 0,
                        left: 0,
                        top: 0,
                        bits: Vec::new(),
                    }
                });
            continue;
        }
        let Some(glyph) = pending.as_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("DWIDTH ") {
            glyph.advance = rest
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| format!("dwidth {rest:?}"))?;
        } else if let Some(rest) = line.strip_prefix("BBX ") {
            let numbers: Vec<i32> = rest
                .split_whitespace()
                .map(|n| n.parse().unwrap_or(i32::MIN))
                .collect();
            let [w, h, left, top] = numbers[..] else {
                return Err(format!("bbx {rest:?}"));
            };
            glyph.width = u8::try_from(w).map_err(|_| format!("width {w}"))?;
            glyph.height = u8::try_from(h).map_err(|_| format!("height {h}"))?;
            glyph.left = i8::try_from(left).map_err(|_| format!("left {left}"))?;
            glyph.top = i8::try_from(top).map_err(|_| format!("top {top}"))?;
        } else if line == "BITMAP" {
            in_bitmap = true;
            rows.clear();
        } else if line == "ENDCHAR" {
            let glyph = pending.take().expect("a glyph ends only after it starts");
            if glyph.advance == 0 || glyph.advance > 31 {
                // Zero-advance marks cannot be placed by this renderer, and nothing wider
                // than two cells belongs in a table cut for one panel.
                continue;
            }
            let stride = glyph.stride();
            if usize::from(glyph.height) * stride != rows.len() {
                return Err(format!(
                    "U+{:04X} has {} bitmap bytes for {stride}x{}",
                    glyph.code,
                    rows.len(),
                    glyph.height
                ));
            }
            let mut glyph = glyph;
            glyph.bits = rows.clone();
            glyphs.push(glyph);
        } else if in_bitmap {
            for byte in line.as_bytes().chunks(2) {
                let text = std::str::from_utf8(byte).map_err(|_| "bitmap row")?;
                rows.push(u8::from_str_radix(text, 16).map_err(|_| format!("row {text:?}"))?);
            }
        }
    }
    glyphs.sort_by_key(|glyph| glyph.code);
    if glyphs.is_empty() {
        return Err("the BDF has no glyphs".into());
    }
    for pair in glyphs.windows(2) {
        if pair[0].code == pair[1].code {
            return Err(format!("U+{:04X} appears twice", pair[0].code));
        }
    }
    Ok(glyphs)
}

/// The device's copy: a fixed header, then code points, packed metrics, offsets and bit-planes.
pub fn encode(glyphs: &[Glyph]) -> Result<Vec<u8>, String> {
    let count = glyphs.len();
    let cp_off = HEADER;
    let meta_off = cp_off + count * 2;
    let off_off = meta_off + count * 4;
    let bits_off = off_off + count * 4;
    let bits_len: usize = glyphs.iter().map(|glyph| glyph.bits.len()).sum();
    let mut out = vec![0u8; bits_off + bits_len];
    out[..4].copy_from_slice(MAGIC);
    out[4..6].copy_from_slice(&(count as u16).to_le_bytes());
    out[6] = PIXELS;
    out[7] = ASCENT;
    out[8] = DESCENT;
    out[12..16].copy_from_slice(&(cp_off as u32).to_le_bytes());
    out[16..20].copy_from_slice(&(meta_off as u32).to_le_bytes());
    out[20..24].copy_from_slice(&(off_off as u32).to_le_bytes());
    out[24..28].copy_from_slice(&(bits_off as u32).to_le_bytes());
    let mut at = 0usize;
    for (index, glyph) in glyphs.iter().enumerate() {
        if !(6..=12).contains(&glyph.advance) {
            return Err(format!("U+{:04X} advances {}", glyph.code, glyph.advance));
        }
        if glyph.width > 15
            || glyph.height > 31
            || !(-16..=15).contains(&glyph.left)
            || !(-16..=15).contains(&glyph.top)
        {
            return Err(format!(
                "U+{:04X} does not fit the packed metrics",
                glyph.code
            ));
        }
        out[cp_off + index * 2..cp_off + index * 2 + 2].copy_from_slice(&glyph.code.to_le_bytes());
        let meta = u32::from(glyph.advance)
            | u32::from(glyph.height) << 6
            | u32::from(glyph.width) << 11
            | u32::from(glyph.left as u8 & 0x1f) << 16
            | u32::from(glyph.top as u8 & 0x1f) << 21;
        out[meta_off + index * 4..meta_off + index * 4 + 4].copy_from_slice(&meta.to_le_bytes());
        out[off_off + index * 4..off_off + index * 4 + 4]
            .copy_from_slice(&(at as u32).to_le_bytes());
        out[bits_off + at..bits_off + at + glyph.bits.len()].copy_from_slice(&glyph.bits);
        at += glyph.bits.len();
    }
    Ok(out)
}

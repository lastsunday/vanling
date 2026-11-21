//! Setting a run of text into lines the panel can draw.
//!
//! Break opportunities follow UAX #14's shape for the classes a 320 px panel meets, with
//! CLReq's line-start and line-end prohibitions on top: no line starts on `。` or ends on
//! `「`. Measurement and drawing call the same breaker, so a line that fits when measured is
//! the line that is drawn.

use super::font::{MISSING, Metrics};

/// Lines one paragraph may hold; anything longer is cut and ellipsised.
pub const MAX_LINES: usize = 32;

/// One line of a paragraph, as a byte range of the run it came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub start: usize,
    /// End of the line's ink; trailing spaces are outside it.
    pub end: usize,
    /// Columns the ink occupies.
    pub width: usize,
    /// Places a justified line may widen: its spaces and the breaks between ideographs.
    pub gaps: u16,
}

/// A run broken into lines.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph {
    pub lines: [Line; MAX_LINES],
    pub count: usize,
    /// The run had more than [`MAX_LINES`] lines, or more than the caller allowed.
    pub truncated: bool,
}

impl Default for Paragraph {
    fn default() -> Self {
        Self {
            lines: [Line::default(); MAX_LINES],
            count: 0,
            truncated: false,
        }
    }
}

/// Rows one line occupies at `metrics`.
pub fn line_height(metrics: Metrics) -> usize {
    metrics.height()
}

/// The width the run takes on one line at `metrics`.
pub fn max_content(text: &[u8], metrics: Metrics) -> usize {
    let mut widest = 0;
    let mut run = 0;
    let mut at = 0;
    while let Some(cluster) = Cluster::next(text, at, metrics) {
        at = cluster.end;
        if let Some(next) = hard_break(text, cluster) {
            at = next;
            widest = widest.max(run);
            run = 0;
            continue;
        }
        run += cluster.width;
    }
    widest.max(run)
}

/// Break `text` into lines no wider than `width`, at most `max_lines` of them.
pub fn wrap(text: &[u8], width: usize, max_lines: usize, metrics: Metrics) -> Paragraph {
    let width = width.max(1);
    let max_lines = max_lines.clamp(1, MAX_LINES);
    let ellipsis = metrics.advance('…');
    let mut paragraph = Paragraph::default();
    let mut at = 0usize;
    while paragraph.count < max_lines {
        let mut pen = 0usize;
        let mut used = 0usize;
        let mut gaps = 0u16;
        let mut last_break: Option<(usize, usize, usize, u16)> = None;
        let mut end = at;
        let mut cursor = at;
        let mut broke = None;
        let mut previous = None;
        let mut first = true;
        while let Some(cluster) = Cluster::next(text, cursor, metrics) {
            if let Some(next) = hard_break(text, cluster) {
                broke = Some((next, end, used, gaps, false));
                break;
            }
            let break_before =
                previous.is_none_or(|previous: Class| cluster.breaks_after(previous));
            if break_before {
                last_break = Some((cluster.start, end, pen, gaps));
            }
            if pen + cluster.width > width && !first {
                broke = Some(match last_break {
                    Some((start, ink_end, ink_width, ink_gaps)) if start > at => {
                        (start, ink_end, ink_width, ink_gaps, false)
                    }
                    _ => (cluster.start, end, pen, gaps, true),
                });
                break;
            }
            pen += cluster.width;
            cursor = cluster.end;
            if !cluster.is_space() {
                end = cluster.end;
                used = pen;
                if !first && break_before {
                    gaps += 1;
                }
            }
            previous = Some(cluster.class);
            first = false;
        }
        let (next, ink_end, ink_width, ink_gaps, hard) = match broke {
            Some((next, ink_end, ink_width, ink_gaps, hard)) => {
                (next, ink_end, ink_width, ink_gaps, hard)
            }
            None => {
                if end > at {
                    (cursor, end, used, gaps, false)
                } else {
                    break;
                }
            }
        };
        let _ = hard;
        paragraph.lines[paragraph.count] = Line {
            start: at,
            end: ink_end,
            width: ink_width,
            gaps: ink_gaps,
        };
        paragraph.count += 1;
        at = next;
        while text.get(at) == Some(&b' ') || text.get(at) == Some(&b'\t') {
            at += 1;
        }
        if at >= text.len() {
            break;
        }
    }
    if paragraph.count == 0 {
        paragraph.count = 1;
    }
    if at < text.len() && paragraph.count > 0 {
        paragraph.truncated = true;
        let last = &mut paragraph.lines[paragraph.count - 1];
        while last.end > last.start && last.width + ellipsis > width {
            let previous = previous_cluster(text, last.start, last.end, metrics);
            match previous {
                Some(cluster) => {
                    last.end = cluster.start;
                    last.width = last.width.saturating_sub(cluster.width);
                    if cluster.is_space() {
                        last.gaps = last.gaps.saturating_sub(1);
                    }
                }
                None => break,
            }
        }
        if last.width + ellipsis > width {
            last.end = last.start;
            last.width = 0;
            last.gaps = 0;
        }
    }
    paragraph
}

/// Where a hard line break ends, or `None` when `cluster` is not one. `\r\n` is one break.
fn hard_break(text: &[u8], cluster: Cluster) -> Option<usize> {
    match cluster.ch {
        '\n' => Some(cluster.end),
        '\r' => Some(cluster.end + usize::from(text.get(cluster.end) == Some(&b'\n'))),
        _ => None,
    }
}

fn previous_cluster(text: &[u8], start: usize, end: usize, metrics: Metrics) -> Option<Cluster> {
    let mut previous = None;
    let mut at = start;
    while at < end {
        let cluster = Cluster::next(text, at, metrics)?;
        if cluster.end >= end {
            break;
        }
        previous = Some(cluster);
        at = cluster.end;
    }
    previous
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Hyphen,
    Open,
    Close,
    Wide,
    Mark,
}

#[derive(Clone, Copy)]
struct Cluster {
    start: usize,
    end: usize,
    width: usize,
    class: Class,
    ch: char,
}

impl Cluster {
    fn next(text: &[u8], at: usize, metrics: Metrics) -> Option<Self> {
        let (ch, len) = decode(text, at)?;
        Some(Self {
            start: at,
            end: at + len,
            width: metrics.advance(ch),
            class: class(ch),
            ch,
        })
    }

    fn is_space(self) -> bool {
        matches!(self.class, Class::Space)
    }

    /// Whether a line may break between `previous` and this cluster: the UAX #14 pairs that
    /// matter here, plus CLReq §6.1.1's prohibitions.
    fn breaks_after(self, previous: Class) -> bool {
        match previous {
            Class::Open => false,
            Class::Mark => false,
            Class::Space | Class::Hyphen => self.class != Class::Close,
            Class::Close | Class::Word => self.class == Class::Wide || self.class == Class::Space,
            Class::Wide => self.class != Class::Close && self.class != Class::Mark,
        }
    }
}

/// The line-break class of `ch`, for the scripts a panel that draws ASCII and GB2312 meets.
fn class(ch: char) -> Class {
    match ch {
        '\n' | '\r' => Class::Space,
        ' ' | '\t' | '\u{200B}' | '\u{3000}' => Class::Space,
        '\u{00A0}' | '\u{2060}' | '\u{FEFF}' => Class::Word,
        '-' | '\u{00AD}' | '\u{2010}' => Class::Hyphen,
        '(' | '[' | '{' | '\u{3008}' | '\u{300A}' | '\u{300C}' | '\u{300E}' | '\u{3010}'
        | '\u{3014}' | '\u{3016}' | '\u{3018}' | '\u{301A}' | '\u{FF08}' | '\u{FF3B}'
        | '\u{FF5B}' | '\u{2018}' | '\u{201C}' | '\u{00AB}' => Class::Open,
        ')' | ']' | '}' | ',' | '.' | ';' | ':' | '!' | '?' | '\u{3001}' | '\u{3002}'
        | '\u{3009}' | '\u{300B}' | '\u{300D}' | '\u{300F}' | '\u{3011}' | '\u{3015}'
        | '\u{3017}' | '\u{3019}' | '\u{301B}' | '\u{FF09}' | '\u{FF0C}' | '\u{FF0E}'
        | '\u{FF1A}' | '\u{FF1B}' | '\u{FF01}' | '\u{FF1F}' | '\u{FF3D}' | '\u{FF5D}'
        | '\u{2019}' | '\u{201D}' | '\u{00BB}' | '\u{2026}' => Class::Close,
        '\u{2014}' | '\u{2015}' | '\u{30FB}' | '\u{FF65}' => Class::Mark,
        '\u{2E80}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FE30}'..='\u{FE4F}'
        | '\u{FF00}'..='\u{FFEF}'
        | '\u{20000}'..='\u{2FA1F}' => Class::Wide,
        _ => Class::Word,
    }
}

/// The next code point and its byte length.
pub fn next_char(text: &[u8], at: usize) -> Option<(char, usize)> {
    decode(text, at)
}

/// One UTF-8 code point and its length. Invalid bytes are one replacement character wide, so
/// a corrupt run still advances.
fn decode(text: &[u8], at: usize) -> Option<(char, usize)> {
    let first = *text.get(at)?;
    let (len, min) = match first {
        0x00..=0x7F => return Some((char::from(first), 1)),
        0xC2..=0xDF => (2, 0x80),
        0xE0..=0xEF => (3, 0x800),
        0xF0..=0xF4 => (4, 0x10000),
        _ => return Some((char::REPLACEMENT_CHARACTER, 1)),
    };
    let bytes = text.get(at..at + len)?;
    let mut code = u32::from(first & (0x7F >> len));
    for &byte in &bytes[1..] {
        if byte & 0xC0 != 0x80 {
            return Some((char::REPLACEMENT_CHARACTER, 1));
        }
        code = code << 6 | u32::from(byte & 0x3F);
    }
    match char::from_u32(code) {
        Some(ch) if code >= min && code != 0xFFFE && code != 0xFFFF => Some((ch, len)),
        _ => Some((char::REPLACEMENT_CHARACTER, 1)),
    }
}

/// What a code point the subset does not carry is drawn as.
pub const FALLBACK: char = MISSING;

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[u8], width: usize) -> Vec<(usize, usize)> {
        let paragraph = wrap(text, width, MAX_LINES, Metrics::default());
        paragraph.lines[..paragraph.count]
            .iter()
            .map(|line| (line.start, line.end))
            .collect()
    }

    #[test]
    fn an_ascii_word_moves_to_the_next_line_whole() {
        assert_eq!(lines(b"HELLO WORLD", 6 * 6), vec![(0, 5), (6, 11)]);
    }

    #[test]
    fn a_word_wider_than_the_line_is_cut_rather_than_run_off() {
        let paragraph = wrap(b"ABCDEFGH", 3 * 6, MAX_LINES, Metrics::default());
        assert!(paragraph.count >= 2);
        for line in &paragraph.lines[..paragraph.count] {
            assert!(line.width <= 3 * 6);
        }
    }

    #[test]
    fn an_ideograph_breaks_between_any_two() {
        let paragraph = wrap("中文中文".as_bytes(), 24, MAX_LINES, Metrics::default());
        assert_eq!(paragraph.count, 2);
        assert_eq!(paragraph.lines[0].width, 24);
    }

    #[test]
    fn a_line_never_starts_on_a_closing_mark() {
        let paragraph = wrap("中文。中文".as_bytes(), 24, MAX_LINES, Metrics::default());
        let text = "中文。中文".as_bytes();
        let second = &text[paragraph.lines[1].start..];
        assert!(
            !second.starts_with("。".as_bytes()),
            "the break moved before the mark"
        );
    }

    #[test]
    fn a_line_never_ends_on_an_opening_mark() {
        let paragraph = wrap("中文「中文".as_bytes(), 24, MAX_LINES, Metrics::default());
        let text = "中文「中文".as_bytes();
        let first = &text[paragraph.lines[0].start..paragraph.lines[0].end];
        assert!(!first.ends_with("「".as_bytes()));
    }

    #[test]
    fn a_hyphen_is_where_the_break_is_taken() {
        let paragraph = wrap(b"AB-CD", 3 * 6, MAX_LINES, Metrics::default());
        assert_eq!(paragraph.lines[0].end, 3, "the break follows the hyphen");
    }

    #[test]
    fn trailing_spaces_are_not_ink_and_do_not_earn_a_line() {
        let paragraph = wrap(b"AB   ", 12, MAX_LINES, Metrics::default());
        assert_eq!(paragraph.count, 1);
        assert_eq!(paragraph.lines[0].width, 12);
    }

    #[test]
    fn a_line_that_does_not_fit_is_ellipsised() {
        let paragraph = wrap(b"ABCDEFGH", 3 * 6, 1, Metrics::default());
        assert!(paragraph.truncated);
        assert_eq!(paragraph.count, 1);
        assert!(paragraph.lines[0].width < 3 * 6);
    }

    #[test]
    fn a_newline_is_a_hard_break_and_a_blank_line_survives() {
        assert_eq!(lines(b"a\nb", 100), vec![(0, 1), (2, 3)]);
        assert_eq!(lines(b"a\n\nb", 100), vec![(0, 1), (2, 2), (3, 4)]);
    }

    #[test]
    fn a_carriage_return_breaks_and_its_line_feed_does_not_break_twice() {
        assert_eq!(lines(b"a\rb", 100), vec![(0, 1), (2, 3)]);
        assert_eq!(lines(b"a\r\nb", 100), vec![(0, 1), (3, 4)]);
    }

    #[test]
    fn max_content_stops_at_every_kind_of_line_break() {
        assert_eq!(max_content(b"AB\nCDEF", Metrics::default()), 4 * 6);
        assert_eq!(max_content(b"AB\rCDEF", Metrics::default()), 4 * 6);
        assert_eq!(max_content(b"AB\r\nCDEF", Metrics::default()), 4 * 6);
    }

    #[test]
    fn max_content_is_the_whole_run_on_one_line() {
        assert_eq!(max_content(b"AB CDE F", Metrics::default()), 8 * 6);
        assert_eq!(max_content("中文".as_bytes(), Metrics::default()), 24);
    }

    #[test]
    fn every_byte_range_is_on_a_character_boundary() {
        let text = "a中b。c".as_bytes();
        let paragraph = wrap(text, 12, MAX_LINES, Metrics::default());
        for line in &paragraph.lines[..paragraph.count] {
            assert!(core::str::from_utf8(&text[line.start..line.end]).is_ok());
        }
    }

    #[test]
    fn invalid_utf8_still_advances() {
        let paragraph = wrap(b"a\xffb", 6 * 4, MAX_LINES, Metrics::default());
        assert_eq!(paragraph.count, 1);
    }
}

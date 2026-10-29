//! The scan list as the page's JSON: its length before it is written, and one row at a time
//! after.

use crate::drivers::network::{AccessPoint, ApList};

use super::wire::{Bounded, Counting, Sink, push_i64, push_usize};

/// The room one piece of an answer needs in the caller's buffer: a scan row, or a response head.
///
/// The row decides it — 282 bytes for a 32-octet name of control characters — and a row that does
/// not fit is a list that stops mid-answer without saying so, so a test pins the number.
pub const PIECE_BYTES: usize = 288;

/// One piece of the scan list, as [`ScanJson`] hands them over.
#[derive(Clone, Copy, Debug)]
pub enum Piece {
    Static(&'static [u8]),
    Row { ap: AccessPoint, saved: bool },
}

/// The list the page draws, as the pieces it is made of.
pub struct ScanJson<'a> {
    aps: &'a ApList,
    stage: Stage,
}

enum Stage {
    Open,
    Row(usize),
    Comma(usize),
    Close,
    Done,
}

impl<'a> ScanJson<'a> {
    pub fn new(aps: &'a ApList) -> Self {
        Self {
            aps,
            stage: Stage::Open,
        }
    }
}

impl Iterator for ScanJson<'_> {
    type Item = Piece;

    fn next(&mut self) -> Option<Piece> {
        let rows = self.aps.len();
        match self.stage {
            Stage::Open => {
                self.stage = if rows == 0 {
                    Stage::Close
                } else {
                    Stage::Row(0)
                };
                Some(Piece::Static(b"["))
            }
            Stage::Row(index) => {
                let ap = self.aps.get(index)?;
                self.stage = if index + 1 < rows {
                    Stage::Comma(index + 1)
                } else {
                    Stage::Close
                };
                Some(Piece::Row {
                    ap,
                    saved: self.aps.is_saved(index),
                })
            }
            Stage::Comma(index) => {
                self.stage = Stage::Row(index);
                Some(Piece::Static(b","))
            }
            Stage::Close => {
                self.stage = Stage::Done;
                Some(Piece::Static(b"]"))
            }
            Stage::Done => None,
        }
    }
}

/// The length of the list [`ScanJson`] hands over, for the head that has to name it:
/// counted by writing the list into a sink that keeps nothing, so this number and the bytes
/// it names cannot disagree.
pub fn scan_json_len(aps: &ApList) -> usize {
    let mut count = Counting::default();
    for piece in ScanJson::new(aps) {
        let taken = match piece {
            Piece::Static(bytes) => count.write(bytes),
            Piece::Row { ap, saved } => write_json_row(&mut count, &ap, saved),
        };
        debug_assert!(taken, "a sink that only counts takes everything");
    }
    count.written()
}

/// One row of the list, written into `out`.
pub fn scan_json_row(ap: &AccessPoint, saved: bool, out: &mut [u8]) -> Option<usize> {
    let mut sink = Bounded::new(out);
    write_json_row(&mut sink, ap, saved).then(|| sink.written())
}

fn write_json_row(sink: &mut impl Sink, ap: &AccessPoint, saved: bool) -> bool {
    sink.write(b"{\"ssid\":")
        && push_json_string(sink, ap.ssid.as_bytes())
        && sink.write(b",\"bssid\":\"")
        && push_bssid(sink, ap.bssid)
        && sink.write(b"\",\"rssi\":")
        && push_i64(sink, i64::from(ap.rssi))
        && sink.write(b",\"bars\":")
        && push_usize(sink, usize::from(ap.bars()))
        && sink.write(b",\"secured\":")
        && sink.write(if ap.secured { b"true" } else { b"false" })
        && sink.write(b",\"saved\":")
        && sink.write(if saved { b"true" } else { b"false" })
        && sink.write(b"}")
}

fn push_json_string(out: &mut impl Sink, bytes: &[u8]) -> bool {
    out.write(b"\"")
        && match core::str::from_utf8(bytes) {
            Ok(text) => text.chars().all(|character| push_json_char(out, character)),
            Err(_) => bytes
                .iter()
                .all(|&byte| push_json_char(out, char::from(byte))),
        }
        && out.write(b"\"")
}

fn push_json_char(out: &mut impl Sink, character: char) -> bool {
    match character {
        '"' => out.write(b"\\\""),
        '\\' => out.write(b"\\\\"),
        control if (control as u32) < 0x20 => {
            let code = control as u32;
            let digits = [hex_digit((code >> 4) as u8), hex_digit(code as u8 & 0xf)];
            out.write(b"\\u00") && out.write(&digits)
        }
        printable => {
            let mut buffer = [0u8; 4];
            out.write(printable.encode_utf8(&mut buffer).as_bytes())
        }
    }
}

fn push_bssid(out: &mut impl Sink, bssid: [u8; 6]) -> bool {
    for (index, octet) in bssid.iter().enumerate() {
        if index > 0 && !out.write(b":") {
            return false;
        }
        let digits = [hex_digit(octet >> 4), hex_digit(octet & 0xf)];
        if !out.write(&digits) {
            return false;
        }
    }
    true
}

fn hex_digit(value: u8) -> u8 {
    if value < 10 {
        b'0' + value
    } else {
        b'A' + value - 10
    }
}

#[cfg(test)]
pub(crate) fn list(rows: &[(&str, i8, bool)]) -> ApList {
    use crate::drivers::network::Ssid;
    use alloc::vec::Vec;

    let aps = rows
        .iter()
        .enumerate()
        .map(|(index, (name, rssi, secured))| AccessPoint {
            ssid: Ssid::from_octets(name.as_bytes()),
            bssid: [index as u8; 6],
            channel: 6,
            rssi: *rssi,
            secured: *secured,
        })
        .collect::<Vec<_>>();
    ApList::from_ranked(&aps)
}

#[cfg(test)]
pub(crate) fn json(aps: &ApList) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::new();
    let mut row = [0u8; PIECE_BYTES];
    for piece in ScanJson::new(aps) {
        match piece {
            Piece::Static(bytes) => out.extend_from_slice(bytes),
            Piece::Row { ap, saved } => {
                let len = scan_json_row(&ap, saved, &mut row).expect("a row that fits");
                out.extend_from_slice(&row[..len]);
            }
        }
    }
    assert_eq!(
        out.len(),
        scan_json_len(aps),
        "the length the head names is the list's own"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::network::{SSID_BYTES, Ssid};
    use alloc::string::String;

    #[test]
    fn the_longest_row_a_name_can_make_fits_the_buffer_the_device_writes_one_into() {
        let row = AccessPoint {
            ssid: Ssid::from_octets(&[0x01; SSID_BYTES]),
            bssid: [0xff; 6],
            channel: 11,
            rssi: i8::MIN,
            secured: false,
        };
        let mut out = [0u8; PIECE_BYTES];
        let len = scan_json_row(&row, false, &mut out).expect("the longest row fits");
        assert_eq!(len, 282);
        assert_eq!(
            scan_json_row(&row, false, &mut out[..len - 1]),
            None,
            "and a byte less does not"
        );
    }

    #[test]
    fn the_scan_json_carries_what_the_page_draws() {
        let aps = list(&[("living", -55, true), ("caf\u{e9}", -70, false)]);
        let text = String::from_utf8(json(&aps)).expect("json is text");
        assert!(text.starts_with('['), "{text}");
        assert!(text.contains("\"ssid\":\"living\""), "{text}");
        assert!(text.contains("\"bars\":3"), "{text}");
        assert!(text.contains("\"secured\":true"), "{text}");
        assert!(text.contains("\"saved\":false"), "{text}");
        assert!(text.contains("\"bssid\":\"00:00:00:00:00:00\""), "{text}");
        assert!(text.contains("\"ssid\":\"caf\u{e9}\""), "{text}");
    }

    #[test]
    fn every_piece_of_an_empty_list_is_a_bracket() {
        assert_eq!(String::from_utf8(json(&ApList::empty())).unwrap(), "[]");
    }

    #[test]
    fn an_address_is_carried_in_the_case_the_panel_prints_it() {
        let row = AccessPoint {
            bssid: [0xaa, 0x0f, 0x5c, 0x00, 0xff, 0x01],
            ..AccessPoint::EMPTY
        };
        let text = String::from_utf8(json(&ApList::from_ranked(&[row]))).expect("json is text");
        assert!(text.contains("\"bssid\":\"AA:0F:5C:00:FF:01\""), "{text}");
    }

    #[test]
    fn a_name_that_is_not_text_is_still_a_row() {
        let row = AccessPoint {
            ssid: Ssid::from_octets(b"Caf\xe9"),
            ..AccessPoint::EMPTY
        };
        let aps = ApList::from_ranked(&[row]);
        let text = String::from_utf8(json(&aps)).expect("json is text");
        assert!(text.contains("\"ssid\":\"Caf\u{e9}\""), "{text}");
    }

    #[test]
    fn the_saved_mark_comes_from_the_list_it_was_made_for() {
        let mut aps = list(&[("home", -50, true), ("work", -60, true)]);
        aps.mark_saved(0b10);
        let text = String::from_utf8(json(&aps)).expect("json is text");
        assert!(text.contains("\"ssid\":\"home\",\"bssid\":\"00:00:00:00:00:00\",\"rssi\":-50,\"bars\":4,\"secured\":true,\"saved\":false}"), "{text}");
        assert!(text.contains("\"saved\":true}"), "{text}");
    }

    #[test]
    fn a_quote_in_a_name_does_not_end_the_string() {
        let aps = list(&[("say \"hi\"", -50, true)]);
        let text = String::from_utf8(json(&aps)).expect("json is text");
        assert!(text.contains(r#""ssid":"say \"hi\"""#), "{text}");
        assert_eq!(
            text.matches('{').count(),
            1,
            "the escaped quotes are still inside one row"
        );
    }
}

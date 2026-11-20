//! Network, setup and question text: the words the link's readings become.

use crate::services::network::controller::{ControlKind, confirm_verb};
use crate::services::network::shape::{
    DIALOG_ANSWER_W, DIALOG_GLYPHS, LIST_ROWS, ToolbarCell, toolbar_cell,
};
use crate::shared::intent::Confirm;
use crate::shared::intent::{Key, Layer};
use crate::shared::model::Entry;
use iot_core::drivers::network::{
    AccessPoint, Ipv4, LinkPhase, LinkStatus, NetworkError, SSID_BYTES, SetupPass,
};
use iot_core::provision::SETUP_PASS_LEN;
use iot_core::ui::panel::GLYPH;
use iot_core::ui::text::{fit, write_u16, write_u64};

/// Room a name needs in bytes: an SSID's own maximum, plus the mark that says it was cut.
pub(super) const NAME_BYTES: usize = SSID_BYTES + 1;

/// Room a dialog line needs in bytes. Its text is ASCII, but the name it names is not.
pub(super) const DIALOG_BYTES: usize = SSID_BYTES + 16;

/// Pixels a network name is given on a scan-list row.
pub(super) const NETWORK_NAME_W: usize = 24 * GLYPH;

/// The key grid's own words: what each control prints, and what the reveal says while the
/// draft is on screen.
pub fn key_label(key: Key, layer: Layer) -> &'static [u8] {
    match key {
        Key::Layer => match layer {
            Layer::Abc => b"123",
            Layer::Digits => b"#&?",
            Layer::Symbols => b"abc",
        },
        Key::Shift => b"SH",
        Key::Backspace => b"<-",
        Key::Connect => b"OK",
        Key::Cancel => b"X",
        Key::Reveal => b"SHOW",
        Key::Char(_) => b"",
    }
}

pub(super) const HIDE_LABEL: &[u8] = b"HIDE";

/// The word a control's box carries.
pub const fn control_label(kind: ControlKind) -> &'static [u8] {
    match kind {
        ControlKind::Cancel => b"CANCEL",
        ControlKind::Scan => b"SCAN",
        ControlKind::Wait => b"WAIT",
        ControlKind::Leave => b"LEAVE",
        ControlKind::Forget => b"FORGET",
        ControlKind::Keep => b"KEEP",
        ControlKind::AccessPoint => b"AP",
        ControlKind::Panel => b"PANEL",
        ControlKind::Pass => b"PASS",
        ControlKind::NoPass => b"NO PASS",
    }
}

const _: () = assert!(
    control_label(ControlKind::Cancel).len() * GLYPH <= toolbar_cell(ToolbarCell::Left).w,
    "the way out of setup mode does not fit the cell the strip gives it"
);
const _: () = assert!(
    control_label(ControlKind::AccessPoint).len() * GLYPH <= toolbar_cell(ToolbarCell::Right).w
        && control_label(ControlKind::Panel).len() * GLYPH <= toolbar_cell(ToolbarCell::Right).w,
    "the host switch does not fit the cell the strip gives it"
);
const _: () = assert!(
    control_label(ControlKind::NoPass).len() * GLYPH <= DIALOG_ANSWER_W,
    "the box's own answer cannot hold the word of what it does"
);

/// The scan list's window, as `first-last/total`, on the page's title row.
pub fn format_window(top: u8, rows: usize, buf: &mut [u8; 12]) -> &[u8] {
    let first = usize::from(top) + 1;
    let last = (usize::from(top) + LIST_ROWS).min(rows);
    let mut n = write_u64(first as u64, buf, 0);
    buf[n] = b'-';
    n = write_u64(last as u64, buf, n + 1);
    buf[n] = b'/';
    n = write_u64(rows as u64, buf, n + 1);
    &buf[..n]
}

// The draft as the row above the keyboard shows it: masked, or as typed while the reveal is on.
//
// The *tail* of it rather than the head, because the end is what the finger just typed and a
// character to fix is at the end of a passphrase about as often as anywhere. What does not fit is
// marked with a leading `+` rather than silently dropped, and the mark is one of the row's own
// glyphs: the row's width is what the columns above leave it, so a draft a glyph wider than that
// would run into the phase word beside it.
pub fn format_psk<'a>(entry: &Entry, revealed: bool, buf: &'a mut [u8]) -> &'a [u8] {
    let shown = entry.len().min(buf.len());
    let more = entry.len() > shown;
    let tail = if more { shown - 1 } else { shown };
    let mut n = 0;
    if more {
        buf[0] = b'+';
        n = 1;
    }
    let bytes = entry.as_bytes();
    for (slot, byte) in buf[n..n + tail]
        .iter_mut()
        .zip(&bytes[bytes.len() - tail..])
    {
        *slot = match (revealed, byte.is_ascii_graphic()) {
            (true, true) => *byte,
            (true, false) => b'?',
            (false, _) => b'*',
        };
    }
    &buf[..n + tail]
}

/// The link's phase as one four-glyph word, and a failure as what failed rather than as the
/// fact of failing: the driver logs its own error code with the call that produced it, so
/// the panel's job is to say which of the kinds it was.
pub fn format_phase(phase: LinkPhase, buf: &mut [u8; 4]) -> &[u8] {
    let word: &[u8] = match phase {
        LinkPhase::Off => b"OFF",
        LinkPhase::Scanning => b"SCAN",
        LinkPhase::Connecting => b"JOIN",
        LinkPhase::Connected => b"UP",
        LinkPhase::Failed(NetworkError::Busy) => b"BUSY",
        LinkPhase::Failed(NetworkError::InvalidArguments) => b"NAME",
        LinkPhase::Failed(NetworkError::NoMemory) => b"MEM",
        LinkPhase::Failed(NetworkError::NotFound) => b"NOAP",
        LinkPhase::Failed(NetworkError::Credentials) => b"AUTH",
        LinkPhase::Failed(NetworkError::Driver) => b"DRV",
    };
    buf[..word.len()].copy_from_slice(word);
    &buf[..word.len()]
}

/// The address DHCP handed out, as a dotted quad, or a single dash while there is none.
pub fn format_address(address: Option<Ipv4>, buf: &mut [u8; 16]) -> &[u8] {
    let Some(address) = address else {
        buf[0] = b'-';
        return &buf[..1];
    };
    let mut n = 0;
    for (index, octet) in address.octets().iter().enumerate() {
        if index > 0 {
            buf[n] = b'.';
            n += 1;
        }
        n = write_u16(u16::from(*octet), buf, n);
    }
    &buf[..n]
}

/// Signal strength as four glyphs, filled for each bar the reading earns.
pub fn format_bars(bars: u8, buf: &mut [u8; 4]) -> &[u8] {
    let bars = bars.min(4);
    for (index, glyph) in buf.iter_mut().enumerate() {
        *glyph = if (index as u8) < bars { b'#' } else { b'.' };
    }
    &buf[..]
}

/// A row's name: the SSID when the radio reported text, and the BSSID when it did not. The
/// name is cut by pixels, because a hanzi is two cells wide and three bytes long.
pub fn format_network_name(ap: AccessPoint, width: usize, buf: &mut [u8; NAME_BYTES]) -> &[u8] {
    match ap.ssid.as_display_text() {
        Some(name) => fit(name, width, buf),
        None => {
            let n = write_bssid(&ap.bssid, buf);
            &buf[..n]
        }
    }
}

/// The consequence under the question: what the action costs, in the box's own width.
pub fn body(confirm: Confirm) -> &'static [u8] {
    match confirm {
        Confirm::Leave => LEAVE_BODY,
        Confirm::Forget(_) => FORGET_BODY,
    }
}

const LEAVE_BODY: &[u8] = b"THE LINK GOES DOWN";
const FORGET_BODY: &[u8] = b"THE SAVED KEY IS ERASED";

const ASSOCIATING_BODY: &[u8] = b"WAIT FOR THE NETWORK";
const ASSOCIATED_BODY: &[u8] = b"WAIT FOR AN ADDRESS";

/// The announcement's title: the same line every time, so it is the line itself.
pub const SETUP_NOTICE_TITLE: &[u8] = b"SETUP MODE";
const SETUP_NOTICE_PREFIX: &[u8] = b"AP ";

/// What the box says while the access point it names is still being brought up or retuned.
pub const SETUP_STARTING_BODY: &[u8] = b"STARTING";

/// The setup access point's own name, derived from the board's address here rather than
/// carried in the state: the network task derives it the same way, and two copies of one
/// name are one copy too many.
pub fn format_setup_name(mac: Option<[u8; 6]>, buf: &mut [u8; NAME_BYTES]) -> &[u8] {
    let Some(mac) = mac else {
        return b"";
    };
    let name = iot_core::provision::setup_ap_ssid(crate::assets::AP_PREFIX, mac);
    let Some(text) = name.as_display_text() else {
        return b"";
    };
    let text = text.as_bytes();
    let taken = text.len().min(buf.len());
    buf[..taken].copy_from_slice(&text[..taken]);
    &buf[..taken]
}

/// The announcement's second line: the access point a phone has to join.
pub fn format_setup_notice_body(mac: Option<[u8; 6]>, buf: &mut [u8; NAME_BYTES]) -> &[u8] {
    let mut name = [0u8; NAME_BYTES];
    let name = format_setup_name(mac, &mut name);
    if name.is_empty() {
        return b"";
    }
    let taken = name.len().min(buf.len() - SETUP_NOTICE_PREFIX.len());
    buf[..SETUP_NOTICE_PREFIX.len()].copy_from_slice(SETUP_NOTICE_PREFIX);
    buf[SETUP_NOTICE_PREFIX.len()..SETUP_NOTICE_PREFIX.len() + taken]
        .copy_from_slice(&name[..taken]);
    &buf[..SETUP_NOTICE_PREFIX.len() + taken]
}

const SETUP_PASS_PREFIX: &[u8] = b"PASS ";
const SETUP_PASS_OPEN: &[u8] = b"OPEN, NO PASSWORD";

/// The access point's passphrase, or the fact that it has none.
pub fn format_setup_pass<'a>(
    password: Option<&SetupPass>,
    buf: &'a mut [u8; NAME_BYTES],
) -> &'a [u8] {
    let Some(password) = password else {
        let taken = SETUP_PASS_OPEN.len().min(buf.len());
        buf[..taken].copy_from_slice(&SETUP_PASS_OPEN[..taken]);
        return &buf[..taken];
    };
    let text = password.as_bytes();
    let taken = text.len().min(buf.len() - SETUP_PASS_PREFIX.len());
    buf[..SETUP_PASS_PREFIX.len()].copy_from_slice(SETUP_PASS_PREFIX);
    buf[SETUP_PASS_PREFIX.len()..SETUP_PASS_PREFIX.len() + taken].copy_from_slice(&text[..taken]);
    &buf[..SETUP_PASS_PREFIX.len() + taken]
}

const JOINING_WORD: &[u8] = b"JOINING";
const JOINED_WORD: &[u8] = b"JOINED";

const _: () = assert!(
    LEAVE_BODY.len() <= DIALOG_GLYPHS && FORGET_BODY.len() <= DIALOG_GLYPHS,
    "a confirmation's consequence is wider than the line the box gives it"
);
const _: () = assert!(
    control_label(ControlKind::Leave).len() * GLYPH <= DIALOG_ANSWER_W
        && control_label(ControlKind::Forget).len() * GLYPH <= DIALOG_ANSWER_W,
    "a confirmation's own button cannot hold the word of the action it performs"
);
const _: () = assert!(
    ASSOCIATING_BODY.len() <= DIALOG_GLYPHS && ASSOCIATED_BODY.len() <= DIALOG_GLYPHS,
    "what the radio is waiting on is wider than the line the box gives it"
);
const _: () = assert!(
    JOINING_WORD.len() + 2 <= DIALOG_GLYPHS && JOINED_WORD.len() + 2 <= DIALOG_GLYPHS,
    "the join box's own word leaves no room for the network it is about, and its space between"
);
const _: () = assert!(
    SETUP_NOTICE_TITLE.len() <= DIALOG_GLYPHS && SETUP_NOTICE_PREFIX.len() + 12 <= DIALOG_GLYPHS,
    "the setup announcement is wider than the box it is drawn in"
);
const _: () = assert!(
    SETUP_STARTING_BODY.len() <= DIALOG_GLYPHS,
    "what the box says while the access point starts is wider than its line"
);
const _: () = assert!(
    SETUP_PASS_PREFIX.len() + SETUP_PASS_LEN <= DIALOG_GLYPHS
        && SETUP_PASS_OPEN.len() <= DIALOG_GLYPHS,
    "the setup passphrase is wider than the line the box gives it"
);

/// The question itself: the verb, and the network it is about in quotes.
pub fn format_question(confirm: Confirm, link: LinkStatus, buf: &mut [u8; DIALOG_BYTES]) -> &[u8] {
    let verb = control_label(confirm_verb(confirm));
    buf[..verb.len()].copy_from_slice(verb);
    let mut n = verb.len();
    let Some(ap) = confirm.network(link) else {
        return &buf[..n];
    };
    buf[n] = b' ';
    buf[n + 1] = b'"';
    n += 2;
    let mut name = [0u8; NAME_BYTES];
    let name = format_network_name(ap, (DIALOG_GLYPHS - n - 2) * GLYPH, &mut name);
    buf[n..n + name.len()].copy_from_slice(name);
    n += name.len();
    buf[n] = b'"';
    buf[n + 1] = b'?';
    &buf[..n + 2]
}

/// The join box's own title: the word for the stage the radio has reached, and the network
/// the attempt is for. The word follows the radio's own report; the name follows the row the
/// page handed over, which it has before the radio does.
pub fn format_joining_title(
    associated: bool,
    name: Option<AccessPoint>,
    buf: &mut [u8; DIALOG_BYTES],
) -> &[u8] {
    let word = if associated {
        JOINED_WORD
    } else {
        JOINING_WORD
    };
    buf[..word.len()].copy_from_slice(word);
    let mut n = word.len();
    let Some(ap) = name else {
        return &buf[..n];
    };
    buf[n] = b' ';
    n += 1;
    let mut name = [0u8; NAME_BYTES];
    let name = format_network_name(ap, (DIALOG_GLYPHS - n) * GLYPH, &mut name);
    buf[n..n + name.len()].copy_from_slice(name);
    &buf[..n + name.len()]
}

/// What the join box says under the word: which of the two things the attempt is waiting
/// on.
pub fn joining_body(associated: bool) -> &'static [u8] {
    if associated {
        ASSOCIATED_BODY
    } else {
        ASSOCIATING_BODY
    }
}

fn write_bssid(bssid: &[u8; 6], buf: &mut [u8; NAME_BYTES]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut n = 0;
    for (index, octet) in bssid.iter().enumerate() {
        if index > 0 {
            buf[n] = b':';
            n += 1;
        }
        buf[n] = HEX[usize::from(octet >> 4)];
        buf[n + 1] = HEX[usize::from(octet & 0x0f)];
        n += 2;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [ControlKind; 10] = [
        ControlKind::Cancel,
        ControlKind::Scan,
        ControlKind::Wait,
        ControlKind::Leave,
        ControlKind::Forget,
        ControlKind::Keep,
        ControlKind::AccessPoint,
        ControlKind::Panel,
        ControlKind::Pass,
        ControlKind::NoPass,
    ];

    #[test]
    fn every_control_kind_has_a_word_and_no_two_share_one() {
        let mut words = Vec::new();
        for kind in KINDS {
            let word = control_label(kind);
            assert!(!word.is_empty(), "{kind:?} has nothing to put on its box");
            assert!(
                !words.contains(&word),
                "{kind:?} repeats another box's word"
            );
            words.push(word);
        }
    }

    #[test]
    fn a_controls_word_names_what_it_does() {
        assert_eq!(control_label(ControlKind::Cancel), b"CANCEL");
        assert_eq!(control_label(ControlKind::Scan), b"SCAN");
        assert_eq!(control_label(ControlKind::Wait), b"WAIT");
        assert_eq!(control_label(ControlKind::Leave), b"LEAVE");
        assert_eq!(control_label(ControlKind::Forget), b"FORGET");
        assert_eq!(control_label(ControlKind::Keep), b"KEEP");
        assert_eq!(control_label(ControlKind::AccessPoint), b"AP");
        assert_eq!(control_label(ControlKind::Panel), b"PANEL");
        assert_eq!(control_label(ControlKind::Pass), b"PASS");
        assert_eq!(control_label(ControlKind::NoPass), b"NO PASS");
    }

    #[test]
    fn the_layer_key_names_the_layer_it_switches_to() {
        assert_eq!(key_label(Key::Layer, Layer::Abc), b"123");
        assert_eq!(key_label(Key::Layer, Layer::Digits), b"#&?");
        assert_eq!(key_label(Key::Layer, Layer::Symbols), b"abc");
    }

    #[test]
    fn a_control_key_carries_its_own_word_and_a_character_has_none() {
        assert_eq!(key_label(Key::Shift, Layer::Abc), b"SH");
        assert_eq!(key_label(Key::Backspace, Layer::Abc), b"<-");
        assert_eq!(key_label(Key::Connect, Layer::Abc), b"OK");
        assert_eq!(key_label(Key::Cancel, Layer::Abc), b"X");
        assert_eq!(key_label(Key::Reveal, Layer::Abc), b"SHOW");
        assert_eq!(key_label(Key::Char(0), Layer::Abc), b"");
    }

    #[test]
    fn a_chinese_name_is_drawn_whole_and_cut_by_pixels() {
        let ap = AccessPoint {
            ssid: iot_core::drivers::network::Ssid::from_octets("网络设置".as_bytes()),
            bssid: [0xaa; 6],
            ..AccessPoint::default()
        };
        let mut buf = [0u8; NAME_BYTES];
        assert_eq!(
            format_network_name(ap, 12 * 4, &mut buf),
            "网络设置".as_bytes()
        );
        assert_eq!(
            format_network_name(ap, 12 * 2 + 6, &mut buf),
            "网络~".as_bytes(),
            "the mark takes one cell of the room"
        );
        assert_eq!(
            format_network_name(ap, 12 * 2, &mut buf),
            "网~".as_bytes(),
            "a hanzi is two cells, so the cut does not split one"
        );
    }

    #[test]
    fn a_name_that_is_not_text_falls_back_to_its_bssid() {
        let ap = AccessPoint {
            ssid: iot_core::drivers::network::Ssid::from_octets(b"Caf\xe9"),
            bssid: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab],
            ..AccessPoint::default()
        };
        let mut buf = [0u8; NAME_BYTES];
        assert_eq!(
            format_network_name(ap, NETWORK_NAME_W, &mut buf),
            b"01:23:45:67:89:AB"
        );
    }

    #[test]
    fn the_join_box_names_the_row_the_page_handed_over() {
        let ap = AccessPoint {
            ssid: iot_core::drivers::network::Ssid::from_octets(b"neighbour"),
            ..AccessPoint::default()
        };
        let mut buf = [0u8; DIALOG_BYTES];
        assert_eq!(
            format_joining_title(false, Some(ap), &mut buf),
            b"JOINING neighbour"
        );
        assert_eq!(joining_body(false), b"WAIT FOR THE NETWORK");
    }

    #[test]
    fn the_join_boxes_word_follows_the_radio_and_its_body_says_what_it_waits_on() {
        let ap = AccessPoint {
            ssid: iot_core::drivers::network::Ssid::from_octets(b"cafe"),
            ..AccessPoint::default()
        };
        let mut buf = [0u8; DIALOG_BYTES];
        assert_eq!(
            format_joining_title(true, Some(ap), &mut buf),
            b"JOINED cafe"
        );
        assert_eq!(joining_body(true), b"WAIT FOR AN ADDRESS");
    }

    #[test]
    fn a_join_with_no_row_says_only_the_stage() {
        let mut buf = [0u8; DIALOG_BYTES];
        assert_eq!(format_joining_title(false, None, &mut buf), b"JOINING");
        assert_eq!(format_joining_title(true, None, &mut buf), b"JOINED");
    }
}

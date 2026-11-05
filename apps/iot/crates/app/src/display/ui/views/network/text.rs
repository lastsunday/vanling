//! Network, setup and question text: the words the link's readings become.

use crate::controls::{
    DIALOG_ANSWER_W, DIALOG_GLYPHS, Key, LIST_ROWS, Layer, ToolbarCell, toolbar_cell,
};
use crate::model::state::{Confirm, Entry};
use crate::model::state::{ControlKind, confirm_verb};
use iot_core::drivers::network::{
    AccessPoint, Ipv4, LinkPhase, LinkStatus, NetworkError, SSID_BYTES, SetupPass,
};
use iot_core::overlay::{GLYPH_PITCH, OVERLAY_GAP};
use iot_core::provision::SETUP_PASS_LEN;

use iot_ui::text::{write_u16, write_u64};

/// Glyphs a network name is given on a scan-list row.
pub(super) const NETWORK_NAME_GLYPHS: usize = 24;

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
    control_label(ControlKind::Cancel).len() * GLYPH_PITCH - OVERLAY_GAP
        <= toolbar_cell(ToolbarCell::Left).w,
    "the way out of setup mode does not fit the cell the strip gives it"
);
const _: () = assert!(
    control_label(ControlKind::AccessPoint).len() * GLYPH_PITCH - OVERLAY_GAP
        <= toolbar_cell(ToolbarCell::Right).w
        && control_label(ControlKind::Panel).len() * GLYPH_PITCH - OVERLAY_GAP
            <= toolbar_cell(ToolbarCell::Right).w,
    "the host switch does not fit the cell the strip gives it"
);
const _: () = assert!(
    control_label(ControlKind::NoPass).len() * GLYPH_PITCH - OVERLAY_GAP <= DIALOG_ANSWER_W,
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

/// A row's name: the SSID when the font can draw it, and the BSSID when it cannot.
pub fn format_network_name(ap: AccessPoint, glyphs: usize, buf: &mut [u8; SSID_BYTES]) -> &[u8] {
    let glyphs = match ap.ssid.as_display_text() {
        Some(name) => write_name(name.as_bytes(), glyphs, buf),
        None => write_bssid(&ap.bssid, buf),
    };
    &buf[..glyphs]
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

/// The setup access point's own name, derived from the board's address here rather than
/// carried in the state: the network task derives it the same way, and two copies of one
/// name are one copy too many.
pub fn format_setup_name(mac: Option<[u8; 6]>, buf: &mut [u8; SSID_BYTES]) -> &[u8] {
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
pub fn format_setup_notice_body(mac: Option<[u8; 6]>, buf: &mut [u8; SSID_BYTES]) -> &[u8] {
    let mut name = [0u8; SSID_BYTES];
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
    buf: &'a mut [u8; SSID_BYTES],
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
    control_label(ControlKind::Leave).len() * GLYPH_PITCH <= DIALOG_ANSWER_W
        && control_label(ControlKind::Forget).len() * GLYPH_PITCH <= DIALOG_ANSWER_W,
    "a confirmation's own button cannot hold the word of the action it performs"
);
const _: () = assert!(
    ASSOCIATING_BODY.len() <= DIALOG_GLYPHS && ASSOCIATED_BODY.len() <= DIALOG_GLYPHS,
    "what the radio is waiting on is wider than the line the box gives it"
);
const _: () = assert!(
    DIALOG_GLYPHS <= SSID_BYTES,
    "a name folded into the box's line does not fit the buffer it is folded in"
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
    SETUP_PASS_PREFIX.len() + SETUP_PASS_LEN <= DIALOG_GLYPHS
        && SETUP_PASS_OPEN.len() <= DIALOG_GLYPHS,
    "the setup passphrase is wider than the line the box gives it"
);

/// The question itself: the verb, and the network it is about in quotes.
pub fn format_question(confirm: Confirm, link: LinkStatus, buf: &mut [u8; DIALOG_GLYPHS]) -> &[u8] {
    let verb = control_label(confirm_verb(confirm));
    buf[..verb.len()].copy_from_slice(verb);
    let mut n = verb.len();
    let Some(ap) = confirm.network(link) else {
        return &buf[..n];
    };
    buf[n] = b' ';
    buf[n + 1] = b'"';
    n += 2;
    let mut name = [0u8; SSID_BYTES];
    let name = format_network_name(ap, DIALOG_GLYPHS - n - 2, &mut name);
    buf[n..n + name.len()].copy_from_slice(name);
    n += name.len();
    buf[n] = b'"';
    buf[n + 1] = b'?';
    &buf[..n + 2]
}

/// The join box's own title: the word for the stage the radio has reached, and the network
/// the attempt is for.
pub fn format_joining_title(ap: Option<AccessPoint>, buf: &mut [u8; DIALOG_GLYPHS]) -> &[u8] {
    let word = if ap.is_some() {
        JOINED_WORD
    } else {
        JOINING_WORD
    };
    buf[..word.len()].copy_from_slice(word);
    let mut n = word.len();
    let Some(ap) = ap else {
        return &buf[..n];
    };
    buf[n] = b' ';
    n += 1;
    let mut name = [0u8; SSID_BYTES];
    let name = format_network_name(ap, DIALOG_GLYPHS - n, &mut name);
    buf[n..n + name.len()].copy_from_slice(name);
    &buf[..n + name.len()]
}

/// What the join box says under the word: which of the two things the attempt is waiting
/// on.
pub fn joining_body(ap: Option<AccessPoint>) -> &'static [u8] {
    if ap.is_some() {
        ASSOCIATED_BODY
    } else {
        ASSOCIATING_BODY
    }
}

fn write_name(name: &[u8], glyphs: usize, buf: &mut [u8; SSID_BYTES]) -> usize {
    if name.len() <= glyphs {
        buf[..name.len()].copy_from_slice(name);
        return name.len();
    }
    let kept = glyphs - 1;
    buf[..kept].copy_from_slice(&name[..kept]);
    buf[kept] = b'~';
    glyphs
}

fn write_bssid(bssid: &[u8; 6], buf: &mut [u8; SSID_BYTES]) -> usize {
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
}

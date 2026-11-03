//! Network, setup and question text: the words the link's readings become.

use iot_core::drivers::network::{
    AccessPoint, Ipv4, LinkPhase, LinkStatus, NetworkError, SSID_BYTES, SetupPass,
};
use iot_core::keyboard::{DIALOG_ANSWER_W, DIALOG_GLYPHS};
use iot_core::overlay::GLYPH_PITCH;
use iot_core::provision::SETUP_PASS_LEN;
use iot_core::state::Confirm;

use super::number::write_u16;

/// Glyphs a network name is given on a scan-list row.
pub const NETWORK_NAME_GLYPHS: usize = 24;

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
    let name = iot_core::provision::setup_ap_ssid(mac);
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
    iot_core::state::controls::LEAVE_LABEL.len() * GLYPH_PITCH <= DIALOG_ANSWER_W
        && iot_core::state::controls::FORGET_LABEL.len() * GLYPH_PITCH <= DIALOG_ANSWER_W,
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
    let verb = iot_core::state::controls::confirm_verb(confirm);
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

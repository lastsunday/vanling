//! Turning readings into the glyphs a panel draws.
//!
//! Separated from [`crate::virtual_components::display_light`] so it can be built and tested
//! on the development machine: this is arithmetic over glyph buffers and frame bytes, with no
//! peripheral in it, and the arithmetic is where the bugs were. A reading that formats wider
//! than its buffer indexes past the end, and that only shows up on hardware — twice, in two
//! different pages, before this had a test.
//!
//! Not camera-specific despite the module name: the panel's own coordinate formatting is here
//! too, because it is the same arithmetic and the panel is the only surface that draws it, and
//! so is the Network page's — a phase word, an address and a name are the same kind of thing as
//! a coordinate pair, and the rule they all share is that a reading wider than its buffer
//! indexes past the end. The camera's additions are marked, and the module is unconditional — a
//! gate would leave `display-light` naming a function only some features define.
//!
//! The layout — which block each reading sits in and at what row — belongs to the panel, which
//! is the only thing that knows where the columns are. What is here is how a number becomes
//! the characters that will go in one.

use iot_core::drivers::network::{
    AccessPoint, Ipv4, LinkPhase, LinkStatus, NetworkError, SSID_BYTES, SetupPass,
};
use iot_core::keyboard::{DIALOG_ANSWER_W, DIALOG_GLYPHS};
use iot_core::overlay::GLYPH_PITCH;
use iot_core::provision::SETUP_PASS_LEN;
use iot_core::state::Confirm;

/// A coordinate pair written `x y`.
///
/// Sized for the widest pair a panel-space reading can take — five digits either side of a
/// space — rather than for the five-glyph block it is usually written in. A buffer sized to
/// the block indexes past its end on exactly the readings that are widest, and those are the
/// ones worth printing: a window that reads `319 239` fits anywhere, and one that overflows is
/// wrong only at the interesting value.
pub fn format_pair(pair: Option<(u16, u16)>, buf: &mut [u8; 12]) -> &[u8] {
    match pair {
        Some((x, y)) => {
            let n = write_u16(x, buf, 0);
            buf[n] = b' ';
            let end = write_u16(y, buf, n + 1);
            &buf[..end]
        }
        None => {
            buf[0] = b'-';
            &buf[..1]
        }
    }
}

/// `u32` in a glyph buffer, for counters that outgrow a `u16`.
///
/// Over [`write_u64`] rather than writing digits here, because a second implementation of the
/// same arithmetic is how a caller ends up with one format filling a buffer from the right and
/// every reader taking it from the left — which reads a label one glyph into the value beside
/// it, and overflows the moment the buffer is the narrower of the two.
pub fn format_u32(value: u32, buf: &mut [u8; 12]) -> &[u8] {
    let n = write_u64(u64::from(value), buf, 0);
    &buf[..n]
}

/// Decimal ASCII digits of `value` (no leading zeros) written into the start of `buf`,
/// returned as the filled prefix.
pub fn write_u16(value: u16, buf: &mut [u8], n: usize) -> usize {
    write_u64(u64::from(value), buf, n)
}

/// Decimal ASCII digits of `value` (no leading zeros) written into `buf` from `n`, returned as
/// the index one past the last digit.
///
/// Widened over the `u16` the other formatters take, because a counter outgrows `u16` — frames
/// do after about eight hours at this frame rate, and the test reaches it at once.
pub fn write_u64(mut value: u64, buf: &mut [u8], mut n: usize) -> usize {
    let mut tmp = [0u8; 20];
    let mut len = 0;
    loop {
        tmp[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in tmp[..len].iter().rev() {
        buf[n] = digit;
        n += 1;
    }
    n
}

/// The decimation ratio as `r:c`, from the register's row and column nibbles.
///
/// Two nibbles rather than one number because they are set independently and a ratio written
/// as a single figure hides which axis is wrong.
pub fn write_ratio(buf: &mut [u8; 6], subsample: u8) -> &[u8] {
    buf[0] = b'0' + (subsample >> 4);
    buf[1] = b':';
    buf[2] = b'0' + (subsample & 0x0f);
    &buf[..3]
}

/// Glyphs a network name is given on a scan-list row.
///
/// The panel is [`PANEL_COLUMNS`](iot_core::overlay::PANEL_COLUMNS) wide and a list row puts a
/// name at the page's own left edge with its signal bars right-aligned against the panel, so this
/// is what fits between them with room for the marker an open network carries. Every name is
/// formatted through a width like it, because a row that drew as far as it happened to reach would
/// let a long name run under the bars and off the edge.
pub const NETWORK_NAME_GLYPHS: usize = 24;

/// The link's phase as one four-glyph word, and a failure as what failed rather than as the
/// fact of failing: the driver logs its own error code with the call that produced it, so the
/// panel's job is to say which of the kinds it was. A `NoMemory` here is the one reading
/// that means "this build's heap budget is wrong", and it has to be tellable from a busy radio;
/// `NOAP` and `AUTH` are the two a user can act on — walk closer, or look at the passphrase.
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
///
/// A dash and not `0.0.0.0`: the two are different states — no lease yet, against a lease that
/// covers the whole network — and the phase beside it is what says which.
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
///
/// The bars on a list row and on the access point the link is actually on, so the two are read
/// against the same scale (`bars_for`), and as glyphs rather than as drawn bars because a list
/// is text: a row is a name with its strength in front of it.
pub fn format_bars(bars: u8, buf: &mut [u8; 4]) -> &[u8] {
    let bars = bars.min(4);
    for (index, glyph) in buf.iter_mut().enumerate() {
        *glyph = if (index as u8) < bars { b'#' } else { b'.' };
    }
    &buf[..]
}

/// A row's name: the SSID when the font can draw it, and the BSSID when it cannot.
///
/// The fallback is [`Ssid::as_display_text`](iot_core::drivers::network::Ssid::as_display_text)'s
/// decision, made once in the driver module rather than per surface — a name in an encoding
/// this font has no glyphs for is still a real network, and the address of the radio is what
/// stays drawable. Truncation keeps the head and marks it with a `~`: two networks whose names
/// differ past the fold would otherwise draw the same row, and the row a user picks from is
/// the one thing this list is for.
///
/// Takes the access point by value because it is [`Copy`] and the row is drawn from the state's
/// own copy of it: a borrow would give this function two references in and one out, and the
/// answer comes from the buffer rather than from the row.
pub fn format_network_name(ap: AccessPoint, glyphs: usize, buf: &mut [u8; SSID_BYTES]) -> &[u8] {
    let glyphs = match ap.ssid.as_display_text() {
        Some(name) => write_name(name.as_bytes(), glyphs, buf),
        None => write_bssid(&ap.bssid, buf),
    };
    &buf[..glyphs]
}

/// The verb a confirmation's question and its action share: what the page would do.
///
/// One word for both places, because the button has to be the word the question asked with — a box
/// that asked `LEAVE "cafe"?` and offered a button saying anything else would be a finger reading
/// one action and pressing another.
pub fn format_verb(confirm: Confirm) -> &'static [u8] {
    match confirm {
        Confirm::Leave => LEAVE_VERB,
        Confirm::Forget(_) => FORGET_VERB,
    }
}

/// The consequence under the question: what the action costs, in the box's own width.
///
/// Said at all because both actions are the kind that cannot be undone — a link that is down is a
/// passphrase away from up, and a forgotten network is one the next boot does not join — and a
/// question that named no cost would be a dialog a user learns to dismiss.
pub fn format_body(confirm: Confirm) -> &'static [u8] {
    match confirm {
        Confirm::Leave => LEAVE_BODY,
        Confirm::Forget(_) => FORGET_BODY,
    }
}

const LEAVE_VERB: &[u8] = b"LEAVE";
const FORGET_VERB: &[u8] = b"FORGET";
const LEAVE_BODY: &[u8] = b"THE LINK GOES DOWN";
const FORGET_BODY: &[u8] = b"THE SAVED KEY IS ERASED";

/// What the box says while the radio is on its way: the two stages, which are two different waits.
///
/// Told apart because they are what the user is actually waiting on and they end differently — the
/// first ends in a link or in a refusal, the second in an address or in a link with none — and a
/// single word for both would have the panel say the same thing about a radio that has not answered
/// yet and one that has an access point and no lease.
const ASSOCIATING_BODY: &[u8] = b"WAIT FOR THE NETWORK";
const ASSOCIATED_BODY: &[u8] = b"WAIT FOR AN ADDRESS";

/// The announcement that opens setup mode: what the device is doing, and the name a phone has to
/// join. The address is not part of it — the header's `IP` row carries it for as long as the mode
/// lasts, and a captive portal opens the page without one being typed.
const SETUP_NOTICE_TITLE: &[u8] = b"SETUP MODE";
const SETUP_NOTICE_PREFIX: &[u8] = b"AP ";

/// The announcement's title, which is the same line every time.
pub fn format_setup_notice_title() -> &'static [u8] {
    SETUP_NOTICE_TITLE
}

/// The setup access point's own name, derived from the board's address here rather than carried in
/// the state: the network task derives it the same way, and two copies of one name are one copy too
/// many.
pub fn format_setup_name(mac: Option<[u8; 6]>, buf: &mut [u8; SSID_BYTES]) -> &[u8] {
    let Some(mac) = mac else {
        return b"";
    };
    let name = iot_core::provision::ap_name(mac);
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

/// The passphrase's own line: the label, then the value, or the state said out loud.
///
/// `OPEN, NO PASSWORD` rather than a word or a dash, because an access point with no passphrase is
/// one anybody in range can configure, and the line is where the person who asked for that is told
/// what they asked for. The passphrase is the one thing on this panel that is not the product's own
/// — made per boot, shown rather than remembered.
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

/// The word the join box's title opens with, at each of the two stages.
const JOINING_WORD: &[u8] = b"JOINING";
const JOINED_WORD: &[u8] = b"JOINED";

const _: () = assert!(
    LEAVE_BODY.len() <= DIALOG_GLYPHS && FORGET_BODY.len() <= DIALOG_GLYPHS,
    "a confirmation's consequence is wider than the line the box gives it"
);
const _: () = assert!(
    LEAVE_VERB.len() * GLYPH_PITCH <= DIALOG_ANSWER_W
        && FORGET_VERB.len() * GLYPH_PITCH <= DIALOG_ANSWER_W,
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
///
/// The name is folded to what is left of the line after the verb and its punctuation, which is
/// narrower than the header's own slot. That is the header's job rather than this one's: the whole
/// name stays on the row the question is about, and the question reads with it in view — what the
/// box cannot show is the whole name, and what it must not do is drop the quotes or the question
/// mark that make the line a question.
pub fn format_question(confirm: Confirm, link: LinkStatus, buf: &mut [u8; DIALOG_GLYPHS]) -> &[u8] {
    let verb = format_verb(confirm);
    buf[..verb.len()].copy_from_slice(verb);
    let mut n = verb.len();
    let Some(ap) = confirm.network(link) else {
        // A link the radio has not accepted yet, or one that went down under the box: the question
        // is still the verb, and the box is on its way out — a title that named nothing is better
        // than one naming a network the page is no longer about.
        return &buf[..n];
    };
    buf[n] = b' ';
    buf[n + 1] = b'"';
    n += 2;
    // The closing quote and the question mark are the box's own glyphs and are always drawn; what
    // gives way is the name.
    let mut name = [0u8; SSID_BYTES];
    let name = format_network_name(ap, DIALOG_GLYPHS - n - 2, &mut name);
    buf[n..n + name.len()].copy_from_slice(name);
    n += name.len();
    buf[n] = b'"';
    buf[n + 1] = b'?';
    &buf[..n + 2]
}

/// The join box's own title: the word for the stage the radio has reached, and the network the
/// attempt is for.
///
/// The stage comes from [`LinkStatus::ap`](iot_core::drivers::network::LinkStatus::ap) being there
/// at all rather than from a second argument: the radio fills it in the moment it is on the
/// network, and empty is the whole of what "still being accepted" looks like from here. A title
/// that named no network is a build whose credential nobody typed on the panel — it joined a name
/// out of its own flash or its own build inputs — and the box says the word alone rather than
/// inventing one.
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

/// What the join box says under the word: which of the two things the attempt is waiting on.
///
/// One function rather than two, because which line it is and which word the title carries are one
/// fact about the radio and two drawings of it. See `ASSOCIATING_BODY`.
pub fn format_joining_body(ap: Option<AccessPoint>) -> &'static [u8] {
    if ap.is_some() {
        ASSOCIATED_BODY
    } else {
        ASSOCIATING_BODY
    }
}

/// The name, head-first, into `buf`; returns how many glyphs it occupies.
///
/// The width is the caller's, because it is a surface's question: the scan list has the whole row
/// for a name and the row a passphrase is typed on shares its one row with a phase word and the
/// draft, so the same network is folded differently in the two places.
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

/// The BSSID as `AA:BB:CC:DD:EE:FF`; returns how many glyphs it occupies.
///
/// Upper case because this is the same address the header's own line prints ([`mac_text`]), and the
/// row under it that fell back to an address is read against that line.
///
/// [`mac_text`]: iot_core::provision::mac_text
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

/// Bytes between fingerprint samples. Prime and over the frame's length, so the walk visits a
/// spread of rows instead of one stride's worth of them.
pub const FINGERPRINT_STRIDE: usize = 997;

/// An FNV fold over a sparse sample of the frame, kept to sixteen bits.
///
/// The one number that says whether there is a picture. A sensor whose PLL has not locked
/// streams correctly framed noise with every register reading back correct, so the geometry
/// readings cannot distinguish it from a real picture — only the pixels can. Sparse because a
/// frame is 150 KB and this runs on every painted frame.
pub fn frame_fingerprint(frame: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in frame.iter().step_by(FINGERPRINT_STRIDE) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash >> 16
}

#[cfg(test)]
mod tests {
    use super::{
        NETWORK_NAME_GLYPHS, format_address, format_bars, format_body, format_joining_body,
        format_joining_title, format_network_name, format_pair, format_phase, format_question,
        format_setup_notice_body, format_setup_notice_title, format_setup_pass, format_u32,
        format_verb, frame_fingerprint, write_ratio, write_u64,
    };
    use alloc::vec;
    use alloc::vec::Vec;
    use iot_core::drivers::network::{
        AccessPoint, Ipv4, LinkPhase, LinkStatus, NetworkError, SSID_BYTES, SetupPass, Ssid,
    };
    use iot_core::keyboard::{DIALOG_ANSWER_W, DIALOG_GLYPHS};
    use iot_core::overlay::GLYPH_PITCH;
    use iot_core::state::Confirm;

    #[test]
    fn a_phase_and_a_failure_are_both_four_glyphs() {
        let mut buf = [0u8; 4];
        assert_eq!(format_phase(LinkPhase::Off, &mut buf), b"OFF");
        assert_eq!(format_phase(LinkPhase::Scanning, &mut buf), b"SCAN");
        assert_eq!(format_phase(LinkPhase::Connecting, &mut buf), b"JOIN");
        assert_eq!(format_phase(LinkPhase::Connected, &mut buf), b"UP");
        // Six kinds, six words: a heap that ran out and a radio that said no are the two a
        // reader has to be able to tell apart, and one word for two of them would hide that.
        let words = [
            (NetworkError::Busy, b"BUSY".as_slice()),
            (NetworkError::InvalidArguments, b"NAME".as_slice()),
            (NetworkError::NoMemory, b"MEM".as_slice()),
            (NetworkError::NotFound, b"NOAP".as_slice()),
            (NetworkError::Credentials, b"AUTH".as_slice()),
            (NetworkError::Driver, b"DRV".as_slice()),
        ];
        for (kind, word) in words {
            assert_eq!(format_phase(LinkPhase::Failed(kind), &mut buf), word);
        }
    }

    #[test]
    fn an_address_is_a_dotted_quad_and_no_address_is_a_dash() {
        let mut buf = [0u8; 16];
        assert_eq!(format_address(None, &mut buf), b"-");
        // The reading that overflowed nothing but a lazy buffer: fifteen glyphs is the widest
        // an IPv4 address can be, and this is the one the bench actually hands out.
        assert_eq!(
            format_address(Some(Ipv4::from_octets([192, 168, 1, 136])), &mut buf),
            b"192.168.1.136"
        );
        assert_eq!(
            format_address(Some(Ipv4::from_octets([255, 255, 255, 255])), &mut buf),
            b"255.255.255.255"
        );
        assert_eq!(
            format_address(Some(Ipv4::from_octets([0, 0, 0, 0])), &mut buf),
            b"0.0.0.0",
            "a lease of zero is a reading, not an absence"
        );
    }

    #[test]
    fn bars_are_four_glyphs_of_which_the_reading_fills_some() {
        let mut buf = [0u8; 4];
        assert_eq!(format_bars(0, &mut buf), b"....");
        assert_eq!(format_bars(2, &mut buf), b"##..");
        assert_eq!(format_bars(4, &mut buf), b"####");
        // Clamped rather than trusted: the bar count comes off the wire, and a fifth glyph in a
        // four-glyph buffer is what this function would index past the end on.
        assert_eq!(format_bars(9, &mut buf), b"####");
    }

    #[test]
    fn a_name_that_is_not_text_draws_as_the_address_of_its_radio() {
        let mut buf = [0u8; SSID_BYTES];
        let named = ap(b"neighbour", [0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        assert_eq!(
            format_network_name(named, NETWORK_NAME_GLYPHS, &mut buf),
            b"neighbour"
        );

        let mut latin1 = [0u8; 32];
        latin1[..4].copy_from_slice(&[0x63, 0x61, 0x66, 0xe9]);
        let unnamed = AccessPoint {
            ssid: Ssid::from_octets(&latin1[..4]),
            ..named
        };
        assert_eq!(
            format_network_name(unnamed, NETWORK_NAME_GLYPHS, &mut buf),
            b"02:00:00:00:00:01",
            "the row keeps its address when its name cannot be drawn"
        );
        // And in the same case as the header's own line: the fallback is the one drawing where a
        // user reads two addresses side by side, and `02:00:…` cannot tell two cases apart.
        let lettered = AccessPoint {
            bssid: [0xaa, 0x0f, 0x5c, 0x00, 0xff, 0x01],
            ..unnamed
        };
        assert_eq!(
            format_network_name(lettered, NETWORK_NAME_GLYPHS, &mut buf),
            b"AA:0F:5C:00:FF:01"
        );
        assert_eq!(
            format_network_name(named, 5, &mut buf),
            b"neig~",
            "and a narrower surface folds it to its own width"
        );
    }

    #[test]
    fn a_name_longer_than_the_row_keeps_its_head_and_says_so() {
        let mut buf = [0u8; SSID_BYTES];
        let long = ap(b"neighbour-extension-2.4GHz", [1, 2, 3, 4, 5, 6]);
        let drawn = format_network_name(long, NETWORK_NAME_GLYPHS, &mut buf);
        assert_eq!(drawn.len(), NETWORK_NAME_GLYPHS);
        assert_eq!(
            &drawn[..NETWORK_NAME_GLYPHS - 1],
            b"neighbour-extension-2.4"
        );
        assert_eq!(drawn[NETWORK_NAME_GLYPHS - 1], b'~');

        // Exactly the row's width is not truncated: the marker says "there is more", and a name
        // that ends where the row does has no more to show.
        let exact = ap(&[b'x'; NETWORK_NAME_GLYPHS], [1, 2, 3, 4, 5, 6]);
        assert_eq!(
            format_network_name(exact, NETWORK_NAME_GLYPHS, &mut buf),
            &[b'x'; NETWORK_NAME_GLYPHS][..]
        );
    }

    #[test]
    fn a_question_names_the_network_it_is_about_within_the_boxes_own_line() {
        let mut buf = [0u8; DIALOG_GLYPHS];
        let named = ap(b"neighbour", [1, 2, 3, 4, 5, 6]);

        // A name wider than what is left of the line is folded, and the box's own punctuation
        // stays: the quotes and the question mark are what make the line a question, and a title
        // that gave them up to fit a name would be a row rather than a question.
        let long = ap(b"neighbour-extension-2.4GHz", [1, 2, 3, 4, 5, 6]);
        let drawn = format_question(Confirm::Forget(long), LinkStatus::default(), &mut buf);
        assert_eq!(drawn.len(), DIALOG_GLYPHS);
        assert_eq!(&drawn[..8], b"FORGET \"");
        assert_eq!(&drawn[drawn.len() - 2..], b"\"?");
        assert_eq!(drawn[DIALOG_GLYPHS - 3], b'~', "the fold is marked");

        // A forget's question is about the row the finger held rather than about the link, which is
        // the whole reason the confirmation carries its own network.
        assert_eq!(
            format_question(Confirm::Forget(named), LinkStatus::default(), &mut buf),
            b"FORGET \"neighbour\"?"
        );
    }

    #[test]
    fn a_leave_names_the_network_the_link_is_on() {
        // The network is the link's, read when the question is drawn rather than carried: the row
        // behind the box is the one the link is on, so the question and the highlight cannot come
        // apart. A link with no access point yet is asked about with the verb alone.
        let mut buf = [0u8; DIALOG_GLYPHS];
        let named = ap(b"cafe", [1, 2, 3, 4, 5, 6]);
        let link = LinkStatus {
            ap: Some(named),
            ..LinkStatus::default()
        };
        assert_eq!(
            format_question(Confirm::Leave, link, &mut buf),
            b"LEAVE \"cafe\"?"
        );
        assert_eq!(Confirm::Leave.network(link), Some(named));
        assert_eq!(
            format_question(Confirm::Leave, LinkStatus::default(), &mut buf),
            b"LEAVE"
        );
        assert_eq!(Confirm::Leave.network(LinkStatus::default()), None);
    }

    #[test]
    fn the_setup_announcement_names_the_access_point_this_board_raises() {
        // Derived from the address the header already draws, so the phone's list and the panel
        // name the same device without the state carrying a second copy of the name.
        let mut buf = [0u8; SSID_BYTES];
        assert_eq!(format_setup_notice_title(), b"SETUP MODE");
        assert_eq!(
            format_setup_notice_body(Some([0x24, 0x6f, 0x28, 0x1a, 0x2f, 0xc8]), &mut buf),
            b"AP vanling-2fc8"
        );
        assert_eq!(
            format_setup_notice_body(None, &mut buf),
            b"",
            "a board with no address has no access point to name"
        );
    }

    #[test]
    fn the_box_says_the_passphrase_or_that_there_is_none() {
        // The one line the phone's side cannot be used without, and the state named out loud when
        // there is nothing to type: `NO PASS` would be a word to read past on a network anybody in
        // range can configure.
        let mut buf = [0u8; SSID_BYTES];
        let pass = SetupPass::from_array(*b"k3mq9t7w2x");
        assert_eq!(format_setup_pass(Some(&pass), &mut buf), b"PASS k3mq9t7w2x");
        assert_eq!(format_setup_pass(None, &mut buf), b"OPEN, NO PASSWORD");
        // The value is the station's, at exactly the length it generates; there is no shorter or
        // longer form for this line to cut or to pad.
        let other = SetupPass::from_array(*b"abcdefghij");
        assert_eq!(
            format_setup_pass(Some(&other), &mut buf),
            b"PASS abcdefghij"
        );
    }

    #[test]
    fn the_join_box_says_which_stage_of_the_attempt_and_which_network() {
        // Two stages, two lines: what the user waits on is the network accepting the credential,
        // and then the network handing out an address. The word and the line under it are one fact
        // about the radio, so both are driven by the same thing — whether there is an access point
        // to name yet.
        let mut buf = [0u8; DIALOG_GLYPHS];
        let named = ap(b"neighbour", [1, 2, 3, 4, 5, 6]);
        assert_eq!(format_joining_title(None, &mut buf), b"JOINING");
        assert_eq!(format_joining_body(None), b"WAIT FOR THE NETWORK");
        assert_eq!(
            format_joining_title(Some(named), &mut buf),
            b"JOINED neighbour"
        );
        assert_eq!(format_joining_body(Some(named)), b"WAIT FOR AN ADDRESS");

        // The name gives way rather than the word: what the line is about is the attempt, and a
        // box whose title dropped to a fold marker would say nothing about what it was doing.
        let long = ap(b"neighbour-extension-2.4GHz", [1, 2, 3, 4, 5, 6]);
        let drawn = format_joining_title(Some(long), &mut buf);
        assert_eq!(drawn.len(), DIALOG_GLYPHS);
        assert_eq!(&drawn[..7], b"JOINED ");
        assert_eq!(drawn[DIALOG_GLYPHS - 1], b'~', "the fold is marked");
    }

    #[test]
    fn both_of_the_join_boxes_lines_fit_the_line_the_box_gives_them() {
        for ap in [None, Some(ap(b"neighbour", [1, 2, 3, 4, 5, 6]))] {
            let mut buf = [0u8; DIALOG_GLYPHS];
            assert!(
                format_joining_title(ap, &mut buf).len() <= DIALOG_GLYPHS,
                "the title is wider than the box's own line: {ap:?}"
            );
            assert!(
                format_joining_body(ap).len() <= DIALOG_GLYPHS,
                "what the radio is waiting on is wider than the box's own line"
            );
        }
    }

    #[test]
    fn the_boxes_words_are_the_verbs_own_and_fit_the_box() {
        // The button and the question share their word: a finger that read `LEAVE "cafe"?` and
        // pressed a button saying anything else would be pressing an action it did not read.
        let cafe = ap(b"cafe", [1, 2, 3, 4, 5, 6]);
        for confirm in [Confirm::Leave, Confirm::Forget(cafe)] {
            let verb = format_verb(confirm);
            let mut buf = [0u8; DIALOG_GLYPHS];
            let question = format_question(confirm, LinkStatus::default(), &mut buf);
            assert!(
                question.starts_with(verb),
                "the question is asked with the button's own word"
            );
            assert!(
                verb.len() * GLYPH_PITCH <= DIALOG_ANSWER_W,
                "the word of the action does not fit the button that performs it"
            );
            assert!(
                format_body(confirm).len() <= DIALOG_GLYPHS,
                "the consequence does not fit the line under the question"
            );
        }
    }

    fn ap(ssid: &[u8], bssid: [u8; 6]) -> AccessPoint {
        AccessPoint {
            ssid: Ssid::from_octets(ssid),
            bssid,
            channel: 6,
            rssi: -55,
            secured: true,
        }
    }

    #[test]
    fn a_pair_holds_the_widest_reading_a_sensor_reports() {
        // Five digits either side of a space is nine characters. This is the reading that
        // overflowed an eight-byte buffer on hardware — at a panel corner and on the camera's
        // own readout window, which is wider than the panel it is drawn on.
        let mut buf = [0u8; 12];
        assert_eq!(
            format_pair(Some((319, 239)), &mut buf),
            b"319 239",
            "a panel corner"
        );
        assert_eq!(
            format_pair(Some((1616, 1208)), &mut buf),
            b"1616 1208",
            "the sensor's readout window, which is wider than the panel"
        );
        assert_eq!(
            format_pair(Some((65535, 65535)), &mut buf),
            b"65535 65535",
            "the widest a u16 pair can be"
        );
    }

    #[test]
    fn a_pair_with_no_reading_says_so_in_one_glyph() {
        let mut buf = [0u8; 12];
        assert_eq!(format_pair(None, &mut buf), b"-");
    }

    #[test]
    fn a_counter_reads_left_to_right_from_zero() {
        // Digits written from the right and returned as the left-aligned prefix every reader
        // takes would walk a label into its own value — which is what a right-aligned writer
        // does, and the two directions must never meet in one module.
        let mut buf = [0u8; 12];
        assert_eq!(format_u32(0, &mut buf), b"0");
        assert_eq!(format_u32(7, &mut buf), b"7");
        assert_eq!(format_u32(1234, &mut buf), b"1234");
        assert_eq!(format_u32(99999, &mut buf), b"99999");
    }

    #[test]
    fn a_counter_outgrowing_u16_keeps_counting() {
        // Frames pass 65535 after about four hours at this frame rate, and a test reaches it
        // at once. A formatter that narrowed first would report a running camera as stalled —
        // the one reading that must never come out of a counter.
        let mut buf = [0u8; 12];
        assert_eq!(format_u32(u32::from(u16::MAX), &mut buf), b"65535");
        assert_eq!(format_u32(u32::from(u16::MAX) + 1, &mut buf), b"65536");
        assert_eq!(
            format_u32(u32::MAX, &mut buf),
            b"4294967295",
            "the widest a u32 can be, and the buffer is exactly wide enough"
        );
    }

    #[test]
    fn write_u64_fills_exactly_the_digits_it_was_given() {
        // The primitive both formatters are built on: no leading zeros, no trailing garbage.
        let mut buf = [0u8; 12];
        buf.fill(b'x');
        let n = write_u64(7, &mut buf, 0);
        assert_eq!(&buf[..n], b"7");
        assert_eq!(buf[1], b'x', "nothing written past the prefix");
    }

    #[test]
    fn the_decimation_ratio_reads_as_two_nibbles() {
        let mut buf = [0u8; 6];
        assert_eq!(
            write_ratio(&mut buf, 0x55),
            b"5:5",
            "ratio 5 is full field of view"
        );
        assert_eq!(write_ratio(&mut buf, 0x33), b"3:3");
        assert_eq!(write_ratio(&mut buf, 0x00), b"0:0");
    }

    #[test]
    fn the_fingerprint_separates_a_blank_frame_from_a_picture() {
        let blank: Vec<u8> = vec![0u8; 320 * 240 * 2];
        let mut filled = blank.clone();
        for (index, byte) in filled.iter_mut().enumerate() {
            // A gradient across the frame, so consecutive samples differ as a real picture's do.
            *byte = (index % 251) as u8;
        }
        assert_ne!(
            frame_fingerprint(&blank),
            frame_fingerprint(&filled),
            "a flat frame and a gradient cannot fold to the same number"
        );
        assert_eq!(
            frame_fingerprint(&blank),
            frame_fingerprint(&blank.clone()),
            "the same frame twice folds the same, which is what makes a repeat detectable"
        );
    }
}

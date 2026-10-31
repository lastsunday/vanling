//! Turning readings into the glyphs a page draws.

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

/// Decimal ASCII digits of `value` (no leading zeros) written into `buf` from `n`, returned
/// as the index one past the last digit.
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
pub fn write_ratio(buf: &mut [u8; 6], subsample: u8) -> &[u8] {
    buf[0] = b'0' + (subsample >> 4);
    buf[1] = b':';
    buf[2] = b'0' + (subsample & 0x0f);
    &buf[..3]
}

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
    iot_core::controls::LEAVE_LABEL.len() * GLYPH_PITCH <= DIALOG_ANSWER_W
        && iot_core::controls::FORGET_LABEL.len() * GLYPH_PITCH <= DIALOG_ANSWER_W,
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
    let verb = iot_core::controls::confirm_verb(confirm);
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

/// Bytes between fingerprint samples.
pub const FINGERPRINT_STRIDE: usize = 997;

/// An FNV fold over a sparse sample of the frame, kept to sixteen bits.
pub fn frame_fingerprint(frame: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in frame.iter().step_by(FINGERPRINT_STRIDE) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash >> 16
}

/// The label the Speaker page prints for a sound.
pub const fn sound_label(sound: iot_core::drivers::playback::Sound) -> &'static [u8] {
    use iot_core::drivers::playback::Sound;
    match sound {
        Sound::Tap => b"TAP",
        Sound::Notice => b"NOTICE",
        Sound::Warn => b"WARN",
        Sound::Error => b"ERROR",
        Sound::Rise => b"RISE",
        Sound::Sweep => b"SWEEP",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NETWORK_NAME_GLYPHS, SETUP_NOTICE_TITLE, body, format_address, format_bars,
        format_joining_title, format_network_name, format_pair, format_phase, format_question,
        format_setup_notice_body, format_setup_pass, format_u32, frame_fingerprint, joining_body,
        sound_label, write_ratio, write_u64,
    };
    use alloc::vec;
    use alloc::vec::Vec;
    use iot_core::controls::confirm_verb;
    use iot_core::drivers::network::{
        AccessPoint, Ipv4, LinkPhase, LinkStatus, NetworkError, SSID_BYTES, SetupPass, Ssid,
    };
    use iot_core::drivers::playback::Sound;
    use iot_core::keyboard::{DIALOG_ANSWER_W, DIALOG_GLYPHS};
    use iot_core::overlay::{Block, GLYPH_PITCH, block_value_x, block_x, level_value_end};
    use iot_core::state::Confirm;

    #[test]
    fn a_sound_name_fits_its_block() {
        let value_x = block_value_x(Block::First);
        let next_block = block_x(Block::Second);
        for sound in Sound::CYCLE {
            let label = sound_label(sound);
            let end = level_value_end(value_x, label.len());
            assert!(
                end < next_block,
                "{:?} prints {:?}, which ends at column {end} and reaches into \
                 the next block at {next_block}",
                sound,
                core::str::from_utf8(label).unwrap_or("<非 ASCII>"),
            );
        }
        assert_eq!(
            sound_label(Sound::Notice).len(),
            6,
            "the widest name is the one with a glyph of margin left; if this \
             number moves, the slot is either nearly overflowing or has slack \
             nobody has to design around",
        );
    }

    #[test]
    fn a_phase_and_a_failure_are_both_four_glyphs() {
        let mut buf = [0u8; 4];
        assert_eq!(format_phase(LinkPhase::Off, &mut buf), b"OFF");
        assert_eq!(format_phase(LinkPhase::Scanning, &mut buf), b"SCAN");
        assert_eq!(format_phase(LinkPhase::Connecting, &mut buf), b"JOIN");
        assert_eq!(format_phase(LinkPhase::Connected, &mut buf), b"UP");
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

        let long = ap(b"neighbour-extension-2.4GHz", [1, 2, 3, 4, 5, 6]);
        let drawn = format_question(Confirm::Forget(long), LinkStatus::default(), &mut buf);
        assert_eq!(drawn.len(), DIALOG_GLYPHS);
        assert_eq!(&drawn[..8], b"FORGET \"");
        assert_eq!(&drawn[drawn.len() - 2..], b"\"?");
        assert_eq!(drawn[DIALOG_GLYPHS - 3], b'~', "the fold is marked");

        assert_eq!(
            format_question(Confirm::Forget(named), LinkStatus::default(), &mut buf),
            b"FORGET \"neighbour\"?"
        );
    }

    #[test]
    fn a_leave_names_the_network_the_link_is_on() {
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
        let mut buf = [0u8; SSID_BYTES];
        assert_eq!(SETUP_NOTICE_TITLE, b"SETUP MODE");
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
        let mut buf = [0u8; SSID_BYTES];
        let pass = SetupPass::from_array(*b"k3mq9t7w2x");
        assert_eq!(format_setup_pass(Some(&pass), &mut buf), b"PASS k3mq9t7w2x");
        assert_eq!(format_setup_pass(None, &mut buf), b"OPEN, NO PASSWORD");
        let other = SetupPass::from_array(*b"abcdefghij");
        assert_eq!(
            format_setup_pass(Some(&other), &mut buf),
            b"PASS abcdefghij"
        );
    }

    #[test]
    fn the_join_box_says_which_stage_of_the_attempt_and_which_network() {
        let mut buf = [0u8; DIALOG_GLYPHS];
        let named = ap(b"neighbour", [1, 2, 3, 4, 5, 6]);
        assert_eq!(format_joining_title(None, &mut buf), b"JOINING");
        assert_eq!(joining_body(None), b"WAIT FOR THE NETWORK");
        assert_eq!(
            format_joining_title(Some(named), &mut buf),
            b"JOINED neighbour"
        );
        assert_eq!(joining_body(Some(named)), b"WAIT FOR AN ADDRESS");

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
                joining_body(ap).len() <= DIALOG_GLYPHS,
                "what the radio is waiting on is wider than the box's own line"
            );
        }
    }

    #[test]
    fn the_boxes_words_are_the_verbs_own_and_fit_the_box() {
        let cafe = ap(b"cafe", [1, 2, 3, 4, 5, 6]);
        for confirm in [Confirm::Leave, Confirm::Forget(cafe)] {
            let verb = confirm_verb(confirm);
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
                body(confirm).len() <= DIALOG_GLYPHS,
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
        let mut buf = [0u8; 12];
        assert_eq!(format_u32(0, &mut buf), b"0");
        assert_eq!(format_u32(7, &mut buf), b"7");
        assert_eq!(format_u32(1234, &mut buf), b"1234");
        assert_eq!(format_u32(99999, &mut buf), b"99999");
    }

    #[test]
    fn a_counter_outgrowing_u16_keeps_counting() {
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

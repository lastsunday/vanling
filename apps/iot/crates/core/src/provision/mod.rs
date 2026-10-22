//! Setup mode: the config page a phone is served while this device has no network of its own.
//!
//! A board with no panel has no way to be told which network to join, and a board with one is
//! easier to configure from a phone that already holds the passphrase. Both are answered the same
//! way: an access point with a page behind it, and a name server that answers *everything* with
//! that page's address — which is what turns a phone's "this network has no internet" check into
//! the page opening by itself. Whether the access point asks for a passphrase is the panel's
//! business rather than the protocol's: the value is shown there, and a board without one offers
//! the network open because it has no way to say a value.
//!
//! What lives here is the protocol and none of the radio: no socket, no task, and nothing
//! allocated, so a host test can drive a whole session — a phone asking for the list, picking a
//! row, posting a passphrase — without a radio or a network. The device's own half is `iot-app`'s
//! `provision`.

pub mod dns;
pub mod http;
pub mod route;
pub mod scan;
mod wire;

use crate::drivers::network::{SSID_BYTES, Ssid};

pub use dns::answer;
pub use http::{Body, Cache, Method, ParseError, Request, Response, Status};
pub use route::{Action, MAC_TEXT_BYTES, MAC_TOKEN, SEED_AT, SEED_SLOT, mac_text, page_len, route};
pub use scan::{PIECE_BYTES, Piece, ScanJson, scan_json_len, scan_json_row};

/// Characters in the setup access point's passphrase.
///
/// Ten symbols from the alphabet the generator uses: about 50 bits, which is what a phone's own
/// keypad can be expected to take without a second look, and long enough that a captured handshake
/// is not the afternoon of a GPU's time that six would be.
pub use crate::drivers::network::SETUP_PASS_LEN;

/// The name the setup access point offers, from the board's own address: `vanling-` and the last
/// two octets in lowercase hex.
///
/// The board's address and not the build's: two devices flashed from one image are two networks in
/// one room, and these two octets are what a phone's own scan list shows beside the name.
pub fn ap_name(mac: [u8; 6]) -> Ssid {
    const PREFIX: &[u8] = b"vanling-";
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = [0u8; SSID_BYTES];
    name[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut at = PREFIX.len();
    for octet in [mac[4], mac[5]] {
        name[at] = HEX[usize::from(octet >> 4)];
        name[at + 1] = HEX[usize::from(octet & 0x0f)];
        at += 2;
    }
    Ssid::from_octets(&name[..at])
}

/// The address the setup access point takes, and the one every name resolves to.
///
/// The reference firmware's subnet, and deliberately not a documented captive-portal range: a
/// range nobody has a route for is a page a phone may refuse to open at all, while a phone already
/// on `192.168.4.0/24` reaching the wrong device is real but rare.
pub const ADDRESS: [u8; 4] = [192, 168, 4, 1];

/// The page's address, spelled as a client has to type or be redirected to it.
pub const BASE_URL: &str = "http://192.168.4.1/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_access_point_is_named_for_the_board() {
        // Lowercase, so a name read off a phone screen back to somebody on the bench is
        // unambiguous; four hex digits, because that is all of the address that has to
        // differ and all the room a scan row has beside it.
        assert_eq!(
            ap_name([0xaa, 0x00, 0x00, 0x00, 0x1f, 0xc2]).as_display_text(),
            Some("vanling-1fc2")
        );
    }

    #[test]
    fn two_boards_of_one_build_are_two_names() {
        assert_ne!(
            ap_name([0xaa, 0x00, 0x00, 0x00, 0x00, 0x01]).as_bytes(),
            ap_name([0xaa, 0x00, 0x00, 0x00, 0x00, 0x02]).as_bytes()
        );
    }

    #[test]
    fn leading_zeroes_are_kept() {
        // The one shape where a hand-written hex conversion drops a digit and two boards
        // still look different: `00:07` has to read as `0007`.
        assert_eq!(
            ap_name([0xaa, 0x00, 0x00, 0x00, 0x00, 0x07]).as_display_text(),
            Some("vanling-0007")
        );
    }
}

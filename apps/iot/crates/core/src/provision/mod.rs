//! Setup mode: the config page a phone is served while this device has no network of its
//! own.

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

/// The name the setup access point offers, from the board's own address: `vanling-` and the
/// last two octets in lowercase hex.
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
pub const ADDRESS: [u8; 4] = [192, 168, 4, 1];

/// The page's address, spelled as a client has to type or be redirected to it.
pub const BASE_URL: &str = "http://192.168.4.1/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_access_point_is_named_for_the_board() {
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
        assert_eq!(
            ap_name([0xaa, 0x00, 0x00, 0x00, 0x00, 0x07]).as_display_text(),
            Some("vanling-0007")
        );
    }
}

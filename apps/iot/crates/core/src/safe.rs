//! Safe mode's read-only diagnostic surface.
//!
//! Safe mode exists because the product could not finish a boot. It is deliberately not the
//! setup portal: a phone can read what failed, and nothing on the page can change the device or
//! write credentials. Power-cycling or reflashing is the way out, so the page only reports.

use core::fmt::{self, Write};

use crate::drivers::network::{Ipv4, LinkPhase};
use crate::provision::{Method, Request};

/// The largest status body [`write_status`] can produce. The fields are numbers and one link
/// phase, so this is a bound with room to grow rather than a measured tight fit.
pub const STATUS_BYTES: usize = 640;

/// What the page reports. The caller reads these from its own state; this module never owns a
/// clock, a heap, or a radio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub mac: [u8; 6],
    pub crashed_boots: u16,
    pub saved_networks: usize,
    pub heap_free: usize,
    pub uptime_ms: u32,
    pub phase: LinkPhase,
    pub address: Option<Ipv4>,
    /// Whether the diagnostics are served from the station's address rather than the access
    /// point. It says where to reach the page, not whether the page works.
    pub station: bool,
}

/// What one request asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Status,
    NoContent,
    BadRequest,
}

/// The whole surface: one read-only document, its aliases, and the icon every browser asks
/// for. Everything else is refused rather than redirected, because there is nowhere to go.
pub fn route(request: &Request<'_>) -> Action {
    match (request.method, request.path) {
        (Method::Get, "/" | "/status" | "/health") => Action::Status,
        (Method::Get, "/favicon.ico") => Action::NoContent,
        _ => Action::BadRequest,
    }
}

/// Write the diagnostic body into `out` and return its length. `None` means the caller's
/// buffer was smaller than [`STATUS_BYTES`].
pub fn write_status(out: &mut [u8], facts: &Facts) -> Option<usize> {
    let mut writer = Buffer { out, len: 0 };
    writeln!(
        &mut writer,
        "vanling safe mode\nmac: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n\
         crashed boots: {}\nuptime: {} s\nheap free: {} B\nsaved networks: {}\nlink: {:?}",
        facts.mac[0],
        facts.mac[1],
        facts.mac[2],
        facts.mac[3],
        facts.mac[4],
        facts.mac[5],
        facts.crashed_boots,
        facts.uptime_ms / 1_000,
        facts.heap_free,
        facts.saved_networks,
        facts.phase,
    )
    .ok()?;
    match facts.address {
        Some(address) => {
            let [a, b, c, d] = address.octets();
            writeln!(&mut writer, "address: {a}.{b}.{c}.{d}").ok()?;
        }
        None => writeln!(&mut writer, "address: none").ok()?,
    }
    writeln!(
        &mut writer,
        "mode: {}\nread-only: power-cycle or reflash to leave safe mode",
        if facts.station {
            "station"
        } else {
            "access point"
        }
    )
    .ok()?;
    Some(writer.len)
}

struct Buffer<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl Write for Buffer<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        if end > self.out.len() {
            return Err(fmt::Error);
        }
        self.out[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn facts() -> Facts {
        Facts {
            mac: [0xaa, 0x00, 0x11, 0x22, 0x33, 0x44],
            crashed_boots: 5,
            saved_networks: 2,
            heap_free: 12_345,
            uptime_ms: 61_500,
            phase: LinkPhase::Connected,
            address: Some(Ipv4::from_octets([192, 168, 1, 23])),
            station: true,
        }
    }

    fn page(facts: &Facts) -> String {
        let mut out = [0u8; STATUS_BYTES];
        let len = write_status(&mut out, facts).expect("the documented bound fits");
        String::from_utf8(out[..len].to_vec()).expect("the body is text")
    }

    #[test]
    fn the_page_carries_every_fact_the_boot_left() {
        let text = page(&facts());
        for expected in [
            "vanling safe mode",
            "aa:00:11:22:33:44",
            "crashed boots: 5",
            "uptime: 61 s",
            "heap free: 12345 B",
            "saved networks: 2",
            "link: Connected",
            "address: 192.168.1.23",
            "mode: station",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in {text:?}");
        }
    }

    #[test]
    fn an_unconnected_access_point_says_so() {
        let facts = Facts {
            phase: LinkPhase::Failed(crate::drivers::network::NetworkError::NotFound),
            address: None,
            station: false,
            ..facts()
        };
        let text = page(&facts);
        assert!(text.contains("address: none"), "{text:?}");
        assert!(text.contains("mode: access point"), "{text:?}");
        assert!(text.contains("link: Failed(NotFound)"), "{text:?}");
    }

    #[test]
    fn a_small_buffer_is_refused_instead_of_cut() {
        let mut out = [0u8; 4];
        assert_eq!(write_status(&mut out, &facts()), None);
    }

    #[test]
    fn only_the_read_only_surface_is_served() {
        for path in ["/", "/status", "/health"] {
            let request = Request {
                method: Method::Get,
                path,
                body: &[],
                close: true,
            };
            assert_eq!(route(&request), Action::Status, "{path}");
        }
        let icon = Request {
            method: Method::Get,
            path: "/favicon.ico",
            body: &[],
            close: true,
        };
        assert_eq!(route(&icon), Action::NoContent);
        let write = Request {
            method: Method::Post,
            path: "/",
            body: &[],
            close: true,
        };
        assert_eq!(route(&write), Action::BadRequest);
        let unknown = Request {
            method: Method::Get,
            path: "/submit",
            body: &[],
            close: true,
        };
        assert_eq!(route(&unknown), Action::BadRequest);
    }
}

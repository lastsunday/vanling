//! The network, as the product needs it: a scan list, one link's credentials, and
//! the phase they are in.
//!
//! Two things here are rules rather than plumbing, so they live where a host test
//! can pin them:
//!
//! * **A scan list is ranked, not raw.** Same-name access points are separate rows
//!   (they are separate access points — a phone shows both), so the identity is
//!   the BSSID; strongest first, and whatever does not fit is dropped rather than
//!   drawn below the fold.
//! * **An SSID is bytes.** 802.11 puts arbitrary octets in it, and this panel's
//!   glyph table covers printable ASCII, so whether a name can be *drawn* is a
//!   decision with a fallback, made once here instead of at every call site.
//!
//! No socket, no domain name and no retry policy: this is a link, and what the
//! product does above it is the app's business.

use alloc::vec::Vec;

/// SSID capacity, as 802.11 defines it: 32 octets, not 32 characters.
pub const SSID_BYTES: usize = 32;

/// WPA passphrase capacity: 63 characters plus the terminator the driver keeps.
pub const PASSWORD_BYTES: usize = 64;

/// How many access points a scan keeps. The panel lists this many; the driver is
/// asked for more and drops the rest, so a crowded room shows the strongest
/// rather than an arbitrary prefix of the driver's order.
pub const MAX_LISTED_APS: usize = 8;

/// How many networks the device remembers, strongest-first, newest at the front —
/// the same slot count and ordering the reference firmware uses, so a device that
/// has been to three places reconnects without being told which.
pub const MAX_SAVED_NETWORKS: usize = 10;

/// A network name as the radio reports it: up to [`SSID_BYTES`] octets, which is
/// not necessarily text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ssid {
    octets: [u8; SSID_BYTES],
    len: u8,
}

impl Ssid {
    pub const EMPTY: Self = Self {
        octets: [0; SSID_BYTES],
        len: 0,
    };

    /// Take an SSID from whatever the driver read. Longer than the standard is
    /// truncated rather than refused: a name this long cannot be associated with,
    /// but refusing it here would lose the row entirely, and the operator would
    /// see an empty list instead of a name they can recognise.
    pub fn from_octets(octets: &[u8]) -> Self {
        let len = octets.len().min(SSID_BYTES);
        let mut buf = [0u8; SSID_BYTES];
        buf[..len].copy_from_slice(&octets[..len]);
        Self {
            octets: buf,
            len: len as u8,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.octets[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The name as text, or `None` when it is not something this panel can draw.
    ///
    /// Printable ASCII only, because that is the glyph table. A name in any other
    /// encoding is still a real network and still connectable — it is the drawing
    /// that fails, so the caller falls back to the BSSID.
    pub fn as_display_text(&self) -> Option<&str> {
        core::str::from_utf8(self.as_bytes())
            .ok()
            .filter(|text| text.chars().all(|c| (' '..='~').contains(&c)))
    }
}

/// One access point a scan found.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AccessPoint {
    pub ssid: Ssid,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub rssi: i8,
    /// Whether the beacon asks for a passphrase. An open network is the one case
    /// where the password page can be skipped, so the row has to say so.
    pub secured: bool,
}

impl AccessPoint {
    /// Whether this row carries a name the panel can draw, which decides whether
    /// the list shows the name or the BSSID.
    pub fn name_is_drawable(&self) -> bool {
        self.ssid
            .as_display_text()
            .is_some_and(|text| !text.is_empty())
    }

    /// Signal in the four buckets the list draws as bars.
    ///
    /// The thresholds are the ones the reference firmware's own list uses; a
    /// bucket boundary is a drawing decision, so it is pinned by a test rather
    /// than re-derived per surface.
    pub fn bars(&self) -> u8 {
        bars_for(self.rssi)
    }
}

/// Signal strength in bars, 0–4. Free function so a connected link can be shown
/// with the same scale as a scan row.
pub fn bars_for(rssi: i8) -> u8 {
    match rssi {
        i8::MIN..=-81 => 0,
        -80..=-71 => 1,
        -70..=-61 => 2,
        -60..=-51 => 3,
        _ => 4,
    }
}

/// Rank a driver's scan output into the list the panel shows.
///
/// Deduplicates on BSSID keeping the strongest report of each access point,
/// orders by signal, and truncates. Takes the raw list rather than the driver's
/// type so the rule is testable without a radio.
pub fn rank_aps(raw: &[AccessPoint], limit: usize) -> Vec<AccessPoint> {
    let mut ranked: Vec<AccessPoint> = Vec::new();
    for ap in raw {
        match ranked.iter_mut().find(|seen| seen.bssid == ap.bssid) {
            Some(seen) if ap.rssi > seen.rssi => *seen = *ap,
            Some(_) => {}
            None => ranked.push(*ap),
        }
    }
    ranked.sort_unstable_by_key(|ap| core::cmp::Reverse(ap.rssi));
    ranked.truncate(limit);
    ranked
}

/// The remembered networks, newest at the front.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SavedNetworks {
    entries: [(Ssid, Password); MAX_SAVED_NETWORKS],
    len: u8,
}

impl Default for SavedNetworks {
    fn default() -> Self {
        Self::empty()
    }
}

impl SavedNetworks {
    pub const fn empty() -> Self {
        Self {
            entries: [(Ssid::EMPTY, Password::EMPTY); MAX_SAVED_NETWORKS],
            len: 0,
        }
    }

    /// Remember a network at the front, replacing an entry with the same name.
    ///
    /// Front, because a device is usually being set up for the room it is in; the
    /// full list is what reconnect walks when none of them are in range. Returns
    /// whether it replaced one, so a caller can tell "saved" from "updated" — the
    /// two are the same write and the page words them differently.
    pub fn remember(&mut self, credentials: &Credentials) -> bool {
        let replaced = self.forget_named(&credentials.ssid);
        let last = MAX_SAVED_NETWORKS - 1;
        for index in (1..=last).rev() {
            self.entries[index] = self.entries[index - 1];
        }
        self.entries[0] = (credentials.ssid, credentials.password);
        self.len = (self.len + 1).min(MAX_SAVED_NETWORKS as u8);
        replaced
    }

    /// Drop every entry with this name. Returns whether anything was dropped, so
    /// a caller can tell "the password changed" from "that was never saved".
    pub fn forget_named(&mut self, ssid: &Ssid) -> bool {
        let mut found = false;
        let mut kept = 0usize;
        for index in 0..MAX_SAVED_NETWORKS {
            let entry = self.entries[index];
            if index < self.len as usize && entry.0 == *ssid {
                found = true;
                continue;
            }
            if index < self.len as usize {
                self.entries[kept] = entry;
                kept += 1;
            }
        }
        self.len = kept as u8;
        found
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, index: usize) -> Option<Credentials> {
        let entry = *self.entries.get(index)?;
        (index < self.len as usize).then_some(Credentials {
            ssid: entry.0,
            password: entry.1,
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = Credentials> + '_ {
        (0..self.len as usize).filter_map(|index| self.get(index))
    }
}

/// A passphrase as the radio stores it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Password {
    octets: [u8; PASSWORD_BYTES],
    len: u8,
}

impl Default for Password {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Password {
    pub const EMPTY: Self = Self {
        octets: [0; PASSWORD_BYTES],
        len: 0,
    };

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let len = bytes.len().min(PASSWORD_BYTES);
        let mut buf = [0u8; PASSWORD_BYTES];
        buf[..len].copy_from_slice(&bytes[..len]);
        Self {
            octets: buf,
            len: len as u8,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.octets[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// One network's credentials.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Credentials {
    pub ssid: Ssid,
    pub password: Password,
}

impl Credentials {
    /// Build from text, refusing what the radio would refuse.
    ///
    /// An over-long name or passphrase is rejected rather than truncated, because
    /// the driver rejects it too: a silently shortened passphrase authenticates
    /// against nothing and reads as a wrong password.
    pub fn from_text(ssid: &str, password: &str) -> Result<Self, NetworkError> {
        if ssid.is_empty() || ssid.len() > SSID_BYTES || password.len() >= PASSWORD_BYTES {
            return Err(NetworkError::InvalidArguments);
        }
        Ok(Self {
            ssid: Ssid::from_octets(ssid.as_bytes()),
            password: Password::from_bytes(password.as_bytes()),
        })
    }
}

/// An IPv4 address, kept as octets so nothing here needs a formatting crate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ipv4 {
    octets: [u8; 4],
}

impl Ipv4 {
    pub const fn from_octets(octets: [u8; 4]) -> Self {
        Self { octets }
    }

    pub const fn octets(&self) -> [u8; 4] {
        self.octets
    }
}

/// Where the link is. `Connected` means it has an address: association on its own
/// is not a link the product can do anything with, and a state that called it
/// connected would be claiming an outcome it has not reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LinkPhase {
    #[default]
    Off,
    Scanning,
    Connecting,
    Connected,
    Failed(NetworkError),
}

/// What went wrong, coarsely: the driver logs its own error code with the call
/// that produced it, and this is the kind the state layer and the panel act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetworkError {
    /// The driver is already scanning or associating.
    Busy,
    /// The name or passphrase is not a shape the radio accepts.
    InvalidArguments,
    /// The driver's heap is exhausted — the one error that means "this build's
    /// memory budget is wrong", so it is named rather than folded into "other".
    NoMemory,
    /// The radio refused for a reason of its own.
    Driver,
}

/// The link as the rest of the device reads it: one snapshot, copied rather than
/// shared so the panel can hold it without borrowing the driver.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkStatus {
    pub phase: LinkPhase,
    /// The access point this station is on, once there is one.
    pub ap: Option<AccessPoint>,
    /// The address DHCP handed out, which is what makes the phase `Connected`.
    pub address: Option<Ipv4>,
}

impl LinkStatus {
    /// This status with the stack's current address folded in — the definition in
    /// [`LinkPhase`] as a value: an address makes it `Connected`, losing the address goes
    /// back to `Connecting`, and a failure is left alone.
    pub fn with_address(mut self, address: Option<Ipv4>) -> Self {
        self.phase = match (self.phase, address) {
            (LinkPhase::Failed(kind), _) => LinkPhase::Failed(kind),
            (_, Some(_)) => LinkPhase::Connected,
            (LinkPhase::Connected, None) => LinkPhase::Connecting,
            (phase, None) => phase,
        };
        self.address = address;
        self
    }
}

/// A Wi-Fi station a board wired.
pub trait Station {
    /// The frame-level device the IP stack polls, once the radio is up.
    ///
    /// `Send` because the stack is polled on another executor than the one that
    /// starts the radio, and nothing else about it is constrained here: the stack's
    /// own driver trait belongs to the stack, and this crate does not depend on it.
    type Device: Send;

    /// Scan, keeping at most [`MAX_LISTED_APS`] ranked access points.
    ///
    /// Takes no arguments and no timeout: a scan is one of the radio's jobs, and
    /// the panel has already said what it wants.
    fn scan(&mut self) -> impl Future<Output = Result<Vec<AccessPoint>, NetworkError>>;

    /// Associate and authenticate. Returns once the radio has made the decision,
    /// which is not the same as having an address — see [`LinkPhase`].
    fn connect(
        &mut self,
        credentials: &Credentials,
    ) -> impl Future<Output = Result<(), NetworkError>>;

    /// Leave the access point.
    fn disconnect(&mut self) -> impl Future<Output = Result<(), NetworkError>>;

    /// The link as it stands, without touching the radio.
    fn status(&self) -> LinkStatus;

    /// Hand over what the IP stack currently holds, which is the only way the phase can
    /// follow DHCP: the stack owns the address and the station never sees it.
    ///
    /// `None` puts the phase back to `Connecting` — see [`LinkStatus::with_address`],
    /// which holds the rule so it is one implementation rather than one per station.
    fn note_address(&mut self, address: Option<Ipv4>);

    /// Hand the frame-level device over. `None` once taken.
    fn take_device(&mut self) -> Option<Self::Device>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn ap(ssid: &str, bssid: [u8; 6], rssi: i8) -> AccessPoint {
        AccessPoint {
            ssid: Ssid::from_octets(ssid.as_bytes()),
            bssid,
            channel: 6,
            rssi,
            secured: true,
        }
    }

    #[test]
    fn ranking_keeps_one_row_per_access_point_strongest_first() {
        // Two radios broadcasting the same name are two networks, and the weaker
        // report of each is not a second row.
        let raw = vec![
            ap("living", [0xaa; 6], -70),
            ap("living", [0xaa; 6], -55),
            ap("kitchen", [0xbb; 6], -60),
            ap("attic", [0xcc; 6], -80),
        ];
        let ranked = rank_aps(&raw, MAX_LISTED_APS);
        assert_eq!(ranked.len(), 3, "one row per BSSID");
        assert_eq!(ranked[0].bssid, [0xaa; 6]);
        assert_eq!(ranked[0].rssi, -55, "the stronger report wins");
        assert_eq!(ranked[1].bssid, [0xbb; 6]);
        assert_eq!(ranked[2].bssid, [0xcc; 6]);
    }

    #[test]
    fn ranking_drops_the_weakest_when_the_list_is_full() {
        let raw = (0..12u8)
            .map(|index| ap(&format!("net{index}"), [index; 6], -40 - index as i8))
            .collect::<Vec<_>>();
        let ranked = rank_aps(&raw, MAX_LISTED_APS);
        assert_eq!(ranked.len(), MAX_LISTED_APS);
        assert_eq!(ranked[0].rssi, -40, "strongest kept");
        assert_eq!(
            ranked[MAX_LISTED_APS - 1].rssi,
            -47,
            "and the cut is at the far end of the order, not in the middle"
        );
    }

    #[test]
    fn a_name_the_panel_cannot_draw_is_flagged_rather_than_guessed_at() {
        let ascii = ap("TP-LINK", [1; 6], -50);
        assert!(ascii.name_is_drawable());

        let chinese = AccessPoint {
            ssid: Ssid::from_octets(&[0xe7, 0xa7, 0x9b, 0xe5, 0x8d, 0x8a]),
            ..ap("", [2; 6], -50)
        };
        assert!(!chinese.name_is_drawable());
        assert_eq!(chinese.ssid.as_display_text(), None);
        assert_eq!(
            chinese.ssid.as_bytes(),
            &[0xe7, 0xa7, 0x9b, 0xe5, 0x8d, 0x8a],
            "the name is still carried byte for byte, because connecting needs it"
        );
    }

    #[test]
    fn a_name_that_is_not_text_is_kept_as_bytes() {
        let latin1 = Ssid::from_octets(b"Caf\xe9");
        assert_eq!(latin1.as_bytes(), b"Caf\xe9");
        assert_eq!(latin1.as_display_text(), None);
    }

    #[test]
    fn signal_buckets_have_the_four_tiers_the_list_draws() {
        assert_eq!(bars_for(-45), 4);
        assert_eq!(bars_for(-55), 3);
        assert_eq!(bars_for(-65), 2);
        assert_eq!(bars_for(-75), 1);
        assert_eq!(bars_for(-85), 0);
        assert_eq!(bars_for(i8::MIN), 0);
    }

    #[test]
    fn credentials_refuse_what_the_radio_would_refuse() {
        assert!(Credentials::from_text("net", "hunter2").is_ok());
        assert_eq!(
            Credentials::from_text("", "hunter2"),
            Err(NetworkError::InvalidArguments),
            "an empty name is not a network"
        );
        assert_eq!(
            Credentials::from_text("net", &"x".repeat(PASSWORD_BYTES)),
            Err(NetworkError::InvalidArguments),
            "63 characters is the longest passphrase; 64 is rejected, not truncated"
        );
    }

    #[test]
    fn remembering_puts_the_newest_first_and_replaces_a_repeat() {
        let mut saved = SavedNetworks::empty();
        let home = Credentials::from_text("home", "one").expect("valid");
        let away = Credentials::from_text("away", "two").expect("valid");
        assert!(!saved.remember(&home), "nothing to replace yet");
        assert!(!saved.remember(&away));
        assert_eq!(saved.get(0).expect("newest").ssid, away.ssid);

        let home_again = Credentials::from_text("home", "changed").expect("valid");
        assert!(
            saved.remember(&home_again),
            "the same name was already saved"
        );
        assert_eq!(saved.len(), 2, "a repeat replaces rather than appends");
        assert_eq!(saved.get(0).expect("newest").ssid, home_again.ssid);
        assert_eq!(
            saved.get(0).expect("newest").password.as_bytes(),
            b"changed",
            "and it replaced the passphrase, not the row"
        );
        assert_eq!(saved.get(1).expect("older").ssid, away.ssid);
    }

    #[test]
    fn the_list_is_bounded_and_drops_the_oldest() {
        let mut saved = SavedNetworks::empty();
        for index in 0..MAX_SAVED_NETWORKS + 3 {
            saved.remember(&Credentials::from_text(&format!("net{index}"), "p").expect("valid"));
        }
        assert_eq!(saved.len(), MAX_SAVED_NETWORKS);
        assert_eq!(saved.get(0).expect("newest").ssid.as_bytes(), b"net12");
        assert_eq!(
            saved
                .get(MAX_SAVED_NETWORKS - 1)
                .expect("oldest kept")
                .ssid
                .as_bytes(),
            b"net3"
        );
        assert_eq!(saved.get(MAX_SAVED_NETWORKS), None);
    }

    /// An address on a station that never associated still reads `Connected`: that is the
    /// state the product acts on, and saying anything else would be the claim this type
    /// exists to avoid.
    #[test]
    fn an_address_is_what_makes_the_link_connected() {
        let address = Ipv4::from_octets([192, 168, 1, 136]);
        let connecting = LinkStatus {
            phase: LinkPhase::Connecting,
            ap: None,
            address: None,
        };

        let connected = connecting.with_address(Some(address));
        assert_eq!(connected.phase, LinkPhase::Connected);
        assert_eq!(connected.address, Some(address));

        let lost = connected.with_address(None);
        assert_eq!(
            lost.phase,
            LinkPhase::Connecting,
            "losing the address is back to associated-and-waiting, not off"
        );
        assert_eq!(lost.address, None);
    }

    #[test]
    fn a_failure_is_not_resurrected_by_an_address() {
        let failed = LinkStatus {
            phase: LinkPhase::Failed(NetworkError::InvalidArguments),
            ap: None,
            address: None,
        }
        .with_address(Some(Ipv4::from_octets([10, 0, 0, 2])));

        assert_eq!(
            failed.phase,
            LinkPhase::Failed(NetworkError::InvalidArguments)
        );
    }

    #[test]
    fn an_address_report_leaves_the_other_phases_alone() {
        for phase in [LinkPhase::Off, LinkPhase::Scanning, LinkPhase::Connecting] {
            let status = LinkStatus {
                phase,
                ap: None,
                address: None,
            };
            assert_eq!(
                status.with_address(None).phase,
                phase,
                "{phase:?} is not about the address, so losing it changes nothing"
            );
        }
    }
}

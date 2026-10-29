//! The network, as the product needs it: a scan list, one link's credentials, and the phase
//! they are in.

use alloc::vec::Vec;

/// SSID capacity, as 802.11 defines it: 32 octets, not 32 characters.
pub const SSID_BYTES: usize = 32;

/// WPA passphrase capacity: 63 characters plus the terminator the driver keeps.
pub const PASSWORD_BYTES: usize = 64;

/// How many access points a scan keeps.
pub const MAX_LISTED_APS: usize = 8;

/// How long the loop that surveys an unconfigured device rests between rounds.
pub const SURVEY_PAUSE_MS: u64 = 5_000;

/// How long it rests after a round that found nothing, while an empty round still reads as
/// a hiccup rather than as the truth.
pub const SURVEY_EMPTY_PAUSE_MS: u64 = 1_000;

/// How many empty rounds in a row are still read as that hiccup.
pub const SURVEY_EMPTY_RETRIES: u8 = 3;

/// How long to rest once the room has been read a few times and nobody has acted on it.
pub const SURVEY_IDLE_PAUSE_MS: u64 = 15_000;
pub const SURVEY_IDLE_MAX_PAUSE_MS: u64 = 30_000;

/// How many idle rounds each step of that backoff lasts.
pub const SURVEY_IDLE_ROUNDS: u8 = 3;

/// How long to rest after a survey round, given empty rounds and idle rounds in a row.
pub fn survey_pause_after(front: Front, empty_rounds: u8, idle_rounds: u8) -> Option<u64> {
    if front == Front::AccessPoint {
        return None;
    }
    if matches!(empty_rounds, 1..=SURVEY_EMPTY_RETRIES) {
        return Some(SURVEY_EMPTY_PAUSE_MS);
    }
    Some(match idle_rounds / SURVEY_IDLE_ROUNDS {
        0 => SURVEY_PAUSE_MS,
        1 => SURVEY_IDLE_PAUSE_MS,
        _ => SURVEY_IDLE_MAX_PAUSE_MS,
    })
}

/// Whether the access point this setup raises starts under a passphrase.
pub const fn setup_ap_starts_secured(panel: bool, from_splash: bool) -> bool {
    panel && !from_splash
}

/// How a link stopped being held, as far as [`front_after_link`] is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkEnding {
    Named,
    Refused,
    Left,
}

/// Which front the next stretch of setup mode opens on.
pub const fn front_after_link(start: Front, current: Front, ending: LinkEnding) -> Front {
    match ending {
        LinkEnding::Named | LinkEnding::Left => start,
        LinkEnding::Refused => current,
    }
}

/// How many networks the device remembers, strongest-first, newest at the front — the same
/// slot count and ordering the reference firmware uses, so a device that has been to three
/// places reconnects without being told which.
pub const MAX_SAVED_NETWORKS: usize = 10;

/// Characters in the setup access point's passphrase.
///
/// Ten symbols from the alphabet the station generates it from: about 50 bits, which is what a
/// phone's own keypad can be expected to take without a second look, and long enough that a
/// captured handshake is not the afternoon of a GPU's time that six would be.
pub const SETUP_PASS_LEN: usize = 10;

/// The setup access point's passphrase, as the panel may show it.
///
/// Fixed length rather than an [`Ssid`]: the station generates it at [`SETUP_PASS_LEN`], and this is
/// copied with the link status on every window of every loop, so a 32-byte name field for a
/// ten-byte value is 22 bytes of the main task's own frame paid wherever the link is handled.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SetupPass {
    octets: [u8; SETUP_PASS_LEN],
}

impl SetupPass {
    /// The generated value, of exactly the length the station makes.
    pub const fn from_array(octets: [u8; SETUP_PASS_LEN]) -> Self {
        Self { octets }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.octets
    }
}

/// A network name as the radio reports it: up to [`SSID_BYTES`] octets, which is not
/// necessarily text.
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

    /// Take an SSID from whatever the driver read.
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
    /// Whether the beacon asks for a passphrase.
    pub secured: bool,
}

impl AccessPoint {
    /// The slot an impossible report leaves behind, so the list can be an array rather than
    /// a `Vec` — a `Vec` in the device state would make every snapshot copy an allocation
    /// and every page hold a borrow of the driver's.
    pub const EMPTY: Self = Self {
        ssid: Ssid::EMPTY,
        bssid: [0; 6],
        channel: 0,
        rssi: 0,
        secured: false,
    };

    /// Whether this row carries a name the panel can draw, which decides whether the list
    /// shows the name or the BSSID.
    pub fn name_is_drawable(&self) -> bool {
        self.ssid
            .as_display_text()
            .is_some_and(|text| !text.is_empty())
    }

    /// Whether this row and `other` are the same access point.
    pub fn is_same_network(&self, other: &Self) -> bool {
        self.bssid == other.bssid
    }

    /// Signal in the four buckets the list draws as bars.
    pub fn bars(&self) -> u8 {
        bars_for(self.rssi)
    }
}

/// Signal strength in bars, 0–4.
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

/// The ranked list as the device state holds it: a fixed-capacity value, never the driver's
/// `Vec`, because the state is snapshotted into every page and diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApList {
    entries: [AccessPoint; MAX_LISTED_APS],
    len: u8,
    saved: u8,
}

const _: () = assert!(
    MAX_LISTED_APS <= u8::BITS as usize,
    "the list is wider than the one mark per row a byte can carry"
);

impl Default for ApList {
    fn default() -> Self {
        Self::empty()
    }
}

impl ApList {
    pub const fn empty() -> Self {
        Self {
            entries: [AccessPoint::EMPTY; MAX_LISTED_APS],
            len: 0,
            saved: 0,
        }
    }

    /// Whether the row at `index` is one this device could join again on its own.
    pub fn is_saved(&self, index: usize) -> bool {
        index < self.len() && self.saved & (1 << index) != 0
    }

    /// Take the marks a store put on this list's rows.
    pub fn mark_saved(&mut self, saved: u8) {
        self.saved = saved;
    }

    /// Store a ranked list, keeping at most [`MAX_LISTED_APS`] rows.
    pub fn from_ranked(ranked: &[AccessPoint]) -> Self {
        let mut list = Self::empty();
        let len = ranked.len().min(MAX_LISTED_APS);
        list.entries[..len].copy_from_slice(&ranked[..len]);
        list.len = len as u8;
        list
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, index: usize) -> Option<AccessPoint> {
        (index < self.len as usize).then(|| self.entries[index])
    }

    pub fn iter(&self) -> impl Iterator<Item = AccessPoint> + '_ {
        (0..self.len as usize).map(|index| self.entries[index])
    }
}

/// How much stronger a row has to read than the one above it before the list lets it past.
pub const LIST_HYSTERESIS_DB: i8 = 4;

/// How many rounds in a row a row may go unseen before the list drops it.
pub const LIST_MISSED_ROUNDS: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Listed {
    ap: AccessPoint,
    missed: u8,
}

impl Listed {
    const EMPTY: Self = Self {
        ap: AccessPoint::EMPTY,
        missed: 0,
    };
}

/// The list the page shows, carried from one survey round to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurveyHistory {
    entries: [Listed; MAX_LISTED_APS],
    len: u8,
    fresh: bool,
    published: ApList,
}

impl Default for SurveyHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyHistory {
    pub const fn new() -> Self {
        Self {
            entries: [Listed::EMPTY; MAX_LISTED_APS],
            len: 0,
            fresh: true,
            published: ApList::empty(),
        }
    }

    /// Forget every row, so the next round reads the room from scratch.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Fold one round in and answer the list the page should now show.
    pub fn observe(
        &mut self,
        round: &[AccessPoint],
        saved: Option<&SavedNetworks>,
    ) -> Option<ApList> {
        let known = self.len as usize;
        for row in &mut self.entries[..known] {
            row.missed = row.missed.saturating_add(1);
        }
        for ap in round {
            match self.entries[..self.len as usize]
                .iter_mut()
                .find(|row| row.ap.bssid == ap.bssid)
            {
                Some(row) => {
                    row.ap = *ap;
                    row.missed = 0;
                }
                None if usize::from(self.len) < MAX_LISTED_APS => {
                    self.entries[usize::from(self.len)] = Listed { ap: *ap, missed: 0 };
                    self.len += 1;
                }
                None => {
                    let weakest = self.weakest();
                    if beats(ap.rssi, self.entries[weakest].ap.rssi) {
                        self.entries[weakest] = Listed { ap: *ap, missed: 0 };
                    }
                }
            }
        }
        let mut kept = 0usize;
        for index in 0..usize::from(self.len) {
            let row = self.entries[index];
            if row.missed <= LIST_MISSED_ROUNDS {
                self.entries[kept] = row;
                kept += 1;
            }
        }
        self.len = kept as u8;
        self.settle();

        let mut list = ApList::empty();
        list.len = self.len;
        for (slot, row) in list.entries[..kept].iter_mut().zip(&self.entries[..kept]) {
            *slot = row.ap;
        }
        list.mark_saved(saved.map_or(0, |saved| saved.mask(&list)));
        if !self.fresh && drawn_the_same(&list, &self.published) {
            return None;
        }
        self.fresh = false;
        self.published = list;
        Some(list)
    }

    /// The list the page is showing, marked again against the store as it now is.
    pub fn marked(&mut self, saved: Option<&SavedNetworks>) -> Option<ApList> {
        let mut list = self.published;
        list.mark_saved(saved.map_or(0, |saved| saved.mask(&list)));
        if list.saved == self.published.saved {
            return None;
        }
        self.published = list;
        Some(list)
    }

    fn settle(&mut self) {
        for index in 1..usize::from(self.len) {
            let mut at = index;
            while at > 0 && beats(self.entries[at].ap.rssi, self.entries[at - 1].ap.rssi) {
                self.entries.swap(at, at - 1);
                at -= 1;
            }
        }
    }

    fn weakest(&self) -> usize {
        let mut at = 0;
        for index in 1..usize::from(self.len) {
            let row = self.entries[index];
            let held = self.entries[at];
            let row_is_worse = row.missed > held.missed
                || (row.missed == held.missed && row.ap.rssi < held.ap.rssi);
            if row_is_worse {
                at = index;
            }
        }
        at
    }
}

fn beats(reading: i8, above: i8) -> bool {
    i16::from(reading) - i16::from(above) >= i16::from(LIST_HYSTERESIS_DB)
}

fn drawn_the_same(left: &ApList, right: &ApList) -> bool {
    left.len() == right.len()
        && left.saved == right.saved
        && left.iter().zip(right.iter()).all(|(left, right)| {
            left.bssid == right.bssid
                && left.ssid == right.ssid
                && left.secured == right.secured
                && left.bars() == right.bars()
        })
}

/// Where a saved network was last found: the access point's address and channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkHint {
    pub bssid: [u8; 6],
    pub channel: u8,
}

impl LinkHint {
    /// No shortcut: scan the whole band, which is what a first association does.
    pub const NONE: Self = Self {
        bssid: [0; 6],
        channel: 0,
    };

    pub const fn new(bssid: [u8; 6], channel: u8) -> Self {
        Self { bssid, channel }
    }

    /// Whether this names a channel a radio could be on.
    pub const fn is_known(self) -> bool {
        self.channel != 0
    }
}

/// The remembered networks, newest at the front.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SavedNetworks {
    entries: [(Ssid, Password, LinkHint); MAX_SAVED_NETWORKS],
    len: u8,
}

impl Default for SavedNetworks {
    fn default() -> Self {
        Self::empty()
    }
}

impl SavedNetworks {
    /// A store that holds nothing, for the callers that have no store at all: what they
    /// report to the page is the empty set of marks, which is the truth on a board with no
    /// partition.
    pub const EMPTY: Self = Self {
        entries: [(Ssid::EMPTY, Password::EMPTY, LinkHint::NONE); MAX_SAVED_NETWORKS],
        len: 0,
    };

    pub const fn empty() -> Self {
        Self::EMPTY
    }

    /// Remember a network at the front, replacing one with the same name.
    pub fn remember(&mut self, credentials: &Credentials) -> bool {
        let hint = self.hint_of(&credentials.ssid);
        let replaced = self.forget_named(&credentials.ssid);
        let last = MAX_SAVED_NETWORKS - 1;
        for index in (1..=last).rev() {
            self.entries[index] = self.entries[index - 1];
        }
        self.entries[0] = (credentials.ssid, credentials.password, hint);
        self.len = (self.len + 1).min(MAX_SAVED_NETWORKS as u8);
        replaced
    }

    pub(crate) fn place(&mut self, index: usize, ssid: Ssid, password: Password, hint: LinkHint) {
        if index < MAX_SAVED_NETWORKS {
            self.entries[index] = (ssid, password, hint);
            self.len = self.len.max(index as u8 + 1);
        }
    }

    /// The hint this device keeps for `ssid`, or [`LinkHint::NONE`] for a name it does not
    /// hold.
    pub fn hint_of(&self, ssid: &Ssid) -> LinkHint {
        (0..self.len as usize)
            .find(|index| self.entries[*index].0 == *ssid)
            .map_or(LinkHint::NONE, |index| self.entries[index].2)
    }

    /// Record a hint for a name this device holds.
    pub fn set_hint(&mut self, ssid: &Ssid, hint: LinkHint) -> bool {
        for index in 0..self.len as usize {
            if self.entries[index].0 == *ssid {
                if self.entries[index].2 == hint {
                    return false;
                }
                self.entries[index].2 = hint;
                return true;
            }
        }
        false
    }

    /// Drop every entry with this name.
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

    /// The hint beside entry `index`, or [`LinkHint::NONE`] past the end.
    pub fn hint(&self, index: usize) -> LinkHint {
        if index < self.len as usize {
            self.entries[index].2
        } else {
            LinkHint::NONE
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Credentials> + '_ {
        (0..self.len as usize).filter_map(|index| self.get(index))
    }

    /// Whether this device could join `ssid` again on its own.
    pub fn holds(&self, ssid: &Ssid) -> bool {
        self.iter().any(|credentials| credentials.ssid == *ssid)
    }

    /// The copy this device keeps of `ssid`, for a panel that named a network without a
    /// passphrase.
    pub fn copy_of(&self, ssid: &Ssid) -> Option<Credentials> {
        self.iter().find(|credentials| credentials.ssid == *ssid)
    }

    /// Which of a list's rows this device holds a passphrase for: a bit per row, in the
    /// list's own order.
    pub fn mask(&self, list: &ApList) -> u8 {
        let mut mask = 0u8;
        for index in 0..list.len() {
            let Some(ap) = list.get(index) else {
                continue;
            };
            if self.holds(&ap.ssid) {
                mask |= 1 << index;
            }
        }
        mask
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinRequest {
    Typed(Credentials),
    Held(Ssid),
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

/// Where the link is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LinkPhase {
    #[default]
    Off,
    Scanning,
    Connecting,
    Connected,
    Failed(NetworkError),
}

/// What went wrong, coarsely: the driver logs its own error code with the call that
/// produced it, and this is the kind the state layer and the panel act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetworkError {
    Busy,
    InvalidArguments,
    NoMemory,
    NotFound,
    Credentials,
    Driver,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Front {
    #[default]
    Panel,
    AccessPoint,
}

/// The link as the rest of the device reads it: one snapshot, copied rather than shared so
/// the panel can hold it without borrowing the driver.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkStatus {
    pub phase: LinkPhase,
    /// The access point this station is on, once there is one.
    pub ap: Option<AccessPoint>,
    /// The address DHCP handed out, which is what makes the phase `Connected`.
    pub address: Option<Ipv4>,
    /// Whether setup mode is what the page is about: the access point is up when
    /// [`Self::front`] is [`Front::AccessPoint`].
    pub setup: bool,
    /// Whether the page is the offline list: nothing is saved, no setup mode is running,
    /// and `SCAN | AP` is still offered.
    pub browsing: bool,
    /// Whether setup mode has somewhere to go back to.
    pub setup_cancellable: bool,
    /// Whether this device keeps at least one network.
    pub saved: bool,
    /// Which front is offering setup, and with it whether the access point is up.
    pub front: Front,
    /// The passphrase the setup access point is up with, when it has one.
    pub setup_password: Option<SetupPass>,
}

impl LinkStatus {
    /// Whether there is a link to take down: an attempt in flight or one that worked.
    pub fn is_leavable(&self) -> bool {
        matches!(self.phase, LinkPhase::Connecting | LinkPhase::Connected)
    }

    /// Whether the phone's setup box is the page's screen: the access point is up and a
    /// phone is the front it is up for.
    pub fn shows_setup_box(&self) -> bool {
        self.setup && self.front == Front::AccessPoint
    }

    /// This status as one setup front reports it.
    pub fn on_setup_face(self, front: Front, cancellable: bool) -> Self {
        Self {
            setup: true,
            setup_cancellable: cancellable,
            front,
            ..self
        }
    }

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
    type Device: Send;

    fn scan(&mut self) -> impl Future<Output = Result<Vec<AccessPoint>, NetworkError>>;

    fn begin_connect(&mut self, credentials: &Credentials) -> Result<(), NetworkError>;

    fn begin_connect_hinted(
        &mut self,
        credentials: &Credentials,
        hint: LinkHint,
    ) -> Result<(), NetworkError> {
        let _ = hint;
        self.begin_connect(credentials)
    }

    fn await_connected(&mut self) -> impl Future<Output = Result<(), NetworkError>>;

    fn disconnect(&mut self) -> impl Future<Output = Result<(), NetworkError>>;

    fn status(&self) -> LinkStatus;

    fn note_address(&mut self, address: Option<Ipv4>);

    fn take_device(&mut self) -> Option<Self::Device>;

    fn begin_setup(&mut self, ssid: &Ssid, secured: bool) -> Result<Self::Device, NetworkError>;

    fn end_setup(&mut self) -> Result<(), NetworkError>;
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
    fn the_list_the_state_holds_keeps_the_ranking_and_the_cap() {
        let raw = (0..MAX_LISTED_APS + 4)
            .map(|index| ap(&format!("net{index}"), [index as u8; 6], -40 - index as i8))
            .collect::<Vec<_>>();
        let ranked = rank_aps(&raw, MAX_LISTED_APS);
        let list = ApList::from_ranked(&ranked);

        assert_eq!(list.len(), MAX_LISTED_APS);
        assert_eq!(
            list.get(0).expect("strongest").rssi,
            -40,
            "the stored list is the ranked one, not the raw scan"
        );
        assert_eq!(
            list.get(MAX_LISTED_APS - 1).expect("weakest kept").rssi,
            -47
        );
        assert_eq!(list.get(MAX_LISTED_APS), None, "and it is bounded");
        assert_eq!(list.iter().count(), MAX_LISTED_APS);
    }

    #[test]
    fn an_empty_list_is_a_list() {
        let list = ApList::empty();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.get(0), None);
        assert_eq!(ApList::from_ranked(&[]), list);
        assert_eq!(ApList::default(), list);
    }

    #[test]
    fn storing_a_shorter_list_drops_the_tail() {
        let ranked = vec![ap("near", [1; 6], -40), ap("far", [2; 6], -70)];
        let full = ApList::from_ranked(&ranked);
        assert_eq!(full.len(), 2);

        let again = ApList::from_ranked(&ranked[..1]);
        assert_eq!(again.len(), 1);
        assert_eq!(again.get(1), None);
        assert_ne!(again, full);
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

    #[test]
    fn an_address_is_what_makes_the_link_connected() {
        let address = Ipv4::from_octets([192, 168, 1, 136]);
        let connecting = LinkStatus {
            phase: LinkPhase::Connecting,
            ap: None,
            address: None,
            ..LinkStatus::default()
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
            ..LinkStatus::default()
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
                ..LinkStatus::default()
            };
            assert_eq!(
                status.with_address(None).phase,
                phase,
                "{phase:?} is not about the address, so losing it changes nothing"
            );
        }
    }

    #[test]
    fn an_access_point_starts_secured_only_from_a_running_panel() {
        for (panel, from_splash, secured) in [
            (true, false, true),
            (true, true, false),
            (false, false, false),
            (false, true, false),
        ] {
            assert_eq!(
                setup_ap_starts_secured(panel, from_splash),
                secured,
                "panel {panel}, splash {from_splash}"
            );
        }
    }

    #[test]
    fn a_leave_or_a_pick_opens_setup_where_the_build_put_it() {
        for start in [Front::Panel, Front::AccessPoint] {
            for current in [Front::Panel, Front::AccessPoint] {
                for ending in [LinkEnding::Named, LinkEnding::Left] {
                    assert_eq!(
                        front_after_link(start, current, ending),
                        start,
                        "{ending:?} from {current:?} with the build on {start:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_refused_credential_keeps_the_front_it_was_typed_on() {
        for start in [Front::Panel, Front::AccessPoint] {
            for current in [Front::Panel, Front::AccessPoint] {
                assert_eq!(
                    front_after_link(start, current, LinkEnding::Refused),
                    current,
                    "refused with the build on {start:?}"
                );
            }
        }
    }

    #[test]
    fn a_survey_that_found_something_rests_the_whole_pause() {
        assert_eq!(
            survey_pause_after(Front::Panel, 0, 0),
            Some(SURVEY_PAUSE_MS)
        );
    }

    #[test]
    fn the_access_points_front_never_rests_into_another_round() {
        for (empty, idle) in [(0, 0), (1, 0), (u8::MAX, 0), (0, u8::MAX)] {
            assert_eq!(
                survey_pause_after(Front::AccessPoint, empty, idle),
                None,
                "{empty} empty, {idle} idle rounds"
            );
        }
    }

    #[test]
    fn a_publish_from_setup_reports_its_own_face_not_the_stations_default() {
        let station = LinkStatus {
            phase: LinkPhase::Connected,
            ap: Some(ap("neighbour", [1, 2, 3, 4, 5, 6], -50)),
            address: Some(Ipv4::from_octets([192, 168, 4, 2])),
            setup_password: Some(SetupPass::from_array(*b"k3mq9t7w2x")),
            ..LinkStatus::default()
        };
        for (front, cancellable) in [
            (Front::Panel, true),
            (Front::Panel, false),
            (Front::AccessPoint, true),
            (Front::AccessPoint, false),
        ] {
            let published = station.on_setup_face(front, cancellable);

            assert_eq!(published.front, front);
            assert_eq!(published.setup_cancellable, cancellable);
            assert!(published.setup, "a publish from setup mode says so");
            assert_eq!(published.phase, station.phase);
            assert_eq!(published.ap, station.ap);
            assert_eq!(published.address, station.address);
            assert_eq!(published.setup_password, station.setup_password);
        }
    }

    #[test]
    fn an_empty_place_stops_being_retried_every_second() {
        for rounds in 1..=SURVEY_EMPTY_RETRIES {
            assert_eq!(
                survey_pause_after(Front::Panel, rounds, 0),
                Some(SURVEY_EMPTY_PAUSE_MS),
                "{rounds} empty rounds"
            );
        }
        assert_eq!(
            survey_pause_after(Front::Panel, SURVEY_EMPTY_RETRIES + 1, 0),
            Some(SURVEY_PAUSE_MS)
        );
        assert_eq!(
            survey_pause_after(Front::Panel, u8::MAX, 0),
            Some(SURVEY_PAUSE_MS),
            "a count that long is still an empty place"
        );
    }

    #[test]
    fn a_room_nobody_acts_on_is_read_less_and_less_often() {
        let steps = [
            (0, SURVEY_PAUSE_MS),
            (SURVEY_IDLE_ROUNDS - 1, SURVEY_PAUSE_MS),
            (SURVEY_IDLE_ROUNDS, SURVEY_IDLE_PAUSE_MS),
            (2 * SURVEY_IDLE_ROUNDS - 1, SURVEY_IDLE_PAUSE_MS),
            (2 * SURVEY_IDLE_ROUNDS, SURVEY_IDLE_MAX_PAUSE_MS),
            (u8::MAX, SURVEY_IDLE_MAX_PAUSE_MS),
        ];
        for (idle, pause) in steps {
            assert_eq!(
                survey_pause_after(Front::Panel, 0, idle),
                Some(pause),
                "{idle} idle rounds"
            );
        }
        assert_eq!(
            survey_pause_after(
                Front::Panel,
                SURVEY_EMPTY_RETRIES + 1,
                2 * SURVEY_IDLE_ROUNDS
            ),
            Some(SURVEY_IDLE_MAX_PAUSE_MS)
        );
    }

    fn reading(bssid: u8, rssi: i8) -> AccessPoint {
        ap("net", [bssid; 6], rssi)
    }

    #[test]
    fn a_reading_within_the_band_does_not_move_a_row() {
        let mut history = SurveyHistory::new();
        assert!(
            history
                .observe(&[reading(1, -60), reading(2, -70)], None)
                .is_some()
        );

        let list = history
            .observe(&[reading(1, -60), reading(2, -58)], None)
            .expect("the bars moved even though the order did not");
        assert_eq!(
            list.get(0).expect("first").bssid,
            [1; 6],
            "the row keeps its place"
        );
        assert_eq!(
            list.get(1).expect("second").rssi,
            -58,
            "and its reading is the round's, not the one it held"
        );

        let list = history
            .observe(&[reading(1, -60), reading(2, -50)], None)
            .expect("the order moved");
        assert_eq!(list.get(0).expect("first").bssid, [2; 6]);
        assert_eq!(list.get(1).expect("second").bssid, [1; 6]);
    }

    #[test]
    fn a_row_survives_the_rounds_that_missed_it() {
        let mut history = SurveyHistory::new();
        let mut shown = history
            .observe(&[reading(1, -50), reading(2, -60), reading(3, -70)], None)
            .expect("the first round is always a change");
        for round in 1..=LIST_MISSED_ROUNDS {
            if let Some(list) = history.observe(&[reading(1, -50), reading(3, -70)], None) {
                shown = list;
            }
            assert_eq!(
                shown.len(),
                3,
                "the row is still listed after {round} missed rounds"
            );
            assert!(
                shown.iter().any(|row| row.bssid == [2; 6]),
                "and it is that row"
            );
        }

        let list = history
            .observe(&[reading(1, -50), reading(3, -70)], None)
            .expect("the row leaving the page is a change");
        assert_eq!(list.len(), 2);
        assert!(!list.iter().any(|row| row.bssid == [2; 6]));
    }

    #[test]
    fn a_round_that_draws_the_same_rows_is_not_a_change() {
        let mut history = SurveyHistory::new();
        assert!(
            history
                .observe(&[reading(1, -60), reading(2, -70)], None)
                .is_some()
        );
        assert_eq!(
            history.observe(&[reading(1, -60), reading(2, -70)], None),
            None,
            "the same room twice is one redraw"
        );
        assert_eq!(
            history.observe(&[reading(1, -56), reading(2, -70)], None),
            None,
            "and four decibels inside the same bar bucket is still the same page"
        );
        assert!(
            history
                .observe(&[reading(1, -65), reading(2, -70)], None)
                .is_some(),
            "but a bar that changed is a page that changed"
        );
    }

    #[test]
    fn a_newcomer_takes_the_weakest_rows_place_when_the_list_is_full() {
        let mut history = SurveyHistory::new();
        let full: Vec<AccessPoint> = (0..MAX_LISTED_APS)
            .map(|index| reading(index as u8 + 1, -50 - index as i8))
            .collect();
        assert!(history.observe(&full, None).is_some());

        let mut round = full[..MAX_LISTED_APS - 1].to_vec();
        round.push(reading(0xee, -30));
        let list = history
            .observe(&round, None)
            .expect("the row that left the page is a change");
        assert_eq!(list.len(), MAX_LISTED_APS, "the list is still full");
        assert_eq!(
            list.get(0).expect("first").bssid,
            [0xee; 6],
            "the newcomer is the strongest row there is"
        );
        assert!(
            !list
                .iter()
                .any(|row| row.bssid == [MAX_LISTED_APS as u8; 6]),
            "and the row it displaced is gone"
        );
    }

    #[test]
    fn entering_the_page_reads_the_room_again() {
        let mut history = SurveyHistory::new();
        let round = [reading(1, -60)];
        assert!(history.observe(&round, None).is_some());
        assert_eq!(history.observe(&round, None), None);
        history.clear();
        assert!(
            history.observe(&round, None).is_some(),
            "a list nobody has been shown yet is a change"
        );
    }

    fn store(names: &[&str]) -> SavedNetworks {
        let mut store = SavedNetworks::empty();
        for name in names {
            let credentials =
                Credentials::from_text(name, "latte").expect("a credential the radio takes");
            store.remember(&credentials);
        }
        store
    }

    #[test]
    fn a_list_is_marked_with_the_rows_this_device_holds() {
        let list = ApList::from_ranked(&[
            ap("one", [1; 6], -50),
            ap("two", [2; 6], -55),
            ap("three", [3; 6], -60),
            ap("four", [4; 6], -65),
        ]);
        let mut marked = list;
        marked.mark_saved(store(&["two", "four"]).mask(&list));
        assert!(!marked.is_saved(0));
        assert!(marked.is_saved(1));
        assert!(!marked.is_saved(2));
        assert!(marked.is_saved(3));
        assert!(!marked.is_saved(4), "a row that is not there is not marked");
        assert_eq!(
            store(&["two", "four"]).mask(&ApList::empty()),
            0,
            "a list with no rows has no marks"
        );
        assert_eq!(
            SavedNetworks::empty().mask(&list),
            0,
            "and a store with nothing in it marks nothing"
        );
    }

    #[test]
    fn the_copy_of_a_name_is_the_passphrase_it_was_saved_with() {
        let mut saved = SavedNetworks::empty();
        let first = Credentials::from_text("one", "latte").expect("a credential the radio takes");
        let second = Credentials::from_text("two", "mocha").expect("a credential the radio takes");
        saved.remember(&first);
        saved.remember(&second);

        assert_eq!(saved.copy_of(&second.ssid), Some(second));
        assert_eq!(saved.copy_of(&first.ssid), Some(first));
        assert_eq!(
            saved.copy_of(&Ssid::from_octets(b"neighbour")),
            None,
            "a name the store has never seen has no copy"
        );
        assert_eq!(
            SavedNetworks::empty().copy_of(&first.ssid),
            None,
            "and neither has one on a board that saved nothing"
        );
    }

    #[test]
    fn a_round_that_only_moved_the_marks_is_a_round_the_page_hears_about() {
        let round = [reading(1, -60), reading(2, -70)];
        let mut history = SurveyHistory::new();
        assert!(history.observe(&round, None).is_some());
        assert_eq!(
            history.observe(&round, Some(&store(&["net"]))),
            Some({
                let mut list = ApList::from_ranked(&round);
                list.mark_saved(0b11);
                list
            }),
            "the same rows with a new mark are a list the page draws differently"
        );
        assert_eq!(
            history.observe(&round, Some(&store(&["net"]))),
            None,
            "and the same rows with the same marks are not"
        );
    }

    #[test]
    fn a_forget_is_the_same_list_with_the_mark_taken_off() {
        let round = [reading(1, -60), reading(2, -70)];
        let mut history = SurveyHistory::new();
        assert!(history.observe(&round, Some(&store(&["net"]))).is_some());

        let list = history.marked(None).expect("the marks moved");
        assert!(!list.is_saved(0) && !list.is_saved(1), "both rows lost it");
        assert_eq!(list.len(), 2, "and the rows themselves are the ones it had");
        assert_eq!(
            history.marked(None),
            None,
            "a list whose marks are already the store's is not republished"
        );
        let list = history
            .marked(Some(&store(&["net"])))
            .expect("a save is the same answer the other way round");
        assert!(
            list.is_saved(0) && list.is_saved(1),
            "both rows carry it again"
        );
    }
}

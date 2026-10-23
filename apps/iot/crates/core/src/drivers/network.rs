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

/// How long the loop that surveys an unconfigured device rests between rounds.
///
/// A scan is the radio's own job and takes a few hundred milliseconds on this part; this is the
/// pause a page gets between rounds. Here rather than in the loop that does the resting because
/// it is a policy about how often the radio is asked, which is the same kind of fact as how many
/// rows it keeps — and because a number only the device can exercise is one that has to be
/// pinned by a test instead.
pub const SURVEY_PAUSE_MS: u64 = 5_000;

/// How long it rests after a round that found nothing, while an empty round still reads as a
/// hiccup rather than as the truth.
///
/// Retried soon rather than shown as the truth for a whole interval: one round in twelve came
/// back empty on this bench with the driver reporting success, and a page must not print
/// "nothing around" over a list that the next round would fill.
pub const SURVEY_EMPTY_PAUSE_MS: u64 = 1_000;

/// How many empty rounds in a row are still read as that hiccup.
///
/// A place with nothing in range — a device still in its box, an access point that is off —
/// answers every round the same way, and retrying those every second would leave the radio
/// scanning for most of every second for as long as the device waits to be set up. Past this
/// many, an empty round is the answer until the next round asks again.
pub const SURVEY_EMPTY_RETRIES: u8 = 3;

/// How long to rest once the room has been read a few times and nobody has acted on it.
///
/// The second and third steps of the backoff [`survey_pause_after`] walks: five seconds while
/// somebody may still be deciding, then a quarter of a minute, then half a minute.
pub const SURVEY_IDLE_PAUSE_MS: u64 = 15_000;
pub const SURVEY_IDLE_MAX_PAUSE_MS: u64 = 30_000;

/// How many idle rounds each step of that backoff lasts.
///
/// Counted in rounds rather than in seconds so the steps are the same shape as the pause before
/// them: three rounds at five seconds is a quarter of a minute before the pause itself grows,
/// which is about as long as a user looks at a list before deciding.
pub const SURVEY_IDLE_ROUNDS: u8 = 3;

/// How long to rest after a survey round, given empty rounds and idle rounds in a row. Only an
/// empty round is retried soon; otherwise the pause grows in three steps as nobody acts, because a
/// room that is not changing does not need a new list every five seconds.
///
/// `None` is the access point's front, which scans only when the page asks: a round takes the radio
/// off the access point.
pub fn survey_pause_after(front: Front, empty_rounds: u8, idle_rounds: u8) -> Option<u64> {
    // A phone reading the page and a radio scanning the room are one radio: a round takes the
    // access point off the air for its length, and a request that lands in one is a page the
    // phone reports as missing. So the access point's front does not rest and then look again — it
    // looks when the page asks it to, which is the page's own "look around" and nothing else.
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
///
/// The splash entry starts open so a phone joins by tapping the name, with `PASS` one tap away.
/// A running entry starts secured where a screen can show the passphrase; a board with no screen
/// stays open, because a secret nobody can read is a network nobody can join.
pub const fn setup_ap_starts_secured(panel: bool, from_splash: bool) -> bool {
    panel && !from_splash
}

/// How many networks the device remembers, strongest-first, newest at the front —
/// the same slot count and ordering the reference firmware uses, so a device that
/// has been to three places reconnects without being told which.
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
    /// The slot an impossible report leaves behind, so the list can be an array
    /// rather than a `Vec` — a `Vec` in the device state would make every snapshot
    /// copy an allocation and every page hold a borrow of the driver's.
    pub const EMPTY: Self = Self {
        ssid: Ssid::EMPTY,
        bssid: [0; 6],
        channel: 0,
        rssi: 0,
        secured: false,
    };

    /// Whether this row carries a name the panel can draw, which decides whether
    /// the list shows the name or the BSSID.
    pub fn name_is_drawable(&self) -> bool {
        self.ssid
            .as_display_text()
            .is_some_and(|text| !text.is_empty())
    }

    /// Whether this row and `other` are the same access point.
    ///
    /// The one field that does not move under a row: a rescan rewrites every reading of every
    /// network, so two rows of one access point differ in everything a page draws and in the signal
    /// it shows — and what identifies it, here as in a `Credentials`, is the BSSID.
    pub fn is_same_network(&self, other: &Self) -> bool {
        self.bssid == other.bssid
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

/// The ranked list as the device state holds it: a fixed-capacity value, never the driver's
/// `Vec`, because the state is snapshotted into every page and diagnostics.
///
/// Each row's saved mark travels with the list: it is a fact about these rows in this order, and
/// one bit per row is all the page asks for.
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
    ///
    /// Truncates rather than refuses, because the caller's list is already the
    /// strongest-first one: what does not fit is the weakest, which is what the
    /// panel would have dropped anyway.
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
///
/// A scan is one reading of a radio whose readings move a few decibels from round to round, so an
/// order taken from a single round is a different order every round — a list whose rows swap under
/// the finger that is aiming at one. A row has to beat the row above it by this much to take its
/// place: noise cannot move a row, and a network that really is stronger does.
pub const LIST_HYSTERESIS_DB: i8 = 4;

/// How many rounds in a row a row may go unseen before the list drops it.
///
/// A round that missed a network in range is as ordinary as one that found it — the survey saw an
/// empty round one time in twelve on this bench — so a row dropped the first round it is missing is
/// a row that vanishes from under the finger aiming at it. One missed round is not evidence.
pub const LIST_MISSED_ROUNDS: u8 = 2;

/// One row of the list as it is carried from round to round: the last reading of an access point,
/// and how many rounds in a row the scan has not reported it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Listed {
    ap: AccessPoint,
    missed: u8,
}

impl Listed {
    /// The slot an empty list leaves behind, the way [`ApList::empty`] leaves one.
    const EMPTY: Self = Self {
        ap: AccessPoint::EMPTY,
        missed: 0,
    };
}

/// The list the page shows, carried from one survey round to the next.
///
/// The rule it exists for: a list is an order a finger aims at, and a scan is one noisy reading of
/// the room. What each round is folded into is the list already on the page rather than a new one —
/// a row keeps its place until something beats it by [`LIST_HYSTERESIS_DB`], keeps its last reading
/// through a round that missed it, and is dropped only after [`LIST_MISSED_ROUNDS`] of them.
/// Strongest first is still what the order means; what moved is how much a reading has to change
/// before the order does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurveyHistory {
    entries: [Listed; MAX_LISTED_APS],
    len: u8,
    /// Whether the next round is the first after [`clear`](Self::clear), and so is a change to the
    /// page whatever it says — a list nobody has been shown yet is not "the same list".
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
    ///
    /// What a page entered after a long rest wants: the rounds that filled the list were read up to
    /// half a minute apart, and a list assembled out of them is the room as the clock worked its way
    /// to it rather than as it is.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Fold one round in and answer the list the page should now show. `None` means the page
    /// would draw the same rows; marks count, because the page draws them. The store is optional
    /// because the partition is: `None` says no marks without carrying an empty store.
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
                    // Seen again, so the rounds it was missing stop counting against it.
                    row.missed = 0;
                }
                None if usize::from(self.len) < MAX_LISTED_APS => {
                    self.entries[usize::from(self.len)] = Listed { ap: *ap, missed: 0 };
                    self.len += 1;
                }
                // The list is full: a newcomer takes the weakest row's place when it beats it. That
                // is the only way a network that was not in the first round can ever be shown —
                // without it, a list that filled up once would never admit anything again.
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
        // Marked before the comparison rather than after: a round that saved or forgot a network
        // draws differently, and the comparison is what decides whether the page hears about it.
        list.mark_saved(saved.map_or(0, |saved| saved.mask(&list)));
        if !self.fresh && drawn_the_same(&list, &self.published) {
            return None;
        }
        self.fresh = false;
        self.published = list;
        Some(list)
    }

    /// The list the page is showing, marked again against the store as it now is.
    ///
    /// A forget changes which rows this device holds without changing what is in range, so the round
    /// that follows would answer the same rows — and until it comes the page would keep a mark for a
    /// network that is gone. Answering the last list again is what puts the store's own report on
    /// it, and `None` when the marks come out the same: the round after a save would otherwise
    /// redraw a list that already says so.
    pub fn marked(&mut self, saved: Option<&SavedNetworks>) -> Option<ApList> {
        let mut list = self.published;
        list.mark_saved(saved.map_or(0, |saved| saved.mask(&list)));
        if list.saved == self.published.saved {
            return None;
        }
        self.published = list;
        Some(list)
    }

    /// Put the rows back in order, moving one up only where it clearly beats the row above it.
    ///
    /// An insertion walk rather than a sort, and not by preference: the order a hysteresis band
    /// defines is not a total order — a row within the band of the one above and that one within the
    /// band of the one above *it* says nothing about the outer two — and a sort handed a comparator
    /// that is not one is free to answer anything. What this does is defined on its own: a row walks
    /// up past the rows it beats, and the rest keep the places the last round left them in.
    fn settle(&mut self) {
        for index in 1..usize::from(self.len) {
            let mut at = index;
            while at > 0 && beats(self.entries[at].ap.rssi, self.entries[at - 1].ap.rssi) {
                self.entries.swap(at, at - 1);
                at -= 1;
            }
        }
    }

    /// The row a newcomer should displace: one the round has already missed if there is one, and
    /// the weakest of the rest otherwise.
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

/// Whether `reading` is far enough above `above` to take its place in the list.
fn beats(reading: i8, above: i8) -> bool {
    i16::from(reading) - i16::from(above) >= i16::from(LIST_HYSTERESIS_DB)
}

/// Whether two lists would draw the same page.
///
/// A row's drawing is its name — or its BSSID where the name is not text — its bars, its open marker
/// and the mark the store puts on it, so a round that moved a reading inside the bucket it already
/// drew, or repeated it exactly, is not a change to the page. The readings themselves are
/// deliberately not compared: they move every round, and a page redrawn for that is a page redrawn
/// for nothing.
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
///
/// A hint, not a fact: every use may be wrong and the association retries without it. It saves a
/// full-band scan, which on this radio is seconds rather than milliseconds. [`Self::NONE`] is "not
/// known"; channel 0 is not a channel.
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
    /// A store that holds nothing, for the callers that have no store at all: what they report to
    /// the page is the empty set of marks, which is the truth on a board with no partition.
    pub const EMPTY: Self = Self {
        entries: [(Ssid::EMPTY, Password::EMPTY, LinkHint::NONE); MAX_SAVED_NETWORKS],
        len: 0,
    };

    pub const fn empty() -> Self {
        Self::EMPTY
    }

    /// Remember a network at the front, replacing one with the same name. Returns whether it
    /// replaced, so the page can say "saved" or "updated". A known name keeps its hint; a new one
    /// starts at [`LinkHint::NONE`].
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

    /// Put one entry in a fixed position, for a list that is being read back rather than
    /// added to.
    ///
    /// [`remember`](Self::remember) is the wrong shape for a stored list: it inserts at
    /// the front, so replaying a record entry by entry through it would reverse the list
    /// on every load. Where an entry sits is something the record already says.
    pub(crate) fn place(&mut self, index: usize, ssid: Ssid, password: Password, hint: LinkHint) {
        if index < MAX_SAVED_NETWORKS {
            self.entries[index] = (ssid, password, hint);
            self.len = self.len.max(index as u8 + 1);
        }
    }

    /// The hint this device keeps for `ssid`, or [`LinkHint::NONE`] for a name it does not hold.
    pub fn hint_of(&self, ssid: &Ssid) -> LinkHint {
        (0..self.len as usize)
            .find(|index| self.entries[*index].0 == *ssid)
            .map_or(LinkHint::NONE, |index| self.entries[index].2)
    }

    /// Record a hint for a name this device holds. Returns whether it changed, which is what keeps
    /// a link that did not move from writing the flash on every association.
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

    /// The copy this device keeps of `ssid`, for a panel that named a network without a passphrase.
    ///
    /// [`holds`](Self::holds) is the question the page asks and this is the one the radio asks: a
    /// row the page shows as saved is a row whose name is here, and what goes on the air is the
    /// secret beside it. The two are one walk apart and neither is a secret on its own.
    pub fn copy_of(&self, ssid: &Ssid) -> Option<Credentials> {
        self.iter().find(|credentials| credentials.ssid == *ssid)
    }

    /// Which of a list's rows this device holds a passphrase for: a bit per row, in the list's own
    /// order.
    ///
    /// The one question the page asks about the store, answered in the shape the page asks it in.
    /// The names themselves stay here: they are three hundred bytes, the page has no use for them
    /// beyond marking the eight rows in front of it, and what joins them is a secret with exactly
    /// one reader — the radio.
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

/// What the panel asked the link for: a passphrase it typed, or a network to join with the copy this
/// device keeps.
///
/// One message with two shapes rather than two planes, because they are the same question asked with
/// different amounts of help — "join this network, here is the passphrase" and "join this network,
/// you have the passphrase" — and the network task answers both in the same place.
///
/// A name rather than a credential in the second shape because the store belongs to the network
/// task: the page can say *which* network a finger picked and not what its passphrase is, and a name
/// the store does not hold is answered on the page, which is the only place a passphrase can be
/// typed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinRequest {
    /// The credential the keyboard's own hand-over key produced.
    Typed(Credentials),
    /// A row the panel picked without typing, because this device keeps a passphrase for it.
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
    /// The access point is not one the radio can find.
    ///
    /// Split out of the general refusal because it is the one a user can act on by looking at
    /// where the device is standing, rather than by looking at what was typed.
    NotFound,
    /// The access point refused the credential: association got as far as the handshake and
    /// stopped there.
    ///
    /// The other half of that split, and the likelier of the two at a panel — a passphrase typed
    /// on a keypad whose keys hold four characters each. Nothing else distinguishes it: the
    /// driver reports a disconnect and the credential is the only thing the user controls.
    Credentials,
    /// The radio refused for a reason of its own.
    Driver,
}

/// Which of the two ways into setup mode the device is offering.
///
/// One mode, two front ends, and one of them at a time: the panel's own list and the access point
/// whose page a phone fills in answer the same question — which network to join — and both end on
/// the same [`Credentials`] plane. What differs is the radio: the page is served over the access
/// point, and the access point shares the radio with the scan the list is made of, so a device
/// that is being typed on at the panel has no reason to beacon a network nobody is using.
///
/// Which one is offered is the build's answer at boot and the person's after that, so this is the
/// value a decision travels in rather than a mode the radio switches on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Front {
    /// The panel: the scan list, the keys, and no access point.
    #[default]
    Panel,
    /// The access point: its setup page fills the screen, and the panel's list waits behind the box.
    AccessPoint,
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
    /// Whether setup mode is what the page is about: the access point is up when
    /// [`Self::front`] is [`Front::AccessPoint`]. The network task owns this; every reader is
    /// already reading the link.
    pub setup: bool,
    /// Whether the page is the offline list: nothing is saved, no setup mode is running, and
    /// `SCAN | AP` is still offered. `AP` is the one explicit way into setup from here — a hold
    /// is not, because a device outside setup mode must not raise an access point by accident.
    pub browsing: bool,
    /// Whether setup mode has somewhere to go back to. `CANCEL` is only offered when it has:
    /// otherwise it would take the access point down and raise it again.
    pub setup_cancellable: bool,
    /// Whether this device keeps at least one network. The store's answer, filled in by the
    /// caller that holds it for the same reason [`Self::setup_cancellable`] is: a board with no
    /// screen reads it as "this device has never been set up", which is amber on its lamp.
    pub saved: bool,
    /// Which front is offering setup, and with it whether the access point is up. See [`Front`].
    pub front: Front,
    /// The passphrase the setup access point is up with, when it has one.
    ///
    /// `None` is open: what a board with no screen can offer, or what a panel asked for. The
    /// station generates it once per boot and it leaves only on the panel; logging it or writing
    /// it into the page would let anyone in range read it.
    pub setup_password: Option<SetupPass>,
}

impl LinkStatus {
    /// Whether there is a link to take down: an attempt in flight or one that worked. The same two
    /// phases are the ones with nothing to scan for. [`Self::ap`] is not part of it, because it is
    /// set only after the radio accepts.
    pub fn is_leavable(&self) -> bool {
        matches!(self.phase, LinkPhase::Connecting | LinkPhase::Connected)
    }

    /// Whether the phone's setup box is the page's screen: the access point is up and a phone is
    /// the front it is up for. One answer for what to draw, what a tap means, and whether the
    /// access point should exist.
    pub fn shows_setup_box(&self) -> bool {
        self.setup && self.front == Front::AccessPoint
    }

    /// This status as one setup front reports it. The station cannot answer `front` or the way
    /// back, so a publish that set only `setup` would close the box a second after it opened and
    /// turn `CANCEL` into `SCAN`.
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

    /// Set the credential and start associating. Split from [`Self::await_connected`] so the panel
    /// can draw the attempt while it runs; a wrong passphrase is refused only when the handshake
    /// times out. A credential the driver rejects fails here, and the phase is the refusal.
    fn begin_connect(&mut self, credentials: &Credentials) -> Result<(), NetworkError>;

    /// [`Self::begin_connect`] with the access point and channel this network was last found on.
    /// The hint may be stale; the caller retries once with [`LinkHint::NONE`] rather than treating
    /// the credential as refused. The default ignores it.
    fn begin_connect_hinted(
        &mut self,
        credentials: &Credentials,
        hint: LinkHint,
    ) -> Result<(), NetworkError> {
        let _ = hint;
        self.begin_connect(credentials)
    }

    /// Wait for the radio's own answer to the attempt [`Self::begin_connect`] started. Returns once
    /// the radio has made the decision, which is not the same as having an address — see
    /// [`LinkPhase`].
    fn await_connected(&mut self) -> impl Future<Output = Result<(), NetworkError>>;

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

    /// Raise setup's access point under `ssid`, WPA2 when `secured`, and hand over its frame
    /// interface. It is a second device beside [`Self::take_device`]'s, because the page's list is
    /// a scan and the scan is the station's. Dropping it releases the access point; the mode is
    /// [`Self::end_setup`]'s.
    ///
    /// The name comes from the caller because it carries a board's identity. The passphrase stays
    /// in the station: it is this boot's random value and the panel reads it from [`Self::status`].
    fn begin_setup(&mut self, ssid: &Ssid, secured: bool) -> Result<Self::Device, NetworkError>;

    /// Put the radio back in station mode on its own, taking the access point down.
    ///
    /// Separate from dropping the device because the two belong to different owners: the
    /// device is the stack's and the mode is the controller's, so a caller that dropped the
    /// one without the other would leave a beaconed network nothing is serving.
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

    /// A scan that found nothing is not a scan that found the old networks: the
    /// stored list is replaced whole, because a page that kept drawing rows the
    /// radio can no longer see would be showing a network the device cannot join.
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
    fn a_survey_that_found_something_rests_the_whole_pause() {
        // The count is of rounds in a row that found nothing, so zero means the last round found
        // something — the survey working, and no reason to ask the radio again a second later.
        assert_eq!(
            survey_pause_after(Front::Panel, 0, 0),
            Some(SURVEY_PAUSE_MS)
        );
    }

    #[test]
    fn the_access_points_front_never_rests_into_another_round() {
        // A phone reading the page and a radio scanning the room are one radio. What looks again
        // is the page's own request, so no clock here asks for a round — not after an empty place,
        // and not after a room nobody acts on.
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
        // The station's status knows the radio's half of the link and neither of the two fields the
        // panel's strip and its box are drawn from. A publish that spread it and set only `setup`
        // would report the panel's front with no way out on every scan round — a box that closes
        // itself a second after it opens, and a `CANCEL` that turns into `SCAN` on the same clock.
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
            // And the radio's half is handed through untouched: a phase, an address or a
            // passphrase lost here would be a page a round behind the truth.
            assert_eq!(published.phase, station.phase);
            assert_eq!(published.ap, station.ap);
            assert_eq!(published.address, station.address);
            assert_eq!(published.setup_password, station.setup_password);
        }
    }

    #[test]
    fn an_empty_place_stops_being_retried_every_second() {
        // The first empty rounds come back fast, because one in twelve did on this bench, and past
        // that an empty round is the answer: a device in a room with nothing in range would
        // otherwise have the radio scanning for most of every second for as long as it waits to be
        // set up.
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
        // Three steps and no more: a list nobody has touched is still the truth about the room, and
        // the radio is not the part of the device that has to spend anything on it.
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
        // An empty place backs off too — on the idle clock, once the retries are spent.
        assert_eq!(
            survey_pause_after(
                Front::Panel,
                SURVEY_EMPTY_RETRIES + 1,
                2 * SURVEY_IDLE_ROUNDS
            ),
            Some(SURVEY_IDLE_MAX_PAUSE_MS)
        );
    }

    /// One row of a round: the name is the name the page draws, and the BSSID is what the history
    /// keys on.
    fn reading(bssid: u8, rssi: i8) -> AccessPoint {
        ap("net", [bssid; 6], rssi)
    }

    #[test]
    fn a_reading_within_the_band_does_not_move_a_row() {
        // The complaint this rule answers: a scan's readings move two or three decibels from round
        // to round, so an order taken from one of them is a list whose rows swap under the finger
        // aiming at one of them. Two decibels is not news about which network is stronger.
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

        // Ten decibels is news, and the row takes the place it has earned.
        let list = history
            .observe(&[reading(1, -60), reading(2, -50)], None)
            .expect("the order moved");
        assert_eq!(list.get(0).expect("first").bssid, [2; 6]);
        assert_eq!(list.get(1).expect("second").bssid, [1; 6]);
    }

    #[test]
    fn a_row_survives_the_rounds_that_missed_it() {
        // A round that missed a network in range is ordinary — one in twelve came back empty on
        // this bench — so a row dropped on the first miss is a row that vanishes from under the
        // finger that was aiming at it.
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
        // What a repaint costs on this panel is a whole frame, and a reading that moved inside the
        // bucket it already drew is a round with nothing to draw.
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
        // Without this, a list that filled up in the first round could never admit anything again:
        // a network that came into range later would be dropped on every round it appeared in.
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

    /// A store that holds these names, which is all a mark is made of.
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
        // The one question the page asks of the store, answered in the shape it asks it in: a mark
        // per row of the list in front of it rather than the names themselves, which are three
        // hundred bytes the page has no use for — and what joins them is a secret with one reader.
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
        // What a store join puts on the air: the name is what the page can say, and the passphrase
        // beside it is what the radio needs. A name that is not here is `None` rather than empty
        // credentials, because an empty passphrase is a *valid* answer — an open network — and the
        // caller has to tell "the device holds this" from "the device holds nothing for this".
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
        // The marks are part of what the page draws, so a save or a forget is a redraw — and a
        // round that moved nothing else is otherwise deliberately not published at all.
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
        // A forget changes which rows this device holds without changing what is in range, so the
        // page has to be told about the list it is already showing rather than waiting for a round
        // that would answer the same rows.
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

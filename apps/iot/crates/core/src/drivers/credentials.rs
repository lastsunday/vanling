//! Credentials at rest: what the saved-network list is written as, and the rule that
//! picks between the two slots it is written in.
//!
//! Flash is not a file. A sector is erased before it can be written, and a power cut can
//! land between those two, so a half-written record has to read as *nothing* rather than
//! as a network with a nonsense name. Three things make that true, each pinned by a test
//! below rather than trusted:
//!
//! * **A record describes itself.** Magic, version, entry count and a checksum over
//!   everything before it: a record that lost bytes, or one a future version packs
//!   differently, fails one of those and is dropped.
//! * **Two slots, newest wins.** A save erases the slot that is *not* current, so the
//!   copy from before it survives losing power in the middle of the save. What the store
//!   holds after that is the list as it was, and it holds nothing only when neither slot
//!   can be read — which is what it holds on a part that has never been written.
//! * **A part that will not answer is not a device that cannot boot.** An unreadable
//!   region is an empty store with a reason attached: what it costs is the session, and
//!   [`Opened`] is what lets the caller say which of the two it is.
//!
//! The region is a trait because everything above it is arithmetic, and arithmetic worth
//! getting right is worth testing where it can be run: the device's side is three calls
//! into its flash driver, while the slot rule, the checksum and the parser are what would
//! turn one network into another.
//!
//! `@/records/iot/wifi.md`.

use crate::drivers::network::{
    Credentials, MAX_SAVED_NETWORKS, PASSWORD_BYTES, Password, SSID_BYTES, SavedNetworks, Ssid,
};

/// The erase granularity of the parts this product is built for, and so the size of one
/// slot: a slot is written by erasing it whole and writing the record, never in place.
pub const SLOT_BYTES: u32 = 4096;

/// How many slots the store keeps.
///
/// Two, and the second one is the point: it is the record from before the save that is
/// happening now, so a save that is interrupted leaves the list it was replacing. A third
/// slot would only hold another copy of a generation the store already has.
pub const SLOTS: usize = 2;

/// The whole region a store needs, which is what its partition has to be.
pub const REGION_BYTES: u32 = SLOT_BYTES * SLOTS as u32;

/// One entry as the record keeps it: each length inside a fixed window, so where an entry
/// begins is arithmetic rather than a parse.
///
/// Fixed-width rather than packed because the sector is mostly empty either way, and
/// because entries that move when a name gets shorter would have to be parsed before the
/// record's own checksum could be trusted to say the parse was worth doing. The two spare
/// bytes are what keeps an entry — and so a whole record — a whole number of words, which
/// is what every write to this part has to be.
const ENTRY_BYTES: usize = 1 + SSID_BYTES + 1 + PASSWORD_BYTES + 2;

/// The bytes before the entries: magic, version, entry count, body length, sequence.
const HEADER_BYTES: usize = 12;

/// The checksum that closes a record.
const CRC_BYTES: usize = 4;

/// The four bytes a record starts with.
///
/// A slot that was never written reads as all ones and a slot left half-erased reads as
/// anything at all, so this is the first thing a reader checks about either.
const MAGIC: [u8; 4] = *b"VLNC";

/// The encoding this file writes, and the only one it reads.
const VERSION: u8 = 1;

/// The most a record can occupy.
pub const RECORD_BYTES: usize = HEADER_BYTES + MAX_SAVED_NETWORKS * ENTRY_BYTES + CRC_BYTES;

const _: () = assert!(ENTRY_BYTES.is_multiple_of(4));
const _: () = assert!(RECORD_BYTES.is_multiple_of(4));
const _: () = assert!(RECORD_BYTES as u32 <= SLOT_BYTES);

/// A region of flash the store can keep its records in: whole sectors, erased before they
/// are written, addressed from the region's own start.
pub trait FlashRegion {
    /// Fill `into` from `offset`.
    fn read(&mut self, offset: u32, into: &mut [u8]) -> Result<(), RegionError>;

    /// Write `bytes` at `offset`, which may only touch bytes that are erased.
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), RegionError>;

    /// Put `bytes` from `offset` back to erased.
    fn erase(&mut self, offset: u32, bytes: u32) -> Result<(), RegionError>;
}

/// Why a region would not do what it was asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionError {
    /// The part would not answer.
    Refused,
    /// The request is not one this part can carry out: a write that is not word-aligned.
    /// Told apart from a refusal because it is the caller's mistake rather than the
    /// hardware's, and a log that blamed the flash for it would send the next reader to
    /// the wrong place.
    Misaligned,
    /// The request runs past the region. Also the caller's mistake, and also not silicon.
    OutOfRange,
}

/// What opening a store found.
///
/// Three answers rather than two, because "nothing saved yet" and "cannot save" are the
/// same empty list and only one of them is a device waiting to be set up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opened {
    /// A valid record, and the store below holds what it said.
    Loaded,
    /// Nothing valid in either slot: a part that has never been written, or one whose
    /// save was interrupted before it landed.
    Blank,
    /// The part would not answer. The store below is empty and will stay empty.
    Unreadable(RegionError),
}

/// The saved-network list, and the slot it was last written to.
pub struct Store<R> {
    region: R,
    networks: SavedNetworks,
    /// Which record this is, of the ones this device has written. What makes the newer of
    /// two valid slots the one to keep. It only stops telling them apart if a device is
    /// set up four billion times, which no device reaches.
    sequence: u32,
    /// The slot the next save writes, which is never the one holding `sequence`.
    next: usize,
}

impl<R: FlashRegion> Store<R> {
    /// Read the region and keep the newest record in it.
    ///
    /// Never fails, because every way this can go wrong is something the device can be
    /// run with: a part that has never been written, a record that lost bytes, and a
    /// region that will not answer all come back as a store, and [`Opened`] says which of
    /// them it was.
    pub fn open(mut region: R) -> (Self, Opened) {
        let mut newest: Option<(usize, u32, SavedNetworks)> = None;
        let mut unreadable = None;
        for slot in 0..SLOTS {
            match read_slot(&mut region, slot) {
                Ok(Some((sequence, networks))) => {
                    if newest.as_ref().is_none_or(|(_, best, _)| sequence > *best) {
                        newest = Some((slot, sequence, networks));
                    }
                }
                Ok(None) => {}
                Err(error) => unreadable = Some(error),
            }
        }
        let (store, opened) = match newest {
            Some((slot, sequence, networks)) => (
                Self {
                    region,
                    networks,
                    sequence,
                    next: (slot + 1) % SLOTS,
                },
                Opened::Loaded,
            ),
            None => (
                Self {
                    region,
                    networks: SavedNetworks::empty(),
                    sequence: 0,
                    next: 0,
                },
                match unreadable {
                    Some(error) => Opened::Unreadable(error),
                    None => Opened::Blank,
                },
            ),
        };
        (store, opened)
    }

    /// The list as the store holds it: newest first, which is the order a reconnect walks
    /// it in.
    pub fn networks(&self) -> &SavedNetworks {
        &self.networks
    }

    /// Add a network and write the whole list to the slot the last save did not use.
    ///
    /// Returns whether the name was already there — the same answer
    /// [`SavedNetworks::remember`] gives, because a caller that means to say "saved" or
    /// "updated" has to know which write this was.
    ///
    /// An error means the part refused. The list in hand is still the one this session
    /// runs with, so the device keeps working; what is lost is the next boot's copy, and
    /// only that.
    pub fn remember(&mut self, credentials: &Credentials) -> Result<bool, RegionError> {
        let replaced = self.networks.remember(credentials);
        self.write_out()?;
        Ok(replaced)
    }

    /// Drop every saved network with this name and write the whole list to the other slot.
    ///
    /// Returns whether anything was dropped, so a caller can say "forgotten" rather than claim a
    /// write that only rewrote the list as it was.
    ///
    /// The same write as [`Self::remember`] and the same failure: what a refusal costs is the next
    /// boot's copy, and the list this session runs with is the one it just handed to `forget`.
    ///
    /// A forget is a save of a shorter list, so the slot rule is the same one and so is what an
    /// interrupted write costs: the record from before it survives, which for a forget is the list
    /// *with* the name — an erase that lost power did not happen, rather than one that half happened.
    pub fn forget(&mut self, ssid: &Ssid) -> Result<bool, RegionError> {
        if !self.networks.forget_named(ssid) {
            return Ok(false);
        }
        self.write_out()?;
        Ok(true)
    }

    /// The record the write makes: one sequence number on, into the slot the last save did not
    /// use.
    fn write_out(&mut self) -> Result<(), RegionError> {
        self.sequence = self.sequence.wrapping_add(1);
        let mut record = [0u8; RECORD_BYTES];
        let written = encode(&self.networks, self.sequence, &mut record);
        let at = self.next as u32 * SLOT_BYTES;
        self.region.erase(at, SLOT_BYTES)?;
        self.region.write(at, &record[..written])?;
        self.next = (self.next + 1) % SLOTS;
        Ok(())
    }
}

/// One slot's record, or `None` when what is there is not one this version wrote.
fn read_slot<R: FlashRegion>(
    region: &mut R,
    slot: usize,
) -> Result<Option<(u32, SavedNetworks)>, RegionError> {
    let at = slot as u32 * SLOT_BYTES;
    // The header first. A blank part answers this one short read and nothing else, so the
    // common case of a device that has never been set up costs two header reads rather
    // than two record-sized ones.
    let mut header = [0u8; HEADER_BYTES];
    region.read(at, &mut header)?;
    if header[..MAGIC.len()] != MAGIC || header[4] != VERSION {
        return Ok(None);
    }
    let count = header[5] as usize;
    let body = u16::from_le_bytes([header[6], header[7]]) as usize;
    if count > MAX_SAVED_NETWORKS || body != count * ENTRY_BYTES {
        return Ok(None);
    }
    let mut record = [0u8; RECORD_BYTES];
    let total = HEADER_BYTES + body + CRC_BYTES;
    region.read(at, &mut record[..total])?;
    Ok(decode(&record[..total]))
}

/// Write `networks` into `record` as sequence `sequence`, returning the bytes used.
fn encode(networks: &SavedNetworks, sequence: u32, record: &mut [u8; RECORD_BYTES]) -> usize {
    let body = networks.len() * ENTRY_BYTES;
    record[..MAGIC.len()].copy_from_slice(&MAGIC);
    record[4] = VERSION;
    record[5] = networks.len() as u8;
    record[6..8].copy_from_slice(&(body as u16).to_le_bytes());
    record[8..12].copy_from_slice(&sequence.to_le_bytes());
    for (index, credentials) in networks.iter().enumerate() {
        let entry = HEADER_BYTES + index * ENTRY_BYTES;
        let ssid = credentials.ssid.as_bytes();
        let password = credentials.password.as_bytes();
        record[entry] = ssid.len() as u8;
        record[entry + 1..entry + 1 + ssid.len()].copy_from_slice(ssid);
        record[entry + 1 + SSID_BYTES] = password.len() as u8;
        record[entry + 2 + SSID_BYTES..entry + 2 + SSID_BYTES + password.len()]
            .copy_from_slice(password);
    }
    let end = HEADER_BYTES + body;
    let checksum = crc32(&record[..end]).to_le_bytes();
    record[end..end + CRC_BYTES].copy_from_slice(&checksum);
    end + CRC_BYTES
}

/// Read a record back, or `None` if a single byte of it is not the byte that was written.
///
/// Everything a record says about itself is checked before any of it is believed, and the
/// checksum is what makes the rest of this trustworthy: the lengths and the count come off
/// the part, and a reader that trusted them would index past its own buffer on a record it
/// lost bytes of.
fn decode(record: &[u8]) -> Option<(u32, SavedNetworks)> {
    if record.len() < HEADER_BYTES + CRC_BYTES
        || record[..MAGIC.len()] != MAGIC
        || record[4] != VERSION
    {
        return None;
    }
    let count = record[5] as usize;
    let body = u16::from_le_bytes([record[6], record[7]]) as usize;
    if count > MAX_SAVED_NETWORKS || body != count * ENTRY_BYTES {
        return None;
    }
    let end = HEADER_BYTES + body;
    if end + CRC_BYTES > record.len() {
        return None;
    }
    let declared = u32::from_le_bytes(record[end..end + CRC_BYTES].try_into().ok()?);
    if declared != crc32(&record[..end]) {
        return None;
    }
    let sequence = u32::from_le_bytes(record[8..12].try_into().ok()?);
    let mut networks = SavedNetworks::empty();
    for index in 0..count {
        let entry = HEADER_BYTES + index * ENTRY_BYTES;
        let ssid_len = record[entry] as usize;
        let password_len = record[entry + 1 + SSID_BYTES] as usize;
        if ssid_len > SSID_BYTES || password_len > PASSWORD_BYTES {
            return None;
        }
        networks.place(
            index,
            Ssid::from_octets(&record[entry + 1..entry + 1 + ssid_len]),
            Password::from_bytes(
                &record[entry + 2 + SSID_BYTES..entry + 2 + SSID_BYTES + password_len],
            ),
        );
    }
    Some((sequence, networks))
}

/// The checksum every record and PNG share (CRC-32), computed a bit at a time.
///
/// A table would be a kilobyte of a part's flash to save a few thousand iterations of the
/// one call that runs this: on a record this size, once per save and once per boot.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let carry = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & carry);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region in RAM, with the two ways a real one goes wrong available to a test.
    struct Fake {
        bytes: [u8; REGION_BYTES as usize],
        /// Every call fails: a part that will not answer.
        dead: bool,
        /// Erase works and the write after it does not: the save that lost power.
        torn: bool,
    }

    impl Fake {
        /// What an erased part reads as.
        fn blank() -> Self {
            Self {
                bytes: [0xFF; REGION_BYTES as usize],
                dead: false,
                torn: false,
            }
        }

        fn network(name: &str, password: &str) -> Credentials {
            Credentials::from_text(name, password).expect("a name and passphrase the radio takes")
        }
    }

    /// Borrowed rather than owned, so the bytes outlive the store that used them and the
    /// next `open` is the reset this stands in for.
    impl FlashRegion for &mut Fake {
        fn read(&mut self, offset: u32, into: &mut [u8]) -> Result<(), RegionError> {
            if self.dead {
                return Err(RegionError::Refused);
            }
            let from = offset as usize;
            into.copy_from_slice(&self.bytes[from..from + into.len()]);
            Ok(())
        }

        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), RegionError> {
            if self.dead || self.torn {
                return Err(RegionError::Refused);
            }
            let from = offset as usize;
            self.bytes[from..from + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }

        fn erase(&mut self, offset: u32, bytes: u32) -> Result<(), RegionError> {
            if self.dead {
                return Err(RegionError::Refused);
            }
            let from = offset as usize;
            self.bytes[from..from + bytes as usize].fill(0xFF);
            Ok(())
        }
    }

    #[test]
    fn a_part_that_was_never_written_reads_as_blank() {
        let mut fake = Fake::blank();
        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Blank);
        assert!(store.networks().is_empty());
    }

    #[test]
    fn a_saved_network_is_still_there_after_a_reset() {
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        assert_eq!(
            store.remember(&Fake::network("neighbour", "hunter2")),
            Ok(false)
        );

        // A reset: the bytes stay, everything the store held in RAM does not.
        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Loaded);
        assert_eq!(store.networks().len(), 1);
        assert_eq!(
            store.networks().get(0),
            Some(Fake::network("neighbour", "hunter2"))
        );
        assert_eq!(store.networks().get(1), None);
    }

    #[test]
    fn the_list_keeps_its_order_and_replaces_a_name_in_place() {
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        for index in 0..MAX_SAVED_NETWORKS {
            assert_eq!(
                store.remember(&Fake::network(&format!("net{index}"), "p")),
                Ok(false)
            );
        }
        // The same name again: replaced rather than an eleventh entry, and moved to the
        // front, which is where a reconnect looks first.
        assert_eq!(store.remember(&Fake::network("net3", "changed")), Ok(true));

        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().len(), MAX_SAVED_NETWORKS);
        assert_eq!(
            store.networks().get(0),
            Some(Fake::network("net3", "changed"))
        );
        assert_eq!(
            store.networks().get(MAX_SAVED_NETWORKS - 1),
            Some(Fake::network("net0", "p"))
        );
    }

    #[test]
    fn the_newest_of_two_valid_records_is_the_one_that_loads() {
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        for name in ["one", "two", "three"] {
            store.remember(&Fake::network(name, "p")).expect("writable");
        }
        // Three saves over two slots leave both of them holding a valid record, which is
        // what the sequence number is for: without it the store would have no way to say
        // which of the two was the later one.
        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Loaded);
        assert_eq!(store.networks().get(0), Some(Fake::network("three", "p")));
        assert_eq!(store.networks().get(1), Some(Fake::network("two", "p")));
    }

    #[test]
    fn a_save_that_lost_power_keeps_the_list_from_before_it() {
        let mut fake = Fake::blank();
        {
            let (mut store, _) = Store::open(&mut fake);
            store
                .remember(&Fake::network("one", "p"))
                .expect("writable");
            store
                .remember(&Fake::network("two", "p"))
                .expect("writable");
        }

        // The next save erases its slot and then loses power: what was in that slot is
        // gone, and the record from before the save is what is left to answer.
        fake.torn = true;
        {
            let (mut store, _) = Store::open(&mut fake);
            assert_eq!(
                store.remember(&Fake::network("three", "p")),
                Err(RegionError::Refused)
            );
        }
        fake.torn = false;

        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Loaded);
        assert_eq!(store.networks().get(0), Some(Fake::network("two", "p")));
        assert_eq!(store.networks().get(1), Some(Fake::network("one", "p")));
        assert_eq!(store.networks().get(2), None);
    }

    #[test]
    fn a_record_whose_tail_never_landed_is_not_a_record() {
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        store
            .remember(&Fake::network("halfwritten", "p"))
            .expect("writable");

        // The checksum is the last thing written and the first thing missed: a save that
        // lost power part way leaves a header that reads and a tail that is still erased.
        let written = HEADER_BYTES + ENTRY_BYTES + CRC_BYTES;
        fake.bytes[written - CRC_BYTES..written].fill(0xFF);

        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Blank);
        assert!(store.networks().is_empty());
    }

    #[test]
    fn a_part_that_will_not_answer_is_an_empty_store_that_says_which() {
        let mut fake = Fake::blank();
        fake.dead = true;
        let (mut store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Unreadable(RegionError::Refused));
        assert!(store.networks().is_empty());
        // The session keeps the credential it was handed and only the next boot's copy is
        // lost, so this is a reason to report rather than a device that stops.
        assert_eq!(
            store.remember(&Fake::network("nowhere", "p")),
            Err(RegionError::Refused)
        );
    }

    #[test]
    fn a_forgotten_network_is_gone_after_a_reset_and_the_rest_keeps_its_order() {
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        for name in ["one", "two", "three"] {
            store.remember(&Fake::network(name, "p")).expect("writable");
        }
        // Newest at the front, so the middle of the list is the middle of the three.
        assert_eq!(store.forget(&Ssid::from_octets(b"two")), Ok(true));

        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Loaded);
        assert_eq!(store.networks().len(), 2);
        assert_eq!(store.networks().get(0), Some(Fake::network("three", "p")));
        assert_eq!(store.networks().get(1), Some(Fake::network("one", "p")));
        assert_eq!(store.networks().get(2), None);
    }

    #[test]
    fn forgetting_a_name_this_device_does_not_hold_writes_nothing() {
        let mut fake = Fake::blank();
        {
            let (mut store, _) = Store::open(&mut fake);
            store
                .remember(&Fake::network("one", "p"))
                .expect("writable");
        }
        let after_the_save = fake.bytes;
        {
            let (mut store, _) = Store::open(&mut fake);
            assert_eq!(store.forget(&Ssid::from_octets(b"other")), Ok(false));
            assert_eq!(
                store.networks().len(),
                1,
                "a name it does not hold is not a forget"
            );
        }
        assert_eq!(
            fake.bytes, after_the_save,
            "and it is not a write either: nothing was erased to say so"
        );
    }

    #[test]
    fn a_forget_that_lost_power_keeps_the_name_it_was_erasing() {
        // A forget is a save of a shorter list, so it is the same two slots and the same torn
        // write: the record from before it survives, which for a forget is the list *with* the
        // name — an erase that lost power did not happen.
        let mut fake = Fake::blank();
        {
            let (mut store, _) = Store::open(&mut fake);
            for name in ["one", "two"] {
                store.remember(&Fake::network(name, "p")).expect("writable");
            }
        }
        fake.torn = true;
        {
            let (mut store, _) = Store::open(&mut fake);
            assert_eq!(
                store.forget(&Ssid::from_octets(b"two")),
                Err(RegionError::Refused)
            );
        }
        fake.torn = false;

        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().len(), 2);
        assert_eq!(store.networks().get(0), Some(Fake::network("two", "p")));
    }

    #[test]
    fn the_keys_are_the_names_and_not_what_joins_them() {
        // What a snapshot carries is what the page marks a row with, so a passphrase that came
        // along would be a secret with a reader that has no use for one. The page's own copy of
        // this answer is a mark per row of the list it is showing, which is [`SavedNetworks::mask`].
        let mut networks = SavedNetworks::empty();
        networks.remember(&Fake::network("one", "secret"));
        networks.remember(&Fake::network("two", "other"));

        assert!(networks.holds(&Ssid::from_octets(b"one")));
        assert!(networks.holds(&Ssid::from_octets(b"two")));
        assert!(!networks.holds(&Ssid::from_octets(b"three")));
        assert!(!SavedNetworks::empty().holds(&Ssid::from_octets(b"one")));
    }

    #[test]
    fn the_widest_name_and_the_longest_passphrase_round_trip() {
        // Both ends of an entry are sized from the radio's own limits, so this is where an
        // off-by-one in the layout would land.
        let credentials = Fake::network(&"s".repeat(SSID_BYTES), &"p".repeat(PASSWORD_BYTES - 1));
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        store.remember(&credentials).expect("writable");
        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().get(0), Some(credentials));
    }

    #[test]
    fn a_network_with_no_passphrase_round_trips() {
        // An open access point is a credential with nothing in it rather than a missing
        // one, and a zero length that read back as a name would be the same bug as a
        // truncated one in the other direction.
        let credentials = Fake::network("cafe", "");
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        store.remember(&credentials).expect("writable");
        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().get(0), Some(credentials));
    }
}

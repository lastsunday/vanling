//! Credentials at rest: what the saved-network list is written as, and the rule that picks
//! between the two slots it is written in.

use crate::drivers::network::{
    Credentials, LinkHint, MAX_SAVED_NETWORKS, PASSWORD_BYTES, Password, SSID_BYTES, SavedNetworks,
    Ssid,
};

/// The erase granularity of the parts this product is built for, and so the size of one
/// slot: a slot is written by erasing it whole and writing the record, never in place.
pub const SLOT_BYTES: u32 = 4096;

/// How many slots the store keeps.
pub const SLOTS: usize = 2;

/// The whole region a store needs, which is what its partition has to be.
pub const REGION_BYTES: u32 = SLOT_BYTES * SLOTS as u32;

const ENTRY_BYTES: usize = 1 + SSID_BYTES + 1 + PASSWORD_BYTES + 2 + 8;

const HEADER_BYTES: usize = 12;

const CRC_BYTES: usize = 4;

const MAGIC: [u8; 4] = *b"VLNC";

const VERSION: u8 = 2;

/// The most a record can occupy.
pub const RECORD_BYTES: usize = HEADER_BYTES + MAX_SAVED_NETWORKS * ENTRY_BYTES + CRC_BYTES;

const _: () = assert!(ENTRY_BYTES.is_multiple_of(4));
const _: () = assert!(RECORD_BYTES.is_multiple_of(4));
const _: () = assert!(RECORD_BYTES as u32 <= SLOT_BYTES);

/// A region of flash the store can keep its records in: whole sectors, erased before they
/// are written, addressed from the region's own start.
pub trait FlashRegion {
    fn read(&mut self, offset: u32, into: &mut [u8]) -> Result<(), RegionError>;

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), RegionError>;

    fn erase(&mut self, offset: u32, bytes: u32) -> Result<(), RegionError>;
}

/// Why a region would not do what it was asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionError {
    Refused,
    Misaligned,
    OutOfRange,
}

/// What opening a store found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opened {
    Loaded,
    Blank,
    Unreadable(RegionError),
}

/// The saved-network list, and the slot it was last written to.
pub struct Store<R> {
    region: R,
    networks: SavedNetworks,
    sequence: u32,
    next: usize,
}

impl<R: FlashRegion> Store<R> {
    /// Read the region and keep the newest record in it.
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
    pub fn remember(&mut self, credentials: &Credentials) -> Result<bool, RegionError> {
        let replaced = self.networks.remember(credentials);
        self.write_out()?;
        Ok(replaced)
    }

    /// Add a network that has just been associated with, recording where it was found in
    /// the same write rather than writing the list twice.
    pub fn remember_with_hint(
        &mut self,
        credentials: &Credentials,
        hint: LinkHint,
    ) -> Result<bool, RegionError> {
        let replaced = self.networks.remember(credentials);
        if hint.is_known() {
            self.networks.set_hint(&credentials.ssid, hint);
        }
        self.write_out()?;
        Ok(replaced)
    }

    /// Record where a saved network was found.
    pub fn learn(&mut self, ssid: &Ssid, hint: LinkHint) -> Result<bool, RegionError> {
        if !self.networks.set_hint(ssid, hint) {
            return Ok(false);
        }
        self.write_out()?;
        Ok(true)
    }

    /// Drop every saved network with this name and write the whole list to the other slot.
    pub fn forget(&mut self, ssid: &Ssid) -> Result<bool, RegionError> {
        if !self.networks.forget_named(ssid) {
            return Ok(false);
        }
        self.write_out()?;
        Ok(true)
    }

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

fn read_slot<R: FlashRegion>(
    region: &mut R,
    slot: usize,
) -> Result<Option<(u32, SavedNetworks)>, RegionError> {
    let at = slot as u32 * SLOT_BYTES;
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

fn encode(networks: &SavedNetworks, sequence: u32, record: &mut [u8; RECORD_BYTES]) -> usize {
    let body = networks.len() * ENTRY_BYTES;
    record[..MAGIC.len()].copy_from_slice(&MAGIC);
    record[4] = VERSION;
    record[5] = networks.len() as u8;
    record[6..8].copy_from_slice(&(body as u16).to_le_bytes());
    record[8..12].copy_from_slice(&sequence.to_le_bytes());
    for index in 0..networks.len() {
        let Some(credentials) = networks.get(index) else {
            continue;
        };
        let hint = networks.hint(index);
        let entry = HEADER_BYTES + index * ENTRY_BYTES;
        let ssid = credentials.ssid.as_bytes();
        let password = credentials.password.as_bytes();
        record[entry] = ssid.len() as u8;
        record[entry + 1..entry + 1 + ssid.len()].copy_from_slice(ssid);
        record[entry + 1 + SSID_BYTES] = password.len() as u8;
        record[entry + 2 + SSID_BYTES..entry + 2 + SSID_BYTES + password.len()]
            .copy_from_slice(password);
        let hint_at = entry + 2 + SSID_BYTES + PASSWORD_BYTES;
        record[hint_at..hint_at + 6].copy_from_slice(&hint.bssid);
        record[hint_at + 6] = hint.channel;
    }
    let end = HEADER_BYTES + body;
    let checksum = crc32(&record[..end]).to_le_bytes();
    record[end..end + CRC_BYTES].copy_from_slice(&checksum);
    end + CRC_BYTES
}

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
        let hint_at = entry + 2 + SSID_BYTES + PASSWORD_BYTES;
        let hint = LinkHint::new(
            record[hint_at..hint_at + 6].try_into().ok()?,
            record[hint_at + 6],
        );
        networks.place(
            index,
            Ssid::from_octets(&record[entry + 1..entry + 1 + ssid_len]),
            Password::from_bytes(
                &record[entry + 2 + SSID_BYTES..entry + 2 + SSID_BYTES + password_len],
            ),
            if hint.is_known() {
                hint
            } else {
                LinkHint::NONE
            },
        );
    }
    Some((sequence, networks))
}

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

    struct Fake {
        bytes: [u8; REGION_BYTES as usize],
        dead: bool,
        torn: bool,
    }

    impl Fake {
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
    fn a_hint_survives_a_reset_and_a_retyped_password() {
        let mut fake = Fake::blank();
        let ssid = Fake::network("neighbour", "hunter2").ssid;
        let hint = LinkHint::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x07], 6);
        {
            let (mut store, _) = Store::open(&mut fake);
            store
                .remember_with_hint(&Fake::network("neighbour", "hunter2"), hint)
                .expect("writable");
        }

        let (mut store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Loaded);
        assert_eq!(store.networks().hint_of(&ssid), hint);

        store
            .remember(&Fake::network("neighbour", "other"))
            .expect("writable");
        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().hint_of(&ssid), hint);
        assert_eq!(
            store.networks().get(0),
            Some(Fake::network("neighbour", "other"))
        );
    }

    #[test]
    fn learning_a_channel_writes_once_and_only_when_it_moves() {
        let mut fake = Fake::blank();
        let network = Fake::network("neighbour", "hunter2");
        let first = LinkHint::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x07], 6);
        let second = LinkHint::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x08], 11);
        let (mut store, _) = Store::open(&mut fake);
        store.remember(&network).expect("writable");
        assert_eq!(store.networks().hint_of(&network.ssid), LinkHint::NONE);

        assert_eq!(store.learn(&network.ssid, first), Ok(true));
        assert_eq!(
            store.learn(&network.ssid, first),
            Ok(false),
            "the same access point on the same channel is not a new write"
        );
        assert_eq!(store.learn(&network.ssid, second), Ok(true));
        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().hint_of(&network.ssid), second);
    }

    #[test]
    fn a_version_one_record_is_read_as_nothing() {
        let mut fake = Fake::blank();
        let network = Fake::network("neighbour", "hunter2");
        let mut networks = SavedNetworks::empty();
        networks.remember(&network);
        let mut record = [0u8; RECORD_BYTES];
        let written = encode(&networks, 1, &mut record);
        record[4] = 1;
        let end = HEADER_BYTES + networks.len() * ENTRY_BYTES;
        let checksum = crc32(&record[..end]).to_le_bytes();
        record[end..end + CRC_BYTES].copy_from_slice(&checksum);
        fake.bytes[..written].copy_from_slice(&record[..written]);

        let (store, opened) = Store::open(&mut fake);
        assert_eq!(opened, Opened::Blank);
        assert!(store.networks().is_empty());
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
        let credentials = Fake::network(&"s".repeat(SSID_BYTES), &"p".repeat(PASSWORD_BYTES - 1));
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        store.remember(&credentials).expect("writable");
        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().get(0), Some(credentials));
    }

    #[test]
    fn a_network_with_no_passphrase_round_trips() {
        let credentials = Fake::network("cafe", "");
        let mut fake = Fake::blank();
        let (mut store, _) = Store::open(&mut fake);
        store.remember(&credentials).expect("writable");
        let (store, _) = Store::open(&mut fake);
        assert_eq!(store.networks().get(0), Some(credentials));
    }
}

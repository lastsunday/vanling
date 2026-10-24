//! The saved networks, as this board's flash holds them.
//!
//! The record format and the slot rule are core's (`core/src/drivers/credentials.rs`); what
//! is here is the part that needs a part: the partition table and the three flash calls a
//! region makes. The store is opened at bring-up rather than at the first save, because a
//! partition that cannot be read is something the boot has to say — a device whose saves go
//! nowhere looks exactly like one that has never been set up.
//!
//! `@/records/iot/wifi.md`.

use esp_bootloader_esp_idf::partitions::{PARTITION_TABLE_MAX_LEN, read_partition_table};
use esp_storage::{FlashStorage, FlashStorageError};
use iot_core::drivers::credentials::{FlashRegion, Opened, RegionError, Store};

/// The label the partition table gives the saved networks; the tables are
/// `apps/iot/partitions/`.
const PARTITION_LABEL: &str = "creds";

/// The store over this board's credential partition.
pub type CredentialStore = Store<CredsRegion<'static>>;

/// One partition of flash, addressed from its own start.
///
/// The store counts offsets from zero inside its own region, so every address here is the
/// partition's own offset plus what the store asked for, and every range is checked against
/// the partition's length rather than the part's.
pub struct CredsRegion<'d> {
    flash: FlashStorage<'d>,
    base: u32,
    len: u32,
}

/// Read the partition table and open the store in the partition it names.
///
/// `None` when this board's table has no `creds` partition, or the table itself will not
/// read: a device that cannot keep a network is still a device that can join one, and the
/// page is where that difference shows.
pub fn load(mut flash: FlashStorage<'static>) -> Option<CredentialStore> {
    // The table is read into this frame rather than kept: what the store needs of an entry is
    // where the partition is and how long it is, and both are plain numbers by the time the
    // borrow of the buffer ends.
    let mut table = [0u8; PARTITION_TABLE_MAX_LEN];
    let entry = match read_partition_table(&mut flash, &mut table) {
        Ok(table) => table
            .iter()
            .find(|entry| entry.label_as_str() == PARTITION_LABEL)
            .map(|entry| (entry.offset(), entry.len())),
        Err(error) => {
            log::error!("[CREDS] this part's partition table would not read: {error:?}");
            None
        }
    };
    let Some((base, len)) = entry else {
        log::warn!(
            "[CREDS] this board's table has no `{PARTITION_LABEL}` partition, so no network \
             will be kept"
        );
        return None;
    };

    let (store, opened) = Store::open(CredsRegion { flash, base, len });
    match opened {
        Opened::Loaded => {
            let count = store.networks().len();
            log::info!(
                "[CREDS] {count} saved network{} on this device",
                if count == 1 { "" } else { "s" }
            );
        }
        Opened::Blank => log::info!("[CREDS] no network saved on this device yet"),
        Opened::Unreadable(error) => log::warn!(
            "[CREDS] the saved networks would not read ({error:?}), so nothing will be kept"
        ),
    }
    Some(store)
}

impl FlashRegion for CredsRegion<'_> {
    fn read(&mut self, offset: u32, into: &mut [u8]) -> Result<(), RegionError> {
        let at = self.address(offset, into.len() as u32)?;
        self.flash.read(at, into).map_err(refusal)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), RegionError> {
        let at = self.address(offset, bytes.len() as u32)?;
        self.flash.write(at, bytes).map_err(refusal)
    }

    fn erase(&mut self, offset: u32, bytes: u32) -> Result<(), RegionError> {
        let at = self.address(offset, bytes)?;
        // `to` is exclusive: the part erases `[from, to)`.
        self.flash.erase(at, at + bytes).map_err(refusal)
    }
}

impl CredsRegion<'_> {
    /// Where the store's own offset lands, or why it does not land inside this region.
    fn address(&self, offset: u32, bytes: u32) -> Result<u32, RegionError> {
        if offset.checked_add(bytes).is_some_and(|end| end <= self.len) {
            Ok(self.base + offset)
        } else {
            Err(RegionError::OutOfRange)
        }
    }
}

/// What a refusal from the part means to the store above.
///
/// The two named kinds are the caller's mistakes rather than silicon's, and the store's log
/// would send the next reader to the wrong place if they came back as one more refusal.
fn refusal(error: FlashStorageError) -> RegionError {
    match error {
        FlashStorageError::NotAligned => RegionError::Misaligned,
        FlashStorageError::OutOfBounds => RegionError::OutOfRange,
        _ => RegionError::Refused,
    }
}

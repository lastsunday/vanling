//! The saved networks, as this board's flash holds them.

use esp_bootloader_esp_idf::partitions::{PARTITION_TABLE_MAX_LEN, read_partition_table};
use esp_storage::{FlashStorage, FlashStorageError};
use iot_core::drivers::credentials::{FlashRegion, Opened, RegionError, Store};

const PARTITION_LABEL: &str = "creds";

/// The store over this board's credential partition.
pub type CredentialStore = Store<CredentialRegion<'static>>;

/// One partition of flash, addressed from its own start.
pub struct CredentialRegion<'d> {
    flash: FlashStorage<'d>,
    base: u32,
    len: u32,
}

/// Read the partition table and open the store in the partition it names.
pub fn load(mut flash: FlashStorage<'static>) -> Option<CredentialStore> {
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

    let (store, opened) = Store::open(CredentialRegion { flash, base, len });
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

/// Open the saved networks straight from the flash peripheral.
///
/// Safe mode does not construct a `Board`, so it cannot take the store the board would have
/// loaded. The S3 keeps its multicore parking here for the same reason the board does: a flash
/// call while the other core runs is not safe on that part.
pub fn load_from_peripherals(
    flash: esp_hal::peripherals::FLASH<'static>,
) -> Option<CredentialStore> {
    let flash = FlashStorage::new(flash);
    #[cfg(feature = "lckfb-szpi-esp32s3")]
    let flash = flash.multicore_auto_park();
    load(flash)
}

impl FlashRegion for CredentialRegion<'_> {
    fn read(&mut self, offset: u32, into: &mut [u8]) -> Result<(), RegionError> {
        let at = self.address(offset, into.len() as u32)?;
        self.flash.read(at, into).map_err(refusal)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), RegionError> {
        let at = self.address(offset, bytes.len() as u32)?;
        iot_chip_esp::with_other_core_parked(|| self.flash.write(at, bytes)).map_err(refusal)
    }

    fn erase(&mut self, offset: u32, bytes: u32) -> Result<(), RegionError> {
        let at = self.address(offset, bytes)?;
        iot_chip_esp::with_other_core_parked(|| self.flash.erase(at, at + bytes)).map_err(refusal)
    }
}

impl CredentialRegion<'_> {
    fn address(&self, offset: u32, bytes: u32) -> Result<u32, RegionError> {
        if offset.checked_add(bytes).is_some_and(|end| end <= self.len) {
            Ok(self.base + offset)
        } else {
            Err(RegionError::OutOfRange)
        }
    }
}

fn refusal(error: FlashStorageError) -> RegionError {
    match error {
        FlashStorageError::NotAligned => RegionError::Misaligned,
        FlashStorageError::OutOfBounds => RegionError::OutOfRange,
        _ => RegionError::Refused,
    }
}

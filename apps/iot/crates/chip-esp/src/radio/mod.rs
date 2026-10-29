//! The heap the Wi-Fi driver links against, where that memory comes from, and the register
//! state the driver expects to find before it starts.

use esp_alloc::MemoryCapability;

#[cfg(feature = "esp32c6")]
mod esp32c6;
#[cfg(feature = "esp32s3")]
mod esp32s3;

#[cfg(feature = "esp32c6")]
use esp32c6::{TOTAL_BYTES, add_regions};
#[cfg(feature = "esp32s3")]
use esp32s3::{TOTAL_BYTES, add_regions};

#[cfg(feature = "esp32c6")]
pub use esp32c6::ungate_radio;
#[cfg(feature = "esp32s3")]
pub use esp32s3::ungate_radio;

/// Hand the driver's heap to `esp-alloc`.
pub fn init_radio_heap() {
    let (dram_seg, reclaimed) = add_regions();
    match reclaimed {
        Some(reclaimed) => log::info!(
            "[WIFI] driver heap: {dram_seg} B in dram_seg + {reclaimed} B above it \
             (dcache-reclaimed), both internal"
        ),
        None => log::info!("[WIFI] driver heap: {dram_seg} B in dram_seg, internal"),
    }
}

/// What the driver's heap has left, for the report that decides whether this build's memory
/// budget was right.
pub fn radio_heap_free() -> usize {
    esp_alloc::HEAP.free_caps(MemoryCapability::Internal.into())
}

/// What the driver's heap was given, so a free-bytes line can be read as a fraction.
pub const fn radio_heap_total() -> usize {
    TOTAL_BYTES
}

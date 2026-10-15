//! The heap the Wi-Fi driver links against, where that memory comes from, and the register
//! state the driver expects to find before it starts.
//!
//! The driver is a closed blob that allocates through `malloc`, so the symbols have to exist
//! in the final binary. `esp-alloc` provides them; this module is the memory behind them, and
//! *that* is the whole difficulty here — this board's internal DRAM is spoken for.
//!
//! **Both regions are `MemoryCapability::Internal`**, on every part: the driver's pbuf pools
//! go to the MAC's DMA, which cannot reach PSRAM, so an external region would satisfy none of
//! the requests that matter.
//!
//! **Whether there are two regions is per part.** The one above `dram_seg` (0x3FCF0000) is
//! DRAM the second-stage bootloader used for its flash cache, which esp-hal's linker leaves
//! to the heap ("not used as static memory, leaving to the heap") — but only the parts whose
//! data cache can be reclaimed have a macro that can place a static there, which today is the
//! S3 alone, and it is only there once the cache is halved to buy 32 KiB back
//! (`scripts/iot-xtensa.sh`). A part with neither gets the whole budget out of `dram_seg`,
//! and that difference is the part's own arm: one file per part, beside this one.
//!
//! What each part is left with after this is in `@/records/iot/wifi.md`.

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

/// Hand the driver's heap to `esp-alloc`. Call once, before the radio starts, and from
/// whichever entry point owns the runtime.
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
///
/// `NoMemory` from the driver is the symptom this is meant to catch before the bench does, so
/// it is a number the log can carry rather than a guess.
pub fn radio_heap_free() -> usize {
    esp_alloc::HEAP.free_caps(MemoryCapability::Internal.into())
}

/// What the driver's heap was given, so a free-bytes line can be read as a fraction. Both
/// numbers come from here because a report that prints one and re-derives the other
/// somewhere else is a report that can disagree with itself.
pub const fn radio_heap_total() -> usize {
    TOTAL_BYTES
}

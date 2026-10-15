//! The heap the Wi-Fi driver links against, and where its memory comes from.
//!
//! The driver is a closed blob that allocates through `malloc`, so the symbols have to
//! exist in the final binary. `esp-alloc` provides them; this module is the memory behind
//! them, and *that* is the whole difficulty here — this board's internal DRAM is spoken for.
//!
//! Two regions, both `MemoryCapability::Internal`: the driver's pbuf pools go to the MAC's
//! DMA, which cannot reach PSRAM.
//!
//! * **Above `dram_seg`** (0x3FCF0000): DRAM the second-stage bootloader used for its
//!   flash cache, which esp-hal's linker leaves to the heap ("not used as static memory,
//! leaving to the heap"). Its size is 64 KiB minus the data cache, so it is empty at the
//!   default cache — `scripts/iot-xtensa.sh` halves that to buy 32 KiB back.
//! * **In `dram_seg`**: affordable only because the app's own heap moved to PSRAM, which
//!   took the main stack from 63,788 B (1,748 B under the asserted floor) to 125,788 B with
//!   39,809 B in use.
//!
//! What each half is worth when the driver asks is in `@/records/iot/wifi.md`.

use esp_alloc::{HeapRegion, MemoryCapability};
use esp_hal::ram;

/// The whole of what a 32 KB data cache leaves above `dram_seg`.
const RECLAIMED_BYTES: usize = 32 * 1024;

/// Half of the driver's heap, in `dram_seg`, bought with the main stack's measured
/// headroom. Half rather than all of it because the stack is the one thing on this part
/// whose peak is instrumented, so giving away 32 KiB of it is a measurement rather than a
/// gamble — and the other half stays where nothing has to be traded for it.
const INTERNAL_BYTES: usize = 32 * 1024;

/// The space the bootloader's flash cache gave back.
///
/// `unstable(dcache_reclaimed)`, **not** `reclaimed`, and the two are not neighbours:
/// `reclaimed` places a static in `.dram2_uninit`, which lands in `dram2_seg` — the region
/// this repository's main-stack script reaches into — so a region placed there pushes the
/// stack out and the link fails with "cannot move location counter backwards".
///
/// Uninitialised rather than zeroed, for the same reason: the loader does not write that
/// region, so `[0; N]` would claim a zeroing that never happens and leave the driver reading
/// whatever the bootloader left there.
#[ram(unstable(dcache_reclaimed))]
static mut RECLAIMED_MEM: [core::mem::MaybeUninit<u8>; RECLAIMED_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; RECLAIMED_BYTES];

/// The other half, in `dram_seg` like any other static, because that is the only place
/// left on this part that is internal DRAM and not already spoken for.
static mut INTERNAL_MEM: [core::mem::MaybeUninit<u8>; INTERNAL_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; INTERNAL_BYTES];

/// Hand the driver's heap to `esp-alloc`. Call once, before the radio starts, and from
/// whichever entry point owns the runtime.
pub fn init_radio_heap() {
    // SAFETY: called exactly once, before anything allocates — each region is a private
    // static that nothing else names, and `esp-alloc`'s heap is the one the driver's
    // `malloc` resolves to. Their contents are uninitialised on purpose, which is what the
    // allocator is for.
    unsafe {
        esp_alloc::HEAP.add_region(HeapRegion::new(
            core::ptr::addr_of_mut!(INTERNAL_MEM) as *mut u8,
            INTERNAL_BYTES,
            MemoryCapability::Internal.into(),
        ));
        esp_alloc::HEAP.add_region(HeapRegion::new(
            core::ptr::addr_of_mut!(RECLAIMED_MEM) as *mut u8,
            RECLAIMED_BYTES,
            MemoryCapability::Internal.into(),
        ));
    }
    log::info!(
        "[WIFI] driver heap: {INTERNAL_BYTES} B in dram_seg + {RECLAIMED_BYTES} B above it \
         (dcache-reclaimed), both internal"
    );
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
    INTERNAL_BYTES + RECLAIMED_BYTES
}

use esp_alloc::{HeapRegion, MemoryCapability};

/// The driver's share of `dram_seg`, the only internal DRAM left that nothing has to be
/// traded for.
///
/// **48 KiB, the whole documented need**: this is the one part with no reclaimed region to
/// make up a difference, so this budget is the whole of it.
const INTERNAL_BYTES: usize = 48 * 1024;

/// Internal DRAM out of `dram_seg`, a plain static: the only place left on this part that
/// nothing else has spoken for.
static mut INTERNAL_MEM: [core::mem::MaybeUninit<u8>; INTERNAL_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; INTERNAL_BYTES];

/// What this part's one region adds up to, for [`super::radio_heap_total`].
pub(crate) const TOTAL_BYTES: usize = INTERNAL_BYTES;

/// Hands this part's one region to `esp-alloc` and reports what it was.
pub(crate) fn add_regions() -> (usize, Option<usize>) {
    // SAFETY: called exactly once, before anything allocates — the region is a private static
    // that nothing else names, and `esp-alloc`'s heap is the one the driver's `malloc`
    // resolves to. Its contents are uninitialised on purpose, which is what the allocator is
    // for.
    unsafe {
        esp_alloc::HEAP.add_region(HeapRegion::new(
            core::ptr::addr_of_mut!(INTERNAL_MEM) as *mut u8,
            INTERNAL_BYTES,
            MemoryCapability::Internal.into(),
        ));
    }
    (INTERNAL_BYTES, None)
}

/// Nothing to do on this part, and that is the whole arm.
///
/// esp-radio's `init()` on the C6 is three calls that are all empty here: `init_clocks()`
/// says "done in esp-hal", `reset_wifi_mac()` is empty, and `enable_wifi_power_domain()`
/// is `#[cfg(not(any(soc_has_pmu, esp32c2)))]` while this part has `soc_has_pmu` and no
/// `APB_CTRL` to write. The clock ungating that *is* left — `MODEM_SYSCON`/`MODEM_LPCON` —
/// the driver does itself when it starts (`os_adapter` calls `enable_wifi(true)`).
pub fn ungate_radio() {}

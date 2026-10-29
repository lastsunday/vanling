use esp_alloc::{HeapRegion, MemoryCapability};

const INTERNAL_BYTES: usize = 48 * 1024;

static mut INTERNAL_MEM: [core::mem::MaybeUninit<u8>; INTERNAL_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; INTERNAL_BYTES];

pub(crate) const TOTAL_BYTES: usize = INTERNAL_BYTES;

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

/// Nothing to do on this part, and that is the whole arm.  esp-radio's `init()` on the C6
/// is three calls that are all empty here: `init_clocks()` says "done in esp-hal",
/// `reset_wifi_mac()` is empty, and `enable_wifi_power_domain()` is
/// `#[cfg(not(any(soc_has_pmu, esp32c2)))]` while this part has `soc_has_pmu` and no
/// `APB_CTRL` to write.
pub fn ungate_radio() {}

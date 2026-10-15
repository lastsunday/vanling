use super::Window;

/// What internal DRAM this part offers a general heap, as a static.
///
/// A static rather than a window into something bigger because this part has no external
/// memory to map, which leaves the app's own allocations competing with the driver's heap and
/// the main stack for internal DRAM. `@/records/iot/wifi.md` has what is left over.
pub(crate) fn window() -> (Window, &'static str) {
    (
        Window {
            start: core::ptr::addr_of_mut!(DRAM_HEAP) as usize,
            len: DRAM_HEAP_BYTES,
        },
        "internal DRAM",
    )
}

/// Uninitialised rather than zeroed: the loader does not write this region, so `[0; N]`
/// would claim a zeroing that never happens.
static mut DRAM_HEAP: [core::mem::MaybeUninit<u8>; DRAM_HEAP_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; DRAM_HEAP_BYTES];

const DRAM_HEAP_BYTES: usize = 64 * 1024;

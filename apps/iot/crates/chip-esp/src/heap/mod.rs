//! Where this part's general-purpose heap comes from.

use core::sync::atomic::{AtomicPtr, Ordering};

use embedded_alloc::LlffHeap as Heap;

#[cfg(feature = "esp32c6")]
mod esp32c6;
#[cfg(feature = "esp32s3")]
mod esp32s3;

/// The general-purpose heap a product image starts with: the renderer registry, device state
/// and driver buffers live here, while the frame buffer is the board's own static, shared with
/// the camera DMA. The S3's is PSRAM and the C6's internal DRAM
/// (`@/records/iot/hardware-constraints.md`).
#[cfg(feature = "esp32s3")]
pub const PRODUCT_HEAP_BYTES: usize = 96 * 1024;
#[cfg(feature = "esp32c6")]
pub const PRODUCT_HEAP_BYTES: usize = 64 * 1024;

#[cfg(feature = "esp32c6")]
use esp32c6::window;
#[cfg(feature = "esp32s3")]
use esp32s3::window;

static HEAP: AtomicPtr<Heap> = AtomicPtr::new(core::ptr::null_mut());

#[derive(Clone, Copy, Debug)]
pub(crate) struct Window {
    pub(crate) start: usize,
    pub(crate) len: usize,
}

/// Give `allocator` this part's memory for `bytes` of a general-purpose heap.
pub fn init_heap(allocator: &Heap, bytes: usize) {
    let (window, source) = window();
    if bytes > window.len {
        log::error!(
            "[HEAP] asked for {bytes} B of a {}-byte {source} window, resetting",
            window.len
        );
        esp_hal::system::software_reset();
    }
    // SAFETY: called exactly once, before any allocation. The window is either a static this
    // module owns or memory `chip_init_at` mapped and read a pattern back through, and
    // nothing else is handed it — the allocator taking it over is this call.
    unsafe { allocator.init(window.start, bytes) };
    HEAP.store(core::ptr::from_ref(allocator).cast_mut(), Ordering::Release);
    log::info!(
        "[HEAP] {bytes} B at {:#x} in {source}, of a {}-byte window",
        window.start,
        window.len
    );
}

/// What is left of the general-purpose heap.
pub fn heap_free() -> usize {
    let heap = HEAP.load(Ordering::Acquire);
    if heap.is_null() {
        return 0;
    }
    // SAFETY: the pointer is the one `init_heap` was handed, which its owner keeps alive for
    // the life of the program. `free` reads the allocator's own bookkeeping and nothing here
    // writes through this pointer at all.
    unsafe { (*heap).free() }
}

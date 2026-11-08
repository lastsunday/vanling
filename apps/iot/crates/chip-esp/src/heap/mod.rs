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
///
/// The S3's number is a slice of the 8 MiB PSRAM window, so it costs no internal DRAM: it is
/// sized well past the boot peak on purpose, because the earlier 96 KiB left ~1.6 KB free and
/// turned the next `Box` into an allocation failure rather than a slow frame.
#[cfg(feature = "esp32s3")]
pub const PRODUCT_HEAP_BYTES: usize = 512 * 1024;
#[cfg(feature = "esp32c6")]
pub const PRODUCT_HEAP_BYTES: usize = 64 * 1024;

#[cfg(feature = "esp32c6")]
use esp32c6::try_window;
#[cfg(feature = "esp32s3")]
use esp32s3::try_window;

static HEAP: AtomicPtr<Heap> = AtomicPtr::new(core::ptr::null_mut());

#[derive(Clone, Copy, Debug)]
pub(crate) struct Window {
    pub(crate) start: usize,
    pub(crate) len: usize,
}

/// Why the product's general-purpose heap was not handed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeapError {
    /// This part maps no window at all: the S3's PSRAM is absent or did not read back.
    Unavailable { source: &'static str },
    /// The window exists, but it is smaller than the product asked for.
    TooLarge {
        asked: usize,
        available: usize,
        source: &'static str,
    },
}

/// Give `allocator` this part's memory for `bytes` of a general-purpose heap.
///
/// Returns the reason instead of resetting, so safe mode can keep its serial console after a
/// heap window this boot cannot use. The product path is [`init_heap`], which resets.
pub fn try_init_heap(allocator: &Heap, bytes: usize) -> Result<(), HeapError> {
    let (window, source) = try_window()?;
    if bytes > window.len {
        return Err(HeapError::TooLarge {
            asked: bytes,
            available: window.len,
            source,
        });
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
    Ok(())
}

/// Give `allocator` this part's memory for `bytes`, or reset the part.
pub fn init_heap(allocator: &Heap, bytes: usize) {
    if let Err(error) = try_init_heap(allocator, bytes) {
        log::error!("[HEAP] {error:?}, resetting");
        crate::crashed_reset();
    }
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

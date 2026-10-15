//! Where this part's general-purpose heap comes from.
//!
//! One call, because where the memory lives is a per-part fact and a caller that has to know
//! it does not survive the second chip: on the S3 it is PSRAM that has to be mapped and read
//! back before anything may point an allocator at it, on the C6 it is a static in internal
//! DRAM. The part's own arm is where that difference lives — one file per part, beside this
//! one — and it travels no further than the log line, so a reader can tell which part a
//! memory number came from and nothing else has to know.
//!
//! What is *not* here is the size: how much a build allocates is its own business, and two
//! parts with different budgets are better served by callers that say what they need than by
//! one number that fits whichever part happens to compile.

use embedded_alloc::Heap;

#[cfg(feature = "esp32c6")]
mod esp32c6;
#[cfg(feature = "esp32s3")]
mod esp32s3;

#[cfg(feature = "esp32c6")]
use esp32c6::window;
#[cfg(feature = "esp32s3")]
use esp32s3::window;

/// A window of memory an allocator may be pointed at.
///
/// Both arms hand one of these back. The name of the memory is *not* a field: it is a string
/// for the log line, so each arm returns it beside the window rather than a variant only one
/// part's build could construct.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Window {
    pub(crate) start: usize,
    pub(crate) len: usize,
}

/// Give `allocator` this part's memory for `bytes` of a general-purpose heap.
///
/// Call once, before anything allocates, and from the entry point that owns the runtime. A
/// part that cannot offer a window at all resets inside its own arm rather than being
/// reported from here: the one caller that could report it is a runtime that has nowhere to
/// put the report.
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
    log::info!(
        "[HEAP] {bytes} B at {:#x} in {source}, of a {}-byte window",
        window.start,
        window.len
    );
}

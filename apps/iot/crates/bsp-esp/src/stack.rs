//! Main-stack high-water tracking, so a deep paint path is measured, not assumed.

use core::sync::atomic::{AtomicU32, Ordering};

// Deepest main-stack use observed so far, in bytes below the stack ceiling.
// A rendering path that can walk tens of kilobytes down the stack is the kind
// of thing that overflows silently after an innocent refactor, so the peak is
// measured rather than assumed; see [`note_stack_high_water`].
static STACK_HIGH_WATER: AtomicU32 = AtomicU32::new(0);

pub(crate) fn main_stack_bytes() -> usize {
    unsafe extern "C" {
        static _stack_start: u8;
        static _stack_end: u8;
    }
    core::ptr::addr_of!(_stack_start) as usize - core::ptr::addr_of!(_stack_end) as usize
}

fn frame_address() -> usize {
    let probe = 0_u8;
    core::ptr::addr_of!(probe) as usize
}

pub(crate) fn note_stack_high_water() {
    let used = (main_stack_bytes() - (frame_address() - main_stack_floor())) as u32;
    STACK_HIGH_WATER.fetch_max(used, Ordering::Relaxed);
}

pub(crate) fn high_water() -> usize {
    STACK_HIGH_WATER.load(Ordering::Relaxed) as usize
}

fn main_stack_floor() -> usize {
    unsafe extern "C" {
        static _stack_end: u8;
    }
    core::ptr::addr_of!(_stack_end) as usize
}

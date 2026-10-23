//! Main-stack high-water tracking, so a deep paint path is measured, not assumed.
//!
//! The ceiling is the main task's stack, which every cooperative task shares; the probe is
//! called at the bottom of whichever paint path draws deepest, so the peak is that path's
//! working set rather than the whole stack's size.

use core::sync::atomic::{AtomicU32, Ordering};

/// Deepest main-stack use observed so far, in bytes below the stack ceiling.
/// A rendering path that can walk tens of kilobytes down the stack is the kind
/// of thing that overflows silently after an innocent refactor, so the peak is
/// measured rather than assumed; see [`note_stack_high_water`].
static STACK_HIGH_WATER: AtomicU32 = AtomicU32::new(0);

/// The main stack the rtos main task (and with it every cooperative task) runs
/// on: ceiling minus guarded floor. Logged at bring-up so a stack-guard trip's
/// cause — a paint that really reached the guard, versus a layout that shrank
/// the stack — reads off one boot line instead of a bisect.
pub(crate) fn main_stack_bytes() -> usize {
    unsafe extern "C" {
        static _stack_start: u8;
        static _stack_end: u8;
    }
    core::ptr::addr_of!(_stack_start) as usize - core::ptr::addr_of!(_stack_end) as usize
}

/// This frame's address on the main stack, standing in for the stack pointer.
/// A local sits at most a frame's own locals below `sp`, which is noise next to
/// the kilobytes this measures, and reading it keeps the crate on stable: the
/// honest `mov {0}, sp` needs `#![feature(asm_experimental_arch)]`, which this
/// crate cannot take because it also builds for the host.
fn frame_address() -> usize {
    let probe = 0_u8;
    core::ptr::addr_of!(probe) as usize
}

/// Records how far this frame sits below the stack ceiling, keeping the deepest
/// value seen. Call it at the bottom of the deepest call chain worth knowing
/// about: the peak is then the stack the chain needed, not the whole stack's
/// size, and headroom is the difference.
pub(crate) fn note_stack_high_water() {
    let used = (main_stack_bytes() - (frame_address() - main_stack_floor())) as u32;
    STACK_HIGH_WATER.fetch_max(used, Ordering::Relaxed);
}

/// The deepest use recorded so far, in bytes below the stack ceiling.
pub(crate) fn high_water() -> usize {
    STACK_HIGH_WATER.load(Ordering::Relaxed) as usize
}

/// The main stack's low address, the floor the ceiling is measured from.
fn main_stack_floor() -> usize {
    unsafe extern "C" {
        static _stack_end: u8;
    }
    core::ptr::addr_of!(_stack_end) as usize
}

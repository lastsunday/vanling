use super::Window;

pub(crate) fn window() -> (Window, &'static str) {
    (
        Window {
            start: core::ptr::addr_of_mut!(DRAM_HEAP) as usize,
            len: DRAM_HEAP_BYTES,
        },
        "internal DRAM",
    )
}

static mut DRAM_HEAP: [core::mem::MaybeUninit<u8>; DRAM_HEAP_BYTES] =
    [const { core::mem::MaybeUninit::uninit() }; DRAM_HEAP_BYTES];

const DRAM_HEAP_BYTES: usize = 64 * 1024;

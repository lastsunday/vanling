//! Playback isolation probe: speaker only, looping the catalogue forever.
//!
//! One future for the feed, no `DeviceManager`, no intent bus, no renderer, no capture
//! poll — so a stall reported here belongs to the transmit path rather than to the
//! shared executor the product build's three tasks contend for. It alternates the two
//! sounds because they are built differently (a synthesised pair against a stored
//! PCM), so a stall only one provokes cannot hide behind whichever was playing.
//!
//! What it runs is the product's own feed loop and the product's own counters, so a
//! number here and a number from the product describe the same schedule. What it
//! cannot catch is anything needing the interrupt executor: it feeds inline, so a
//! fault that only appears there belongs to the product build.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::Heap;

type Board = iot_bsp_esp::Board<'static>;

/// Only the speaker's own allocation: the probe never paints a panel, so it has
/// none of the frame buffer the product image reserves.
static mut HEAP_MEM: [u8; 32 * 1024] = [0; 32 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; 32 * 1024]>(),
        );
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::info!("[PROBE] audio-only build: no panel, touch, motion or capture");

    // The feed's software interrupt is dropped: this probe runs the feed
    // inline, so a fault that needs the interrupt executor is the product's to
    // find rather than this probe's.
    let (board, timg0, from_cpu_intr, _feed_intr) = match Board::new_audio_only(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[PROBE] audio bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    iot_app::run_audio_only(board).await
}

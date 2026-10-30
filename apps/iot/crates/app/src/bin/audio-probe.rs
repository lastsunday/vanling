//! Playback isolation probe: speaker only, looping the catalogue forever.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::LlffHeap as Heap;

type Board = iot_bsp_esp::Board<'static>;

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

    let (board, timg0, from_cpu_intr, _feed_intr) = match Board::new_audio_only(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[PROBE] audio bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    iot_app::bench::probe::audio::run(board).await
}

//! Echo cancellation on a real room: the speaker playing while the microphone listens, and
//! the canceller fed both.
#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::LlffHeap as Heap;

type Board = iot_bsp_esp::Board<'static>;

// The product's heap. `Board::new` brings up panel and camera whether or not this probe paints
// them: 32 KB died there with `memory allocation of 9048 bytes failed` (8 KB SPI scratch + the
// boxed camera), so the figure matches the product's.
const HEAP_BYTES: usize = 64 * 1024;

static mut HEAP_MEM: [u8; HEAP_BYTES] = [0; HEAP_BYTES];

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; HEAP_BYTES]>(),
        );
    }

    // 240 MHz as the product runs it: at 80 MHz the delay search alone busts a 30 ms poll.
    let peripherals = iot_chip_esp::chip_init_at(esp_hal::clock::CpuClock::_240MHz);
    iot_chip_esp::init_logging();
    log::info!("[ECHO] room probe: speaker and microphone together, no panel or motion");

    let (mut board, timg0, from_cpu_intr, feed_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[ECHO] bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);

    let Some(capture) = board.take_capture() else {
        log::error!("[ECHO] no microphone wired, nothing to cancel from; resetting");
        esp_hal::system::software_reset();
    };

    let feed = iot_chip_esp::start_feed_executor(feed_intr);
    iot_app::bench::probe::echo_room::run(board, capture, feed).await
}

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use iot_core::drivers::board::{HasPanel, HasWifi};
use iot_core::drivers::network::Front;

type Board = iot_bsp_esp::Board<'static>;

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

// Sized for the renderer registry, device state and driver buffers; the frame buffer is the
// board's static, shared with the camera DMA and the panel's SPI. The two parts differ because
// the S3 heap is PSRAM and the C6 heap is internal DRAM (`@/records/iot/hardware-constraints.md`).
#[cfg(feature = "esp32c6")]
const HEAP_BYTES: usize = 64 * 1024;
#[cfg(feature = "esp32s3")]
const HEAP_BYTES: usize = 96 * 1024;

#[cfg(feature = "esp32c6-devkitc-1")]
const FRONT: Front = Front::AccessPoint;
#[cfg(feature = "lckfb-szpi-esp32s3")]
const FRONT: Front = Front::Panel;

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let (mut board, feed) = boot_board();

    let station = board.take_wifi();

    let saved = board.take_credentials();
    let network = iot_app::Network::of(station.is_some());
    let panel = board.panel();

    match join(
        iot_app::run(
            board,
            iot_app::Wiring {
                clock: iot_chip_esp::Clock,
                feed,
                network,
            },
        ),
        iot_app::network::serve(station, saved, FRONT, panel),
    )
    .await {}
}

fn boot_board() -> (Board, iot_app::Feed) {
    let peripherals = iot_chip_esp::chip_init_for_product();
    iot_chip_esp::init_logging();
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);
    iot_app::network::report_heap_free(iot_chip_esp::heap_free);

    log::info!("[IOT] boot complete");

    // Bound before the call rather than matched at `start_executors`: the arm form measured
    // 1,860 B more text on the S3 for the same behaviour.
    let startup = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            iot_chip_esp::reset();
        }
    };

    start_executors(startup)
}

#[cfg(feature = "esp32s3")]
fn start_executors(startup: iot_bsp_esp::Startup<'static>) -> (Board, iot_app::Feed) {
    let (board, timg0, from_cpu_intr, feed_intr) = startup;
    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    let feed = iot_chip_esp::start_feed_executor(feed_intr);
    (board, iot_app::Feed::Interrupt(feed))
}

#[cfg(feature = "esp32c6")]
fn start_executors(startup: iot_bsp_esp::Startup<'static>) -> (Board, iot_app::Feed) {
    let (board, timg0, from_cpu_intr) = startup;
    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    (board, iot_app::Feed::Inline)
}

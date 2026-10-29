#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use iot_core::drivers::board::{HasPanel, HasWifi};
use iot_core::drivers::network::Front;

/// The board this firmware is wired for. One alias because the board features
/// are mutually exclusive, so `iot-bsp-esp` re-exports a single `Board` name
/// either way and there is nothing for two arms to disambiguate.
type Board = iot_bsp_esp::Board<'static>;

/// Heap for the pluggable renderer registry and other runtime allocation.
#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

/// Sized for the renderer registry, device state and the driver's buffers.
///
/// No longer holds the frame buffer: that is the board's static now — the camera's DMA and
/// the panel's SPI share it — so it is 150 KB out of DRAM that this used to duplicate on the
/// heap. What is left is small allocations, and the heap that fits them leaves the main stack
/// the rest. Measured against `@/records/iot/hardware-constraints.md`.
///
/// The two parts ask for different numbers because they are different memories. On the S3 the
/// heap is a slice of mapped PSRAM, and the page's six socket buffers are sized to a whole
/// segment each rather than to a window — 256 B windows cost a round trip apiece, which is what
/// made a refresh look like a page that never loaded (`@/records/iot/wifi.md`). On the C6 the
/// heap is all the internal DRAM the chip layer offers, and 64 KB is what setup mode was measured
/// to fit in with the page's future on it; asking for more there is a reset with the reason in
/// the log.
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
    // The driver's heap before the app's: the radio allocates from its own regions the moment
    // the station starts, and where those regions are is this part's business.
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);
    // What is left of it, for the link to report once setup mode's own frame is on the heap.
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

/// The S3's bring-up hands back a second software interrupt for the feed: folding the capture
/// envelope or painting the panel holds the cooperative executor past the feed's deadline.
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

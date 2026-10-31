#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use iot_core::drivers::board::{HasSurface, HasWifi};
use iot_core::state::SurfaceRole;

type Board = iot_bsp_esp::Board<'static>;

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let (mut board, feed) = boot_board();

    let station = board.take_wifi();

    let saved = board.take_credentials();
    let network = station.is_some();
    let surfaces = board.take_surfaces().expect("board has a wired surface");
    let has_pages = surfaces
        .iter()
        .any(|wired| wired.role() == SurfaceRole::Pages);

    match join(
        iot_app::run(
            board,
            iot_app::Wiring {
                clock: iot_chip_esp::Clock,
                feed,
                network,
                surfaces,
            },
        ),
        iot_app::net::serve(
            station,
            saved,
            iot_bsp_esp::FRONT,
            has_pages,
            iot_app::workers::NET,
            &iot_app::input::INTENT_BUS,
        ),
    )
    .await {}
}

fn boot_board() -> (Board, iot_app::Feed) {
    let peripherals = iot_chip_esp::chip_init_for_product();
    iot_chip_esp::init_logging();
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, iot_chip_esp::PRODUCT_HEAP_BYTES);
    iot_app::net::report_heap_free(iot_chip_esp::heap_free);

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

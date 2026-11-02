#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;

use embassy_executor::Spawner;
use iot_core::drivers::board::{HasSurface, HasWifi};
use iot_core::state::SurfaceRole;

type Board = iot_bsp_esp::Board<'static>;

/// The driver half's pickup slot. Core 1 is already running when the coordinator builds the
/// bundle, so the two sides meet here rather than at a call.
#[cfg(feature = "esp32s3")]
static IO_QUEUE: iot_app::IoQueue = iot_app::IoQueue::new();

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let (mut board, feed, io) = boot_board();

    let station = board.take_wifi();

    let saved = board.take_credentials();
    let network = station.is_some();
    let surfaces = board.take_surfaces().expect("board has a wired surface");
    let has_pages = surfaces
        .iter()
        .any(|wired| wired.role() == SurfaceRole::Pages);

    // The network worker is the board's: its driver types are only nameable here, so it is
    // spawned here like every other worker the composition root owns. `run` never learns the
    // radio exists.
    log::info!("[IOT] starting the net worker");
    spawner.spawn(network_task(station, saved, has_pages).expect("the network task fits its slot"));

    iot_app::run(
        board,
        iot_app::Wiring {
            clock: iot_chip_esp::Clock,
            feed,
            network,
            surfaces,
            io,
            spawner,
        },
    )
    .await
}

/// The network worker's task. A task cannot be generic, so the concrete station and flash
/// types are named at the composition point that took them off the board.
#[embassy_executor::task]
async fn network_task(
    station: Option<Box<iot_bsp_esp::components::wifi::EspStation>>,
    saved: Option<Box<iot_bsp_esp::components::credentials::CredentialStore>>,
    has_pages: bool,
) -> ! {
    iot_app::net::serve(iot_app::net::NetBoot {
        station,
        saved,
        host: iot_bsp_esp::SETUP_HOST,
        has_pages,
    })
    .await
}

fn boot_board() -> (Board, iot_app::Feed, iot_app::IoPlacement) {
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
fn start_executors(
    startup: iot_bsp_esp::Startup<'static>,
) -> (Board, iot_app::Feed, iot_app::IoPlacement) {
    let (board, timer, tick, feed, panel) = startup;
    iot_chip_esp::start_rtos(timer.timer0, tick);
    let feed = iot_chip_esp::start_feed_executor(feed);
    // The driver half follows the second core when there is one, and the placement is decided
    // here because this is the only place that knows whether it was started: without a panel
    // the board hands out no way to start it, and nothing waits for a pickup that never comes.
    let mut placement = iot_app::IoPlacement::Inline;
    if let Some(panel) = panel {
        iot_chip_esp::start_panel_core(panel.cpu_control, panel.intr, move |spawner| {
            spawner.spawn(
                iot_bsp_esp::panel_service(panel.bus).expect("the panel task fits its slot"),
            );
            spawner.spawn(iot_app::run_io_from(&IO_QUEUE).expect("the driver task fits its slot"));
        });
        placement = iot_app::IoPlacement::Elsewhere(&IO_QUEUE);
    }
    (board, iot_app::Feed::Interrupt(feed), placement)
}

#[cfg(feature = "esp32c6")]
fn start_executors(
    startup: iot_bsp_esp::Startup<'static>,
) -> (Board, iot_app::Feed, iot_app::IoPlacement) {
    let (board, timg0, from_cpu_intr) = startup;
    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    (board, iot_app::Feed::Inline, iot_app::IoPlacement::Inline)
}

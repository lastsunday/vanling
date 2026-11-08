#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use esp_hal::peripherals::Peripherals;
use esp_hal::timer::timg::TimerGroup;
use iot_core::drivers::board::{HasSurface, HasWifi};
use iot_core::drivers::surface::SurfaceRole;

type Board = iot_bsp_esp::Board<'static>;

/// The driver half's pickup slot. Core 1 is already running when the coordinator builds the
/// bundle, so the two sides meet here rather than at a call.
#[cfg(feature = "esp32s3")]
static IO_QUEUE: iot_app::IoQueue = iot_app::IoQueue::new();

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let (mut board, feed, io) = boot_board().await;

    // The coordinator's own executor feeds this; a frozen panel therefore resets rather than
    // standing there forever. `boot_board` has already started the timer driver it needs.
    iot_chip_esp::start_watchdog(spawner);

    let surfaces = board.take_surfaces().expect("board has a wired surface");
    let has_pages = surfaces
        .iter()
        .any(|wired| wired.role() == SurfaceRole::Pages);

    // The radio is the board's: its station and store are taken off the board right here, and
    // the worker that uses them is built here. `run` starts the closure without learning what
    // the radio is.
    let wifi = match board.take_wifi() {
        Some(station) => iot_app::Wifi::Worker(iot_app::runtime::feature::network::worker(
            station,
            board.take_credentials(),
            iot_bsp_esp::SETUP_HOST,
            has_pages,
        )),
        None => {
            log::info!("[WIFI] this build wired no radio, so the link stays off");
            iot_app::Wifi::Absent
        }
    };

    iot_app::run(
        board,
        iot_app::Wiring {
            clock: iot_chip_esp::Clock,
            feed,
            wifi,
            surfaces,
            io,
            spawner,
            second_core_peak,
        },
    )
    .await
}

#[cfg(feature = "esp32s3")]
fn second_core_peak() -> Option<(usize, usize)> {
    iot_chip_esp::panel_core_stack_peak()
}

#[cfg(feature = "esp32c6")]
fn second_core_peak() -> Option<(usize, usize)> {
    None
}

async fn boot_board() -> (Board, iot_app::Feed, iot_app::IoPlacement) {
    let peripherals = iot_chip_esp::chip_init_for_product();
    iot_chip_esp::init_logging();

    let boot = iot_chip_esp::note_boot();
    if boot.is_safe() {
        // No board bring-up and no watchdog: the station and one read-only page are the
        // whole surface. The heap is allowed to be unavailable here, which is why this
        // path uses `try_init_heap` and parks instead of resetting into safe mode again.
        iot_chip_esp::enter_safe_mode();
        iot_chip_esp::init_radio_heap();
        match iot_chip_esp::try_init_heap(&HEAP, iot_chip_esp::PRODUCT_HEAP_BYTES) {
            Ok(()) => iot_app::runtime::feature::network::report_heap_free(iot_chip_esp::heap_free),
            Err(error) => {
                log::error!("[SAFE] no general heap ({error:?}); the console is all that is left");
                iot_chip_esp::park_safe_mode(&boot);
            }
        }

        #[allow(non_snake_case)]
        let Peripherals {
            WIFI,
            FLASH,
            TIMG0,
            FROM_CPU_INTR0,
            ..
        } = peripherals;
        let timer = TimerGroup::new(TIMG0);
        iot_chip_esp::start_rtos(timer.timer0, FROM_CPU_INTR0);
        iot_chip_esp::ungate_radio();
        let station = iot_bsp_esp::components::wifi::start(WIFI);
        let saved = iot_bsp_esp::components::credentials::load_from_peripherals(FLASH)
            .map(alloc::boxed::Box::new);
        iot_app::runtime::feature::network::safe::run(station, saved, boot.record.crashed_boots())
            .await
    }

    // The boot-sized arm goes before the first driver timeout and before board bring-up:
    // a hang from here on is a reset, and after enough resets in a row safe mode keeps it
    // from becoming an endless one.
    iot_chip_esp::arm_boot_watchdog();

    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, iot_chip_esp::PRODUCT_HEAP_BYTES);
    iot_app::runtime::feature::network::report_heap_free(iot_chip_esp::heap_free);

    log::info!("[IOT] boot complete");

    // Bound before the call rather than matched at `start_executors`: the arm form measured
    // 1,860 B more text on the S3 for the same behaviour.
    let startup = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            iot_chip_esp::crashed_reset();
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

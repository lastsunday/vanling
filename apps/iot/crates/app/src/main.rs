#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use iot_core::drivers::board::HasWifi;
use iot_core::drivers::network::Front;
use iot_core::state::Panel;

/// The board this firmware is wired for. One alias because the board features
/// are mutually exclusive, so `iot-bsp-esp` re-exports a single `Board` name
/// either way and there is nothing for two arms to disambiguate.
type Board = iot_bsp_esp::Board<'static>;

/// Heap for the pluggable renderer registry and other runtime allocation.
#[global_allocator]
static HEAP: embedded_alloc::Heap = embedded_alloc::Heap::empty();

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

#[cfg(feature = "esp32c6")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    // The driver's heap before the app's: the radio allocates from its own regions the moment
    // the station starts, and where those regions are is this part's business.
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);
    // What is left of it, for the link to report once setup mode's own frame is on the heap.
    iot_app::network::report_heap_free(iot_chip_esp::heap_free);

    log::info!("[IOT] boot ok");

    // A transient boot NACK must not leave the screen black: reset and retry.
    // This board's bring-up hands back no second software interrupt, and it drives
    // no speaker either, so there is no feed to move and no executor to move it to.
    let (mut board, timg0, from_cpu_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    // The driver's start needs the scheduler, so the station is taken here rather than in the
    // board's own bring-up; the domain has to be powered and the modem clocks un-gated before
    // it, and nothing in this driver version does that on this part's build.
    iot_chip_esp::ungate_radio();
    let station = board.take_wifi();

    // The link runs beside the product rather than inside `run`: the station's futures are not
    // `Send` and are polled on this same cooperative executor, which is what lets the radio and
    // the panel share the one stack the product has to fit them into.
    // The saved networks are taken before the board goes into `run`: they are the credential a
    // boot with no build input comes up with, and `run` has no use for them.
    let saved = board.take_credentials();
    match join(
        iot_app::run(
            board,
            iot_chip_esp::Part,
            iot_app::SpeakerRunner::Inline,
            iot_app::Link::of(station.is_some()),
            Panel::Absent,
        ),
        // No panel on this part, so the access point is the only way setup mode can be reached:
        // it is up from the first second a device with nothing configured boots, and there is no
        // list here to offer instead. A build that never reaches this branch with a product's
        // credentials is why the access point's front is the value rather than a runtime choice.
        iot_app::network::serve(station, saved, Front::AccessPoint, false),
    )
    .await {}
}

#[cfg(feature = "esp32s3")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // 240 MHz, not the 80 MHz `chip_init` hands back: the AEC's delay search alone
    // busts the 30 ms audio tick at 80 MHz, and the governor may only lower from
    // here. `set_cpu_clock` never raises the bias, so starting lower would leave
    // the speed the product needs most without one trimmed for it.
    let peripherals = iot_chip_esp::chip_init_at(esp_hal::clock::CpuClock::_240MHz);
    iot_chip_esp::init_logging();
    // The driver's heap before the app's: the radio allocates from its own regions the moment
    // the station starts, and where those regions are is this part's business.
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);
    // What is left of it, for the link to report once setup mode's own frame is on the heap.
    iot_app::network::report_heap_free(iot_chip_esp::heap_free);

    log::info!("[IOT] boot ok");
    // A transient boot NACK is retried from a clean chip. The panel is still dark
    // here, so a failure that repeats reads as a dark screen instead of the last
    // run's frame.
    let (mut board, timg0, from_cpu_intr, feed_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    let feed = iot_chip_esp::start_feed_executor(feed_intr);
    // The driver's start needs the scheduler, so the station is taken here rather than in the
    // board's own bring-up; the domain has to be powered and the modem clocks un-gated before
    // it, and nothing in this driver version does that on this part's build.
    iot_chip_esp::ungate_radio();
    let station = board.take_wifi();

    // The link runs beside the product rather than inside `run`: the station's futures are not
    // `Send` and are polled on this same cooperative executor, which is what lets the radio and
    // the panel share the one stack the product has to fit them into.
    // The saved networks are taken before the board goes into `run`: they are the credential a
    // boot with no build input comes up with, and `run` has no use for them.
    let saved = board.take_credentials();
    match join(
        iot_app::run(
            board,
            iot_chip_esp::Part,
            iot_app::SpeakerRunner::Interrupt(feed),
            iot_app::Link::of(station.is_some()),
            Panel::Wired,
        ),
        // The panel is the front end a device with one starts on: the access point is a network
        // anybody in range can join, and it is up only while somebody has asked for its page. The
        // list needs no radio of its own beyond the scan it already runs.
        iot_app::network::serve(station, saved, Front::Panel, true),
    )
    .await {}
}

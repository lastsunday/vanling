#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use iot_core::drivers::board::HasWifi;

/// The board this firmware is wired for. One alias because the board features
/// are mutually exclusive, so `iot-bsp-esp` re-exports a single `Board` name
/// either way and there is nothing for two arms to disambiguate.
type Board = iot_bsp_esp::Board<'static>;

/// Heap for the pluggable renderer registry and other runtime allocation.
#[global_allocator]
static HEAP: embedded_alloc::Heap = embedded_alloc::Heap::empty();

/// Sized for the renderer registry, the device state and the driver's own buffers.
///
/// No longer holds the frame buffer: that is the board's static now — the camera's DMA and
/// the panel's SPI share it — so it is 150 KB out of DRAM that this used to duplicate on the
/// heap. What is left is small allocations, and the heap that fits them leaves the main stack
/// the rest. Measured against `@/records/iot/hardware-constraints.md`.
///
/// The same number on both parts, which is not the same cost: on the S3 it is a slice of
/// mapped PSRAM, on the C6 it is all of the internal DRAM the chip layer offers a heap, so
/// asking for more there is a reset with the reason in the log.
const HEAP_BYTES: usize = 64 * 1024;

#[cfg(feature = "esp32c6")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    // The driver's heap before the app's: the radio allocates from its own regions the moment
    // the station starts, and where those regions are is this part's business.
    iot_chip_esp::init_radio_heap();
    iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);

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
        ),
        iot_app::network::serve(station, saved),
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

    log::info!("[IOT] boot ok");
    // A transient boot NACK must not leave the screen black: reset and retry.
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
        ),
        iot_app::network::serve(station, saved),
    )
    .await {}
}

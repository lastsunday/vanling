#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;

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
static mut HEAP_MEM: [u8; HEAP_BYTES] = [0; HEAP_BYTES];

/// Heap size in bytes.
const HEAP_BYTES: usize = 64 * 1024;

#[cfg(feature = "esp32c6")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(core::ptr::addr_of_mut!(HEAP_MEM) as usize, HEAP_BYTES);
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::info!("[IOT] boot ok");

    // A transient boot NACK must not leave the screen black: reset and retry.
    // This board's bring-up hands back no second software interrupt, and it drives
    // no speaker either, so there is no feed to move and no executor to move it to.
    let (board, timg0, from_cpu_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    iot_app::run(board, iot_app::SpeakerRunner::Inline).await
}

#[cfg(feature = "esp32s3")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(core::ptr::addr_of_mut!(HEAP_MEM) as usize, HEAP_BYTES);
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::info!("[IOT] boot ok");
    // A transient boot NACK must not leave the screen black: reset and retry.
    let (board, timg0, from_cpu_intr, feed_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    let feed = iot_chip_esp::start_feed_executor(feed_intr);
    iot_app::run(board, iot_app::SpeakerRunner::Interrupt(feed)).await
}

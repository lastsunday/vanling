#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;

#[cfg(feature = "esp32c6-devkitc-1")]
type Board = iot_bsp_esp::Board<'static>;

#[cfg(feature = "lckfb-szpi-esp32s3")]
type Board = iot_bsp_esp::Board<'static>;

/// Heap for the pluggable renderer registry and other runtime allocation.
#[global_allocator]
static HEAP: embedded_alloc::Heap = embedded_alloc::Heap::empty();

/// Sized to hold the LCD frame buffer (240×320×2 B) plus renderer registry headroom.
static mut HEAP_MEM: [u8; 200 * 1024] = [0; 200 * 1024];

#[cfg(feature = "esp32c6")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; 200 * 1024]>(),
        );
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::info!("[IOT] boot ok");

    // A transient boot NACK must not leave the screen black: reset and retry.
    let (board, timg0, from_cpu_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    iot_app::run(board).await
}

#[cfg(feature = "esp32s3")]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; 200 * 1024]>(),
        );
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::info!("[IOT] boot ok");

    // A transient boot NACK must not leave the screen black: reset and retry.
    let (board, timg0, from_cpu_intr) = match Board::new(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[IOT] board init failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);
    iot_app::run(board).await
}

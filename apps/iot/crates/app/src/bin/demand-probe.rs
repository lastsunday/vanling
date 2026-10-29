//! Motion demand matrix: walks every (configuration × cadence) cell the driver can reach
//! reports what each one costs, so the cost table carries measurements rather than
//! arithmetic done from a datasheet.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::LlffHeap as Heap;

type Board = iot_bsp_esp::Board<'static>;

static mut HEAP_MEM: [u8; 16 * 1024] = [0; 16 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; 16 * 1024]>(),
        );
    }

    let peripherals = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();

    let (mut board, timg0, from_cpu_intr, _feed_intr) = match Board::new_motion_only(peripherals) {
        Ok(startup) => startup,
        Err(error) => {
            log::error!("[PROBE] motion bring-up failed, resetting: {error:?}");
            esp_hal::system::software_reset();
        }
    };

    iot_chip_esp::start_rtos(timg0.timer0, from_cpu_intr);

    let Some(motion) = board.take_motion_driver() else {
        log::error!("[PROBE] no IMU wired, no matrix to walk");
        loop {
            embassy_time::Timer::after(embassy_time::Duration::from_secs(1)).await;
        }
    };
    iot_app::probe::motion::run(motion).await
}

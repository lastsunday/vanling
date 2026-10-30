//! What changing the CPU clock costs on this chip, and what it leaves behind.
//!
//! **Outside esp-hal's contract**: its docs say the frequencies "become frozen and cannot be
//! changed" once applied, so the switch writes the registers directly. It works because the chip
//! comes up at 240 MHz and the voltage bias trimmed there is never lowered — a chip brought up
//! slow has no such bias to fall back on.
//!
//! No board: the questions are the clock tree and the system timer. The second core is up
//! because the ROM's tick calibration is per-core.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::LlffHeap as Heap;
use esp_hal::clock::CpuClock;
use esp_hal::system::{Cpu, Stack};
use esp_hal::timer::timg::TimerGroup;

// **80 MHz, because that is what the product boots at** — `chip_init()` hands the same preset
// back — so the probe measures the climb back to 240 on a bias trimmed for less. Measured not to
// bite (`@/records/iot/clock.md`).
const CLOCK: CpuClock = CpuClock::_80MHz;

static mut HEAP_MEM: [u8; 8 * 1024] = [0; 8 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

const CORE1_STACK_BYTES: usize = 8 * 1024;

type StaticCell<T> = esp_hal::__macro_implementation::static_cell::StaticCell<T>;

static CORE1_STACK: StaticCell<Stack<CORE1_STACK_BYTES>> = StaticCell::new();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // SAFETY: called exactly once before any allocation; the region is a
    // private static never aliased elsewhere.
    unsafe {
        HEAP.init(
            core::ptr::addr_of_mut!(HEAP_MEM) as usize,
            core::mem::size_of::<[u8; 8 * 1024]>(),
        );
    }

    #[allow(non_snake_case)]
    let esp_hal::peripherals::Peripherals {
        TIMG0,
        FROM_CPU_INTR0,
        FROM_CPU_INTR1,
        CPU_CTRL,
        ..
    } = iot_chip_esp::chip_init_at(CLOCK);
    iot_chip_esp::init_logging();
    iot_chip_esp::start_rtos(TimerGroup::new(TIMG0).timer0, FROM_CPU_INTR0);
    iot_chip_esp::start_second_core(
        CPU_CTRL,
        FROM_CPU_INTR1,
        CORE1_STACK.init(Stack::new()),
        |spawner| {
            spawner.spawn(core1_measure().expect("a task of this shape fits its pool slot"));
        },
    );
    log::info!("[CLOCK] brought up at {CLOCK:?}");

    iot_app::bench::probe::clock::measure().await;
    park().await
}

async fn park() -> ! {
    loop {
        core::future::pending::<()>().await
    }
}

#[embassy_executor::task]
async fn core1_measure() {
    log::info!(
        "[CLOCK] second core waiting on cpu {}",
        Cpu::current() as usize
    );
    iot_app::bench::probe::clock::core1_task().await;
    log::info!("[CLOCK] second core done");
    loop {
        core::hint::spin_loop();
    }
}

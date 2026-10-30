//! What the linear AEC costs per audio tick, on this chip, and whether the second core
//! changes the answer. **It does not**: 20+ runs gave identical digits on both cores.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embedded_alloc::LlffHeap as Heap;
use esp_hal::clock::CpuClock;
use esp_hal::system::{Cpu, Stack};
use esp_hal::timer::timg::TimerGroup;

const CLOCK: CpuClock = CpuClock::_240MHz;

static mut HEAP_MEM: [u8; 16 * 1024] = [0; 16 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

static GO: Signal<CriticalSectionRawMutex, u64> = Signal::new();

const CORE1_STACK_BYTES: usize = 16 * 1024;

type StaticCell<T> = esp_hal::__macro_implementation::static_cell::StaticCell<T>;

static CORE1_STACK: StaticCell<Stack<CORE1_STACK_BYTES>> = StaticCell::new();

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
    log::info!("[ECHO] brought up at {CLOCK:?}");

    let cpu_hz = u64::from(esp_hal::clock::cpu_clock().as_hz());
    banner(0);
    iot_app::bench::probe::echo::measure(cpu_hz).await;

    GO.signal(cpu_hz);
    park().await
}

fn banner(core: u8) {
    log::info!("[ECHO] ==================== cpu {core} ====================",);
}

async fn park() -> ! {
    loop {
        core::future::pending::<()>().await
    }
}

#[embassy_executor::task]
async fn core1_measure() {
    let cpu_hz = GO.wait().await;
    log::info!(
        "[ECHO] second core closure running on cpu {}",
        Cpu::current() as usize
    );
    banner(1);
    iot_app::bench::probe::echo::measure(cpu_hz).await;
    log::info!("[ECHO] second core done");
    loop {
        core::hint::spin_loop();
    }
}

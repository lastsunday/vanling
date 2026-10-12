//! What changing the CPU clock costs on this chip, and what it leaves behind.
//!
//! **This probe runs outside esp-hal's contract.** esp-hal documents that the
//! clock frequencies "become frozen and cannot be changed" once applied, so the
//! switch under test writes the registers directly. The reason it can be run at
//! all is that the chip is brought up at 240 MHz and the voltage bias esp-hal
//! trims there is never lowered — a chip brought up slow has no such bias to
//! fall back on.
//!
//! No board and no peripheral: the questions are about the clock tree and the
//! system timer, and driving a speaker would only add variables. The one thing
//! that does need bringing up is the second core, because the ROM's tick
//! calibration is per-core and this board runs two executors.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embedded_alloc::Heap;
use esp_hal::clock::CpuClock;
use esp_hal::system::{Cpu, Stack};
use esp_hal::timer::timg::TimerGroup;

/// The clock this image brings the chip up at.
///
/// **80 MHz, because that is what the product boots at** — `chip_init()` hands
/// the same preset back — so the probe measures the configuration that actually
/// runs, including the climb back to 240 on a bias esp-hal trimmed for less.
/// Measured not to bite; see `docs/content/records/iot/clock.md`.
const CLOCK: CpuClock = CpuClock::_80MHz;

/// Only the log line's formatting and the executor's own allocations.
static mut HEAP_MEM: [u8; 8 * 1024] = [0; 8 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

const CORE1_STACK_BYTES: usize = 8 * 1024;

type StaticCell<T> = esp_hal::__macro_implementation::static_cell::StaticCell<T>;

static CORE1_STACK: StaticCell<Stack<CORE1_STACK_BYTES>> = StaticCell::new();

static CORE1_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

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

    // Taken by field rather than used through `peripherals`, because each is a move
    // and a partial move would leave the rest of the set unusable.
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
    start_core1(CPU_CTRL, FROM_CPU_INTR1);
    log::info!("[CLOCK] brought up at {CLOCK:?}");

    iot_app::probe::clock::measure().await;
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
    iot_app::probe::clock::core1_task().await;
    log::info!("[CLOCK] second core done");
    loop {
        core::hint::spin_loop();
    }
}

/// Boots the second core and gives it [`core1_measure`].
///
/// `intr1` is the SMP scheduler's. This probe drives no speaker, so the interrupt
/// the product gives the speaker's feed is free here, and taking it keeps the probe
/// from having to change which interrupt the board hands back.
fn start_core1(
    cpu_control: esp_hal::peripherals::CPU_CTRL<'static>,
    intr1: esp_hal::peripherals::FROM_CPU_INTR1<'static>,
) {
    let stack = CORE1_STACK.init(Stack::new());
    esp_rtos::start_second_core(cpu_control, intr1, stack, || {
        let executor = CORE1_EXECUTOR.init(esp_rtos::embassy::Executor::new());
        executor.run(|spawner| {
            spawner.spawn(core1_measure().expect("a task of this shape fits its pool slot"));
        });
    });
}

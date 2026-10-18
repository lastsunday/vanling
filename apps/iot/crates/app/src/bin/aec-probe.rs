//! What the linear AEC costs per audio tick, on this chip, and whether the
//! second core changes the answer. **It does not** — the same measurement on core
//! 0 and core 1 gave identical digits across 20+ runs. Every loop measured on
//! this board has run at an IPC of 0.09–0.33; that is a memory-system property,
//! not an instruction-fetch one, and only a second core could have told the two
//! apart.
//!
//! No board, no peripheral, no product task: the canceller is arithmetic on two
//! streams, and the only thing a bring-up could add is noise to the measurement.
//! What the measurement needs is a clock and a log, and `chip_init` plus
//! `start_rtos` between them provide both.
//!
//! The two runs do not overlap: core 0 measures, signals, and parks before core 1
//! starts, so they do not compete for the memory the question is about. The
//! product leaves core 1 entirely idle today — its `FROM_CPU_INTR1` is a second
//! timer interrupt source on the *same* core.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embedded_alloc::Heap;
use esp_hal::clock::CpuClock;
use esp_hal::system::{Cpu, Stack};
use esp_hal::timer::timg::TimerGroup;

/// The clock this image brings the chip up at.
///
/// Flip this and rebuild to compare; everything else in the image is identical
/// between the two figures.
///
/// esp-hal warns that changing the PLL breaks USB Serial/JTAG, which is the
/// console this probe reports through. The 240 preset is not that: it pins
/// `pll_clk` to 480 like the others and moves only the divider. The console has
/// stayed up through every figure below, which is what the choice rests on.
const CLOCK: CpuClock = CpuClock::_240MHz;

/// Only the log line's formatting and the executor's own allocations.
static mut HEAP_MEM: [u8; 16 * 1024] = [0; 16 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

/// Core 1 to core 0: the clock to measure against, and the go-ahead.
///
/// A signal rather than a bare flag because the two cores are separate executors,
/// and because it carries the value core 1 cannot read for itself — it never ran
/// `chip_init`, so the clock was programmed on the other core.
static GO: Signal<CriticalSectionRawMutex, u64> = Signal::new();

/// The second core's stack. Larger than the multicore probe's 8 KiB: that one
/// runs a counter round trip, this one runs a measurement whose transform holds
/// a 512-point working buffer and a 256-entry twiddle table by value.
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
    // `intr1` is the SMP scheduler's. This probe drives no speaker, so the interrupt the
    // product gives the speaker's feed is free here, and taking it keeps the probe from
    // having to change which interrupt the board hands back.
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
    iot_app::probe::echo::measure(cpu_hz).await;

    // Only now, with core 0 parked behind the signal and its allocator idle.
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
    iot_app::probe::echo::measure(cpu_hz).await;
    log::info!("[ECHO] second core done");
    loop {
        core::hint::spin_loop();
    }
}

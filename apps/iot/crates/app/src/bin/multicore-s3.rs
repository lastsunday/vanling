//! Probes whether starting the second core survives the main stack sitting under it: the
//! ROM boots the APP core and its startup prologue runs on its own boot stack at the top of
//! `dram2_seg`, so an unreserved main stack has its *top* overwritten — and the top is where
//! the thread-mode executor keeps its `ThreadFlag`. What the second core runs is irrelevant;
//! the damage predates any closure. The hazard and the reservation that answers it are
//! documented where the reservation lives,
//! `crates/app/linker/esp32s3-main-stack.x`.
//!
//! [`CASE`] selects which shape to run; one case per image keeps a fault from being
//! attributed to the wrong deviation:
//!
//!   1  esp-hal's `embassy_multicore` example, transcribed.
//!   2  the same, plus a board-sized value held live on `main`'s stack across the bring-up —
//!      the product's shape, and the one that fails without the reservation.
//!
//! Both cores run the same round trip [`ROUNDS`] times — core 0 sends a counter, core 1
//! echoes it — and a case logging its completed round trips on both cores has survived.

#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embedded_alloc::Heap;
use esp_hal::system::{Cpu, Stack};
use esp_hal::timer::timg::TimerGroup;

/// Which shape to run. Change it and reflash; one case per image keeps a fault from
/// being attributed to the wrong deviation.
const CASE: u8 = 2;

/// Echoes in each direction before a case reports success.
const ROUNDS: u32 = 8;

/// The second core's stack, at the example's 8 KiB rather than a comfortable size: a stack
/// larger than the task needs would hide a reservation mistake rather than expose it.
const CORE1_STACK_BYTES: usize = 8 * 1024;

/// The size of the product's `Board`, which `main` holds live on its stack across the
/// second core's bring-up. An array of that size reproduces the hazard — the point of the
/// case is the *stack depth*, not the board — without needing the whole peripheral set.
const BOARD_BYTES: usize = 4_304;

/// The probe allocates nothing itself, but the scheduler and the logger both reach
/// for the global allocator, so one has to exist.
static mut HEAP_MEM: [u8; 16 * 1024] = [0; 16 * 1024];

#[global_allocator]
static HEAP: Heap = Heap::empty();

/// Core 0 to core 1, the counter under test.
static TO_CORE1: Signal<CriticalSectionRawMutex, u32> = Signal::new();

/// Core 1 back to core 0, so core 0 only reports success for echoes it received.
static TO_CORE0: Signal<CriticalSectionRawMutex, u32> = Signal::new();

/// The static container this tree reaches for: esp-hal's own `StaticCell`, re-exported for
/// its macros rather than a declared dependency of its own.
type StaticCell<T> = esp_hal::__macro_implementation::static_cell::StaticCell<T>;

/// The second core's stack, in a `StaticCell` because [`esp_rtos::start_second_core`] wants
/// a `&'static mut` and the stack has to outlive the executor that runs on it.
static CORE1_STACK: StaticCell<Stack<CORE1_STACK_BYTES>> = StaticCell::new();

/// The second core's thread-mode executor, built inside the closure that runs there. A
/// static rather than a local because `Executor::run` borrows its executor for as long as it
/// runs, which is forever.
static CORE1_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

/// The second core's half: echo back whatever core 0 sends.
#[embassy_executor::task]
async fn core1_echo() {
    log::info!("[MC] core 1 running on cpu {}", Cpu::current() as usize);
    for expected in 1..=ROUNDS {
        let got = TO_CORE1.wait().await;
        if got != expected {
            log::error!("[MC] core 1 expected {expected}, got {got}");
            return;
        }
        log::info!("[MC] core 1 echoing {got}");
        TO_CORE0.signal(got);
    }
    log::info!("[MC] core 1 finished {ROUNDS} round trips");
    loop {
        core::hint::spin_loop();
    }
}

/// Core 0's half. `board` is `None` in case 1 and the board-sized array in case 2, where
/// holding it by value for the whole round trip keeps it live across every await — and so
/// across the second core's bring-up, which is the stack home the ROM's prologue lands on.
#[embassy_executor::task]
async fn core0_echo(board: Option<[u8; BOARD_BYTES]>) {
    if let Some(board) = board {
        let sum = board.iter().fold(0u32, |a, &b| a.wrapping_add(b as u32));
        log::info!("[MC] core 0 took the board, sum={sum:08x}");
    }
    for i in 1..=ROUNDS {
        TO_CORE1.signal(i);
        let got = TO_CORE0.wait().await;
        if got != i {
            log::error!("[MC] core 0 expected {i}, got {got}");
            return;
        }
    }
    match board {
        Some(_) => log::info!("[MC] core 0 finished {ROUNDS} round trips holding the board"),
        None => log::info!("[MC] core 0 finished {ROUNDS} round trips"),
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Differs from esp-hal's `embassy_multicore` example in three ways, all of them this
/// tree's: the vendored `esp-rtos` takes the SMP interrupt as an argument, `iot-chip-esp`
/// starts the scheduler from a timer plus it, and the log facade is `log`.
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
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
    } = iot_chip_esp::chip_init();
    iot_chip_esp::init_logging();
    log::info!(
        "[MC] multi-core isolation, case {CASE}, on cpu {}",
        Cpu::current() as usize
    );

    // Built before the bring-up and consumed after it, so the compiler cannot shorten its
    // life to the call.
    let board = match CASE {
        2 => {
            let board = [0xABu8; BOARD_BYTES];
            log::info!("[MC] holding {BOARD_BYTES} B of stack across the bring-up");
            Some(board)
        }
        _ => None,
    };

    iot_chip_esp::start_rtos(TimerGroup::new(TIMG0).timer0, FROM_CPU_INTR0);
    start_core1(CPU_CTRL, FROM_CPU_INTR1);
    log::info!("[MC] second core started");
    spawner.spawn(core0_echo(board).expect("a task of this shape fits its pool slot"));
    park().await
}

/// Parks this task for the rest of the run: spawning only *creates* a task, so `main` has to
/// yield for the executor to ever poll it. A spin would keep the thread executor for the
/// task that has to run.
async fn park() -> ! {
    loop {
        core::future::pending::<()>().await
    }
}

/// Boots the second core and gives it [`core1_echo`].
///
/// `intr1` is the SMP scheduler's. This probe wires no speaker, so the interrupt the
/// product gives the speaker's feed is free here, and taking it keeps the probe from
/// having to change which interrupt the board hands back.
fn start_core1(
    cpu_control: esp_hal::peripherals::CPU_CTRL<'static>,
    intr1: esp_hal::peripherals::FROM_CPU_INTR1<'static>,
) {
    let stack = CORE1_STACK.init(Stack::new());
    esp_rtos::start_second_core(cpu_control, intr1, stack, || {
        log::info!("[MC] second core closure running");
        let executor = CORE1_EXECUTOR.init(esp_rtos::embassy::Executor::new());
        executor.run(|spawner| {
            spawner.spawn(core1_echo().expect("a task of this shape fits its pool slot"));
        });
    });
}

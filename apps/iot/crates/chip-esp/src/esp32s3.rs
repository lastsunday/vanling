use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use crate::heap::Window;
use embassy_executor::{SendSpawner, Spawner};
use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::clock::CpuClock;
use esp_hal::peripherals::{
    CPU_CTRL, FROM_CPU_INTR0, FROM_CPU_INTR1, LPWR, PSRAM, Peripherals, SYSTEM,
};
use esp_hal::psram::{Psram, PsramConfig};
use esp_hal::rom::ets_update_cpu_frequency_rom;
use esp_hal::system::Stack;
use iot_core::demand::{DemandField, Speed, SpeedTable};

/// Brings the chip up, optionally at a clock other than [`CpuClock`]'s default.
///
/// [`CpuClock`]'s `Default` arm is `_80MHz` on this part, so the default here is
/// 80 MHz on a chip rated for 240. That is a board-level fact every cost in this
/// tree inherits.
///
/// The 240 MHz preset is not kept out on the strength of esp-hal's warning that
/// changing the PLL breaks USB Serial/JTAG, which is this repository's console.
/// That warning is about the PLL, and the presets do not touch it: all three
/// pin `pll_clk` to 480 and differ only in the divider, so the console is not on
/// the path. Measured at 240 MHz with the console intact — `clock-probe`, and
/// the AEC cost figures that needed the clock.
pub fn chip_init_at(clock: CpuClock) -> Peripherals {
    esp_app_desc!();
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(clock));
    init_psram();
    peripherals
}

pub fn chip_init() -> Peripherals {
    chip_init_at(CpuClock::_80MHz)
}

/// What [`psram_window`] found on this part, for [`crate::heap`]. Three answers because the two failures lead
/// apart: no PSRAM on the module is hardware to go looking for, a window that does not
/// read back is this build's SPI timing.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PsramWindow {
    /// The window, mapped and confirmed to read back what was written to it.
    Ready(Window),
    /// Nothing to map — this part has no PSRAM, or esp-hal could not bring one up.
    Absent,
    /// Mapped, and a pattern written to it did not come back unchanged. The size it mapped
    /// is reported where the pattern failed rather than carried here: a caller with no window
    /// to offer has nothing to do with one.
    Unusable,
}

/// The PSRAM handle. It outlives the program because the window below is the mapping it
/// put in place, and `Psram` is the only thing holding the peripheral it was given.
static PSRAM_HANDLE: esp_hal::__macro_implementation::static_cell::StaticCell<Psram> =
    esp_hal::__macro_implementation::static_cell::StaticCell::new();

const PSRAM_ABSENT: u8 = 0;
const PSRAM_UNUSABLE: u8 = 1;
const PSRAM_READY: u8 = 2;

/// What [`init_psram`] found, published for [`psram_window`].
///
/// Atomics rather than a `StaticCell` because that cell hands a reference to whoever
/// initializes it and gives a reader nothing at all. Relaxed ordering is enough: this is
/// written on the boot path and read by the entry point that booted it, so the one caller
/// in between is what orders the two.
static PSRAM_STATE: AtomicU8 = AtomicU8::new(PSRAM_ABSENT);
static PSRAM_START: AtomicUsize = AtomicUsize::new(0);
static PSRAM_LEN: AtomicUsize = AtomicUsize::new(0);

/// The PSRAM on this part, or what is wrong with it.
///
/// `Absent` is also the answer before [`chip_init_at`] has run: nothing is mapped yet,
/// and a window that does not exist is not one to hand an allocator.
pub(crate) fn psram_window() -> PsramWindow {
    match PSRAM_STATE.load(Ordering::Relaxed) {
        PSRAM_READY => PsramWindow::Ready(Window {
            start: PSRAM_START.load(Ordering::Relaxed),
            len: PSRAM_LEN.load(Ordering::Relaxed),
        }),
        PSRAM_UNUSABLE => PsramWindow::Unusable,
        _ => PsramWindow::Absent,
    }
}

/// Maps this part's PSRAM once, as part of bringing the chip up: the window has to exist
/// before anyone can hand its address to an allocator, and a general heap is among the
/// first things an entry point sets up.
///
/// `PsramConfig::default()` leaves the SPI timing as esp-hal derives it for this part.
/// Naming a core clock here would be wrong: [`set_cpu_clock`] moves the CPU and leaves
/// APB alone, so the CPU frequency is not what the SPI block is clocked from.
fn init_psram() {
    // `Psram::new` wants the PSRAM peripheral, and that peripheral is one field of the
    // `Peripherals` this module hands back: taking it out would be a partial move and
    // leave nothing to return. esp-hal's `init` builds `Peripherals` by stealing these
    // singletons, so stealing this one yields that same handle rather than a second one.
    //
    // SAFETY: nothing downstream of `chip_init_at` reaches for PSRAM — every board
    // destructures `Peripherals` with a `..` catch-all — so this handle is the only one
    // that touches those registers, and `Psram` keeps it for the rest of the program.
    let psram = PSRAM_HANDLE.init(Psram::new(
        unsafe { PSRAM::steal() },
        PsramConfig::default(),
    ));
    let (start, len) = psram.raw_parts();

    // esp-hal reports a PSRAM it could not bring up as an empty range, so the length is
    // the whole of "did it come up". A window that mapped and then fails the pattern is
    // kept apart from that: the bytes are there, and something about reaching them is
    // wrong, which is a different thing to go looking for.
    let (state, window) = match len {
        0 => (PSRAM_ABSENT, None),
        len => {
            let window = Window {
                start: start as usize,
                len,
            };
            if reads_back(window) {
                (PSRAM_READY, Some(window))
            } else {
                log::error!(
                    "[PSRAM] mapped {} B at {:#x}, and the pattern did not read back",
                    window.len,
                    window.start
                );
                (PSRAM_UNUSABLE, Some(window))
            }
        }
    };
    if let Some(window) = window {
        PSRAM_START.store(window.start, Ordering::Relaxed);
        PSRAM_LEN.store(window.len, Ordering::Relaxed);
    }
    PSRAM_STATE.store(state, Ordering::Relaxed);
}

/// Whether a pattern written to the window comes back unchanged: 4 KiB at each end (a
/// mapping covering only part of the window passes a check in the middle), stepping by
/// seven from a non-zero base so stale contents cannot pass.
fn reads_back(window: Window) -> bool {
    const SPAN: usize = 4 * 1024;
    const STEP: usize = 7;
    const BASE: usize = 11;

    let expected = |index: usize| (index * STEP + BASE) as u8;

    [0, window.len.saturating_sub(SPAN)].into_iter().all(|at| {
        // SAFETY: `at + SPAN` is inside the window `window` names, which is mapped for
        // the life of the program, and nothing else has been handed this memory — the
        // allocator that will own it is initialised after the chip is up.
        let bytes = unsafe { core::slice::from_raw_parts_mut(window.start as *mut u8, window.len) };
        let span = &mut bytes[at..at + SPAN];
        for (index, byte) in span.iter_mut().enumerate() {
            *byte = expected(index);
        }
        span.iter()
            .enumerate()
            .all(|(index, &byte)| byte == expected(index))
    })
}

/// What each plane needs on this part, from worst cases measured here.
///
/// A worst case is a hold in wall time, so it belongs to the chip that was
/// holding: the same deadline on a slower core is a longer hold and a faster one
/// a shorter, and neither transfers.
///
/// Each row is that part's own measured hold times three, against the 30 ms one
/// audio tick may occupy, so a row under 10 ms may sit slow and one over it may
/// not. The multiplier is deliberately pessimistic — measured downclocking
/// stretches a hold by about 2x rather than 3x, because part of the cost is I2C
/// wait that does not track the clock. Both directions of that reasoning, and the
/// rows themselves, are in `docs/content/records/iot/clock.md`.
pub struct Table;

impl SpeedTable for Table {
    fn speed_for(field: DemandField) -> Speed {
        match field {
            DemandField::Interaction | DemandField::Motion => Speed::Slow,
            DemandField::Audio | DemandField::Camera | DemandField::Display => Speed::Fast,
        }
    }
}

/// Puts the chip at the clock [`Speed`] names.
///
/// The three-way mapping lives here because the presets are a fact about this part:
/// `CpuClock` has three of them, and a chip with fewer cannot honour the same
/// vocabulary. This part can honour all three, so what this returns equals what it
/// was given; a part that cannot says so through the same return value.
///
/// [`set_cpu_clock`] is `unsafe` for its one requirement — interrupts disabled
/// across the register writes — and this discharges it with the critical section
/// that function already takes.
pub fn set_speed(speed: Speed) -> Speed {
    // SAFETY: the callee's only requirement is interrupts disabled across the register
    // writes, which the critical section it takes itself provides.
    unsafe { set_cpu_clock(clock_for(speed)) };
    speed
}

const fn clock_for(speed: Speed) -> CpuClock {
    match speed {
        Speed::Slow => CpuClock::_80MHz,
        Speed::Normal => CpuClock::_160MHz,
        Speed::Fast => CpuClock::_240MHz,
    }
}

/// Moves the CPU to another preset while the chip is running.
///
/// **This is outside esp-hal's contract.** Its own docs say the clock
/// frequencies "become frozen and cannot be changed" once applied, and this
/// reaches past that to write the registers directly. What esp-hal keeps sealed
/// is the voltage bias — `regi2c` and the PVT helpers are `pub(crate)` or
/// private — so this writes the frequency, the LDO divider and the ROM's tick
/// calibration, and leaves the bias where init put it. Never lowering that bias
/// makes every frequency below 240 run over-biased, which is the conservative
/// direction; measured in `docs/content/records/iot/clock.md`, including the
/// climb from an 80 MHz boot. Nothing outside the CPU moves: the PLL stays at 480
/// MHz, APB is untouched, and the system timer is clocked from XTAL alone.
///
/// # Safety
///
/// Interrupts must be disabled across the register writes, or a handler can
/// observe the LDO divider and the frequency divider disagreeing — the core
/// briefly running at one frequency against a divider set for another. Only the
/// calling core's ROM tick calibration is updated; `ets_update_cpu_frequency_rom`
/// documents this as per-core, so a second core's `ets_delay_us` is mis-scaled
/// afterwards. Measured to cost nothing observable here, but that is one workload
/// on one board, not a guarantee for code that depends on it elsewhere.
pub unsafe fn set_cpu_clock(clock: CpuClock) {
    // esp-hal's own mapping, with `pll_clk` pinned at 480 and the source at PLL
    // for every preset — which is why only `cpuperiod_sel` actually differs.
    let (period, mhz) = match clock {
        CpuClock::_80MHz => (0, 80),
        CpuClock::_160MHz => (1, 160),
        CpuClock::_240MHz => (2, 240),
        // Guessing a divider here would leave the core running at a frequency
        // nothing here can name, so a preset this HAL grew later is refused
        // rather than mapped to a neighbour.
        other => panic!("{other:?} has no divider this function knows"),
    };

    critical_section::with(|_| {
        SYSTEM::regs().cpu_per_conf().modify(|_, w| unsafe {
            w.cpuperiod_sel().bits(period);
            w.pll_freq_sel().bit(true)
        });
        SYSTEM::regs().sysclk_conf().modify(|_, w| unsafe {
            w.pre_div_cnt().bits(0);
            w.soc_clk_sel().bits(1)
        });
        LPWR::regs()
            .date()
            .modify(|_, w| unsafe { w.ldo_slave().bits(0x7 >> (mhz / 80)) });
        ets_update_cpu_frequency_rom(mhz);
    })
}

pub fn init_logging() {
    log::set_logger(&crate::logging::LOGGER).expect("logger already set");
    log::set_max_level(log::LevelFilter::Info);
}

pub fn start_rtos(timer: impl esp_rtos::TimerSource, int0: FROM_CPU_INTR0<'static>) {
    esp_rtos::start(timer, int0);
}

/// The one higher-priority executor on this board, on `FROM_CPU_INTR1` —
/// `INTR0` is already the cooperative scheduler's own. A `StaticCell` because
/// [`InterruptExecutor::start`] wants a `&'static mut self` and the executor is not
/// `Copy`.
static FEED_EXECUTOR: esp_hal::__macro_implementation::static_cell::StaticCell<
    esp_rtos::embassy::InterruptExecutor<1>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

/// Starts the executor the speaker's feed loop runs on, and hands back its spawner.
///
/// Not joined onto the cooperative scheduler because tasks sharing one executor
/// round-robin and none preempts another — folding the capture envelope holds it
/// ~17 ms, a panel write up to 16 ms — and this board slaves the capture's clock to
/// the transmit unit, making a late feed both a gap in the sound and a stalled
/// microphone.
///
/// Exactly one level above the thread: higher and the feed would starve input and
/// render instead.
pub fn start_feed_executor(intr1: FROM_CPU_INTR1<'static>) -> SendSpawner {
    let executor = FEED_EXECUTOR.init(esp_rtos::embassy::InterruptExecutor::new(intr1));
    executor.start(esp_hal::interrupt::Priority::Priority2)
}

/// The second APP core's thread-mode executor. A `StaticCell` because `esp_rtos::embassy::Executor::run`
/// borrows its executor for as long as it runs, which is forever.
static SECOND_CORE_EXECUTOR: esp_hal::__macro_implementation::static_cell::StaticCell<
    esp_rtos::embassy::Executor,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

/// Boots the second APP core and runs `init` on a thread-mode executor there.
///
/// Lives beside [`start_rtos`]/[`start_feed_executor`] because starting a core is this
/// layer's job, not a probe's: the caller owns only the stack it hands in — a probe picks
/// that size on purpose, and a larger one would hide the reservation mistake the multicore
/// probe exists to expose — while building the executor and reaching for `esp_rtos` stays
/// here.
///
/// `intr1` is the SMP scheduler's, and the scheduler must already be running ([`start_rtos`]).
pub fn start_second_core<const STACK_SIZE: usize>(
    cpu_control: CPU_CTRL<'static>,
    intr1: FROM_CPU_INTR1<'static>,
    stack: &'static mut Stack<STACK_SIZE>,
    init: impl FnOnce(Spawner) + Send + 'static,
) {
    esp_rtos::start_second_core(cpu_control, intr1, stack, move || {
        let executor = SECOND_CORE_EXECUTOR.init(esp_rtos::embassy::Executor::new());
        executor.run(init);
    });
}

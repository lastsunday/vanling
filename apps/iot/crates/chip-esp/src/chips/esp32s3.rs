use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use crate::heap::Window;
use embassy_executor::{SendSpawner, Spawner};
use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::clock::CpuClock;
use esp_hal::peripherals::{
    CPU_CTRL, FROM_CPU_INTR0, FROM_CPU_INTR1, FROM_CPU_INTR2, LPWR, PSRAM, Peripherals, SYSTEM,
};
use esp_hal::psram::{Psram, PsramConfig};
use esp_hal::rom::ets_update_cpu_frequency_rom;
use esp_hal::system::Stack;
use iot_core::demand::{DemandKind, Speed, SpeedTable};

/// Brings the chip up at `clock`.
///
/// esp-hal's default is 80 MHz on a part rated for 240; every cost in this tree inherits
/// that. The presets pin `pll_clk` to 480 and differ only in the divider, so they do not
/// touch the PLL that USB Serial/JTAG depends on (`clock-probe` measured 240 MHz with the
/// console intact).
pub fn chip_init_at(clock: CpuClock) -> Peripherals {
    esp_app_desc!();
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(clock));
    init_psram();
    peripherals
}

pub fn chip_init() -> Peripherals {
    chip_init_at(CpuClock::_80MHz)
}

/// The clock the product boots at: 240 MHz, not the part's 80 MHz default.
///
/// The AEC's delay search already busts the 30 ms audio tick at 80 MHz, and the governor
/// only lowers from here.
pub fn chip_init_for_product() -> Peripherals {
    chip_init_at(CpuClock::_240MHz)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PsramWindow {
    Ready(Window),
    Absent,
    Unusable,
}

static PSRAM_HANDLE: esp_hal::__macro_implementation::static_cell::StaticCell<Psram> =
    esp_hal::__macro_implementation::static_cell::StaticCell::new();

const PSRAM_ABSENT: u8 = 0;
const PSRAM_UNUSABLE: u8 = 1;
const PSRAM_READY: u8 = 2;

static PSRAM_STATE: AtomicU8 = AtomicU8::new(PSRAM_ABSENT);
static PSRAM_START: AtomicUsize = AtomicUsize::new(0);
static PSRAM_LEN: AtomicUsize = AtomicUsize::new(0);

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

fn init_psram() {
    // `Psram::new` needs the PSRAM peripheral, which is one field of the `Peripherals` this
    // module returns: taking it out would be a partial move. esp-hal's `init` builds
    // `Peripherals` by stealing these singletons, so stealing it again yields the same handle.
    //
    // SAFETY: nothing downstream of `chip_init_at` reaches for PSRAM — every board
    // destructures with a `..` catch-all — so this handle is the only one touching those
    // registers.
    let psram = PSRAM_HANDLE.init(Psram::new(
        unsafe { PSRAM::steal() },
        PsramConfig::default(),
    ));
    let (start, len) = psram.raw_parts();

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
                    "[HEAP] mapped {} B at {:#x}, and the pattern did not read back",
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

fn reads_back(window: Window) -> bool {
    const SPAN: usize = 4 * 1024;
    const STEP: usize = 7;
    const BASE: usize = 11;

    let expected = |index: usize| (index * STEP + BASE) as u8;

    [0, window.len.saturating_sub(SPAN)].into_iter().all(|at| {
        // SAFETY: `at + SPAN` is inside the mapped window; the allocator that owns it starts
        // after the chip is up.
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

pub struct Speeds;

impl SpeedTable for Speeds {
    fn speed_for(kind: DemandKind) -> Speed {
        match kind {
            DemandKind::Interaction | DemandKind::Motion => Speed::Slow,
            DemandKind::Audio | DemandKind::Camera | DemandKind::Display => Speed::Fast,
        }
    }
}

/// Puts the chip at the clock [`Speed`] names.
pub fn set_speed(speed: Speed) -> Speed {
    // SAFETY: the callee's only requirement is interrupts disabled across the register
    // writes, which the critical section it takes itself provides.
    unsafe { set_cpu_clock(clock_for(speed)) };
    speed
}

const fn clock_for(speed: Speed) -> CpuClock {
    preset_for(speed).0
}

const fn preset_for(speed: Speed) -> (CpuClock, u32) {
    match speed {
        Speed::Slow => (CpuClock::_80MHz, 80),
        Speed::Normal => (CpuClock::_160MHz, 160),
        Speed::Fast => (CpuClock::_240MHz, 240),
    }
}

pub(crate) const fn clock_mhz(speed: Speed) -> u32 {
    preset_for(speed).1
}

/// Moves the CPU to another preset while running.
///
/// Outside what esp-hal supports: it seals the clock, so this writes the frequency, the LDO
/// divider and the ROM tick calibration directly, and leaves the voltage bias at init's 240 MHz
/// value (over-biased below it, the conservative direction). The PLL, APB and system timer are
/// untouched. Measured in `docs/content/records/iot/clock.md`.
///
/// # Safety
///
/// Interrupts must be disabled across the register writes, or a handler can see the LDO divider
/// and the frequency divider disagree. Only the calling core's ROM tick calibration is updated,
/// so a second core's `ets_delay_us` is mis-scaled afterwards.
pub unsafe fn set_cpu_clock(clock: CpuClock) {
    // esp-hal's mapping: `pll_clk` stays at 480 and the source is PLL, so only
    // `cpuperiod_sel` differs.
    let (period, mhz) = match clock {
        CpuClock::_80MHz => (0, 80),
        CpuClock::_160MHz => (1, 160),
        CpuClock::_240MHz => (2, 240),
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

pub fn start_rtos(timer: impl esp_rtos::TimerSource, int0: FROM_CPU_INTR0<'static>) {
    esp_rtos::start(timer, int0);
}

static FEED_EXECUTOR: esp_hal::__macro_implementation::static_cell::StaticCell<
    esp_rtos::embassy::InterruptExecutor<2>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

/// Starts the executor the speaker's feed loop runs on.
///
/// Not on the cooperative scheduler: a task there is held by the capture envelope (~17 ms) or a
/// panel write (up to 16 ms), and the capture clock is slaved to the transmit unit, so a late
/// feed is both a gap in the sound and a stalled microphone.
///
/// `FROM_CPU_INTR1` is what `esp_rtos::start_second_core` uses, so this takes INTR2. Priority 2
/// is one level above the thread; higher would starve input and render.
pub fn start_feed_executor(intr2: FROM_CPU_INTR2<'static>) -> SendSpawner {
    let executor = FEED_EXECUTOR.init(esp_rtos::embassy::InterruptExecutor::new(intr2));
    executor.start(esp_hal::interrupt::Priority::Priority2)
}

static SECOND_CORE_EXECUTOR: esp_hal::__macro_implementation::static_cell::StaticCell<
    esp_rtos::embassy::Executor,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

/// Boots the second APP core and runs `init` on a thread-mode executor there.
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

/// Holds the other core still, with this core's interrupts masked, across a flash write.
///
/// `esp-storage`'s `multicore_auto_park` lets interrupts back on between ROM calls, so a
/// pending scheduler interrupt can run while the other core is still parked and both wait
/// on each other forever (esp-rs/esp-hal#6438; this board stops dead milliseconds after a
/// credential is first saved).
pub fn with_other_core_parked<R>(f: impl FnOnce() -> R) -> R {
    critical_section::with(|_| {
        // SAFETY: `esp_rtos::start_second_core` consumed `CPU_CTRL`, and nothing else writes
        // the park registers after boot. `Cpu::other` names the core that is not running this
        // one, which is what `park_core` requires.
        let mut control = esp_hal::system::CpuControl::new(unsafe { CPU_CTRL::steal() });
        let held = esp_hal::system::Cpu::other().find(|core| esp_hal::system::is_running(*core));
        if let Some(core) = held {
            unsafe { control.park_core(core) };
        }
        let out = f();
        if let Some(core) = held {
            control.unpark_core(core);
        }
        out
    })
}

const PANEL_CORE_STACK_BYTES: usize = 24 * 1024;

/// What the second core's stack is filled with before it starts, so the deepest word it ever
/// wrote can be found afterwards. A whole word rather than a byte, because `0xA5` turns up in
/// ordinary data.
const STACK_PAINT: u8 = 0xA5;

/// The bytes above the stack's floor that the scan skips: esp-hal parks its entry closure
/// there before the core runs. A real overflow lands in the canary check, not in this reading.
const STACK_SCRUB_BYTES: usize = 512;

static PANEL_CORE_STACK: esp_hal::__macro_implementation::static_cell::StaticCell<
    Stack<PANEL_CORE_STACK_BYTES>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

static PANEL_CORE_STACK_BOTTOM: AtomicUsize = AtomicUsize::new(0);

/// How deep the second core's stack has been, in bytes, and how big it is.
///
/// `None` until [`start_panel_core`] has painted the stack. The reading is the historical
/// maximum and only meaningful from an executor on that core. The firmware's own guard is
/// `ESP_RTOS_CONFIG_SW_TASK_OVERFLOW_DETECTION`; this is headroom accounting.
pub fn panel_core_stack_peak() -> Option<(usize, usize)> {
    let bottom = PANEL_CORE_STACK_BOTTOM.load(Ordering::Relaxed);
    if bottom == 0 {
        return None;
    }
    let size = PANEL_CORE_STACK_BYTES;
    // Up from the floor to the first word that is no longer paint. The ceiling is not written
    // either, so walking down from it would stop at the first word. ~0.4 ms per 5 s report.
    let deepest = (STACK_SCRUB_BYTES..size)
        .step_by(4)
        .find(|&at| {
            // SAFETY: this is the module's own static stack, and a cooperative executor cannot
            // switch tasks between these reads.
            (0..4).any(|byte| unsafe {
                core::ptr::read_volatile((bottom as *const u8).add(at + byte)) != STACK_PAINT
            })
        })
        .unwrap_or(size);
    Some((size - deepest, size))
}

/// Boots the second core with a 24 KiB stack for the panel's transfers and every blocking
/// driver. That memory comes out of the main stack's wall — `.stack` fills what is left — so
/// this is a trade, not an addition. [`panel_core_stack_peak`] reports the depth it was
/// chosen for.
pub fn start_panel_core(
    cpu_control: CPU_CTRL<'static>,
    intr1: FROM_CPU_INTR1<'static>,
    init: impl FnOnce(Spawner) + Send + 'static,
) {
    let stack = PANEL_CORE_STACK.init(Stack::new());
    // SAFETY: the region is uninitialized and the second core has not started; it takes these
    // bytes as its stack after this write.
    unsafe {
        core::ptr::write_bytes(
            stack.mem.as_mut_ptr() as *mut u8,
            STACK_PAINT,
            PANEL_CORE_STACK_BYTES,
        );
    }
    PANEL_CORE_STACK_BOTTOM.store(stack.bottom() as usize, Ordering::Relaxed);
    start_second_core(cpu_control, intr1, stack, init);
}

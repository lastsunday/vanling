use embassy_executor::SendSpawner;
use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::clock::CpuClock;
use esp_hal::peripherals::{FROM_CPU_INTR0, FROM_CPU_INTR1, LPWR, Peripherals, SYSTEM};
use esp_hal::rom::ets_update_cpu_frequency_rom;
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
    esp_hal::init(esp_hal::Config::default().with_cpu_clock(clock))
}

pub fn chip_init() -> Peripherals {
    chip_init_at(CpuClock::_80MHz)
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

use embassy_executor::SendSpawner;
use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::clock::CpuClock;
use esp_hal::peripherals::{FROM_CPU_INTR0, FROM_CPU_INTR1, Peripherals};

/// Brings the chip up, optionally at a clock other than [`CpuClock`]'s default.
///
/// [`CpuClock`]'s `Default` arm is `_80MHz` on this part, so the default here is
/// 80 MHz on a chip rated for 240. That is a board-level fact every cost in this
/// tree inherits, and the only reason it is not simply the default is that
/// esp-hal notes its 160/240 presets change the PLL away from the 480 MHz the chip
/// boots on, which it says breaks USB Serial/JTAG -- and this repository's console
/// is USB Serial/JTAG. `[据称]` untested either way; the argument is esp-hal's
/// comment, not a measurement here.
pub fn chip_init_at(clock: CpuClock) -> Peripherals {
    esp_app_desc!();
    esp_hal::init(esp_hal::Config::default().with_cpu_clock(clock))
}

pub fn chip_init() -> Peripherals {
    chip_init_at(CpuClock::_80MHz)
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

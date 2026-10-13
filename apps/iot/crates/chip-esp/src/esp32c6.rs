use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals};
use iot_core::demand::{DemandField, Speed, SpeedTable};

pub fn chip_init() -> Peripherals {
    esp_app_desc!();
    esp_hal::init(esp_hal::Config::default())
}

pub fn init_logging() {
    log::set_logger(&crate::logging::LOGGER).expect("logger already set");
    log::set_max_level(log::LevelFilter::Info);
}

/// Hands the scheduler the timer and interrupt line wired up by the board.
pub fn start_rtos(timer: impl esp_rtos::TimerSource, int0: FROM_CPU_INTR0<'static>) {
    esp_rtos::start(timer, int0);
}

/// **No row here is measured.** Every entry is the pessimistic answer, and that
/// is deliberate: a worst case nobody has taken on this part is not a small one.
///
/// The S3's rows are wall-clock holds measured on the S3, so they say nothing
/// about how long this part holds — sourcing them here would import numbers that
/// do not transfer. Whoever measures a plane on this part fills its row and cites
/// where; until then nothing runs below the clock this part boots at.
pub struct Table;

impl SpeedTable for Table {
    fn speed_for(_field: DemandField) -> Speed {
        Speed::Fast
    }
}

/// Accepts the request and does not move the clock: esp-hal offers no runtime
/// clock change on this part.
///
/// [`CpuClock`] here is `_80MHz` and `_160MHz`, so [`Speed::Fast`] and
/// [`Speed::Normal`] are the same clock and [`Speed::Slow`] is the only other one.
/// Every arm answers `Normal` — the clock stays where [`chip_init`] left it, and
/// the caller is told which of the two it would have had.
pub fn set_speed(_speed: Speed) -> Speed {
    Speed::Normal
}

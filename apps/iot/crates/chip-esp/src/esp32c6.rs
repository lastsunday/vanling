use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals};
use iot_core::demand::{DemandKind, Speed, SpeedTable};

pub fn chip_init() -> Peripherals {
    esp_app_desc!();
    esp_hal::init(esp_hal::Config::default())
}

/// The same bring-up every image on this part uses: the product and its probes share one
/// clock because this part offers no runtime change to choose between.
pub fn chip_init_for_product() -> Peripherals {
    chip_init()
}

/// Hands the scheduler the timer and interrupt line wired up by the board.
pub fn start_rtos(timer: impl esp_rtos::TimerSource, int0: FROM_CPU_INTR0<'static>) {
    esp_rtos::start(timer, int0);
}

pub struct Table;

impl SpeedTable for Table {
    fn speed_for(_field: DemandKind) -> Speed {
        Speed::Fast
    }
}

/// Accepts the request and does not move the clock: esp-hal offers no runtime
/// clock change on this part.
///
/// [`esp_hal::clock::CpuClock`] here is `_80MHz` and `_160MHz`, so [`Speed::Fast`] and
/// [`Speed::Normal`] are the same clock and [`Speed::Slow`] is the only other one.
/// Every arm answers `Normal` — the clock stays where [`chip_init`] left it, and
/// the caller is told which of the two it would have had.
pub fn set_speed(_speed: Speed) -> Speed {
    Speed::Normal
}

/// The frequency this part boots at and stays at: [`set_speed`] moves nothing here, so this is
/// the only number a report can name.
pub(crate) const fn clock_mhz(_speed: Speed) -> u32 {
    160
}

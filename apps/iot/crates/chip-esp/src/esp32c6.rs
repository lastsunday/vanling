use esp_bootloader_esp_idf::esp_app_desc;
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals};

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

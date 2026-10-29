use esp_hal::gpio::{Input, InputConfig, Pull};
use iot_core::drivers::input::Button;

/// Momentary push button, active-low with an internal pull-up.
pub struct PullButton<'d> {
    input: Input<'d>,
}

impl PullButton<'static> {
    pub fn new(pin: impl esp_hal::gpio::InputPin + 'static) -> Self {
        let config = InputConfig::default().with_pull(Pull::Up);
        Self {
            input: Input::new(pin, config),
        }
    }
}

impl Button for PullButton<'_> {
    fn is_pressed(&self) -> bool {
        self.input.is_low()
    }
}

impl crate::hardware::SelfCheck for PullButton<'_> {
    fn name(&self) -> &'static str {
        "button"
    }

    fn check(&mut self) -> crate::hardware::Status {
        crate::hardware::Status::declared()
    }
}

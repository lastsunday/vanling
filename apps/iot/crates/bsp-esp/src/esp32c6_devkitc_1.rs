use alloc::{boxed::Box, vec, vec::Vec};
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals};
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal_smartled::buffer_size;
use iot_core::drivers::board::Board as BoardTrait;
use iot_core::drivers::input::{BUTTON_SCAN_MS, ButtonScanner, DoubleClickAggregator, PollEntry};
use smart_leds::RGB8;

pub use crate::components::button::PullButton;
pub use crate::components::ws2812::{RMT_FREQ_HZ, Ws2812RgbLed};

/// How many RGB blocks are on the board (DevKitC-1 has a single addressable LED).
const LEDS: usize = 1;

/// Board-level support for the ESP32-C6-DevKitC-1.
pub struct Board<'d> {
    /// The onboard WS2812 addressable RGB LED.
    pub light: Option<Ws2812RgbLed<'d, { buffer_size::<RGB8>(LEDS) }>>,
    /// The onboard boot button.
    pub button: Option<PullButton<'d>>,
}

/// Completion of the chip-level wiring, handed to the application entry point.
pub type Startup<'a> = (
    Board<'a>,
    TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    FROM_CPU_INTR0<'static>,
);

impl Board<'static> {
    /// Wire up the board from the chip's remaining `Peripherals`.
    pub fn new(peripherals: Peripherals) -> Result<Startup<'static>, esp_hal_smartled::Error> {
        #[allow(non_snake_case)]
        let Peripherals {
            RMT,
            GPIO8,
            GPIO9,
            TIMG0,
            FROM_CPU_INTR0,
            ..
        } = peripherals;

        let rmt = Rmt::new(RMT, Rate::from_hz(RMT_FREQ_HZ)).expect("failed to initialize RMT");
        let light = Ws2812RgbLed::new(rmt.channel0, GPIO8, LEDS)?;
        let button = PullButton::new(GPIO9);
        let timg0 = TimerGroup::new(TIMG0);

        Ok((
            Self {
                light: Some(light),
                button: Some(button),
            },
            timg0,
            FROM_CPU_INTR0,
        ))
    }
}

impl BoardTrait for Board<'static> {}

impl iot_core::drivers::board::HasLight for Board<'static> {
    type Light = Ws2812RgbLed<'static, { buffer_size::<RGB8>(LEDS) }>;

    fn take_lights(&mut self) -> Option<Vec<Self::Light>> {
        self.light.take().map(|light| vec![light])
    }
}

impl iot_core::drivers::board::HasInput for Board<'static> {
    fn take_input(&mut self) -> Option<Vec<PollEntry>> {
        self.button.take().map(|button| {
            vec![PollEntry::new(
                0,
                Box::new(ButtonScanner::new(button)),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            )]
        })
    }
}

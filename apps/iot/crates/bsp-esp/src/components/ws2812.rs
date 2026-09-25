use esp_hal::Blocking;
use esp_hal::gpio::interconnect::PeripheralOutput;
use esp_hal::rmt::TxChannelCreator;
use esp_hal::time::Rate;
use esp_hal_smartled::{RmtSmartLeds, WS2812_TIMING, color_order};
use iot_core::diagnostics::DiagnosticsSink;
use iot_core::drivers::light::{Fill, Rgb, RgbLight};
use smart_leds::{RGB8, SmartLedsWrite};

/// RMT base clock the WS2812 timing table is calibrated against; the RMT
/// peripheral on the board must be initialized at the same frequency.
pub const RMT_FREQ_HZ: u32 = 80_000_000;

/// WS2812 addressable RGB LED strip. `BUFFER_SIZE` is the RMT pulse buffer
/// size for the whole strip (`buffer_size::<RGB8>(leds)`), resolved by the
/// board wiring.
pub struct Ws2812RgbLed<'d, const BUFFER_SIZE: usize> {
    driver: RmtSmartLeds<'d, BUFFER_SIZE, Blocking, RGB8, color_order::Grb>,
    led_count: usize,
}

impl<'d, const BUFFER_SIZE: usize> Ws2812RgbLed<'d, BUFFER_SIZE> {
    pub fn new<Ch, P>(
        channel: Ch,
        pin: P,
        led_count: usize,
    ) -> Result<Self, esp_hal_smartled::Error>
    where
        Ch: TxChannelCreator<'d, Blocking>,
        P: PeripheralOutput<'d>,
    {
        let clock = Rate::from_hz(RMT_FREQ_HZ);
        let driver = RmtSmartLeds::new_with_memsize(WS2812_TIMING, channel, pin, 2, clock)?;
        Ok(Self { driver, led_count })
    }
}

impl<const BUFFER_SIZE: usize> RgbLight for Ws2812RgbLed<'_, BUFFER_SIZE> {
    fn set_fill(&mut self, _fill: Fill, color: Rgb) {
        let data = RGB8::new(color.0, color.1, color.2);
        let strip = core::iter::repeat_n(data, self.led_count);
        self.driver
            .write(strip)
            .expect("failed to write WS2812 LED");
    }
}

// A plain strip has no digits to overlay; the trait's default no-op sink keeps
// the surface usable as a diagnostic-free light.
impl<const BUFFER_SIZE: usize> DiagnosticsSink for Ws2812RgbLed<'_, BUFFER_SIZE> {}

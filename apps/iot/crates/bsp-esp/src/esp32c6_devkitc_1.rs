use crate::hardware;
use alloc::{boxed::Box, vec, vec::Vec};
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals, WIFI};
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal_smartled::buffer_size;
use esp_storage::FlashStorage;

use crate::components::credentials::CredentialStore;
use iot_core::drivers::board::{Board as BoardTrait, HasWifi};
use iot_core::drivers::input::PollEntry;
use iot_core::drivers::motion::MotionCapabilities;
use iot_core::drivers::network::SetupHost;
use iot_core::drivers::playback::UnwiredSpeaker;
use iot_core::drivers::surface::WiredSurface;
use smart_leds::RGB8;

pub use crate::components::button::PullButton;
pub use crate::components::ws2812::{RMT_FREQ_HZ, Ws2812RgbLed};

const LEDS: usize = 1;

pub const SETUP_HOST: SetupHost = SetupHost::AccessPoint;

/// Board-level support for the ESP32-C6-DevKitC-1.
pub struct Board<'d> {
    /// The onboard WS2812 addressable RGB LED.
    pub light: Option<Ws2812RgbLed<'d, { buffer_size::<RGB8>(LEDS) }>>,
    /// The onboard boot button.
    pub button: Option<PullButton<'d>>,
    radio: Option<WIFI<'static>>,
    credentials: Option<Box<CredentialStore>>,
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
            WIFI,
            FLASH,
            ..
        } = peripherals;

        let credentials =
            crate::components::credentials::load(FlashStorage::new(FLASH)).map(Box::new);

        let rmt = Rmt::new(RMT, Rate::from_hz(RMT_FREQ_HZ)).expect("failed to initialize RMT");
        let mut light = Ws2812RgbLed::new(rmt.channel0, GPIO8, LEDS)?;
        let mut button = PullButton::new(GPIO9);
        let timg0 = TimerGroup::new(TIMG0);

        hardware::identity();
        hardware::Report::of([&mut light, &mut button]).finish();

        Ok((
            Self {
                light: Some(light),
                button: Some(button),
                radio: Some(WIFI),
                credentials,
            },
            timg0,
            FROM_CPU_INTR0,
        ))
    }

    /// The saved networks, for the link that keeps them.
    pub fn take_credentials(&mut self) -> Option<Box<CredentialStore>> {
        self.credentials.take()
    }
}

impl BoardTrait for Board<'static> {}

impl iot_core::drivers::board::HasSurface for Board<'static> {
    fn take_surfaces(&mut self) -> Option<Vec<WiredSurface>> {
        self.light
            .take()
            .map(|light| vec![WiredSurface::Lamp(Box::new(light))])
    }
}

impl iot_core::drivers::board::HasInput for Board<'static> {
    fn take_input(&mut self) -> Option<Vec<PollEntry>> {
        self.button
            .take()
            .map(|button| vec![PollEntry::button(0, button)])
    }
}

impl iot_core::drivers::board::HasMotion for Board<'static> {
    fn take_motion(&mut self) -> Option<PollEntry> {
        None
    }

    fn motion_capabilities(&self) -> MotionCapabilities {
        MotionCapabilities::EMPTY
    }
}

impl iot_core::drivers::board::HasAudio for Board<'static> {
    fn take_audio(
        &mut self,
        _sink: &'static dyn iot_core::drivers::audio::CaptureSink,
    ) -> Option<PollEntry> {
        None
    }
}

impl iot_core::drivers::board::HasPlayback for Board<'static> {
    type Speaker = UnwiredSpeaker;

    fn take_playback(&mut self) -> Option<Box<Self::Speaker>> {
        None
    }
}

impl iot_core::drivers::board::HasCamera for Board<'static> {
    fn take_camera(&mut self) -> Option<Box<dyn iot_core::drivers::camera::FrameSource>> {
        None
    }
}

impl HasWifi for Board<'static> {
    type Station = crate::components::wifi::EspStation;

    fn take_wifi(&mut self) -> Option<Box<Self::Station>> {
        iot_chip_esp::ungate_radio();
        self.radio.take().and_then(crate::components::wifi::start)
    }
}

use crate::post::{self, Provenance, State};
use alloc::{boxed::Box, vec, vec::Vec};
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals, WIFI};
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal_smartled::buffer_size;
use esp_storage::FlashStorage;

use crate::components::credentials::CredentialStore;
use iot_core::demand::PlaneKey;
use iot_core::drivers::board::{Board as BoardTrait, HasWifi};
use iot_core::drivers::input::{BUTTON_SCAN_MS, ButtonScanner, DoubleClickAggregator, PollEntry};
use iot_core::drivers::motion::MotionCapabilities;
use iot_core::drivers::playback::UnwiredSpeaker;
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
    /// The on-board radio peripheral, unopened. It stays here rather than being built in
    /// `new` because the driver's start needs the scheduler, and the scheduler is the app's
    /// to start — see [`HasWifi`].
    radio: Option<WIFI<'static>>,
    /// The saved networks, over the partition this board's table gives them. Read at bring-up
    /// and handed to the link by [`Board::take_credentials`].
    ///
    /// Boxed because of where it travels: the link's futures are polled inside the one task
    /// the whole product shares, so every frame that carries the list by value is that many
    /// hundred bytes of the main stack's own region — and the region is what the static data
    /// around a 64 KB stack has to fit in.
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

        // Read before the LED and the button: a partition that will not read is said while
        // the boot still has the frame for the table. No auto-parking here — this part has one
        // core, so there is no other one to park; see the S3 board for the other arm.
        let credentials =
            crate::components::credentials::load(FlashStorage::new(FLASH)).map(Box::new);

        let rmt = Rmt::new(RMT, Rate::from_hz(RMT_FREQ_HZ)).expect("failed to initialize RMT");
        let light = Ws2812RgbLed::new(rmt.channel0, GPIO8, LEDS)?;
        let button = PullButton::new(GPIO9);
        let timg0 = TimerGroup::new(TIMG0);

        // This board reports almost nothing, and that is the point of the
        // report: **the product's panel, microphones, speaker, touch and camera
        // simply are not here**, so a reader who sees two `[POST]` blocks from
        // two builds knows which hardware each number came from without having
        // to know the feature sets.
        post::identity();
        post::line("rgb-led", State::Found, Provenance::Asserted);
        post::line("button", State::Found, Provenance::Asserted);
        for name in ["panel", "touch", "motion", "mic", "speaker", "camera"] {
            post::line(name, State::Absent, Provenance::Asserted);
        }
        post::summary(2, 0, 6);

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
    ///
    /// Taken rather than borrowed: the task that holds the link is the one that writes a
    /// credential the panel named, and it is the only writer there is.
    pub fn take_credentials(&mut self) -> Option<Box<CredentialStore>> {
        self.credentials.take()
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
                PlaneKey::INTERACTION,
                Box::new(ButtonScanner::new(button)),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            )]
        })
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
        self.radio.take().and_then(crate::components::wifi::start)
    }
}

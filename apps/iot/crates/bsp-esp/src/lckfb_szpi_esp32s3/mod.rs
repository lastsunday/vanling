use crate::hardware;
use alloc::{boxed::Box, vec, vec::Vec};
use core::cell::RefCell;
use embassy_embedded_hal::shared_bus::I2cDeviceError;
use embassy_embedded_hal::shared_bus::blocking::i2c::I2cDevice;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embedded_hal::spi::SpiBus;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::dma::DmaBufError;
use esp_hal::gpio::{DriveMode, Level, Output, OutputConfig};
use esp_hal::i2c::master as i2c_master;
use esp_hal::i2s::master as i2s_master;
use esp_hal::ledc::channel as ledc_channel;
use esp_hal::ledc::channel::ChannelIFace as _;
use esp_hal::ledc::timer as ledc_timer;
use esp_hal::ledc::timer::TimerIFace as _;
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::peripherals::{
    DMA_CH0, FROM_CPU_INTR0, FROM_CPU_INTR1, GPIO39, GPIO40, GPIO41, GPIO42, LEDC, Peripherals,
    SPI3, WIFI,
};
use esp_hal::spi::master as spi_master;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{dma_rx_buffer, dma_tx_buffer};
use esp_storage::FlashStorage;
use iot_core::config::MotionConfig;
use iot_core::demand::DemandKey;
use iot_core::drivers::audio::{AudioInput, CAPTURE_MS, CaptureSink};
use iot_core::drivers::board::Board as BoardTrait;
use iot_core::drivers::board::{HasAudio, HasCamera, HasPlayback, HasWifi};
use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::input::{
    BUTTON_SCAN_MS, ButtonScanner, DoubleClickAggregator, PassThrough, PollEntry, TOUCH_SCAN_MS,
    TouchGestures,
};
use iot_core::drivers::motion::{MOTION_SCAN_MS, MotionCapabilities, MotionInput};
use iot_core::drivers::network::Front;
use iot_core::drivers::surface::WiredSurface;

use crate::components::backlight::Backlight;
pub use crate::components::button::PullButton;
use crate::components::credentials::CredentialStore;
use crate::components::es7210::{ES7210_I2C_ADDR, Es7210};
use crate::components::es8311::{ChipId, ES8311_I2C_ADDR, ES8311_I2C_ADDR_ALT, Es8311};
use crate::components::ft6336::{self, FT6336_I2C_ADDR, Ft6336};
use crate::components::pca9557::Pca9557;
use crate::components::qmi8658::{
    MOTION_CAPABILITIES, QMI8658_I2C_ADDR, Qmi8658, RecoverableQmi, SUPPORTED_CONFIGS,
};
use crate::components::st7789::{SPI_FREQ_HZ, SPI_MODE, St7789, St7789Error};
use crate::virtual_components::Es7210Rx;
pub use crate::virtual_components::Screen;
use crate::virtual_components::audio as capture;
use crate::virtual_components::audio_out as playback;
use crate::virtual_components::reference;

mod camera;

use camera::frame_arena;

const PCA9557_I2C_ADDR: u8 = 0x19;
const LCD_CS_BIT: u8 = 1 << 0;
const DVP_PWDN_BIT: u8 = 1 << 2;
const PA_EN_PIN: u8 = 1;

const LCD_WIDTH: u16 = 320;
const LCD_HEIGHT: u16 = 240;

const _: () = assert!(
    LCD_WIDTH as usize == iot_core::overlay::PANEL_COLUMNS
        && LCD_HEIGHT as usize == iot_core::overlay::PANEL_ROWS,
    "the panel this board mounts is not the shape the diagnostics layout is written for"
);

const PCA9557_RETRY_ATTEMPTS: u8 = 5;
const PCA9557_RETRY_MS: u32 = 20;
const MOTION_RETRY_ATTEMPTS: u8 = 3;
const MOTION_RETRY_MS: u32 = 10;
const SPI_DMA_BUF_BYTES: usize = 4096;
const BACKLIGHT_PWM_HZ: u32 = 5_000;

static BACKLIGHT_TIMER: esp_hal::__macro_implementation::static_cell::StaticCell<
    ledc_timer::Timer<'static, LowSpeed>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

static I2C_BUS: esp_hal::__macro_implementation::static_cell::StaticCell<SharedI2cBus> =
    esp_hal::__macro_implementation::static_cell::StaticCell::new();

type SharedI2cBus = Mutex<CriticalSectionRawMutex, RefCell<i2c_master::I2c<'static, Blocking>>>;

pub type SharedI2cDevice =
    I2cDevice<'static, CriticalSectionRawMutex, i2c_master::I2c<'static, Blocking>>;

/// Board bring-up failure, aggregating every peripheral error source.
#[derive(Debug)]
pub enum BoardError {
    I2cConfig(i2c_master::ConfigError),
    CameraDma(DmaBufError),
    CameraConfig(esp_hal::lcd_cam::cam::ConfigError),
    I2c(I2cDeviceError<i2c_master::Error>),
    SpiConfig(spi_master::ConfigError),
    Spi(esp_hal::spi::Error),
    Dma(DmaBufError),
    DisplayConfig,
    LedcTimer(ledc_timer::Error),
    LedcChannel(ledc_channel::Error),
    I2sConfig(i2s_master::ConfigError),
    I2sStart(i2s_master::Error),
}

impl From<St7789Error> for BoardError {
    fn from(e: St7789Error) -> Self {
        match e {
            St7789Error::Spi(e) => BoardError::Spi(e),
            St7789Error::Config => BoardError::DisplayConfig,
        }
    }
}

impl From<I2cDeviceError<i2c_master::Error>> for BoardError {
    fn from(e: I2cDeviceError<i2c_master::Error>) -> Self {
        BoardError::I2c(e)
    }
}

impl From<DmaBufError> for BoardError {
    fn from(e: DmaBufError) -> Self {
        BoardError::Dma(e)
    }
}

fn panel_spi(
    spi3: SPI3<'static>,
    dma_ch0: DMA_CH0<'static>,
    sck: GPIO41<'static>,
    mosi: GPIO40<'static>,
    dc: GPIO39<'static>,
) -> Result<(spi_master::SpiDma<'static, Blocking>, Output<'static>), BoardError> {
    let mut spi = spi_master::Spi::new(
        spi3,
        spi_master::Config::default()
            .with_frequency(Rate::from_hz(SPI_FREQ_HZ))
            .with_mode(SPI_MODE),
    )
    .map_err(BoardError::SpiConfig)?
    .with_sck(sck)
    .with_mosi(mosi)
    .with_dma(dma_ch0)
    .with_buffers(
        dma_rx_buffer!(SPI_DMA_BUF_BYTES)?,
        dma_tx_buffer!(SPI_DMA_BUF_BYTES)?,
    );
    let dc = Output::new(dc, Level::Low, OutputConfig::default());
    SpiBus::write(&mut spi, &[0x01]).map_err(BoardError::Spi)?;
    Ok((spi, dc))
}

fn panel_backlight(
    ledc_peripheral: LEDC<'static>,
    channel_pin: GPIO42<'static>,
) -> Result<Backlight, BoardError> {
    let mut ledc = Ledc::new(ledc_peripheral);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let timer = BACKLIGHT_TIMER.init(ledc.timer::<LowSpeed>(ledc_timer::Number::Timer0));
    timer
        .configure(ledc_timer::config::Config {
            duty: ledc_timer::config::Duty::Duty10Bit,
            clock_source: ledc_timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(BACKLIGHT_PWM_HZ),
        })
        .map_err(BoardError::LedcTimer)?;
    let mut channel = ledc.channel::<LowSpeed>(ledc_channel::Number::Channel0, channel_pin);
    channel
        .configure(ledc_channel::config::Config {
            timer: &*timer,
            duty_pct: 0,
            drive_mode: DriveMode::PushPull,
        })
        .map_err(BoardError::LedcChannel)?;
    Ok(Backlight::new(channel))
}

fn retry<E>(
    attempts: u8,
    delay_ms: u32,
    delay: &mut Delay,
    mut op: impl FnMut(&mut Delay) -> Result<(), E>,
) -> Option<E> {
    let mut last = None;
    for attempt in 0..attempts {
        match op(delay) {
            Ok(()) => return None,
            Err(error) => {
                last = Some(error);
                if attempt + 1 < attempts {
                    delay.delay_millis(delay_ms);
                }
            }
        }
    }
    last
}

fn bring_up_i2c(
    i2c: i2c_master::I2c<'static, Blocking>,
) -> Result<(&'static SharedI2cBus, Pca9557<SharedI2cDevice>), BoardError> {
    let bus = I2C_BUS.init(Mutex::new(RefCell::new(i2c)));
    let mut pca9557 = Pca9557::new(I2cDevice::new(bus), PCA9557_I2C_ADDR);
    let mut delay = Delay::new();
    match retry(PCA9557_RETRY_ATTEMPTS, PCA9557_RETRY_MS, &mut delay, |_| {
        pca9557.init(LCD_CS_BIT | DVP_PWDN_BIT, 0xf8)
    }) {
        Some(error) => Err(BoardError::I2c(error)),
        None => Ok((bus, pca9557)),
    }
}

fn probe_es8311(bus: &'static SharedI2cBus, absent_note: &str) -> Option<Es8311<SharedI2cDevice>> {
    let mut es8311 = Es8311::new(I2cDevice::new(bus), ES8311_I2C_ADDR);
    let mut es8311_alt = Es8311::new(I2cDevice::new(bus), ES8311_I2C_ADDR_ALT);
    if es8311.chip_id().is_ok_and(ChipId::is_expected) {
        es8311.init().ok().map(|()| es8311)
    } else if es8311_alt.chip_id().is_ok_and(ChipId::is_expected) {
        es8311_alt.init().ok().map(|()| es8311_alt)
    } else {
        log::warn!("[PLAY] ES8311 not found, {absent_note}");
        None
    }
}

/// The front this board opens setup mode on: it has a panel, so the page is where a person
/// configures it.
pub const FRONT: Front = Front::Panel;

/// Board-level support for the LCKFB SZPI ESP32-S3 display board.
pub struct Board<'d> {
    screen: Option<Box<Screen>>,
    button: Option<PullButton<'d>>,
    touch: Option<Ft6336<SharedI2cDevice>>,
    motion: Option<Qmi8658<SharedI2cDevice>>,
    audio: Option<Box<Es7210Rx<Es7210<SharedI2cDevice>, { playback::REFERENCE_SAMPLES }>>>,
    speaker: Option<Box<playback::Es8311Tx<Es8311<SharedI2cDevice>>>>,
    camera: Option<Box<dyn FrameSource>>,
    radio: Option<WIFI<'static>>,
    credentials: Option<Box<CredentialStore>>,
}

/// Completion of the chip-level wiring, handed to the application entry point.
pub type Startup<'a> = (
    Board<'a>,
    TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    FROM_CPU_INTR0<'static>,
    FROM_CPU_INTR1<'static>,
);

impl Board<'static> {
    pub fn new(peripherals: Peripherals) -> Result<Startup<'static>, BoardError> {
        #[allow(non_snake_case)]
        let Peripherals {
            I2C0,
            I2S0,
            SPI3,
            LEDC,
            GPIO1,
            GPIO2,
            GPIO12,
            GPIO13,
            GPIO14,
            GPIO38,
            GPIO39,
            GPIO40,
            GPIO41,
            GPIO42,
            GPIO45,
            GPIO0,
            TIMG0,
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
            DMA_CH0,
            DMA_CH1,
            LCD_CAM,
            DMA_CH2,
            GPIO3,
            GPIO4,
            GPIO5,
            GPIO6,
            GPIO7,
            GPIO8,
            GPIO9,
            GPIO15,
            GPIO16,
            GPIO17,
            GPIO18,
            GPIO46,
            WIFI,
            FLASH,
            ..
        } = peripherals;

        let credentials =
            crate::components::credentials::load(FlashStorage::new(FLASH).multicore_auto_park())
                .map(Box::new);

        let mut backlight = panel_backlight(LEDC, GPIO42)?;
        backlight.set_level_pct(0);

        let (bus, mut pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;
        let mut delay = Delay::new();

        let (block_spi, dc) = panel_spi(SPI3, DMA_CH0, GPIO41, GPIO40, GPIO39)?;

        let dvp_peripheral = Board::new_dvp(camera::CameraPins {
            LCD_CAM,
            DMA_CH2,
            GPIO3,
            GPIO4,
            GPIO5,
            GPIO6,
            GPIO7,
            GPIO8,
            GPIO9,
            GPIO15,
            GPIO16,
            GPIO17,
            GPIO18,
            GPIO46,
        })?;

        pca9557.set_output(DVP_PWDN_BIT)?;

        let panel = St7789::new(block_spi, dc, LCD_WIDTH, LCD_HEIGHT)?;
        let mut screen = Box::new(Screen::new(0, panel, backlight, &mut frame_arena().0));
        let frame = screen.frame();
        let mut camera = match Board::new_camera(&mut pca9557, bus, dvp_peripheral, frame) {
            Ok(camera) => Some(camera),
            Err(error) => {
                log::warn!("[CAM] camera bring-up failed, the Camera page stays dark: {error:?}");
                None
            }
        };
        let mut button = PullButton::new(GPIO0);
        let mut touch = Ft6336::new(
            I2cDevice::new(bus),
            FT6336_I2C_ADDR,
            ft6336::LANDSCAPE_MAP,
            LCD_WIDTH,
            LCD_HEIGHT,
        );
        let mut motion = Qmi8658::new(I2cDevice::new(bus), QMI8658_I2C_ADDR);
        if let Some(error) = retry(
            MOTION_RETRY_ATTEMPTS,
            MOTION_RETRY_MS,
            &mut delay,
            |delay| motion.init(delay),
        ) {
            log::warn!("[MOTION] QMI8658 init deferred, retrying from the first poll: {error:?}");
        }

        let mut es7210 = Es7210::new(I2cDevice::new(bus), ES7210_I2C_ADDR);
        let capture_codec = match es7210.init() {
            Ok(()) => Some(es7210),
            Err(error) => {
                log::warn!("[AUDIO] ES7210 not found, the Audio page stays dark: {error:?}");
                None
            }
        };

        let speaker_codec = probe_es8311(bus, "the Speaker page stays dark");

        let shared_clocks = speaker_codec.is_some();
        let config = if shared_clocks {
            playback::shared_tdm_config()
        } else {
            capture::tdm_config()
        };
        let (mut audio, mut speaker) = match (capture_codec, speaker_codec) {
            (capture_codec, speaker_codec)
                if capture_codec.is_some() || speaker_codec.is_some() =>
            {
                let i2s = i2s_master::I2s::new(I2S0, DMA_CH1, config)
                    .map_err(BoardError::I2sConfig)?
                    .with_mclk(GPIO38);
                let (tx, rx) = if shared_clocks {
                    let tx = Some(
                        i2s.i2s_tx
                            .with_bclk(GPIO14)
                            .with_ws(GPIO13)
                            .with_dout(GPIO45)
                            .build(),
                    );
                    let rx = i2s.i2s_rx.with_din(GPIO12).build();
                    (tx, rx)
                } else {
                    (
                        None,
                        i2s.i2s_rx
                            .with_bclk(GPIO14)
                            .with_ws(GPIO13)
                            .with_din(GPIO12)
                            .build(),
                    )
                };
                // One cell both audio paths reach: the speaker fills it and the
                // microphone reads it. Made here because this is the only place
                // both drivers are visible — see `virtual_components::reference`
                // for why it has to be shared rather than owned by either.
                let reference = reference::share::<{ playback::REFERENCE_SAMPLES }>();
                let audio = match capture_codec {
                    Some(codec) => {
                        let transfer = rx
                            .read(capture::stream())
                            .map_err(|(error, _, _)| BoardError::I2sStart(error))?;
                        Some(Box::new(Es7210Rx::new(
                            transfer,
                            codec,
                            reference::Side::wired(reference),
                        )))
                    }
                    None => None,
                };
                let speaker = match (tx, speaker_codec) {
                    (Some(tx), Some(codec)) => {
                        pca9557.set_output_bit(PA_EN_PIN, true)?;
                        match playback::Es8311Tx::new(tx, codec, reference::Writer::new(reference))
                        {
                            Ok(speaker) => Some(Box::new(speaker)),
                            Err(error) => {
                                log::error!("[PLAY] stream would not start: {error:?}");
                                None
                            }
                        }
                    }
                    _ => None,
                };
                (audio, speaker)
            }
            _ => (None, None),
        };

        hardware::identity();
        hardware::Report::of([&mut *screen, &mut button, &mut touch, &mut motion])
            .include(audio.as_deref_mut())
            .include(speaker.as_mut())
            .include(camera.as_mut())
            .finish();

        Ok((
            Self {
                screen: Some(screen),
                button: Some(button),
                touch: Some(touch),
                motion: Some(motion),
                audio,
                speaker,
                camera: camera.map(|camera| Box::new(camera) as Box<dyn FrameSource>),
                radio: Some(WIFI),
                credentials,
            },
            TimerGroup::new(TIMG0),
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
        ))
    }

    pub fn new_motion_only(peripherals: Peripherals) -> Result<Startup<'static>, BoardError> {
        #[allow(non_snake_case)]
        let Peripherals {
            I2C0,
            GPIO1,
            GPIO2,
            TIMG0,
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
            ..
        } = peripherals;

        let (bus, _pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;

        let motion = Qmi8658::new(I2cDevice::new(bus), QMI8658_I2C_ADDR);

        Ok((
            Self {
                screen: None,
                button: None,
                touch: None,
                motion: Some(motion),
                audio: None,
                speaker: None,
                camera: None,
                radio: None,
                credentials: None,
            },
            TimerGroup::new(TIMG0),
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
        ))
    }

    pub fn new_audio_only(peripherals: Peripherals) -> Result<Startup<'static>, BoardError> {
        #[allow(non_snake_case)]
        let Peripherals {
            I2C0,
            I2S0,
            GPIO1,
            GPIO2,
            GPIO13,
            GPIO14,
            GPIO38,
            GPIO45,
            TIMG0,
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
            DMA_CH1,
            ..
        } = peripherals;

        let (bus, mut pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;

        let speaker_codec = probe_es8311(bus, "the probe has nothing to drive");

        let speaker = match speaker_codec {
            Some(codec) => {
                pca9557.set_output_bit(PA_EN_PIN, true)?;
                let i2s = i2s_master::I2s::new(I2S0, DMA_CH1, playback::shared_tdm_config())
                    .map_err(BoardError::I2sConfig)?
                    .with_mclk(GPIO38);
                let tx = i2s
                    .i2s_tx
                    .with_bclk(GPIO14)
                    .with_ws(GPIO13)
                    .with_dout(GPIO45)
                    .build();
                match playback::Es8311Tx::new(
                    tx,
                    codec,
                    reference::Writer::new(reference::share::<{ playback::REFERENCE_SAMPLES }>()),
                ) {
                    Ok(speaker) => Some(Box::new(speaker)),
                    Err(error) => {
                        log::error!("[PLAY] stream would not start: {error:?}");
                        None
                    }
                }
            }
            None => None,
        };

        Ok((
            Self {
                screen: None,
                button: None,
                touch: None,
                motion: None,
                audio: None,
                speaker,
                camera: None,
                radio: None,
                credentials: None,
            },
            TimerGroup::new(TIMG0),
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
        ))
    }
}

impl Board<'static> {
    /// The IMU driver itself, for a probe that walks configurations.
    pub fn take_motion_driver(&mut self) -> Option<Qmi8658<SharedI2cDevice>> {
        self.motion.take()
    }

    /// The saved networks, for the link that keeps them.
    pub fn take_credentials(&mut self) -> Option<Box<CredentialStore>> {
        self.credentials.take()
    }

    /// The capture driver itself, for a probe that has to read the canceller's own numbers.
    pub fn take_capture(&mut self) -> Option<Box<dyn crate::virtual_components::EchoCapture>> {
        self.audio
            .take()
            .map(|audio| audio as Box<dyn crate::virtual_components::EchoCapture>)
    }
}

impl BoardTrait for Board<'static> {}

impl iot_core::drivers::board::HasSurface for Board<'static> {
    fn take_surfaces(&mut self) -> Option<Vec<WiredSurface>> {
        self.screen
            .take()
            .map(|screen| vec![WiredSurface::Pages(screen)])
    }
}

impl iot_core::drivers::board::HasInput for Board<'static> {
    fn take_input(&mut self) -> Option<Vec<PollEntry>> {
        let mut entries = Vec::new();
        if let Some(button) = self.button.take() {
            entries.push(PollEntry::new(
                0,
                DemandKey::INTERACTION,
                Box::new(ButtonScanner::new(button)),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            ));
        }
        if let Some(touch) = self.touch.take() {
            entries.push(PollEntry::new(
                0,
                DemandKey::INTERACTION,
                Box::new(touch),
                Box::new(TouchGestures::new()),
                TOUCH_SCAN_MS,
            ));
        }
        (!entries.is_empty()).then_some(entries)
    }
}

impl iot_core::drivers::board::HasMotion for Board<'static> {
    fn take_motion(&mut self) -> Option<PollEntry> {
        self.motion.take().map(|motion| {
            PollEntry::new(
                1,
                DemandKey::MOTION,
                Box::new(MotionInput::new(RecoverableQmi::new(motion, Delay::new()))),
                Box::new(PassThrough),
                MOTION_SCAN_MS,
            )
        })
    }

    fn motion_capabilities(&self) -> MotionCapabilities {
        MOTION_CAPABILITIES
    }

    fn motion_configurations(&self) -> &'static [MotionConfig] {
        SUPPORTED_CONFIGS
    }
}

impl HasPlayback for Board<'static> {
    type Speaker = playback::Es8311Tx<Es8311<SharedI2cDevice>>;

    fn take_playback(&mut self) -> Option<Box<Self::Speaker>> {
        self.speaker.take()
    }
}

impl HasAudio for Board<'static> {
    fn take_audio(&mut self, sink: &'static dyn CaptureSink) -> Option<PollEntry> {
        self.audio.take().map(|audio| {
            PollEntry::new(
                2,
                DemandKey::AUDIO,
                Box::new(AudioInput::new(*audio, sink)),
                Box::new(PassThrough),
                CAPTURE_MS,
            )
        })
    }
}

impl HasCamera for Board<'static> {
    fn take_camera(&mut self) -> Option<Box<dyn FrameSource>> {
        self.camera.take()
    }
}

impl HasWifi for Board<'static> {
    type Station = crate::components::wifi::EspStation;

    fn take_wifi(&mut self) -> Option<Box<Self::Station>> {
        iot_chip_esp::ungate_radio();
        self.radio.take().and_then(crate::components::wifi::start)
    }
}

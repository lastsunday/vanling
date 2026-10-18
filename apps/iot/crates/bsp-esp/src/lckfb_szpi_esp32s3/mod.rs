use crate::post::{self, Provenance, State};
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
use iot_core::demand::PlaneKey;
use iot_core::drivers::audio::{AudioInput, CAPTURE_MS, CaptureSink};
use iot_core::drivers::board::Board as BoardTrait;
use iot_core::drivers::board::{HasAudio, HasCamera, HasPlayback, HasWifi};
use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::input::{
    BUTTON_SCAN_MS, ButtonScanner, DoubleClickAggregator, PassThrough, PollEntry, TOUCH_SCAN_MS,
    TouchGestures,
};
use iot_core::drivers::motion::{MOTION_SCAN_MS, MotionCapabilities, MotionInput};

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
pub use crate::virtual_components::DisplayLight;
use crate::virtual_components::Es7210Rx;
use crate::virtual_components::audio as capture;
use crate::virtual_components::audio_out as playback;
use crate::virtual_components::reference;

mod camera;

use camera::frame_arena;

const PCA9557_I2C_ADDR: u8 = 0x19;
const LCD_CS_BIT: u8 = 1 << 0;
/// The sensor's power-down line, sharing the expander register with the panel's
/// chip-select. **Active low**, so writing it selects the panel and parks the sensor;
/// the camera path clears it to give the sensor a clock.
const DVP_PWDN_BIT: u8 = 1 << 2;
/// Speaker amplifier enable, high active. Driven from the expander rather than a
/// GPIO because the pin is the board's only spare: the amplifier sits between
/// the DAC and the speaker and its own rail, and a board that boots with it
/// driving would pop the speaker on every reset.
const PA_EN_PIN: u8 = 1;

/// The panel's mounted shape, as the product's renderer addresses it.
///
/// The module is a 2.0" ST7789 whose own resolution is 320x240, so this is the
/// only shape it can be written in. `MADCTL` transposes the address window, and
/// getting that wrong does not rotate the picture: a column range of 0..319
/// overruns the controller's 240-wide RAM and the frame comes back cut.
/// `@/records/iot/camera.md`.
const LCD_WIDTH: u16 = 320;
const LCD_HEIGHT: u16 = 240;

// The diagnostics layout is a set of distances on a panel of one particular
// shape, and it is written for this one. A board that mounted a panel of another
// shape would compile every stamp and clip them off the edge instead, so the two
// are asserted against each other here — at the wiring, which is where the
// mounted panel is actually a fact.
const _: () = assert!(
    LCD_WIDTH as usize == iot_core::overlay::PANEL_COLUMNS
        && LCD_HEIGHT as usize == iot_core::overlay::PANEL_ROWS,
    "the panel this board mounts is not the shape the diagnostics layout is written for"
);

/// Boot-time retries for the PCA9557 config write: the bus's first transaction
/// and the one most exposed to a still-settling NACK that would strand the board.
const PCA9557_RETRY_ATTEMPTS: u8 = 5;
const PCA9557_RETRY_MS: u32 = 20;
/// Boot-side QMI8658 fast-path retries; a leftover failure keeps the plane wired
/// and is healed by `RecoverableQmi` from the poll loop.
const MOTION_RETRY_ATTEMPTS: u8 = 3;
const MOTION_RETRY_MS: u32 = 10;
/// DMA copy-buffer size; only flash-resident command tables are copied, the
/// DRAM frame buffer is pushed in place.
const SPI_DMA_BUF_BYTES: usize = 4096;
/// Backlight PWM frequency: high enough to be flicker-free, low enough that
/// the APB-derived divisor stays inside the LEDC timer range.
const BACKLIGHT_PWM_HZ: u32 = 5_000;

/// LEDC channel borrows its timer with the same lifetime, so the timer lives
/// in a `'static` cell instead of on the stack.
static BACKLIGHT_TIMER: esp_hal::__macro_implementation::static_cell::StaticCell<
    ledc_timer::Timer<'static, LowSpeed>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

/// I2C0 shared by PCA9557 (0x19) and FT6336 (0x38): each device holds a
/// blocking `I2cDevice` that takes a critical section around the inner
/// `RefCell`, so the two drivers can never interleave on the wire.
static I2C_BUS: esp_hal::__macro_implementation::static_cell::StaticCell<SharedI2cBus> =
    esp_hal::__macro_implementation::static_cell::StaticCell::new();

type SharedI2cBus = Mutex<CriticalSectionRawMutex, RefCell<i2c_master::I2c<'static, Blocking>>>;

pub type SharedI2cDevice =
    I2cDevice<'static, CriticalSectionRawMutex, i2c_master::I2c<'static, Blocking>>;

/// Board bring-up failure, aggregating every peripheral error source.
#[derive(Debug)]
pub enum BoardError {
    /// I2C bus configuration rejected.
    I2cConfig(i2c_master::ConfigError),
    /// The camera's DMA rejected a buffer, which means the frame arena is not where the
    /// peripheral can reach it.
    CameraDma(DmaBufError),
    /// The camera peripheral rejected its clock or pin configuration.
    CameraConfig(esp_hal::lcd_cam::cam::ConfigError),
    /// An I2C transaction on a shared-bus device failed.
    I2c(I2cDeviceError<i2c_master::Error>),
    /// SPI bus configuration rejected.
    SpiConfig(spi_master::ConfigError),
    /// An SPI transfer (CS priming or panel init) failed.
    Spi(esp_hal::spi::Error),
    /// DMA copy/descriptor buffer construction rejected.
    Dma(DmaBufError),
    /// The panel rejected the requested window/size.
    DisplayConfig,
    /// The LEDC backlight timer rejected its PWM configuration.
    LedcTimer(ledc_timer::Error),
    /// The LEDC backlight channel rejected its configuration.
    LedcChannel(ledc_channel::Error),
    /// The I2S master rejected its clock or format configuration.
    I2sConfig(i2s_master::ConfigError),
    /// The I2S capture transfer could not be started.
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

/// The panel's SPI bus and its data/command line, as both entry points need
/// them. `DMA_CH0` is fixed here because the capture gets the next channel.
///
/// The bus is primed with one byte before chip-select drops: the pins glitch on
/// their first transfer (esp-idf #15703) and the panel ignores a byte while
/// chip-select is high. DMA pushes whole frames without the per-FIFO poll that
/// caps the panel at ~17 fps, and the copy buffers stage flash-resident command
/// tables DMA cannot read in place.
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

/// The panel's LEDC backlight channel, as both entry points need them. Duty 0
/// keeps the active-low line pulled low (bright) from boot until the first
/// renderer write.
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

/// Runs `op` until it succeeds or `attempts` is spent, and returns the last
/// failure. The board's bring-up retries the two peripherals whose first
/// transaction is the one most exposed to a NACK from a part still settling;
/// each keeps its own attempt count and delay, because their budgets are
/// separate hardware facts.
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

/// Brings up the shared I2C bus and the expander on it, as both entry points
/// need them.
///
/// The bus is a `StaticCell` rather than a local because the panel's touch
/// controller, the accelerometer, the microphone and the DAC each hold their own
/// `I2cDevice` over it for the board's lifetime. `port` and the two pins are
/// taken as arguments rather than named here: they are this board's wiring, and
/// esp-hal's pin bounds are private, so the caller configures the bus and hands
/// the configured peripheral in.
///
/// The expander is retried because its first transaction is the one most exposed
/// to a NACK from a peripheral that has not finished settling, and a stranded
/// board is a black screen where a later failure is only a missing page.
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

/// Probes the ES8311 at both addresses and brings up whichever answers.
///
/// The part has a single address pin, so which of the two a board strapped it to
/// is a property of the board and not of the driver — hence two attempts rather
/// than one configured address. `absent_note` says what the caller loses when
/// neither answers, because the product build keeps the panel, the button and
/// the microphone and only loses the page, while the probe has nothing left to
/// drive. A board with no DAC fitted is a real configuration, not a failure.
fn probe_es8311(bus: &'static SharedI2cBus, absent_note: &str) -> Option<Es8311<SharedI2cDevice>> {
    let mut es8311 = Es8311::new(I2cDevice::new(bus), ES8311_I2C_ADDR);
    let mut es8311_alt = Es8311::new(I2cDevice::new(bus), ES8311_I2C_ADDR_ALT);
    if es8311.chip_id().is_ok_and(ChipId::is_expected) {
        es8311.init().ok().map(|()| es8311)
    } else if es8311_alt.chip_id().is_ok_and(ChipId::is_expected) {
        es8311_alt.init().ok().map(|()| es8311_alt)
    } else {
        log::warn!("[SPEAKER] ES8311 not found, {absent_note}");
        None
    }
}

/// Board-level support for the LCKFB SZPI ESP32-S3 display board.
pub struct Board<'d> {
    light: Option<DisplayLight>,
    button: Option<PullButton<'d>>,
    touch: Option<Ft6336<SharedI2cDevice>>,
    motion: Option<Qmi8658<SharedI2cDevice>>,
    audio: Option<Es7210Rx<Es7210<SharedI2cDevice>, { playback::REFERENCE_SAMPLES }>>,
    speaker: Option<Box<playback::Es8311Tx<Es8311<SharedI2cDevice>>>>,
    camera: Option<Box<dyn FrameSource>>,
    /// The radio peripheral, unopened. It stays here rather than being built in
    /// `new` because the driver's start needs the scheduler, and the scheduler is
    /// the app's to start — see [`HasWifi`].
    radio: Option<WIFI<'static>>,
    /// The saved networks, over the partition the table in `partitions/` gives them. Read
    /// at bring-up; handed to the link by [`Board::take_credentials`].
    ///
    /// Boxed because of where it travels: the link's futures are polled inside the one task
    /// the whole product shares, so every frame that carries the list by value is that many
    /// hundred bytes of the main stack's own region — and the region is what the static data
    /// around a 64 KB stack has to fit in.
    credentials: Option<Box<CredentialStore>>,
}

/// Completion of the chip-level wiring, handed to the application entry point.
///
/// The two software interrupts are the two executors this board runs on.
/// `FROM_CPU_INTR0` starts the cooperative scheduler every ordinary task shares;
/// `FROM_CPU_INTR1` is handed back for the higher-priority executor the speaker's
/// feed runs on, because the feed has a hard deadline that a cooperative task
/// cannot be held to.
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
            // Handed back so the app can run the speaker's feed on its own
            // higher-priority executor; see `Startup`.
            FROM_CPU_INTR1,
            DMA_CH0,
            // The panel's SPI owns channel 0; audio capture gets the next one, and the
            // camera's DVP receive the one after it.
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
            // The radio, kept for `HasWifi::take_wifi`: the driver's start needs the
            // scheduler, which the app starts after this returns.
            WIFI,
            FLASH,
            ..
        } = peripherals;

        // Read before anything else is brought up, so a partition that will not read is
        // said while the boot still has the frame for the table. The store is what the
        // panel's own credential goes into and what a boot with no build input comes up
        // with.
        //
        // Auto-parking the other core is the difference between this part and the C6: a
        // write needs the flash to itself, and this one has a second core that could be
        // running code out of it. This build never starts that core, but nothing above this
        // layer can tell, and a strategy that has to be revisited when something does is a
        // strategy that will be forgotten.
        let credentials =
            crate::components::credentials::load(FlashStorage::new(FLASH).multicore_auto_park())
                .map(Box::new);

        let (bus, mut pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;
        let mut delay = Delay::new();

        let (block_spi, dc) = panel_spi(SPI3, DMA_CH0, GPIO41, GPIO40, GPIO39)?;

        // The camera's peripheral is built here, before anything else is touched, because
        // building it enables the module clock and takes the clock pin over — which is why
        // the sensor is programmed after it and not before.
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
        // The surface owns the frame buffer and the camera borrows it, so the two DMAs
        // address one region rather than two.
        let mut light = DisplayLight::new(
            0,
            panel,
            panel_backlight(LEDC, GPIO42)?,
            &mut frame_arena().0,
        );
        // A sensor that is not fitted is a board without a camera, not a board that cannot
        // boot: the page that needs it stays dark and the rest are unaffected.
        let camera = match Board::new_camera(&mut pca9557, bus, dvp_peripheral, light.frame()) {
            Ok(camera) => Some(Box::new(camera) as Box<dyn FrameSource>),
            Err(error) => {
                log::warn!("[CAM] camera bring-up failed, the Camera page stays dark: {error:?}");
                None
            }
        };
        let button = PullButton::new(GPIO0);
        let touch = Ft6336::new(
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

        // One I2S, one pair of BCLK and WS pins, two codecs, so the pin pair is
        // wired once in either layout and both codecs are timed from one divider
        // — a microphone and a speaker a hertz apart is a capture and a tap
        // that sound like two devices.
        //
        // The two units run in separate clock domains and neither can be the
        // clock owner alone, so the transmit unit drives the pins and the
        // capture is held in slave mode behind it (see
        // [`playback::shared_tdm_config`]).
        let shared_clocks = speaker_codec.is_some();
        let config = if shared_clocks {
            playback::shared_tdm_config()
        } else {
            capture::tdm_config()
        };
        let (audio, speaker) = match (capture_codec, speaker_codec) {
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
                        Some(Es7210Rx::new(
                            transfer,
                            codec,
                            reference::Side::wired(reference),
                        ))
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
                                log::error!("[SPEAKER] stream would not start: {error:?}");
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

        // What came up, in one place, before the product starts using any of it.
        // Every state is read off the same value the board hands back, so this
        // cannot disagree with what the product will actually find.
        //
        // The panel, touch and IMU are not options here — a board without them
        // fails bring-up outright — so they are `Found` by construction, and
        // their provenance is `Asserted`: the driver succeeding is evidence the
        // bus works, not a readback of what is on it.
        let present = |present: bool| {
            if present { State::Found } else { State::Absent }
        };
        let mut found = 3;
        let mut absent = 0;
        let mut tally = |state: State| {
            match state {
                State::Found => found += 1,
                State::Absent => absent += 1,
                State::Degraded(_) => {}
            }
            state
        };

        post::identity();
        post::line("panel", State::Found, Provenance::Asserted);
        post::line("touch", State::Found, Provenance::Asserted);
        post::line("motion", State::Found, Provenance::Asserted);
        for (name, wired) in [
            ("mic", audio.is_some()),
            ("speaker", speaker.is_some()),
            ("camera", camera.is_some()),
        ] {
            let state = tally(present(wired));
            post::line(name, state, Provenance::Read);
        }
        // Wired, and the wiring measured, but whether the pairing actually
        // cancels is a question this build cannot answer — see the record.
        post::line("aec", State::Found, Provenance::Unverified);
        found += 1;
        post::summary(found, 0, absent);

        Ok((
            Self {
                light: Some(light),
                button: Some(button),
                touch: Some(touch),
                motion: Some(motion),
                audio,
                speaker,
                camera,
                radio: Some(WIFI),
                credentials,
            },
            TimerGroup::new(TIMG0),
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
        ))
    }

    /// Bring-up for the playback isolation probe: the DAC, its amplifier and the
    /// transmit unit, and nothing else.
    ///
    /// The product build holds the capture slaved to the transmit unit (see
    /// [`playback::shared_tdm_config`]), so isolating the speaker means cutting
    /// that coupling too: here the transmit unit drives the same two pins itself
    /// and the receive unit is never built, which it can be because with no
    /// microphone to slave there is nothing to slave.
    ///
    /// The expander is still driven, and only for the amplifier: `PA_EN` is the
    /// board's spare pin, and the panel is held in the same power-down its masks
    /// already select so a probe that never paints the panel also never wakes it.
    /// Everything else is left `None`, so the probe's entry point can refuse to
    /// ask for a capability this bring-up did not wire.
    /// The motion plane alone, for the demand matrix probe.
    ///
    /// The expander is brought up because the IMU is on the same bus and its reset needs it.
    /// The IMU comes back configured but *uninitialized*: this path walks fidelities, so a
    /// bring-up that already wrote one would make the first cell measure a switch nobody
    /// asked for.
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
                light: None,
                button: None,
                touch: None,
                motion: Some(motion),
                audio: None,
                speaker: None,
                camera: None,
                radio: None,
                // The store's peripheral is the probe path's too, but a probe has nothing to
                // keep: it reads its own numbers and exits.
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
            // Handed back with the rest so both bring-ups return one shape. The
            // probe's main ignores it: the probe deliberately feeds on the
            // cooperative executor, to measure the transmit path alone.
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
                    // This bring-up drives the speaker alone and builds no
                    // microphone, so nothing reads the reference it writes. The
                    // cell is still made, so the driver has somewhere real to
                    // write and a probe that later adds a capture does not have to
                    // change this call.
                    reference::Writer::new(reference::share::<{ playback::REFERENCE_SAMPLES }>()),
                ) {
                    Ok(speaker) => Some(Box::new(speaker)),
                    Err(error) => {
                        log::error!("[SPEAKER] stream would not start: {error:?}");
                        None
                    }
                }
            }
            None => None,
        };

        Ok((
            Self {
                light: None,
                button: None,
                touch: None,
                motion: None,
                audio: None,
                speaker,
                // The probe brings its own camera up through `new_camera_only`; this path
                // wires the speaker alone, so it must hand over none.
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
    ///
    /// Not [`HasMotion::take_motion`](iot_core::drivers::board::HasMotion::take_motion): a probe walks the configurations by writing the
    /// registers itself and counting what that caused.
    pub fn take_motion_driver(&mut self) -> Option<Qmi8658<SharedI2cDevice>> {
        self.motion.take()
    }

    /// The saved networks, for the link that keeps them.
    ///
    /// Taken rather than borrowed: the task that holds the link is the one that writes a
    /// credential the panel named, and it is the only writer there is — a second handle
    /// would be a second copy of a list the flash has one of.
    pub fn take_credentials(&mut self) -> Option<Box<CredentialStore>> {
        self.credentials.take()
    }

    /// The capture driver itself, for a probe that has to read the canceller's own
    /// numbers.
    ///
    /// Not [`HasAudio::take_audio`]: that hands over a `PollEntry` boxed behind the
    /// input pipeline, which is right for the product — one capture among several
    /// polled sources — and useless for a probe, which needs the driver to ask what
    /// the echo cancellation did rather than what the envelope says.
    ///
    /// Boxed as a trait object so a probe names no codec, and erased once here
    /// because `Es7210Rx` is generic over the codec that captured it.
    pub fn take_capture(&mut self) -> Option<Box<dyn crate::virtual_components::EchoCapture>> {
        self.audio
            .take()
            .map(|audio| Box::new(audio) as Box<dyn crate::virtual_components::EchoCapture>)
    }
}

impl BoardTrait for Board<'static> {}

impl iot_core::drivers::board::HasLight for Board<'static> {
    type Light = DisplayLight;

    fn take_lights(&mut self) -> Option<Vec<Self::Light>> {
        self.light.take().map(|light| vec![light])
    }
}

impl iot_core::drivers::board::HasInput for Board<'static> {
    fn take_input(&mut self) -> Option<Vec<PollEntry>> {
        let mut entries = Vec::new();
        if let Some(button) = self.button.take() {
            entries.push(PollEntry::new(
                0,
                PlaneKey::INTERACTION,
                Box::new(ButtonScanner::new(button)),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            ));
        }
        if let Some(touch) = self.touch.take() {
            entries.push(PollEntry::new(
                0,
                PlaneKey::INTERACTION,
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
                PlaneKey::MOTION,
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
                PlaneKey::AUDIO,
                Box::new(AudioInput::new(audio, sink)),
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
        self.radio.take().and_then(crate::components::wifi::start)
    }
}

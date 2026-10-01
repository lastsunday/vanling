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
use esp_hal::peripherals::{FROM_CPU_INTR0, FROM_CPU_INTR1, Peripherals};
use esp_hal::spi::master as spi_master;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{dma_rx_buffer, dma_tx_buffer};
use iot_core::drivers::audio::{AudioInput, CAPTURE_MS};
use iot_core::drivers::board::Board as BoardTrait;
use iot_core::drivers::board::{HasAudio, HasPlayback};
use iot_core::drivers::input::{
    BUTTON_SCAN_MS, ButtonScanner, DoubleClickAggregator, PassThrough, PollEntry, TOUCH_SCAN_MS,
    TouchGestures, TouchMap,
};
use iot_core::drivers::motion::{MOTION_SCAN_MS, MotionCapabilities, MotionInput};

use crate::components::backlight::Backlight;
pub use crate::components::button::PullButton;
use crate::components::es7210::{ES7210_I2C_ADDR, Es7210};
use crate::components::es8311::{ChipId, ES8311_I2C_ADDR, ES8311_I2C_ADDR_ALT, Es8311};
use crate::components::ft6336::{FT6336_I2C_ADDR, Ft6336};
use crate::components::pca9557::Pca9557;
use crate::components::qmi8658::{MOTION_CAPABILITIES, QMI8658_I2C_ADDR, Qmi8658, RecoverableQmi};
use crate::components::st7789::{SPI_FREQ_HZ, SPI_MODE, St7789, St7789Error};
pub use crate::virtual_components::DisplayLight;
use crate::virtual_components::Es7210Rx;
use crate::virtual_components::audio as capture;
use crate::virtual_components::audio_out as playback;

const PCA9557_I2C_ADDR: u8 = 0x19;
const LCD_CS_BIT: u8 = 1 << 0;
const DVP_PWDN_BIT: u8 = 1 << 2;
/// Speaker amplifier enable, high active. Driven from the expander rather than a
/// GPIO because the pin is the board's only spare: the amplifier sits between
/// the DAC and the speaker and its own rail, and a board that boots with it
/// driving would pop the speaker on every reset.
const PA_EN_PIN: u8 = 1;
const LCD_WIDTH: u16 = 240;
const LCD_HEIGHT: u16 = 320;
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

type SharedI2cDevice =
    I2cDevice<'static, CriticalSectionRawMutex, i2c_master::I2c<'static, Blocking>>;

/// Board bring-up failure, aggregating every peripheral error source.
#[derive(Debug)]
pub enum BoardError {
    /// I2C bus configuration rejected.
    I2cConfig(i2c_master::ConfigError),
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
    let delay = Delay::new();
    let mut pca9557 = Pca9557::new(I2cDevice::new(bus), PCA9557_I2C_ADDR);
    let mut pca_error = None;
    for attempt in 0..PCA9557_RETRY_ATTEMPTS {
        match pca9557.init(LCD_CS_BIT | DVP_PWDN_BIT, 0xf8) {
            Ok(()) => break,
            Err(error) => {
                pca_error = Some(error);
                if attempt + 1 < PCA9557_RETRY_ATTEMPTS {
                    delay.delay_millis(PCA9557_RETRY_MS);
                }
            }
        }
    }
    match pca_error {
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
    audio: Option<Es7210Rx<Es7210<SharedI2cDevice>>>,
    speaker: Option<Box<playback::Es8311Tx<Es8311<SharedI2cDevice>>>>,
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
            // The panel's SPI owns channel 0; capture gets the next one.
            DMA_CH1,
            ..
        } = peripherals;

        let (bus, mut pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;
        let mut delay = Delay::new();

        // Keep CS high while the SPI/GPIO IO_MUX glitch (#15703) settles. DMA
        // pushes whole frames without the per-FIFO poll that caps the panel at
        // ~17 fps; the copy buffers stage flash-resident command tables DMA
        // cannot read in place.
        let mut block_spi = spi_master::Spi::new(
            SPI3,
            spi_master::Config::default()
                .with_frequency(Rate::from_hz(SPI_FREQ_HZ))
                .with_mode(SPI_MODE),
        )
        .map_err(BoardError::SpiConfig)?
        .with_sck(GPIO41)
        .with_mosi(GPIO40)
        .with_dma(DMA_CH0)
        .with_buffers(
            dma_rx_buffer!(SPI_DMA_BUF_BYTES)?,
            dma_tx_buffer!(SPI_DMA_BUF_BYTES)?,
        );

        let dc = Output::new(GPIO39, Level::Low, OutputConfig::default());

        // Prime the bus before CS drops: the pins glitch on their first transfer
        // (esp-idf #15703) and the panel ignores the byte while CS is high.
        SpiBus::write(&mut block_spi, &[0x01]).map_err(BoardError::Spi)?;

        pca9557.set_output(DVP_PWDN_BIT)?;

        let panel = St7789::new(block_spi, dc, LCD_WIDTH, LCD_HEIGHT)?;

        // Backlight on IO42: duty 0 keeps the active-low line pulled low
        // (bright) from boot until the first renderer write.
        let mut ledc = Ledc::new(LEDC);
        ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
        let timer = BACKLIGHT_TIMER.init(ledc.timer::<LowSpeed>(ledc_timer::Number::Timer0));
        timer
            .configure(ledc_timer::config::Config {
                duty: ledc_timer::config::Duty::Duty10Bit,
                clock_source: ledc_timer::LSClockSource::APBClk,
                frequency: Rate::from_hz(BACKLIGHT_PWM_HZ),
            })
            .map_err(BoardError::LedcTimer)?;
        let mut channel = ledc.channel::<LowSpeed>(ledc_channel::Number::Channel0, GPIO42);
        channel
            .configure(ledc_channel::config::Config {
                timer: &*timer,
                duty_pct: 0,
                drive_mode: DriveMode::PushPull,
            })
            .map_err(BoardError::LedcChannel)?;

        let light = DisplayLight::new(0, panel, Backlight::new(channel));
        let button = PullButton::new(GPIO0);
        let touch = Ft6336::new(
            I2cDevice::new(bus),
            FT6336_I2C_ADDR,
            TouchMap::IDENTITY,
            LCD_WIDTH,
            LCD_HEIGHT,
        );
        let mut motion = Qmi8658::new(I2cDevice::new(bus), QMI8658_I2C_ADDR);
        let mut motion_error = None;
        for attempt in 0..MOTION_RETRY_ATTEMPTS {
            match motion.init(&mut delay) {
                Ok(()) => break,
                Err(error) => {
                    motion_error = Some(error);
                    if attempt + 1 < MOTION_RETRY_ATTEMPTS {
                        delay.delay_millis(MOTION_RETRY_MS);
                    }
                }
            }
        }
        if let Some(error) = motion_error {
            log::warn!("[MOTION] QMI8658 init deferred, retrying from the first poll: {error:?}");
        }

        // ES7210 on I2C0 at 0x41, sharing the bus with the expander and the touch
        // controller. Configured before the I2S starts so the codec is already
        // listening when the clocks appear; a variant with no codec fitted keeps
        // the rest of the plane alive and simply never shows the Audio page.
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
        // — a microphone and a speaker a hertz apart is a capture and a chime
        // that sound like two devices.
        //
        // The transmit unit is the one built with those pins and the capture
        // takes them from inside the peripheral, and the direction is not
        // interchangeable. The two units run in separate clock domains, so the
        // unit that drives the pins is the clock the data on them is timed
        // against, and the other has to be slaved to it (see
        // [`playback::shared_tdm_config`]). Letting the capture drive them made
        // the transmit unit shift its samples out on a divider the ES8311 was not
        // being clocked by, which the bench heard as continuous crackle; giving
        // the pins to the transmit unit instead leaves the capture sampling the
        // ES7210 on a divider that is not the one clocking the part, which it
        // answered with a flat full-scale reading and a dead waveform. Neither
        // unit can be the clock owner on its own, so the transmit unit drives
        // the pins and the capture is held in slave mode behind it.
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
                let audio = match capture_codec {
                    // The codec goes with the transfer rather than being dropped
                    // here: the ring cannot be reconfigured, so anything that
                    // wants to move the input stage's corner after bring-up needs
                    // the part still to hand.
                    Some(codec) => {
                        let transfer = rx
                            .read(capture::stream())
                            .map_err(|(error, _, _)| BoardError::I2sStart(error))?;
                        Some(Es7210Rx::new(transfer, codec))
                    }
                    None => None,
                };
                let speaker = match (tx, speaker_codec) {
                    (Some(tx), Some(codec)) => {
                        // The amplifier comes up after the DAC does, so the
                        // speaker is never driven by a codec that has not been
                        // programmed yet. Pin numbers, not masks, reach the
                        // expander's read-modify-write.
                        pca9557.set_output_bit(PA_EN_PIN, true)?;
                        match playback::Es8311Tx::new(tx, codec) {
                            Ok(speaker) => Some(Box::new(speaker)),
                            // A stream the DMA refused cannot make a sound, so
                            // the page is left off rather than offered a mute
                            // that points at silence.
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

        let timg0 = TimerGroup::new(TIMG0);

        Ok((
            Self {
                light: Some(light),
                button: Some(button),
                touch: Some(touch),
                motion: Some(motion),
                audio,
                speaker,
            },
            timg0,
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
        ))
    }

    /// Bring-up for the playback isolation probe: the DAC, its amplifier and the
    /// transmit unit, and nothing else.
    ///
    /// The product build shares one BCLK/WS pair between both codecs, holding the
    /// capture slaved to the transmit unit (see [`playback::shared_tdm_config`]),
    /// so a probe that is to isolate the speaker from the capture has to cut that
    /// coupling too. Here the transmit unit drives the same two pins itself and
    /// the receive unit is never built — which it can be, because with no
    /// microphone to slave there is nothing to slave, and the transmit unit is
    /// already the clock master the product slaves to. No capture codec, no
    /// capture transfer, no capture poll, and no second DMA direction competing
    /// for the channel.
    ///
    /// The expander is still driven, and only for the amplifier: `PA_EN` is the
    /// board's spare pin, and the panel is held in the same power-down its masks
    /// already select so a probe that never paints the panel also never wakes it.
    /// Everything else is left `None`, so the probe's entry point can refuse to
    /// ask for a capability this bring-up did not wire.
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

        // Probed and initialised exactly as the product build does, at both
        // addresses, so the probe measures the same DAC the product drives.
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
                match playback::Es8311Tx::new(tx, codec) {
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
            },
            TimerGroup::new(TIMG0),
            FROM_CPU_INTR0,
            FROM_CPU_INTR1,
        ))
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
                Box::new(ButtonScanner::new(button)),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            ));
        }
        if let Some(touch) = self.touch.take() {
            entries.push(PollEntry::new(
                0,
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
                Box::new(MotionInput::new(RecoverableQmi::new(motion, Delay::new()))),
                Box::new(PassThrough),
                MOTION_SCAN_MS,
            )
        })
    }

    fn motion_capabilities(&self) -> MotionCapabilities {
        MOTION_CAPABILITIES
    }
}

impl HasPlayback for Board<'static> {
    type Speaker = playback::Es8311Tx<Es8311<SharedI2cDevice>>;

    fn take_playback(&mut self) -> Option<Box<Self::Speaker>> {
        self.speaker.take()
    }
}

impl HasAudio for Board<'static> {
    fn take_audio(&mut self) -> Option<PollEntry> {
        self.audio.take().map(|audio| {
            PollEntry::new(
                2,
                Box::new(AudioInput::new(audio)),
                Box::new(PassThrough),
                CAPTURE_MS,
            )
        })
    }
}

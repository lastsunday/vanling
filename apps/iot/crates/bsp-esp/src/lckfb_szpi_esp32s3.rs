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
use esp_hal::ledc::channel as ledc_channel;
use esp_hal::ledc::channel::ChannelIFace as _;
use esp_hal::ledc::timer as ledc_timer;
use esp_hal::ledc::timer::TimerIFace as _;
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::peripherals::{FROM_CPU_INTR0, Peripherals};
use esp_hal::spi::master as spi_master;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{dma_rx_buffer, dma_tx_buffer};
use iot_core::drivers::board::Board as BoardTrait;
use iot_core::drivers::input::{
    BUTTON_SCAN_MS, ButtonScanner, DoubleClickAggregator, PassThrough, PollEntry, TOUCH_SCAN_MS,
    TouchGestures, TouchMap,
};
use iot_core::drivers::motion::{MOTION_SCAN_MS, MotionCapabilities, MotionInput};

use crate::components::backlight::Backlight;
pub use crate::components::button::PullButton;
use crate::components::ft6336::{FT6336_I2C_ADDR, Ft6336};
use crate::components::pca9557::Pca9557;
use crate::components::qmi8658::{MOTION_CAPABILITIES, QMI8658_I2C_ADDR, Qmi8658, RecoverableQmi};
use crate::components::st7789::{SPI_FREQ_HZ, SPI_MODE, St7789, St7789Error};
pub use crate::virtual_components::DisplayLight;

const PCA9557_I2C_ADDR: u8 = 0x19;
const LCD_CS_BIT: u8 = 1 << 0;
const DVP_PWDN_BIT: u8 = 1 << 2;
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
static I2C_BUS: esp_hal::__macro_implementation::static_cell::StaticCell<
    Mutex<CriticalSectionRawMutex, RefCell<i2c_master::I2c<'static, Blocking>>>,
> = esp_hal::__macro_implementation::static_cell::StaticCell::new();

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

/// Board-level support for the LCKFB SZPI ESP32-S3 display board.
pub struct Board<'d> {
    light: Option<DisplayLight>,
    button: Option<PullButton<'d>>,
    touch: Option<Ft6336<SharedI2cDevice>>,
    motion: Option<Qmi8658<SharedI2cDevice>>,
}

/// Completion of the chip-level wiring, handed to the application entry point.
pub type Startup<'a> = (
    Board<'a>,
    TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    FROM_CPU_INTR0<'static>,
);

impl Board<'static> {
    pub fn new(peripherals: Peripherals) -> Result<Startup<'static>, BoardError> {
        #[allow(non_snake_case)]
        let Peripherals {
            I2C0,
            SPI3,
            LEDC,
            GPIO1,
            GPIO2,
            GPIO39,
            GPIO40,
            GPIO41,
            GPIO42,
            GPIO0,
            TIMG0,
            FROM_CPU_INTR0,
            DMA_CH0,
            ..
        } = peripherals;

        let i2c = i2c_master::I2c::new(I2C0, i2c_master::Config::default())
            .map_err(BoardError::I2cConfig)?
            .with_sda(GPIO1)
            .with_scl(GPIO2);
        let bus = I2C_BUS.init(Mutex::new(RefCell::new(i2c)));
        let mut delay = Delay::new();
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
        if let Some(error) = pca_error {
            return Err(BoardError::I2c(error));
        }

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
        let timg0 = TimerGroup::new(TIMG0);

        Ok((
            Self {
                light: Some(light),
                button: Some(button),
                touch: Some(touch),
                motion: Some(motion),
            },
            timg0,
            FROM_CPU_INTR0,
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

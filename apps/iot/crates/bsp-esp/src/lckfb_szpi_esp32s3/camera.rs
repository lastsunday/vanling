//! The camera↔display isolation probe's wiring: the DVP peripheral, the panel it sends
//! to, and the frame ring between them.
//!
//! Deliberately separate from `Board::new`, so nothing here can change what the product
//! builds or what it wires. The panel handed back is a bare [`St7789`] rather than a
//! [`DisplayLight`] for the same reason: a surface is the product's, and a probe that
//! draws through one is no longer measuring the camera.

use super::*;

use crate::components::gc2145::{
    EXTRACT_RATIO, FORMAT_RGB565, GC2145_I2C_ADDR, Gc2145, Gc2145Error,
};
use crate::virtual_components::camera::{self as dvp, FrameRing, Gc2145Capture};

/// The frame ring, in internal DRAM: `dvp::RING_BYTES` explains why DRAM rather than
/// PSRAM. It is a static rather than a heap allocation because it has to outlive the
/// transfer the peripheral keeps running, and because its size is part of the board's
/// memory budget rather than of what this image happens to ask the heap for.
///
/// Aligned to the cache line an internal-memory DMA region needs, not to the array's own
/// alignment.
#[repr(C, align(32))]
struct FrameRingArena([u8; dvp::RING_BYTES]);

/// `static mut` because the ring reaches the DMA as a raw pointer, which is the only way
/// to express a DMA buffer in this HAL — and the same reason the probes' heaps are
/// `static mut`. Safety rests on `FrameRing::new`'s contract plus there being exactly one
/// camera, derived here and nowhere else.
static mut FRAME_RING: FrameRingArena = FrameRingArena([0; dvp::RING_BYTES]);

/// What the probe is handed: its own panel, the capture, and the timer and interrupt
/// `start_rtos` takes.
pub type CameraOnly = (
    St7789,
    Gc2145Capture<Gc2145<SharedI2cDevice>>,
    TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    FROM_CPU_INTR0<'static>,
);

/// Probe bring-up failure. Everything the shared [`BoardError`] already covers is one
/// variant, so this entry point adds no cases to the product's error surface.
#[derive(Debug)]
pub enum CameraOnlyError {
    /// A peripheral the product shares rejected its configuration or transfer.
    Board(BoardError),
    /// The camera peripheral rejected its configuration.
    CameraConfig(esp_hal::lcd_cam::cam::ConfigError),
    /// The camera's DMA transfer could not be started.
    CameraStart(esp_hal::dma::DmaError),
    /// The GC2145 could not be brought up, identified or programmed.
    Sensor(Gc2145Error<I2cDeviceError<i2c_master::Error>>),
}

impl From<BoardError> for CameraOnlyError {
    fn from(error: BoardError) -> Self {
        Self::Board(error)
    }
}

impl Board<'static> {
    /// Brings up the DVP peripheral, the sensor on the shared I2C bus, and the panel —
    /// and nothing else, so the probe owns the display outright.
    ///
    /// The order below is a hard constraint, not a style preference: the peripheral must
    /// be built **before the sensor is touched**, because building it enables the module
    /// clock, routes the twelve data pins and takes the clock pin over. Until that has
    /// happened the sensor has no clock and stays silent on a bus where every other device
    /// answers, so probing first reads a state the part is not in. The sensor's SCCB is
    /// that same I2C bus, and the expander line is what takes it out of power-down.
    ///
    /// PSRAM is deliberately left in power-down — see `dvp::RING_BYTES`.
    pub fn new_camera_only(peripherals: Peripherals) -> Result<CameraOnly, CameraOnlyError> {
        #[allow(non_snake_case)]
        let Peripherals {
            I2C0,
            SPI3,
            LCD_CAM,
            LEDC,
            GPIO1,
            GPIO2,
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
            GPIO39,
            GPIO40,
            GPIO41,
            GPIO42,
            GPIO46,
            TIMG0,
            FROM_CPU_INTR0,
            DMA_CH0,
            DMA_CH2,
            ..
        } = peripherals;

        let (bus, mut pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;

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
            dma_rx_buffer!(SPI_DMA_BUF_BYTES).map_err(BoardError::from)?,
            dma_tx_buffer!(SPI_DMA_BUF_BYTES).map_err(BoardError::from)?,
        );
        let dc = Output::new(GPIO39, Level::Low, OutputConfig::default());
        // Prime the bus before chip-select drops: the pins glitch on their first transfer
        // and the panel ignores the byte while chip-select is high.
        SpiBus::write(&mut block_spi, &[0x01]).map_err(BoardError::Spi)?;
        // Selects the panel and wakes the sensor in one write, because both lines are
        // active low and share the expander's register. `bring_up_i2c` left the register
        // at `LCD_CS_BIT | DVP_PWDN_BIT`, which parks the sensor and selects the panel, so
        // zero is the state that undoes the first without dropping the second.
        pca9557.set_output(0).map_err(BoardError::from)?;
        // The panel window is the frame geometry itself, so a frame goes out untouched:
        // no `MADCTL` transposition and no CPU transpose.
        let panel = St7789::new(block_spi, dc, dvp::FRAME_WIDTH, dvp::FRAME_HEIGHT)
            .map_err(BoardError::from)?;

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
        // Held for the run: dropping the channel would leave the line wherever the PWM
        // last left it, and the probe has no renderer to ask for brightness again.
        let _backlight = Backlight::new(channel);

        let dvp_peripheral = esp_hal::lcd_cam::cam::Camera::new(
            esp_hal::lcd_cam::LcdCam::new(LCD_CAM).cam,
            DMA_CH2,
            dvp::config(),
        )
        .map_err(CameraOnlyError::CameraConfig)?
        .with_master_clock(GPIO5)
        .with_pixel_clock(GPIO7)
        .with_vsync(GPIO3)
        // Data-enable, not a sync line: the sensor has no HSYNC, so this is the enable pin.
        .with_h_enable(GPIO46)
        .with_data0(GPIO16)
        .with_data1(GPIO18)
        .with_data2(GPIO8)
        .with_data3(GPIO17)
        .with_data4(GPIO15)
        .with_data5(GPIO6)
        .with_data6(GPIO4)
        .with_data7(GPIO9);

        log::info!(
            "[CAM] frame ring is {} KB in internal DRAM over {} descriptors",
            dvp::RING_BYTES / 1024,
            dvp::DESCRIPTOR_COUNT
        );
        // SAFETY: `FRAME_RING` is written once, by `FrameRing::new` below, and this binary
        // builds one camera and never drops it. The reference exists only to produce the
        // pointer, so no two of them ever alias.
        let arena = unsafe { (*core::ptr::addr_of_mut!(FRAME_RING)).0.as_mut_ptr() };

        let mut delay = Delay::new();
        let mut sensor = Gc2145::new(I2cDevice::new(bus), GC2145_I2C_ADDR);
        sensor.init(&mut delay).map_err(CameraOnlyError::Sensor)?;
        sensor
            .set_window(
                dvp::FRAME_WIDTH,
                dvp::FRAME_HEIGHT,
                EXTRACT_RATIO,
                &mut delay,
            )
            .map_err(CameraOnlyError::Sensor)?;
        sensor.set_hmirror(true).map_err(CameraOnlyError::Sensor)?;

        // What the part holds rather than what it was asked for: an output size that reads
        // back right while the ratio is wrong is a picture of the right shape and the
        // wrong content.
        let geometry = sensor
            .read_window_geometry()
            .map_err(CameraOnlyError::Sensor)?;
        let format = sensor.read_format().map_err(CameraOnlyError::Sensor)?;
        let chip_id = sensor.read_id().map_err(CameraOnlyError::Sensor)?;
        log::info!("[CAM] sensor id {chip_id:#06x}");
        log::info!(
            "[CAM] sensor reports {}x{} out, read {}x{} at {},{} ratio {}:{}, format 0x{format:02x} \
             (RGB565 is 0x{FORMAT_RGB565:02x})",
            geometry.out_width,
            geometry.out_height,
            geometry.win_width,
            geometry.win_height,
            geometry.row_start,
            geometry.col_start,
            geometry.subsample >> 4,
            geometry.subsample & 0x0f,
        );
        // Register by register, because the size above reads back as asked for whether or
        // not the part is doing what was asked: it reports the window it was given, not the
        // picture it produced.
        log::info!(
            "[CAM] decim regs: 0x99=0x{:02x} 0x9a=0x{:02x} bins={:02x?} crop={}",
            geometry.subsample,
            geometry.subsample_mode,
            geometry.sub_bins,
            u8::from(geometry.crop_enabled),
        );

        // SAFETY: `arena` addresses `FRAME_RING`, a static of exactly `RING_BYTES` in
        // internal DRAM, so nothing else aliases it and it cannot move or be freed while
        // the ring is in use.
        let ring = unsafe { FrameRing::new(arena) }.map_err(BoardError::Dma)?;
        let transfer = dvp_peripheral
            .receive(ring)
            .map_err(|(error, _camera, _ring)| CameraOnlyError::CameraStart(error))?;
        dvp::report_peripheral_config("after receive");

        Ok((
            panel,
            Gc2145Capture::new(transfer, sensor),
            TimerGroup::new(TIMG0),
            FROM_CPU_INTR0,
        ))
    }
}

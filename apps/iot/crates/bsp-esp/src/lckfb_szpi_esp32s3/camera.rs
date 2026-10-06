//! The camera wiring: the DVP peripheral, the sensor, and the frame arena they share with
//! the panel.
//!
//! Two entry points onto the same wiring. The product goes through [`Board::new_camera`],
//! which hands the capture to the surface that owns the frame buffer. The isolation probe
//! goes through [`Board::new_camera_only`], which builds the panel separately and hands back
//! a bare [`St7789`] — a surface is the product's, and a probe that draws through one is no
//! longer measuring the camera.
//!
//! The order in both is a hard constraint, not a preference: the peripheral is built
//! **before the sensor is touched**, because building it enables the module clock, routes the
//! data pins and takes the clock pin over. Until that has happened the sensor has no clock and
//! stays silent on a bus where every other device answers, so probing first reads a state the
//! part is not in. The sensor's SCCB is that same I²C bus, and the expander line is what takes
//! it out of power-down.
//!
//! PSRAM is deliberately left in power-down — see `dvp::RING_BYTES`.

use super::*;

use crate::components::gc2145::{
    EXTRACT_RATIO, FORMAT_RGB565, GC2145_I2C_ADDR, Gc2145, Gc2145Error,
};
use crate::virtual_components::camera::{self as dvp, FrameRing, Gc2145Capture};

/// The sensor this board mounts, reached over the shared bus.
type BoardCamera = Gc2145Capture<Gc2145<SharedI2cDevice>>;

/// The panel's frame buffer, in internal DRAM: `dvp::RING_BYTES` explains why DRAM rather
/// than PSRAM. It is a static rather than a heap allocation because it has to outlive the
/// transfer the peripheral keeps running, and because its size is part of the board's memory
/// budget rather than of what this image happens to ask the heap for.
///
/// Aligned to the cache line an internal-memory DMA region needs, not to the array's own
/// alignment. `dvp::DMA_ALIGNMENT_BYTES` is that line; this is deliberately stricter than
/// what esp-hal requires, which on this part is four bytes.
#[repr(C, align(32))]
pub(super) struct FrameArena(pub(super) [u8; dvp::RING_BYTES]);

/// `static mut` because the ring reaches the DMA as a raw pointer, which is the only way
/// to express a DMA buffer in this HAL — and the same reason the probes' heaps are
/// `static mut`. Safety rests on `FrameRing::new`'s contract plus there being exactly one
/// camera, derived here and nowhere else.
static mut FRAME_ARENA: FrameArena = FrameArena([0; dvp::RING_BYTES]);

/// Hands out the one region both the camera's DMA and the panel's DMA address.
///
/// The product's surface borrows it for as long as it draws; the camera is handed it again on
/// every call. There is no second path to these bytes — that is what makes the two DMAs safe
/// to share, and it is why the arena's alignment is asserted here rather than left to the
/// type, which constrains the declaration and not where the linker placed it.
pub(crate) fn frame_arena() -> &'static mut FrameArena {
    // `addr_of!` rather than a read of the static: even asking the static where it landed
    // counts as touching a mutable one.
    let placed = core::ptr::addr_of!(FRAME_ARENA);
    // SAFETY: a read of the placement, not of the contents — the bytes stay untouched, and
    // `repr(align)` says how the array is declared, not where it ended up: a build that moved
    // it would still compile and would fail at boot instead.
    let address = unsafe { (*placed).0.as_ptr() as usize };
    assert_eq!(
        address % dvp::DMA_ALIGNMENT_BYTES,
        0,
        "the frame arena is not {}-byte aligned, so the peripheral would pad every chunk and \
         shift the frame",
        dvp::DMA_ALIGNMENT_BYTES
    );
    // SAFETY: the region is a private static named nowhere else, and this is the only place
    // that turns it into a mutable reference — so exactly one caller holds it for as long as
    // it lives, and nothing else aliases it.
    unsafe { &mut *core::ptr::addr_of_mut!(FRAME_ARENA) }
}

/// What the probe is handed: its own panel, the capture, and the timer and interrupt
/// `start_rtos` takes.
pub type CameraOnly = (
    St7789,
    BoardCamera,
    TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    FROM_CPU_INTR0<'static>,
);

/// Bring-up failure. Everything the shared [`BoardError`] already covers is one variant, so
/// this adds no cases to the product's error surface beyond the two the camera itself owns.
#[derive(Debug)]
pub enum CameraError {
    /// A peripheral the product shares rejected its configuration or transfer.
    Board(BoardError),
    /// The camera peripheral rejected its configuration.
    CameraConfig(esp_hal::lcd_cam::cam::ConfigError),
    /// The camera's DMA transfer could not be started.
    CameraStart(esp_hal::dma::DmaError),
    /// The GC2145 could not be brought up, identified or programmed.
    Sensor(Gc2145Error<I2cDeviceError<i2c_master::Error>>),
}

impl From<BoardError> for CameraError {
    fn from(error: BoardError) -> Self {
        Self::Board(error)
    }
}

impl From<esp_hal::lcd_cam::cam::ConfigError> for CameraError {
    fn from(error: esp_hal::lcd_cam::cam::ConfigError) -> Self {
        Self::CameraConfig(error)
    }
}

/// The camera's share of the pins, by wiring order. `Board::new` destructures them too, so
/// both are named here once rather than in two `Peripherals` patterns.
#[allow(non_snake_case)]
pub(super) struct CameraPins {
    pub LCD_CAM: esp_hal::peripherals::LCD_CAM<'static>,
    pub DMA_CH2: esp_hal::peripherals::DMA_CH2<'static>,
    pub GPIO3: esp_hal::peripherals::GPIO3<'static>,
    pub GPIO4: esp_hal::peripherals::GPIO4<'static>,
    pub GPIO5: esp_hal::peripherals::GPIO5<'static>,
    pub GPIO6: esp_hal::peripherals::GPIO6<'static>,
    pub GPIO7: esp_hal::peripherals::GPIO7<'static>,
    pub GPIO8: esp_hal::peripherals::GPIO8<'static>,
    pub GPIO9: esp_hal::peripherals::GPIO9<'static>,
    pub GPIO15: esp_hal::peripherals::GPIO15<'static>,
    pub GPIO16: esp_hal::peripherals::GPIO16<'static>,
    pub GPIO17: esp_hal::peripherals::GPIO17<'static>,
    pub GPIO18: esp_hal::peripherals::GPIO18<'static>,
    pub GPIO46: esp_hal::peripherals::GPIO46<'static>,
}

impl Board<'static> {
    /// Builds the DVP peripheral and takes its pins. Every step here is a one-way door: the
    /// module clock and the pin routes cannot be undone, so a caller that builds this and then
    /// fails has still consumed the peripheral.
    ///
    /// Errors as a [`BoardError`] because the product's bring-up reports a camera that will
    /// not configure the same way it reports a panel that will not — both are the board's
    /// peripherals, and the probe wraps them into one variant.
    pub(super) fn new_dvp(
        pins: CameraPins,
    ) -> Result<esp_hal::lcd_cam::cam::Camera<'static>, BoardError> {
        Ok(esp_hal::lcd_cam::cam::Camera::new(
            esp_hal::lcd_cam::LcdCam::new(pins.LCD_CAM).cam,
            pins.DMA_CH2,
            dvp::config(),
        )
        .map_err(BoardError::CameraConfig)?
        .with_master_clock(pins.GPIO5)
        .with_pixel_clock(pins.GPIO7)
        .with_vsync(pins.GPIO3)
        // Data-enable, not a sync line: the sensor has no HSYNC, so this is the enable pin.
        .with_h_enable(pins.GPIO46)
        .with_data0(pins.GPIO16)
        .with_data1(pins.GPIO18)
        .with_data2(pins.GPIO8)
        .with_data3(pins.GPIO17)
        .with_data4(pins.GPIO15)
        .with_data5(pins.GPIO6)
        .with_data6(pins.GPIO4)
        .with_data7(pins.GPIO9))
    }

    /// Brings up the sensor over the shared bus and arms the capture onto the arena.
    ///
    /// The DVP peripheral must already exist — see the module header for why that ordering is
    /// a hardware fact rather than a style choice.
    pub(super) fn new_sensor_capture(
        dvp_peripheral: esp_hal::lcd_cam::cam::Camera<'static>,
        bus: &'static SharedI2cBus,
        arena: &mut [u8],
    ) -> Result<BoardCamera, CameraError> {
        log::info!(
            "[CAM] frame ring is {} KB in internal DRAM over {} descriptors",
            dvp::RING_BYTES / 1024,
            dvp::DESCRIPTOR_COUNT
        );

        let mut delay = Delay::new();
        let mut sensor = Gc2145::new(I2cDevice::new(bus), GC2145_I2C_ADDR);
        sensor.init(&mut delay).map_err(CameraError::Sensor)?;
        sensor
            .set_window(
                dvp::FRAME_WIDTH,
                dvp::FRAME_HEIGHT,
                EXTRACT_RATIO,
                &mut delay,
            )
            .map_err(CameraError::Sensor)?;
        sensor.set_hmirror(true).map_err(CameraError::Sensor)?;

        // What the part holds rather than what it was asked for: an output size
        // that reads back right while the ratio is wrong is a picture of the right
        // shape and the wrong content.
        let geometry = sensor.read_window_geometry().map_err(CameraError::Sensor)?;
        let format = sensor.read_format().map_err(CameraError::Sensor)?;
        let chip_id = sensor.read_id().map_err(CameraError::Sensor)?;
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
        // Register by register, because the part reports the window it was
        // given, not the picture it produced.
        log::info!(
            "[CAM] decim regs: 0x99=0x{:02x} 0x9a=0x{:02x} bins={:02x?} crop={}",
            geometry.subsample,
            geometry.subsample_mode,
            geometry.sub_bins,
            u8::from(geometry.crop_enabled),
        );

        // SAFETY: `arena` is the board's frame buffer — one region, nothing else names it,
        // and it cannot move or be freed while the ring is in use.
        let ring = unsafe { FrameRing::new(arena.as_mut_ptr()) }.map_err(BoardError::Dma)?;
        let transfer = dvp_peripheral
            .receive(ring)
            .map_err(|(error, _camera, _ring)| CameraError::CameraStart(error))?;
        dvp::report_peripheral_config("after receive");

        Ok(Gc2145Capture::new(transfer, sensor))
    }

    /// The product's camera: the DVP peripheral and the sensor, over the panel's frame buffer.
    ///
    /// Takes the expander too because waking the sensor shares a write with selecting the
    /// panel: both lines are active low and sit in the same register, so one write undoes the
    /// power-down without dropping the panel's chip-select.
    pub(super) fn new_camera(
        pca9557: &mut Pca9557<SharedI2cDevice>,
        bus: &'static SharedI2cBus,
        dvp_peripheral: esp_hal::lcd_cam::cam::Camera<'static>,
        arena: &mut [u8],
    ) -> Result<BoardCamera, CameraError> {
        pca9557.set_output(0).map_err(BoardError::from)?;
        Self::new_sensor_capture(dvp_peripheral, bus, arena)
    }

    /// Brings up the DVP peripheral, the sensor on the shared I²C bus, and the panel —
    /// and nothing else, so the probe owns the display outright.
    pub fn new_camera_only(peripherals: Peripherals) -> Result<CameraOnly, CameraError> {
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
            TIMG0,
            FROM_CPU_INTR0,
            DMA_CH0,
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
            ..
        } = peripherals;

        let (bus, mut pca9557) = bring_up_i2c(
            i2c_master::I2c::new(I2C0, i2c_master::Config::default())
                .map_err(BoardError::I2cConfig)?
                .with_sda(GPIO1)
                .with_scl(GPIO2),
        )?;

        let (block_spi, dc) = panel_spi(SPI3, DMA_CH0, GPIO41, GPIO40, GPIO39)?;
        // Selects the panel and wakes the sensor in one write, because both lines are
        // active low and share the expander's register. `bring_up_i2c` left the register
        // at `LCD_CS_BIT | DVP_PWDN_BIT`, which parks the sensor and selects the panel, so
        // zero is the state that undoes the first without dropping the second.
        pca9557.set_output(0).map_err(BoardError::from)?;
        // The panel window is the frame geometry itself, so a frame goes out untouched:
        // no `MADCTL` transposition and no CPU transpose.
        let panel = St7789::new(block_spi, dc, dvp::FRAME_WIDTH, dvp::FRAME_HEIGHT)
            .map_err(BoardError::from)?;
        // Held for the run: dropping the channel would leave the line wherever the PWM
        // last left it, and the probe has no renderer to ask for brightness again.
        let _backlight = panel_backlight(LEDC, GPIO42)?;

        let dvp_peripheral = Self::new_dvp(CameraPins {
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

        let capture = Self::new_sensor_capture(dvp_peripheral, bus, &mut frame_arena().0)?;

        Ok((panel, capture, TimerGroup::new(TIMG0), FROM_CPU_INTR0))
    }
}

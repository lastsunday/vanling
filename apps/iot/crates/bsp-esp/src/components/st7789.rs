use alloc::boxed::Box;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType, OutputPin};
use embedded_hal::spi::SpiBus;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::gpio::Output;
use esp_hal::spi::Mode;
use esp_hal::spi::master as spi_master;
use mipidsi::interface::{Interface, InterfaceKind};
use mipidsi::models::ST7789;
use mipidsi::options::ColorInversion;
use mipidsi::{Builder, InitError};

pub const SPI_FREQ_HZ: u32 = 80_000_000;
pub const SPI_MODE: Mode = Mode::_2;

const SPI_SCRATCH_BYTES: usize = 8192;

/// Panel bring-up failure.
#[derive(Debug)]
pub enum St7789Error {
    /// The SPI bus rejected a transfer during initialization.
    Spi(esp_hal::spi::Error),
    /// The requested window/size combination is unsupported by the panel.
    Config,
}

/// No-op reset driver: the panel keeps RST unconnected, but `Builder::init`
/// only suppresses its software reset when a pin is provided. Driving a
/// no-op pin expresses "do not soft-reset" without touching a real GPIO.
#[derive(Clone, Copy)]
struct DummyReset;

impl ErrorType for DummyReset {
    type Error = core::convert::Infallible;
}

impl OutputPin for DummyReset {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// No-op delay source, because mipidsi's own inter-command pauses do not suit this panel.
/// The timings that matter are re-added explicitly in [`Self::send_command`].
#[derive(Clone, Copy)]
struct NoDelay;

impl DelayNs for NoDelay {
    fn delay_ns(&mut self, _ns: u32) {}
}

/// 4-wire SPI transport for mipidsi. esp-hal's DMA-backed blocking `SpiDma`
/// is a `SpiBus`, not a shared-bus `SpiDevice`, so we adapt it directly to
/// mipidsi's `Interface` trait to avoid pulling in a bus crate for a single
/// client.
struct Spi4 {
    spi: spi_master::SpiDma<'static, Blocking>,
    dc: Output<'static>,
    buffer: &'static mut [u8],
}

impl Spi4 {
    fn push_word<const N: usize>(
        &mut self,
        word: &[u8; N],
        used: &mut usize,
    ) -> Result<(), esp_hal::spi::Error> {
        if *used + N > self.buffer.len() {
            SpiBus::write(&mut self.spi, &self.buffer[..*used])?;
            *used = 0;
        }
        self.buffer[*used..*used + N].copy_from_slice(word);
        *used += N;
        Ok(())
    }

    fn write_data(&mut self, data: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_high();
        SpiBus::write(&mut self.spi, data)
    }

    fn emit_command(&mut self, command: u8, args: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_low();
        SpiBus::write(&mut self.spi, &[command])?;
        // Match the panel's own DC semantics: the line only goes high for
        // parameter bytes and stays low after argument-less commands.
        if !args.is_empty() {
            self.dc.set_high();
            SpiBus::write(&mut self.spi, args)?;
        }
        Ok(())
    }
}

impl Interface for Spi4 {
    type Word = u8;
    type Error = esp_hal::spi::Error;

    const KIND: InterfaceKind = InterfaceKind::Serial4Line;

    fn send_command(&mut self, command: u8, args: &[u8]) -> Result<(), Self::Error> {
        // mipidsi's init stream carries a delay before every command, all of them
        // suppressed above. Sleep-out is the one that matters — the panel needs 150 ms to
        // settle before anything else — and the rest must not reach it.
        match command {
            0x11 => {
                self.emit_command(command, args)?;
                Delay::new().delay_ms(150);
            }
            0x36 => {
                self.emit_command(0x36, &[MADCTL_LANDSCAPE])?;
                self.emit_command(0x3A, &[0x55])?;
                self.emit_command(0xB0, &[0x00, 0xF0])?;
            }
            0x3A | 0x13 => {}
            _ => self.emit_command(command, args)?,
        }
        Ok(())
    }

    fn send_pixels<const N: usize>(
        &mut self,
        pixels: impl IntoIterator<Item = [Self::Word; N]>,
    ) -> Result<(), Self::Error> {
        self.dc.set_high();
        let mut used = 0usize;
        for word in pixels {
            self.push_word(&word, &mut used)?;
        }
        if used > 0 {
            SpiBus::write(&mut self.spi, &self.buffer[..used])?;
        }
        Ok(())
    }

    fn send_repeated_pixel<const N: usize>(
        &mut self,
        pixel: [Self::Word; N],
        count: u32,
    ) -> Result<(), Self::Error> {
        self.dc.set_high();
        let mut used = 0usize;
        for _ in 0..count {
            self.push_word(&pixel, &mut used)?;
        }
        if used > 0 {
            SpiBus::write(&mut self.spi, &self.buffer[..used])?;
        }
        Ok(())
    }
}

/// Which way round the panel addresses its own frame buffer.
///
/// The controller's frame buffer is 240x320 and the 2.0" module is specified 320x240, so it
/// is that controller mounted to be looked at side-on: `MV` + `MX` turns its window for it.
/// Both numbers are needed and are not interchangeable, and confusing them truncates the
/// picture rather than rotating it — the address window transposes with `MADCTL`, so a
/// column range of 0..319 overruns a 240-wide RAM. `@/records/iot/camera.md`.
const MADCTL_LANDSCAPE: u8 = 0x60;

/// The controller's own frame buffer, which is what mipidsi validates `display_size`
/// against. Not the mounted shape — see [`MADCTL_LANDSCAPE`].
const CONTROLLER_WIDTH: u16 = 240;
const CONTROLLER_HEIGHT: u16 = 320;

/// ST7789 panel on a 4-wire SPI transport. Owns the chip bring-up (`Builder`)
/// and the whole-frame write to VRAM.
pub struct St7789 {
    spi: Spi4,
    width: u16,
    height: u16,
}

impl St7789 {
    /// Brings the panel up to take a `width` x `height` buffer: the shape a frame is
    /// written into, 320x240 for this module. It is deliberately *not* what mipidsi is
    /// told — see [`MADCTL_LANDSCAPE`].
    pub fn new(
        block_spi: spi_master::SpiDma<'static, Blocking>,
        dc: Output<'static>,
        width: u16,
        height: u16,
    ) -> Result<Self, St7789Error> {
        let scratch: &'static mut [u8] =
            Box::leak(alloc::vec![0u8; SPI_SCRATCH_BYTES].into_boxed_slice());
        let iface = Spi4 {
            spi: block_spi,
            dc,
            buffer: scratch,
        };

        // RAMCTRL/COLMOD/MADCTL are emitted by the init shim at the same spot the working
        // legacy driver used, and the panel must receive SLPOUT first. mipidsi's model
        // init writes only `MADCTL` from this size — the frame window is set by
        // `write_frame` below — so what this call needs is a size the model accepts.
        let display = Builder::new(ST7789, iface)
            .display_size(CONTROLLER_WIDTH, CONTROLLER_HEIGHT)
            .invert_colors(ColorInversion::Inverted)
            .reset_pin(DummyReset)
            .init(&mut NoDelay)
            .map_err(|e| match e {
                InitError::Interface(e) => St7789Error::Spi(e),
                InitError::ResetPin(never) => match never {},
                InitError::InvalidConfiguration(_) => St7789Error::Config,
            })?;
        let (iface, _, _) = display.release();

        Ok(Self {
            spi: iface,
            width,
            height,
        })
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Push one full frame to panel VRAM.
    pub fn write_frame(&mut self, frame: &[u8]) -> Result<(), esp_hal::spi::Error> {
        let x_end = self.width - 1;
        let y_end = self.height - 1;
        self.spi.emit_command(0x2A, &[])?;
        self.spi
            .write_data(&[0x00, 0x00, (x_end >> 8) as u8, x_end as u8])?;
        self.spi.emit_command(0x2B, &[])?;
        self.spi
            .write_data(&[0x00, 0x00, (y_end >> 8) as u8, y_end as u8])?;
        self.spi.emit_command(0x2C, &[])?;
        self.spi.write_data(frame)?;
        Ok(())
    }
}

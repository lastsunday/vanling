//! Internal DRAM only, one frame deep: the descriptor chain is a line, not a circle, so
//! descriptors come back only by rebuilding it, which drops what the sensor sends meanwhile.

use esp_hal::dma::aligned::{DmaAlignedMut, DmaAlignedRef};
use esp_hal::dma::{
    BurstConfig, DmaBufError, DmaDescriptor, DmaRxBuffer, InternalBurstConfig, Owner, Preparation,
};
use esp_hal::dma_descriptors_chunk_size;
use esp_hal::lcd_cam::cam::{self, CameraTransfer, EofMode, VhdeMode};
use esp_hal::time::Rate;
use iot_core::drivers::camera::{CameraCounters, FrameAdvance, FrameSource, WindowGeometry};

use crate::components::gc2145::WindowGeometry as SensorGeometry;

/// A sensor that can report the window it is reading out.
pub trait WindowReadable {
    fn window(&mut self) -> Option<SensorGeometry>;
}

impl<D: embedded_hal::i2c::I2c> WindowReadable for crate::components::gc2145::Gc2145<D> {
    fn window(&mut self) -> Option<SensorGeometry> {
        self.read_window_geometry().ok()
    }
}

fn window_geometry<S: WindowReadable>(sensor: &mut S) -> Option<WindowGeometry> {
    let read = sensor.window()?;
    Some(WindowGeometry {
        out_width: read.out_width,
        out_height: read.out_height,
        win_width: read.win_width,
        win_height: read.win_height,
        row_start: read.row_start,
        col_start: read.col_start,
        subsample: read.subsample,
        scalar: read.scalar,
        sub_bins: read.sub_bins,
        crop_enabled: read.crop_enabled,
    })
}

const BYTES_PER_PIXEL: usize = 2;

/// The camera's frame: the panel's full width and height, so the picture fills the glass
/// and the readout is stamped over it.
pub const FRAME_WIDTH: u16 = 320;
pub const FRAME_HEIGHT: u16 = 240;

pub const FRAME_BYTES: usize = FRAME_WIDTH as usize * FRAME_HEIGHT as usize * BYTES_PER_PIXEL;

const CHUNK_BYTES: usize = 3840;

const DESCRIPTORS_PER_FRAME: usize = FRAME_BYTES / CHUNK_BYTES;

// One, and deliberately so: two would let the peripheral refill while the panel reads the
// first, which is the only thing that would raise the frame rate — and the frame period is
// set by the sensor's readout, not by how fast the chain is drained. Doubling the ring buys
// nothing here and costs another 153,600 B of DRAM, which does not fit. Measured frame rates
// per ratio are in `@/records/iot/camera.md`.
const RING_FRAMES: usize = 1;

/// 153,600 B: the panel's whole 150 KB frame buffer, which the board hands the capture. It is
/// not a second allocation — the camera fills the frame and the page stamps its readout over
/// it, so this ring costs no memory of its own. The arena it sits in is what a probe's heap is
/// sized against once `.rwtext` and the panel's SPI buffers take their share of `dram_seg`.
///
/// DRAM rather than PSRAM not for want of the 8 MB this part has: Espressif gates the receive
/// descriptor and data burst bits on `//internal SRAM only` and esp-hal cannot express that
/// split, so a PSRAM ring is always left in a configuration the part streams scrambled into.
/// A descriptor list may not point into PSRAM either. The hardware can be told otherwise —
/// `CONFIG_CAMERA_PSRAM_DMA` in esp-idf's camera driver, itself documented as experimental —
/// so the limit is this HAL's, not the chip's.
pub const RING_BYTES: usize = FRAME_BYTES * RING_FRAMES;

pub const DESCRIPTOR_COUNT: usize = DESCRIPTORS_PER_FRAME * RING_FRAMES;

// Bytes the peripheral's receive counter matches, meaningful only while the frame boundary
// comes from a byte count. Not reliably present: esp-hal programs it before requesting the
// module clock and a write that lands first is dropped silently, which is why
// [`report_peripheral_config`] reads it back.
const EOF_BYTE_LEN: u16 = CHUNK_BYTES as u16 - 1;

/// Bytes every chunk has to be a whole multiple of, and the alignment the frame arena is
/// declared with. Public because the board asserts its placed address against this: a type's
/// alignment constrains the declaration, not where the linker put the thing.
///
/// The DMA pads a receive to its alignment, so a chunk that is not shifts everything after
/// it. In DRAM that alignment is the cache line — the S3's 32 bytes.
///
/// Not what esp-hal asks for: it requires a whole multiple of 4 on this part, because S3
/// internal DRAM is not cached and so needs no cache-line alignment of its own. This is the
/// peripheral's requirement, met deliberately rather than by accident.
pub const DMA_ALIGNMENT_BYTES: usize = 32;

const _: () = assert!(
    FRAME_BYTES.is_multiple_of(CHUNK_BYTES),
    "a frame must be a whole number of descriptors or it cannot be handed out as one slice"
);
const _: () = assert!(CHUNK_BYTES <= 4095, "the descriptor size field is 12 bits");
const _: () = assert!(
    CHUNK_BYTES.is_multiple_of(DMA_ALIGNMENT_BYTES),
    "the DMA pads a receive to its alignment, and a chunk that is not a whole multiple is padded \
     to a size the frame geometry did not account for"
);
// XCLK the sensor runs at, and the peripheral's pixel clock: 24 MHz, the datasheet's master
// clock. Not a free choice — off it the sensor's PLL does not lock and the part streams
// correctly framed noise, which no register read reports. `@/records/iot/camera.md`.
const PIXEL_CLOCK_HZ: u32 = 24_000_000;

const STALL_MS: u64 = 400;

/// Data-enable, not sync-and-enable: the sensor has no HSYNC line, and asking for one
/// leaves the peripheral waiting on a pin that never moves.
pub fn config() -> cam::Config {
    cam::Config::default()
        .with_frequency(Rate::from_hz(PIXEL_CLOCK_HZ))
        .with_vh_de_mode(VhdeMode::De)
        .with_eof_mode(EofMode::ByteLen(EOF_BYTE_LEN))
}

/// A descriptor chain over an arena of internal DRAM, sized in whole frames.
pub struct FrameRing {
    descriptors: DmaAlignedMut<'static, [DmaDescriptor]>,
    buffer: DmaAlignedMut<'static, [u8]>,
    chain_reported: bool,
}

impl FrameRing {
    fn log_descriptors(&self) {
        let base = self.descriptors.as_ptr() as usize;
        for index in [0, 1, DESCRIPTOR_COUNT - 1] {
            let Some(descriptor) = self.descriptors.get(index) else {
                continue;
            };
            log::debug!(
                "[CAM] descriptor {index}: buffer=0x{:08x} size={} next=0x{:08x} owner={}",
                descriptor.buffer as usize,
                descriptor.size(),
                descriptor.next as usize,
                if descriptor.owner() == Owner::Cpu {
                    "cpu"
                } else {
                    "dma"
                }
            );
        }
        let mut walked = 0usize;
        while walked < DESCRIPTOR_COUNT {
            let Some(descriptor) = self.descriptors.get(walked) else {
                break;
            };
            let expected_buffer = self.buffer.as_ptr() as usize + walked * CHUNK_BYTES;
            let expected_next = if walked + 1 < DESCRIPTOR_COUNT {
                base + (walked + 1) * core::mem::size_of::<DmaDescriptor>()
            } else {
                0
            };
            if descriptor.buffer as usize != expected_buffer
                || descriptor.size() != CHUNK_BYTES
                || descriptor.next as usize != expected_next
            {
                log::error!(
                    "[CAM] chain breaks at descriptor {walked}: buffer=0x{:08x} (want 0x{expected_buffer:x}) \
                     size={} (want {CHUNK_BYTES}) next=0x{:08x} (want 0x{expected_next:x})",
                    descriptor.buffer as usize,
                    descriptor.size(),
                    descriptor.next as usize,
                );
                return;
            }
            walked += 1;
        }
        log::info!("[CAM] chain verified: {walked} descriptors, ending at null");
    }

    /// Lays the ring out over `arena`: [`RING_BYTES`] of internal DRAM the caller owns for the
    /// rest of the run.
    ///
    /// **Once per run, and once only.** The descriptor list comes from a `ConstStaticCell`, and
    /// taking one of those a second time panics rather than handing out a second array — so a
    /// caller that loses the ring cannot build another, and must have kept it instead. Every
    /// rebuild after this point goes through [`DmaRxBuffer::prepare`], which is what the
    /// re-arm after each whole frame already uses.
    ///
    /// # Safety
    ///
    /// `arena` must address [`RING_BYTES`] bytes of RAM the DMA can write and the CPU
    /// can read, that nothing else aliases, and that stays valid and unmoved for as long
    /// as this ring is used.
    pub unsafe fn new(arena: *mut u8) -> Result<Self, DmaBufError> {
        // SAFETY: the caller guarantees the extent, ownership and lifetime, which is
        // what this reference asserts.
        let arena: &'static mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(arena, RING_BYTES) };
        let buffer = DmaAlignedMut::new(arena)?;
        let (descriptors, _) = dma_descriptors_chunk_size!(RING_BYTES, 0, CHUNK_BYTES);
        let descriptors = DmaAlignedMut::new(descriptors)?;
        assert_eq!(
            descriptors.len(),
            DESCRIPTOR_COUNT,
            "the descriptor list must be exactly the ring's worth of chunks"
        );

        let mut ring = Self {
            descriptors,
            buffer,
            chain_reported: false,
        };
        let base = ring.descriptors.as_mut_ptr();
        let pixels = ring.buffer.as_mut_ptr();
        for index in 0..DESCRIPTOR_COUNT {
            // SAFETY: `index < DESCRIPTOR_COUNT`, and the descriptor list is a
            // `StaticCell`'d array of exactly that many entries.
            let descriptor = unsafe { &mut *base.add(index) };
            // SAFETY: `index * CHUNK_BYTES` is inside the arena, which the caller
            // guarantees is `RING_BYTES` long and this many chunks long.
            descriptor.buffer = unsafe { pixels.add(index * CHUNK_BYTES) };
            descriptor.set_size(CHUNK_BYTES);
            descriptor.next = if index + 1 < DESCRIPTOR_COUNT {
                // SAFETY: as above.
                unsafe { base.add(index + 1) }
            } else {
                core::ptr::null_mut()
            };
        }
        Ok(ring)
    }
}

/// A ring being filled.
pub struct FrameRingView {
    ring: FrameRing,
    last: Option<usize>,
}

impl FrameRingView {
    /// The newest whole frame the DMA has finished.
    pub fn take_latest(&mut self) -> Option<FramePick> {
        let finished = self.finished_descriptors();
        let complete = finished / DESCRIPTORS_PER_FRAME;
        let newest = complete.checked_sub(1)?;
        let unchanged = self.last == Some(finished);
        self.last = Some(finished);
        Some(FramePick {
            frame: newest,
            finished,
            unchanged,
        })
    }

    /// The pixels of `index`.
    pub fn frame(&self, index: usize) -> Option<&[u8]> {
        if index >= RING_FRAMES {
            return None;
        }
        let start = index * FRAME_BYTES;
        let slice = &self.ring.buffer[start..start + FRAME_BYTES];
        DmaAlignedRef::new(slice)
            .expect("a frame is a whole number of aligned chunks")
            .invalidate();
        Some(slice)
    }

    fn finished_descriptors(&self) -> usize {
        self.ring.descriptors.invalidate();
        self.ring
            .descriptors
            .iter()
            .take_while(|descriptor| descriptor.owner() == Owner::Cpu)
            .count()
    }
}

/// What one poll found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePick {
    /// Index of the newest whole frame.
    pub frame: usize,
    /// Descriptors the DMA had finished, across the whole ring.
    pub finished: usize,
    /// Whether this is the frame the previous poll already answered with.
    pub unchanged: bool,
}

unsafe impl DmaRxBuffer for FrameRing {
    type View = FrameRingView;
    type Final = Self;

    fn prepare(&mut self) -> Preparation {
        let mut next = core::ptr::null_mut();
        for descriptor in self.descriptors.iter_mut().rev() {
            descriptor.next = next;
            next = descriptor;
            descriptor.reset_for_rx();
        }
        self.descriptors.writeback();
        if !self.chain_reported {
            self.chain_reported = true;
            self.log_descriptors();
        }

        Preparation {
            start: self.descriptors.as_mut_ptr(),
            accesses_psram: false,
            // Burst on, 32 bytes. The HAL offers one enum per memory kind and picks
            // between them by `accesses_psram`, so this is the internal setting. Not the
            // 64 the peripheral also takes: at 64 (`ext_mem_bk_size = 2`) writes come out
            // scrambled — complete, correctly sized and correctly framed frames with the
            // pixels wrong inside them.
            burst_transfer: BurstConfig::from(InternalBurstConfig::Enabled),
            check_owner: None,
            auto_write_back: true,
        }
    }

    fn into_view(self) -> Self::View {
        FrameRingView {
            ring: self,
            last: None,
        }
    }

    fn from_view(view: Self::View) -> Self::Final {
        view.ring
    }
}

type Transfer = CameraTransfer<'static, FrameRing>;

/// Logs what the peripheral's configuration registers actually hold, against what the HAL
/// asked for: esp-hal programs them before requesting the module clock, so a write that
/// lands first is dropped silently. Free-standing because it is about the peripheral,
/// answerable with or without a ring.
pub fn report_peripheral_config(label: &str) {
    let ctrl1 = esp_hal::peripherals::LCD_CAM::regs().cam_ctrl1().read();
    let ctrl = esp_hal::peripherals::LCD_CAM::regs().cam_ctrl().read();
    log::debug!(
        "[CAM] {label}: bytelen={} (want {EOF_BYTE_LEN}) stop_en={} vs_eof_en={} two_byte={} \
         vh_de={} start={}",
        ctrl1.cam_rec_data_bytelen().bits(),
        u8::from(ctrl.cam_stop_en().bit_is_set()),
        u8::from(ctrl.cam_vs_eof_en().bit_is_set()),
        u8::from(ctrl1.cam_2byte_en().bit_is_set()),
        u8::from(ctrl1.cam_vh_de_mode_en().bit_is_set()),
        u8::from(ctrl1.cam_start().bit_is_set()),
    );
}

/// What one poll of the capture reports.
pub struct Gc2145Capture<S> {
    // `None` once a re-arm has failed — the one state a capture cannot come back from,
    // since a transfer that does not take has no ring to put back in the chain. An
    // `Option` so a failed repair leaves a capture that reports no frames rather than one
    // that panics on the next poll.
    transfer: Option<Transfer>,
    parked: Option<(esp_hal::lcd_cam::cam::Camera<'static>, FrameRing)>,
    sensor: S,
    frames: u32,
    repeated: u32,
    restarts: u32,
    finished: usize,
    advanced_at: Option<u64>,
    latest: Option<usize>,
    rearm_pending: bool,
    geometry: Option<WindowGeometry>,
}

impl<S> Gc2145Capture<S> {
    /// Takes a transfer armed on a ring built by [`FrameRing::new`], and the sensor that
    /// was brought up alongside it.
    pub fn new(transfer: Transfer, sensor: S) -> Self {
        log::info!(
            "[CAM] {FRAME_WIDTH}x{FRAME_HEIGHT} RGB565, ring {RING_FRAMES} frames \
             ({RING_BYTES} B) over {DESCRIPTOR_COUNT} descriptors, {DESCRIPTORS_PER_FRAME} per \
             frame, {CHUNK_BYTES} B per descriptor",
        );
        Self {
            transfer: Some(transfer),
            parked: None,
            sensor,
            frames: 0,
            repeated: 0,
            restarts: 0,
            finished: 0,
            advanced_at: None,
            latest: None,
            rearm_pending: false,
            geometry: None,
        }
    }
}

impl<S: WindowReadable> Gc2145Capture<S> {
    /// The sensor's readout window as the part reports it, read once and kept.
    pub fn geometry(&mut self) -> Option<WindowGeometry> {
        if self.geometry.is_none() {
            self.geometry = window_geometry(&mut self.sensor);
        }
        self.geometry
    }

    /// The pixels the last [`Self::poll`] reported.
    pub fn latest_frame(&self) -> Option<&[u8]> {
        let frame = self.latest?;
        self.transfer.as_ref()?.frame(frame)
    }

    pub fn sensor(&mut self) -> &mut S {
        &mut self.sensor
    }

    /// Looks at the ring and reports what it found.
    pub fn poll(&mut self, now_ms: u64) -> CameraCounters {
        let Some(transfer) = self.transfer.as_mut() else {
            return self.counters();
        };
        let Some(pick) = transfer.take_latest() else {
            return self.check_stall(now_ms);
        };
        if pick.finished > self.finished {
            self.advanced_at = Some(now_ms);
        }
        self.finished = pick.finished;
        if pick.unchanged {
            self.repeated = self.repeated.saturating_add(1);
        } else {
            self.frames = self.frames.saturating_add(1);
            self.latest = Some(pick.frame);
        }
        if pick.finished >= DESCRIPTOR_COUNT {
            if self.rearm_pending {
                self.rearm_pending = false;
                self.rearm();
            } else {
                self.rearm_pending = true;
            }
        } else {
            self.rearm_pending = false;
            self.check_stall(now_ms);
        }
        self.counters()
    }

    fn check_stall(&mut self, now_ms: u64) -> CameraCounters {
        let advanced_at = *self.advanced_at.get_or_insert(now_ms);
        if now_ms.saturating_sub(advanced_at) <= STALL_MS {
            return self.counters();
        }
        let running = esp_hal::peripherals::LCD_CAM::regs()
            .cam_ctrl1()
            .read()
            .cam_start()
            .bit_is_set();
        log::warn!(
            "[CAM] no descriptor completed in {} ms, stalled at {} of {}, peripheral {} (re-arm {})",
            now_ms.saturating_sub(advanced_at),
            self.finished,
            DESCRIPTOR_COUNT,
            if running { "running" } else { "stopped" },
            self.restarts
        );
        self.rearm();
        self.counters()
    }

    fn rearm(&mut self) {
        let Some(transfer) = self.transfer.take() else {
            return;
        };
        let (camera, buffer) = transfer.stop();
        self.restarts = self.restarts.saturating_add(1);
        self.advanced_at = None;
        self.finished = 0;
        self.latest = None;
        self.rearm_pending = false;
        match camera.receive(buffer) {
            Ok(transfer) => self.transfer = Some(transfer),
            Err((error, _camera, _buffer)) => {
                log::error!("[CAM] capture could not be re-armed, going dark: {error:?}");
            }
        }
    }

    /// The running totals, for a consumer that drains the capture on another core and so
    /// reports from its own loop.
    pub fn counters(&self) -> CameraCounters {
        CameraCounters {
            frames: self.frames,
            repeated: self.repeated,
            restarts: self.restarts,
            finished: self.finished as u32,
        }
    }
}

impl<S: WindowReadable> FrameSource for Gc2145Capture<S> {
    fn advance(&mut self, _frame: &mut [u8], now_ms: u64) -> FrameAdvance {
        let stalled_at = self.finished;
        self.poll(now_ms);
        match self.latest {
            Some(_) => FrameAdvance::Fresh,
            None if self.restarts > 0 && stalled_at >= DESCRIPTOR_COUNT => FrameAdvance::Stalled,
            None => FrameAdvance::Same,
        }
    }

    fn pause(&mut self) {
        let Some(transfer) = self.transfer.take() else {
            return;
        };
        // Keeps the ring, and that is the whole point. `FrameRing::new` cannot be called twice
        // — the descriptor list it draws from is a `ConstStaticCell`, and `take` on one panics
        // on the second call — so a pause that dropped the ring would leave `resume` with no way
        // to hand the chain a buffer, and rebuilding one is the panic rather than the repair.
        let (camera, ring) = transfer.stop();
        self.parked = Some((camera, ring));
        self.advanced_at = None;
        self.finished = 0;
        self.latest = None;
        self.rearm_pending = false;
    }

    fn resume(&mut self, _frame: &mut [u8]) {
        let Some((camera, ring)) = self.parked.take() else {
            return;
        };
        match camera.receive(ring) {
            Ok(transfer) => self.transfer = Some(transfer),
            Err((error, _camera, _ring)) => {
                log::error!("[CAM] capture could not be resumed, going dark: {error:?}");
            }
        }
    }

    fn counters(&self) -> CameraCounters {
        Gc2145Capture::counters(self)
    }

    fn geometry(&mut self) -> Option<WindowGeometry> {
        Gc2145Capture::geometry(self)
    }
}

impl<S: crate::hardware::SelfCheck> crate::hardware::SelfCheck for Gc2145Capture<S> {
    fn name(&self) -> &'static str {
        self.sensor.name()
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        self.sensor.id()
    }

    fn check(&mut self) -> crate::hardware::Status {
        self.sensor.check()
    }
}

impl<S: crate::hardware::Named> crate::hardware::Named for Gc2145Capture<S> {
    const NAME: &'static str = S::NAME;
}

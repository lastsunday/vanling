//! Two hardware facts shape this, [`RING_BYTES`] explaining the first:
//!
//! * **The ring is in internal DRAM, not PSRAM**, and one frame is all that fits.
//! * **The descriptor chain is a line, not a circle**, so descriptors come back only by
//!   rebuilding it — which drops whatever the sensor sends meanwhile. A circle was
//!   measured and does not run on this peripheral.
//!
//! Frames are assembled from a fixed descriptor count, which assumes the part emits exactly
//! the geometry programmed into it. A test pins that; the bus cannot. `@/records/iot/camera.md`.
//!
//! Absent on purpose, as they belong to the product.

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
///
/// A trait rather than a bound on `Gc2145<D>` so [`Gc2145Capture`] stays generic over what it
/// carries: the capture never reads the part itself, and only the readout needs the bus.
pub trait WindowReadable {
    fn window(&mut self) -> Option<SensorGeometry>;
}

impl<D: embedded_hal::i2c::I2c> WindowReadable for crate::components::gc2145::Gc2145<D> {
    fn window(&mut self) -> Option<SensorGeometry> {
        self.read_window_geometry().ok()
    }
}

/// The part's readout window, reshaped for the render layer.
///
/// `subsample_mode` is not carried: it says what the part does with the rows and columns the
/// decimation removes, which changes how the picture looks but not what the window is.
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

/// RGB565, because that is what the panel's frame buffer holds: a frame in this format
/// reaches it with no conversion, and there is no memory for a frame-sized one.
const BYTES_PER_PIXEL: usize = 2;

/// The panel's geometry, which the sensor is configured to match.
pub const FRAME_WIDTH: u16 = 320;
pub const FRAME_HEIGHT: u16 = 240;

pub const FRAME_BYTES: usize = FRAME_WIDTH as usize * FRAME_HEIGHT as usize * BYTES_PER_PIXEL;

/// Bytes per descriptor: one frame divided into forty, and a whole multiple of
/// [`DMA_ALIGNMENT_BYTES`], so a frame arrives as exactly forty descriptors.
const CHUNK_BYTES: usize = 3840;

/// Descriptors one frame spans, which is what makes a frame a slice rather than a walk.
const DESCRIPTORS_PER_FRAME: usize = FRAME_BYTES / CHUNK_BYTES;

/// One, and deliberately so: two would let the peripheral refill while the panel reads the
/// first, which is the only thing that would raise the frame rate — and the frame period is
/// set by the sensor's readout, not by how fast the chain is drained. Doubling the ring buys
/// nothing here and costs another 150 KB of DRAM, which does not fit. Measured frame rates
/// per ratio are in `@/records/iot/camera.md`.
const RING_FRAMES: usize = 1;

/// 150 KB, of a `dram_seg` that is 333 KB before `.rwtext` and the panel's SPI buffers take
/// their share — a probe's heap is sized against what is left.
///
/// DRAM rather than PSRAM not for want of the 8 MB this part has: Espressif gates the receive
/// descriptor and data burst bits on `//internal SRAM only` and esp-hal cannot express that
/// split, so a PSRAM ring is always left in a configuration the part streams scrambled into.
/// A descriptor list may not point into PSRAM either. The hardware can be told otherwise —
/// `CONFIG_CAMERA_PSRAM_DMA` in esp-idf's camera driver, itself documented as experimental —
/// so the limit is this HAL's, not the chip's.
pub const RING_BYTES: usize = FRAME_BYTES * RING_FRAMES;

pub const DESCRIPTOR_COUNT: usize = DESCRIPTORS_PER_FRAME * RING_FRAMES;

/// Bytes the peripheral's receive counter matches, meaningful only while the frame boundary
/// comes from a byte count. Not reliably present: esp-hal programs it before requesting the
/// module clock and a write that lands first is dropped silently, which is why
/// [`report_peripheral_config`] reads it back.
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
const _: () = assert!(
    DESCRIPTOR_COUNT.is_multiple_of(DESCRIPTORS_PER_FRAME),
    "the chain must be a whole number of frames or a re-arm lands mid-frame"
);

/// XCLK the sensor runs at, and the peripheral's pixel clock: 24 MHz, the datasheet's master
/// clock. Not a free choice — off it the sensor's PLL does not lock and the part streams
/// correctly framed noise, which no register read reports. `@/records/iot/camera.md`.
const PIXEL_CLOCK_HZ: u32 = 24_000_000;

/// How long the DMA may complete nothing before the chain is rebuilt.
///
/// A ring-fill deadline, not a frame-period one: the panel copy is synchronous, so between
/// two polls the DMA is part-way through the ring and never through a frame.
const STALL_MS: u64 = 400;

/// Data-enable, not sync-and-enable: the sensor has no HSYNC line, and asking for one leaves
/// the peripheral waiting on a pin that never moves.
///
/// `EofMode::ByteLen`, not `VsyncSignal`: the latter raises one EOF per frame while a
/// descriptor is a fortieth of a frame, so it would overrun the first one.
pub fn config() -> cam::Config {
    cam::Config::default()
        .with_frequency(Rate::from_hz(PIXEL_CLOCK_HZ))
        .with_vh_de_mode(VhdeMode::De)
        .with_eof_mode(EofMode::ByteLen(EOF_BYTE_LEN))
}

/// A descriptor chain over an arena of internal DRAM, sized in whole frames. Owned across a
/// transfer, so not behind a reference — the peripheral holds it as it runs.
pub struct FrameRing {
    descriptors: DmaAlignedMut<'static, [DmaDescriptor]>,
    buffer: DmaAlignedMut<'static, [u8]>,
    /// Whether the chain has been logged. A plain `bool` rather than an atomic because
    /// `prepare` only ever runs on the one executor that owns the capture.
    chain_reported: bool,
}

impl FrameRing {
    /// Logs the first, second and last descriptor's buffer, size and successor, once. A
    /// chain that is not what it was meant to be completes nothing and says no reason why.
    /// Read without invalidating: this is what was *asked for*, not what the DMA did.
    fn log_descriptors(&self) {
        let base = self.descriptors.as_ptr() as usize;
        for index in [0, 1, DESCRIPTOR_COUNT - 1] {
            let Some(descriptor) = self.descriptors.get(index) else {
                continue;
            };
            log::info!(
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
        // Walk it the way the DMA will: a bad link stops the DMA there, which looks like a
        // sensor or a stall and is neither.
        let mut cursor = base;
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
            cursor = descriptor.next as usize;
        }
        log::info!("[CAM] chain verified: {walked} descriptors, ending at 0x{cursor:08x}");
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
        // Validates the region's address, length and alignment — a PSRAM buffer has to
        // be cache-line aligned throughout, which is what the frame geometry above is
        // arranged to satisfy.
        let buffer = DmaAlignedMut::new(arena)?;
        // Descriptors come from a static, so they are in internal memory, which is the
        // only place a descriptor list may live.
        //
        // The three-argument form: the two-argument one's second argument is the *TX*
        // total size, so it would cut the receive side into the HAL's default 4092-byte
        // chunks and overrun the ring.
        let (descriptors, _) = dma_descriptors_chunk_size!(RING_BYTES, RING_BYTES, CHUNK_BYTES);
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
        // Runs once: the arena never moves and descriptors are addressed by index.
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
///
/// What a poll may do while the DMA owns part of it: count what has been finished, and
/// hand out a frame that is entirely finished.
pub struct FrameRingView {
    ring: FrameRing,
    /// The frame the previous poll answered with, so a poll can say whether it found
    /// anything newer.
    last: Option<usize>,
}

impl FrameRingView {
    /// The newest whole frame the DMA has finished. Nothing goes back to the DMA, which is
    /// already past every frame behind it and will not return until the chain is rebuilt.
    pub fn take_latest(&mut self) -> Option<FramePick> {
        let finished = self.finished_descriptors();
        let complete = finished / DESCRIPTORS_PER_FRAME;
        let newest = complete.checked_sub(1)?;
        // "Nothing new" is the DMA not advancing, not the index repeating: with a one-frame
        // ring the index is always 0, so comparing indices would freeze the frame count.
        let unchanged = self.last == Some(finished);
        self.last = Some(finished);
        Some(FramePick {
            frame: newest,
            finished,
            unchanged,
        })
    }

    /// The pixels of `index`. Invalidates the cache first: the DMA wrote these bytes through
    /// memory the CPU has never read, so its cached copy is not the frame.
    pub fn frame(&self, index: usize) -> Option<&[u8]> {
        if index >= RING_FRAMES {
            return None;
        }
        let start = index * FRAME_BYTES;
        let slice = &self.ring.buffer[start..start + FRAME_BYTES];
        // Cannot fail: a frame starts a whole frame from a cache-line aligned base.
        DmaAlignedRef::new(slice)
            .expect("a frame is a whole number of aligned chunks")
            .invalidate();
        Some(slice)
    }

    /// The leading run of descriptors the DMA has finished. A prefix rather than a count:
    /// the DMA fills in order, so a hole would mean nothing after it can be trusted either.
    fn finished_descriptors(&self) -> usize {
        // Descriptors may be cached even in internal memory; freed here.
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

    /// Hands every descriptor back and points the channel at the first. The only place a
    /// linear ring is recycled; idempotent, as the trait requires.
    fn prepare(&mut self) -> Preparation {
        // Built here, not once in `new`: `receive` calls `prepare`, so this runs for the first
        // transfer and every re-arm, and a rebuilt chain cannot be left half-linked.
        //
        // Backwards, with the last descriptor's `next` left null, which is what makes
        // the list a line rather than a circle.
        let mut next = core::ptr::null_mut();
        for descriptor in self.descriptors.iter_mut().rev() {
            descriptor.next = next;
            next = descriptor;
            descriptor.reset_for_rx();
        }
        // The DMA reads the list, so the CPU's cached copy of it is the one that has to
        // be current.
        self.descriptors.writeback();
        // Once, on the first transfer: the chain is the same on every re-arm, and a line
        // printed per re-arm would bury the numbers that vary.
        if !self.chain_reported {
            self.chain_reported = true;
            self.log_descriptors();
        }

        Preparation {
            start: self.descriptors.as_mut_ptr(),
            // Must agree with where the ring is: this bit is what selects the burst
            // configuration below.
            accesses_psram: false,
            // Burst on, 32 bytes. The HAL offers one enum per memory kind and picks
            // between them by `accesses_psram`, so this is the internal setting. Not the
            // 64 the peripheral also takes: at 64 (`ext_mem_bk_size = 2`) writes come out
            // scrambled — complete, correctly sized and correctly framed frames with the
            // pixels wrong inside them.
            burst_transfer: BurstConfig::from(InternalBurstConfig::Enabled),
            // Both of these are what the HAL's own receive buffers use, and nothing here
            // rests on more: a descriptor is only handed back by a re-arm and nothing else
            // reaches it, and the field is documented as ignored for receives.
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
    log::info!(
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
/// A capture: a transfer, drained a frame at a time.
///
/// Nothing on the capture path touches `S`, because the frames are the peripheral's rather
/// than the part's.
pub struct Gc2145Capture<S> {
    /// `None` once a re-arm has failed — the one state a capture cannot come back from,
    /// since a transfer that does not take has no ring to put back in the chain. An
    /// `Option` so a failed repair leaves a capture that reports no frames rather than one
    /// that panics on the next poll.
    transfer: Option<Transfer>,
    /// The peripheral, held while a surface owns the buffer — what [`Self::pause`] recovers,
    /// so a pause costs one stop rather than a whole re-bring-up.
    parked: Option<(esp_hal::lcd_cam::cam::Camera<'static>, FrameRing)>,
    sensor: S,
    frames: u32,
    repeated: u32,
    restarts: u32,
    finished: usize,
    /// When the DMA last completed a descriptor. Not when it was armed: a chain that starts
    /// and stops part-way is the common case and a deadline from the arm never fires.
    advanced_at: Option<u64>,
    /// The frame the last poll reported. Separate so a caller can read it without the
    /// poll's borrow keeping the capture borrowed.
    latest: Option<usize>,
    /// A full chain waiting to be rebuilt, held back one pass so the caller can read the
    /// frame first. See `poll`.
    rearm_pending: bool,
    /// The sensor's readout geometry as last read. Over I²C, so a caller printing it does not
    /// pay for the read on every repaint.
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
    ///
    /// Cached because it is I²C and a readout asks for it every repaint: at 4.5 fps that is
    /// twenty-five reads a second for a number that only changes when the board is
    /// reconfigured. [`Self::forget_geometry`] drops it, so a board that reprograms the window
    /// gets the new one read rather than the old one printed.
    pub fn geometry(&mut self) -> Option<WindowGeometry> {
        if self.geometry.is_none() {
            self.geometry = window_geometry(&mut self.sensor);
        }
        self.geometry
    }

    /// Drops the cached geometry, so the next [`Self::geometry`] reads the part again.
    pub fn forget_geometry(&mut self) {
        self.geometry = None;
    }

    /// The pixels the last [`Self::poll`] reported. Separate from the poll because that has
    /// to mutate the capture, and a borrow would hold it borrowed as long as the caller keeps
    /// this result.
    pub fn latest_frame(&self) -> Option<&[u8]> {
        let frame = self.latest?;
        self.transfer.as_ref()?.frame(frame)
    }

    pub fn sensor(&mut self) -> &mut S {
        &mut self.sensor
    }

    /// Looks at the ring and reports what it found. The whole interface: never blocks,
    /// never waits for a frame, never hands the same frame over twice.
    pub fn poll(&mut self, now_ms: u64) -> CameraCounters {
        let Some(transfer) = self.transfer.as_mut() else {
            return self.counters();
        };
        // `CameraTransfer` derefs to its buffer's view, so this is the ring's.
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
        // The end of the line is the only place worth re-arming from: the next start is
        // there again a frame boundary.
        if pick.finished >= DESCRIPTOR_COUNT {
            // Held back one pass, because rebuilding restarts the DMA into the same frame
            // and clears `latest` — re-arming in the call that reports the frame would take
            // it back.
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

    /// Rebuilds the chain if the DMA has completed nothing for long enough that it is
    /// not going to.
    fn check_stall(&mut self, now_ms: u64) -> CameraCounters {
        let advanced_at = *self.advanced_at.get_or_insert(now_ms);
        if now_ms.saturating_sub(advanced_at) <= STALL_MS {
            return self.counters();
        }
        // One bit splits the two ways this can happen: stopped means the receive FIFO
        // backed up so data is arriving, running means the peripheral waits for data.
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

    /// Puts a fresh transfer on the same ring.
    ///
    /// The peripheral is consumed by a transfer, so the ring comes out of `stop` and
    /// goes straight into `receive`, allocating nothing.
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

/// Bounded by [`WindowReadable`] and nothing else: a capture whose part cannot be read cannot
/// report what the part holds, and a panel printing a number nobody can fetch is worse than
/// one printing nothing.
impl<S: WindowReadable> FrameSource for Gc2145Capture<S> {
    fn advance(&mut self, _frame: &mut [u8], now_ms: u64) -> FrameAdvance {
        let stalled_at = self.finished;
        // `poll` is what repairs the chain, so it has to run every pass — the panel reading
        // the frame afterwards is not a substitute.
        self.poll(now_ms);
        match self.latest {
            // A frame is only ever reported whole: `poll` hands one over once every
            // descriptor of it is finished, and it is not taken back until the *next* poll.
            // So the buffer the DMA just filled is what the caller reads, with nothing to copy
            // it through — which is the whole reason the panel and the camera share one.
            Some(_) => FrameAdvance::Fresh,
            // The chain rebuilt while this pass ran: the counters moved but nothing arrived.
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
        // The ring `pause` took back, over the same arena the caller still owns: its
        // `prepare` is what rebuilds the descriptor chain, which is the one thing this path
        // shares with the re-arm after every whole frame.
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

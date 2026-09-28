//! Continuous microphone capture for the Audio page.
//!
//! The transfer *is* the capture: the DMA refills a ring in internal memory while
//! this module drains it, never waiting on the DMA — it only asks how much has
//! been finished. The engine stops on its own once the ring is full and nothing
//! restarts it, so draining alone does not keep a capture alive; see
//! [`Es7210Rx::restart`].

use esp_hal::Blocking;
use esp_hal::dma::DmaRxStreamBuf;
use esp_hal::dma_rx_stream_buffer;
use esp_hal::i2s::master::I2sRxDmaTransfer;
use esp_hal::i2s::master::{Channels, DataFormat, TdmConfig};
use esp_hal::time::Rate;
use iot_core::drivers::audio::{
    AudioSample, AudioSource, BYTES_PER_POLL, CaptureWatchdog, CornerStep, CornerSweep,
    SAMPLE_RATE_HZ, SampleStream, dbfs, spl,
};

use crate::components::es7210::{HPF_CORNER, HPF_CORNER_CODES, HpfCorner, SPL_OFFSET_DECIBELS};

/// One-shot corner walk at boot, a field aid for telling a room that is genuinely
/// noisy from one that only sounds noisy below the filter's corner. A
/// compile-time switch (not a Cargo feature) for the same reason the panel's
/// debug rows are one: it measures rather than renders, so a production build
/// turns it off by setting this to `false` and the walk — and the four register
/// writes per code that come with it — leaves the binary entirely.
///
/// It walks every corner and returns to [`HPF_CORNER`], so it costs one boot's
/// settling and nothing after: the walk drops itself when it is done.
const HPF_CORNER_WALK: bool = true;

/// Reports the envelope's floor and peak live, every [`CAL_LOG_MS`], for the
/// one-shot anchoring of [`SPL_OFFSET_DECIBELS`] against a known source. A
/// compile-time switch like [`HPF_CORNER_WALK`], off in production builds: flip
/// it for a calibration session and the log — and the poll it costs — leaves
/// the binary entirely once back off.
const CAL_SPL_LOG: bool = false;
const CAL_LOG_MS: u64 = 5_000;

/// Bytes per DMA descriptor. A quarter of the poll period's worth of audio, so
/// the descriptors hand data over at a granularity a 20 ms poll does not notice
/// while the ring still bounds how far behind the panel can fall.
const CHUNK_BYTES: usize = 480;

/// Ring size: three poll periods of capture, so a poll two periods late costs a
/// column of the sweep rather than a gap. Sized in poll periods because that is
/// what the ring absorbs — one period of slack is a ring that is *always* full.
/// Also the capture's liveness yardstick, so ring and watchdog cannot be sized
/// apart.
const STREAM_BYTES: usize = BYTES_PER_POLL * 3;

/// Warn once two poll periods are waiting. A poll that arrives on time finds
/// about one period of audio waiting, so this is crossed only once the input loop
/// is a whole poll behind — past that the loop is the thing falling behind, not
/// the codec, and it is worth saying out loud.
const BACKLOG_WARN_BYTES: usize = BYTES_PER_POLL * 2;

/// Ring depth below which the backlog counts as cleared: one poll period, so a
/// poll hovering on the line does not re-warn every 20 ms.
const BACKLOG_CLEAR_BYTES: usize = BYTES_PER_POLL;

/// The sizing rule the ring and the watchdog are both derived from, asserted
/// because a board that gets it wrong finds out from a capture that looks alive
/// and is not — the one symptom a diagnostic on this path cannot tell from a
/// quiet room.
const _: () = assert!(
    STREAM_BYTES >= BYTES_PER_POLL * 2,
    "the capture ring must hold two poll periods, or a late poll overruns it"
);

/// The transfer a capture runs on. Named so a board can hold one without naming
/// a DMA buffer type, and so the `'static` the boxed capture source needs is
/// written down once, here rather than at each use.
pub type Capture = I2sRxDmaTransfer<'static, Blocking, DmaRxStreamBuf>;

/// The codec-facing half of the wire format: Philips framing, 16 data bits, and a
/// frame left two slots wide, so both slots reach the DMA buffer. `Channels::MONO`
/// selects a slot with a TDM mask and leaves the peripheral's own mono mode off, so
/// the frame is not shortened — a capture sized as one slot per frame fills a ring
/// twice as fast as [`BYTES_PER_POLL`] assumes, and the symptom is a live-looking
/// envelope over a DMA restarted every poll. Both slots carry the same stream —
/// captured into separate envelopes they matched sample for sample — so the fold
/// takes every slot and there is no second microphone to miss.
pub fn tdm_config() -> TdmConfig {
    TdmConfig::new_tdm_philips()
        .with_sample_rate(Rate::from_hz(SAMPLE_RATE_HZ))
        .with_data_format(DataFormat::Data16Channel16)
        .with_channels(Channels::MONO)
}

/// Allocates the capture ring in DMA-capable internal memory.
pub fn stream() -> DmaRxStreamBuf {
    dma_rx_stream_buffer!(STREAM_BYTES, CHUNK_BYTES)
}

/// A microphone capture that folds its DMA ring into an envelope.
///
/// Dropping it stops the transfer, so a board that takes the source out of
/// `take_audio` and drops it also releases the peripheral.
///
/// `C` is the codec driver, held rather than dropped after bring-up: the ring
/// alone cannot be reconfigured, and the corner walk needs the part to be
/// reachable for the half minute it lasts. It is reached only through
/// [`HpfCorner`], so no I2C or register detail reaches the capture.
pub struct Es7210Rx<C> {
    /// `None` once a re-arm has failed — the one state a capture cannot come
    /// back from, since a re-arm that does not take has no ring to put back in
    /// the chain. An `Option` rather than a field the restart path can empty, so
    /// a failed re-arm leaves a source that reports silence.
    transfer: Option<Capture>,
    stream: SampleStream,
    /// Notices the DMA stopping. The transfer cannot report it itself, so the
    /// capture is timed against the clock — see [`CaptureWatchdog`].
    watchdog: CaptureWatchdog,
    /// Latches once a deep backlog has been reported, so a starved input loop
    /// warns once instead of on every poll.
    backlogged: bool,
    /// Repairs made so far, reported to the panel beside the column count.
    restarts: u16,
    /// The corner walk, while one is running. `None` once it is done or has been
    /// abandoned, which is also the state a capture with the switch off is born
    /// in.
    walk: Option<CornerSweep>,
    /// When the calibration log last ran, so it repeats on [`CAL_LOG_MS`] rather
    /// than every poll.
    last_cal_log_ms: u64,
    /// The codec, held for the walk and for nothing else.
    codec: C,
}

impl<C> Es7210Rx<C> {
    /// Takes a transfer armed on [`stream`]'s ring — the one the watchdog is
    /// sized against, so a ring built any other length would be judged by a
    /// deadline that does not describe it — and the codec that captured it.
    pub fn new(transfer: Capture, codec: C) -> Self {
        if HPF_CORNER_WALK {
            log::info!(
                "[AUDIO] HPF corner walk starting: {HPF_CORNER_CODES} codes, \
                 {} ms each, back on corner {HPF_CORNER} after",
                iot_core::drivers::audio::CORNER_SETTLE_MS
            );
        }
        Self {
            transfer: Some(transfer),
            stream: SampleStream::new(),
            watchdog: CaptureWatchdog::new(STREAM_BYTES),
            backlogged: false,
            restarts: 0,
            walk: HPF_CORNER_WALK.then_some(CornerSweep::new(HPF_CORNER_CODES)),
            last_cal_log_ms: 0,
            codec,
        }
    }

    /// Advances the corner walk by one poll and reports what the envelope says
    /// once a code has settled.
    ///
    /// Takes the walk out of `self` for the length of the step, because a step
    /// both reads the walk and reaches the codec and the two are fields of the
    /// same struct. A write that fails abandons the walk rather than retrying:
    /// a codec that will not take a corner write is not going to take the next
    /// one either, and a walk that limps on would report levels from a part in
    /// a state nobody can name. Either way the corner goes back to
    /// [`HPF_CORNER`] on the way out, so a walk that ends early cannot leave the
    /// input stage more open than it found it.
    fn step_walk(&mut self, now_ms: u64)
    where
        C: HpfCorner,
        C::CornerError: core::fmt::Debug,
    {
        let Some(mut walk) = self.walk.take() else {
            return;
        };
        let write = |codec: &mut C, corner: u8| match codec.set_hpf_corner(corner) {
            Ok(()) => None,
            Err(error) => Some(error),
        };
        let step = walk.step(now_ms);
        self.walk = match step {
            CornerStep::Settling => Some(walk),
            CornerStep::Write(code) => match write(&mut self.codec, code) {
                None => Some(walk),
                Some(error) => {
                    log::error!(
                        "[AUDIO] HPF corner {code} would not write, walk abandoned: {error:?}"
                    );
                    None
                }
            },
            CornerStep::Read(code) => {
                // Two readings, because they answer different questions and the
                // walk needs both. `loudest` is what the panel is drawing, so the
                // walk can be read against the screen; `floor` is the median
                // column, which a cough cannot lift, so it is the one that says
                // whether the corner moved the room or whether a person did.
                let envelope = self.stream.envelope();
                log::info!(
                    "[AUDIO] HPF corner {code} reads {} dBFS, floor {} dBFS",
                    dbfs(envelope.loudest_rms()),
                    dbfs(envelope.floor_rms())
                );
                Some(walk)
            }
            CornerStep::Done => None,
        };
        if self.walk.is_none() {
            if let Some(error) = write(&mut self.codec, HPF_CORNER) {
                log::error!(
                    "[AUDIO] HPF corner {HPF_CORNER} would not restore after the walk: {error:?}"
                );
            } else {
                log::info!("[AUDIO] HPF corner walk done, back on corner {HPF_CORNER}");
            }
        }
    }

    /// Folds everything the DMA has finished into the envelope, in a loop rather
    /// than one descriptor per poll so a late poll costs a column rather than a
    /// widening gap. A partly filled descriptor is folded as it stands, the
    /// trailing-byte carry in [`SampleStream`] making a mid-sample boundary free.
    fn drain(&mut self) {
        let Some(transfer) = self.transfer.as_mut() else {
            return;
        };
        while transfer.available_bytes() > 0 {
            let descriptor = transfer.peek();
            let len = descriptor.len();
            self.stream.push_bytes(descriptor);
            transfer.consume(len);
        }
    }

    /// Re-arms a capture whose DMA has stopped: the only way back is to take the
    /// transfer down and set a new one up, so the ring comes out of `stop` and
    /// goes straight into `read`, allocating nothing. It costs one descriptor of
    /// audio, which on a scope is a column with a notch in it.
    fn restart(&mut self, now_ms: u64) {
        let Some(transfer) = self.transfer.take() else {
            return;
        };
        let (i2s_rx, buffer) = transfer.stop();
        self.restarts = self.restarts.saturating_add(1);
        // The new stream starts on an empty ring, so it has to earn its own
        // deadline rather than be judged against the one the dead one left.
        self.watchdog.note_progress(now_ms);
        match i2s_rx.read(buffer) {
            Ok(transfer) => {
                log::info!(
                    "[AUDIO] capture DMA had stopped, re-armed ({})",
                    self.restarts
                );
                self.transfer = Some(transfer);
            }
            Err((error, _i2s_rx, _buffer)) => {
                log::error!("[AUDIO] capture could not be re-armed, going silent: {error:?}");
            }
        }
    }

    /// What a poll reports. Split out so a capture with no transfer left still
    /// answers, holding the last envelope it managed to fold.
    fn summary(&self, now_ms: u64) -> AudioSample {
        AudioSample {
            envelope: self.stream.envelope(),
            // Wall time, not sample count: a panel reading elapsed time should
            // agree with the clock the rest of the UI is stamped from, and a
            // late poll must not make the capture look shorter than it was.
            elapsed_ms: now_ms.min(u64::from(u32::MAX)) as u32,
            restarts: self.restarts,
            dba_lsb: self.stream.dba_lsb(),
        }
    }
}

impl<C> AudioSource for Es7210Rx<C>
where
    C: HpfCorner,
    C::CornerError: core::fmt::Debug,
{
    fn sample(&mut self, now_ms: u64) -> AudioSample {
        let produced = match self.transfer.as_mut() {
            Some(transfer) => transfer.available_bytes(),
            None => return self.summary(now_ms),
        };
        if produced >= BACKLOG_WARN_BYTES && !self.backlogged {
            log::warn!(
                "[AUDIO] capture ring {produced} B deep, the input loop is behind the codec"
            );
            self.backlogged = true;
        } else if produced <= BACKLOG_CLEAR_BYTES {
            self.backlogged = false;
        }
        self.drain();
        // What arrived before the drain, not what is left after it: a ring drained
        // to empty looks identical whether the DMA filled it or not.
        if self.watchdog.stalled(now_ms, produced) {
            self.restart(now_ms);
        }
        // After the drain, so a code's reading covers audio that arrived under
        // the corner that was written for it.
        self.step_walk(now_ms);
        if CAL_SPL_LOG && now_ms.saturating_sub(self.last_cal_log_ms) >= CAL_LOG_MS {
            self.last_cal_log_ms = now_ms;
            let envelope = self.stream.envelope();
            log::info!(
                "[AUDIO] CAL floor {} dB SPL, rms {} dB SPL",
                spl(dbfs(envelope.floor_rms()), SPL_OFFSET_DECIBELS),
                spl(dbfs(envelope.loudest_rms()), SPL_OFFSET_DECIBELS),
            );
        }
        self.summary(now_ms)
    }
}

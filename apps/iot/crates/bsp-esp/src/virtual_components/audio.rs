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
    AudioSample, AudioSource, BYTES_PER_POLL, CaptureWatchdog, SAMPLE_RATE_HZ, SampleStream,
};

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
/// envelope over a DMA restarted every poll. Which slot MIC1 lands in only hardware
/// confirms: flat through a shout means the part put it in the other slot.
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
pub struct Es7210Rx {
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
}

impl Es7210Rx {
    /// Takes a transfer armed on [`stream`]'s ring — the one the watchdog is
    /// sized against, so a ring built any other length would be judged by a
    /// deadline that does not describe it.
    pub const fn new(transfer: Capture) -> Self {
        Self {
            transfer: Some(transfer),
            stream: SampleStream::new(),
            watchdog: CaptureWatchdog::new(STREAM_BYTES),
            backlogged: false,
            restarts: 0,
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
        }
    }
}

impl AudioSource for Es7210Rx {
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
        self.summary(now_ms)
    }
}

//! Speaker playback for the Speaker page.

use alloc::boxed::Box;
use esp_hal::Blocking;
use esp_hal::dma::DmaTxStreamBuf;
use esp_hal::dma_tx_stream_buffer;
use esp_hal::i2s::master::{
    Channels, DataFormat, Error as I2sError, I2sTx, I2sTxDmaTransfer, TdmConfig,
};
use esp_hal::time::Rate;
use iot_core::drivers::aec::wire_position;
use iot_core::drivers::audio::{
    BYTES_PER_FRAME, CAPTURE_BYTES_PER_FRAME, DECIMATION, SAMPLE_RATE_HZ,
};
use iot_core::drivers::playback::{
    PlayStreamWatchdog, Recovery, SampleSource, Speaker, SpeakerFault, SpeakerOp, interleave_mono,
};

use crate::components::es8311::Mute;
use crate::virtual_components::reference::{Writer, samples_for};

// This is the distance between where a sample is *put on the ring* and where it
// is *on the wire* — 120 ms on this board — and it is the whole reason the
// reference is read through a play cursor rather than a write pointer. A
// reference taken at the write pointer would sit thirty times outside the delay
// search's window, and a filter handed one converges on whatever the correlation
// grid happens to favour.
const TRANSMIT_RUNWAY_FRAMES: usize = RING_BYTES / BYTES_PER_FRAME;

// Frames of microphone latency the reference has to reach back across to pair
// with: this board's capture ring, three poll periods.
//
// Stated here rather than read from `audio` so neither driver depends on the
// other, and asserted against the real ring in both — a figure two modules each
// spelled out is free to disagree, and a stale copy makes the pairing stop short
// of the ring's real depth silently.
//
// **Counted in capture frames, not transmit ones.** The receive ring holds one
// sample per frame where the transmit ring holds two — see
// [`CAPTURE_BYTES_PER_FRAME`] — so a capture ring three poll periods deep is
// twice as many *frames* as the same byte count read as transmit frames.
const RECEIVE_LATENCY_FRAMES: usize = 3 * 1_920 / CAPTURE_BYTES_PER_FRAME;

const _: () = assert!(
    RECEIVE_LATENCY_FRAMES <= TRANSMIT_RUNWAY_FRAMES,
    "the capture ring cannot be deeper than the runway the log has to span"
);

/// Kept samples the shared log holds on this board.
pub const REFERENCE_SAMPLES: usize = samples_for(TRANSMIT_RUNWAY_FRAMES, RECEIVE_LATENCY_FRAMES);

const _: () = assert!(
    REFERENCE_SAMPLES * DECIMATION as usize >= TRANSMIT_RUNWAY_FRAMES,
    "the reference log cannot hold this ring's runway"
);

// Bytes per DMA descriptor: a 5 ms slice of the wire at [`SAMPLE_RATE_HZ`] —
// one feed cadence, the sensible grain for a feed to take over as the DMA
// finishes one.
const CHUNK_BYTES: usize = 960;

// Descriptors in the ring. Twenty-four, so the whole ring is a generous slice
// of runway that absorbs the kind of sub-120 ms delay a shared executor can
// occasionally hand the feed loop.
const RING_CHUNKS: usize = 24;

const RING_BYTES: usize = CHUNK_BYTES * RING_CHUNKS;

const PUSH_FRAMES: usize = RING_BYTES / BYTES_PER_FRAME;

/// The capture's wire format, with its clock shared from the transmit unit.
pub fn shared_tdm_config() -> TdmConfig {
    TdmConfig::new_tdm_philips()
        .with_sample_rate(Rate::from_hz(SAMPLE_RATE_HZ))
        .with_data_format(DataFormat::Data16Channel16)
        .with_channels(Channels::MONO)
        .with_signal_loopback(true)
}

/// Allocates the streaming ring in DMA-capable internal memory — once, for the
/// driver's whole life. The macro hands out a single `const` static, so a second
/// call panics and the one ring is reused.
pub fn stream() -> DmaTxStreamBuf {
    dma_tx_stream_buffer!(RING_BYTES, CHUNK_BYTES)
}

type Voice = Box<dyn SampleSource>;

fn render_slice(
    voice: &mut Option<Voice>,
    buf: &mut [u8],
    produced_sound: &mut bool,
    rendered: &mut [i16; PUSH_FRAMES],
) -> usize {
    let frames = (buf.len() / BYTES_PER_FRAME).min(PUSH_FRAMES);
    let mut produced = 0;
    if let Some(voice) = voice.as_mut() {
        produced = voice.fill(&mut rendered[..frames]);
    }
    *produced_sound |= produced != 0;
    rendered[produced..frames].fill(0);

    interleave_mono(&rendered[..frames], buf)
}

/// What playback needs from a codec driver: the mute latch.
pub struct Es8311Tx<C> {
    codec: C,
    transfer: Option<I2sTxDmaTransfer<'static, Blocking, DmaTxStreamBuf>>,
    voice: Option<Voice>,
    tail: bool,
    watchdog: PlayStreamWatchdog,
    restarts: u32,
    pending: Option<Recovery>,
    reference: Writer<REFERENCE_SAMPLES>,
    rendered: [i16; PUSH_FRAMES],
    pushed_frames: usize,
    played_frames: usize,
}

impl<C> Es8311Tx<C> {
    /// Takes the transmit unit and the codec and arms the streaming transfer: the ring is
    /// prefilled with silence and handed to the DMA, which starts the codec's clocks with
    /// it.
    pub fn new(
        i2s: I2sTx<'static, Blocking>,
        codec: C,
        reference: Writer<REFERENCE_SAMPLES>,
    ) -> Result<Self, I2sError> {
        let mut ring = stream();
        // Zero the ring while it is still CPU-owned: the DMA starts the moment
        // `write` arms it, and a boot that handed the codec a garbage buffer
        // would pop the speaker. The silence this queues also covers the codec
        // while it settles into its locked clock.
        ring.push_with(|slot| {
            slot.fill(0);
            slot.len()
        });
        let prefilled = RING_BYTES / BYTES_PER_FRAME;
        let transfer = i2s.write(ring).map_err(|(error, _, _)| error)?;
        Ok(Self {
            codec,
            transfer: Some(transfer),
            voice: None,
            tail: false,
            watchdog: PlayStreamWatchdog::new(RING_BYTES),
            restarts: 0,
            pending: None,
            reference,
            rendered: [0; PUSH_FRAMES],
            pushed_frames: prefilled,
            played_frames: 0,
        })
    }

    fn push(&mut self) -> bool {
        let Self {
            transfer,
            voice,
            rendered,
            reference,
            pushed_frames,
            ..
        } = self;
        let Some(transfer) = transfer.as_mut() else {
            return false;
        };
        let mut produced_sound = false;
        let written =
            transfer.push_with(|slot| render_slice(voice, slot, &mut produced_sound, rendered));
        let frames = written / BYTES_PER_FRAME;
        *pushed_frames += frames;
        reference.write_frames(rendered[..frames].iter().copied());
        produced_sound
    }

    fn advance_play_cursor(&mut self) {
        let Some(transfer) = self.transfer.as_mut() else {
            return;
        };
        let played = wire_position(
            self.pushed_frames,
            PUSH_FRAMES,
            transfer.available_bytes(),
            BYTES_PER_FRAME,
        );
        let moved = played.saturating_sub(self.played_frames);
        self.played_frames = played;
        self.reference.advance_played(moved);
    }

    fn restart(&mut self) {
        let Some(transfer) = self.transfer.take() else {
            return;
        };
        let (i2s_tx, ring) = transfer.stop();
        match i2s_tx.write(ring) {
            Ok(transfer) => {
                self.restarts += 1;
                self.pushed_frames = PUSH_FRAMES;
                self.played_frames = 0;
                log::warn!(
                    "[PLAY] restarted the outgoing DMA, recovery {}",
                    self.restarts
                );
                self.transfer = Some(transfer);
            }
            Err((error, _, _)) => {
                log::error!("[PLAY] the outgoing DMA refused the ring: {error:?}");
                self.voice = None;
                self.tail = false;
            }
        }
    }

    fn recover_drained(&mut self, recovery: Recovery) {
        let sound = if recovery.playing { "playing" } else { "idle" };
        log::warn!(
            "[PLAY] the outgoing DMA ran the ring dry after {} ms idle, {} of {RING_BYTES} bytes free, sound {sound}",
            self.watchdog.idle_limit_ms(),
            recovery.free_bytes,
        );
        self.restart();
    }
}
impl<C> Speaker for Es8311Tx<C>
where
    C: Mute + Send + 'static,
{
    fn play(&mut self, source: Box<dyn SampleSource>) -> Result<(), SpeakerFault> {
        self.voice = Some(source);
        self.tail = false;
        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), SpeakerFault> {
        self.codec.set_muted(muted).map_err(|_| {
            SpeakerFault::new(SpeakerOp::SetMuted, "the codec stopped answering on I2C")
        })
    }

    fn feed(&mut self, now_ms: u64) -> bool {
        self.advance_play_cursor();
        let drained = match self.transfer.as_mut() {
            Some(transfer) => {
                let drained = self.watchdog.report_drained(now_ms, transfer.is_done());
                if drained {
                    self.pending = Some(Recovery {
                        free_bytes: transfer.available_bytes(),
                        playing: self.voice.is_some(),
                    });
                }
                drained
            }
            None => false,
        };
        let pushed_sound = self.push();
        if drained {
            debug_assert!(self.pending.is_some());
        }
        if self.voice.as_ref().is_some_and(|voice| voice.done()) {
            self.voice = None;
        }
        let still = self.voice.is_some() || self.tail || pushed_sound;
        self.tail = pushed_sound;
        still
    }

    fn recover(&mut self) -> Option<Recovery> {
        let recovery = self.pending.take()?;
        self.recover_drained(recovery);
        Some(recovery)
    }
}

impl<C: crate::hardware::SelfCheck> crate::hardware::SelfCheck for Es8311Tx<C> {
    fn name(&self) -> &'static str {
        self.codec.name()
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        self.codec.id()
    }

    fn check(&mut self) -> crate::hardware::Status {
        self.codec.check()
    }
}

impl<C: crate::hardware::Named> crate::hardware::Named for Es8311Tx<C> {
    const NAME: &'static str = C::NAME;
}

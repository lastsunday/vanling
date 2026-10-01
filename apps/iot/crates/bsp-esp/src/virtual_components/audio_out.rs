//! Speaker playback for the Speaker page.
//!
//! The transmit unit is started once, at bring-up, as a *streaming* transfer on a ring
//! the DMA walks round and round: every feed refills the slice the DMA has finished, so
//! the codec holds one locked clock from boot. That same idle stream carries the
//! capture's clock, so it is fed silence between sounds rather than stopped. A push is
//! bounded by what the DMA has consumed, so a feed cannot overwrite unplayed audio.
//!
//! Idleness is not itself a fault: [`PlayStreamWatchdog`] weighs it against a whole
//! ring's playthrough so only a genuine drain counts.

use esp_hal::Blocking;
use esp_hal::dma::DmaTxStreamBuf;
use esp_hal::dma_tx_stream_buffer;
use esp_hal::i2s::master::{
    Channels, DataFormat, Error as I2sError, I2sTx, I2sTxDmaTransfer, TdmConfig,
};
use esp_hal::time::Rate;
use iot_core::drivers::audio::{BYTES_PER_FRAME, SAMPLE_RATE_HZ};
use iot_core::drivers::playback::{
    Pcm, PlayStreamWatchdog, Recovery, SampleSource, Sound, Speaker, SpeakerFault, SpeakerOp, Tone,
};

use crate::components::es8311::Mute;

/// The stored sound [`Sound::Asset`] plays, as mono 16-bit little-endian samples
/// at [`SAMPLE_RATE_HZ`] — the format the DMA transmits, so the file is the wire,
/// and nothing is copied into RAM to reach it.
///
/// The file is opaque, so its recipe (pitches, inharmonic partials, ramps, level)
/// is in the record rather than in a generator the build does not run.
const ASSET: &[u8] = include_bytes!("../assets/asset.pcm");

/// Bytes per DMA descriptor: a 5 ms slice of the wire at [`SAMPLE_RATE_HZ`] —
/// one feed cadence, the sensible grain for a feed to take over as the DMA
/// finishes one.
const CHUNK_BYTES: usize = 960;

/// Descriptors in the ring. Twenty-four, so the whole ring is a generous slice
/// of runway that absorbs the kind of sub-120 ms delay a shared executor can
/// occasionally hand the feed loop.
const RING_CHUNKS: usize = 24;

/// The streaming ring: [`RING_CHUNKS`] descriptors, [`CHUNK_BYTES`] each. Sized
/// as runway rather than against a cadence — 120 ms of audio — so a poll can
/// arrive that late before the DMA runs the ring dry, and large enough to take a
/// feed's whole push in one contiguous slice.
const RING_BYTES: usize = CHUNK_BYTES * RING_CHUNKS;

/// Frames one feed pushes at most, sized to a ring: a feed copies up to a whole
/// ring's worth and the DMA keeps pace with it, so the ring never fills and the
/// transfer never stalls on a slow poll.
const PUSH_FRAMES: usize = RING_BYTES / BYTES_PER_FRAME;

/// The capture's wire format, with its clock shared from the transmit unit.
///
/// `with_signal_loopback` shares BCLK/WS and slaves the receive unit to follow
/// them; it does not carry samples across, and the name is the one misleading
/// thing about it. Without it both units free-run on separate dividers, and each
/// fault is quiet in a way that reads as the other: a speaker on a misaligned
/// clock crackles, a microphone sampled off-clock reads constant full scale that
/// looks like a loud room.
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

/// What the ring is being filled from. The catalogue's two sounds, and silence —
/// which is a voice too, in that a ring is fed either way and the difference is
/// only what goes on it.
enum Voice {
    Chime(Tone),
    Asset(Pcm<'static>),
}

impl SampleSource for Voice {
    fn fill(&mut self, out: &mut [i16]) -> usize {
        match self {
            Self::Chime(tone) => tone.fill(out),
            Self::Asset(asset) => asset.fill(out),
        }
    }

    fn done(&self) -> bool {
        match self {
            Self::Chime(tone) => tone.done(),
            Self::Asset(asset) => asset.done(),
        }
    }
}

/// Renders audio into a ring slice: up to [`PUSH_FRAMES`] frames of the voice's
/// next samples, or silence when there is none, each stereo-ised to the wire
/// format. One frame at a time, so a slice ending mid-ring cannot strand a voice
/// ahead of the DMA. Answers the bytes written, for a bounded hand-off.
fn render_slice(voice: &mut Option<Voice>, buf: &mut [u8], produced_sound: &mut bool) -> usize {
    let mut written = 0;
    for frame in buf.chunks_exact_mut(BYTES_PER_FRAME).take(PUSH_FRAMES) {
        let sample = match voice.as_mut() {
            Some(voice) => {
                let mut single = [0i16; 1];
                *produced_sound |= voice.fill(&mut single) == 1;
                single[0]
            }
            None => 0,
        };
        for slot in frame.chunks_exact_mut(2) {
            slot.copy_from_slice(&sample.to_le_bytes());
        }
        written += BYTES_PER_FRAME;
    }
    written
}

/// What playback needs from a codec driver: the mute latch. The bring-up, the
/// clock row and the registers stay behind I2C in the driver, so the transport
/// never names a register or a bus.
pub struct Es8311Tx<C> {
    codec: C,
    /// The one streaming transfer the driver runs on: armed at bring-up and fed
    /// for its whole life, and rebuilt only for a ring the DMA has genuinely run
    /// dry, because a rebuild costs the codec its clock lock. It owns the
    /// transmit unit and the ring, and through them the clock pins both codecs
    /// are driven from.
    transfer: Option<I2sTxDmaTransfer<'static, Blocking, DmaTxStreamBuf>>,
    /// What is on the ring, `None` when the next feed is silence.
    voice: Option<Voice>,
    /// Whether the last feed put sound on the ring, which a spent source keeps
    /// `feed` true for one cadence after its samples end.
    tail: bool,
    /// What watches the transfer for a ring the DMA has genuinely run dry, so
    /// that the one case worth a clock reset is repaired no more often than it
    /// actually happens.
    watchdog: PlayStreamWatchdog,
    /// How many times a drained ring has been recovered, so a log can say whether
    /// the speaker is coping or falling behind.
    restarts: u32,
    /// What the last [`feed`](Speaker::feed) noticed and could not repair, because
    /// repairing it would have meant logging and allocating from an interrupt.
    /// Taken by [`recover`](Speaker::recover) on the cooperative side.
    pending: Option<Recovery>,
}

impl<C> Es8311Tx<C> {
    /// Takes the transmit unit and the codec and arms the streaming transfer:
    /// the ring is prefilled with silence and handed to the DMA, which starts
    /// the codec's clocks with it. Left alone after that — a feed finding it
    /// momentarily idle is the ordinary state between feeds, not a fault.
    ///
    /// Fallible because the DMA can refuse the ring, and a ring that would not
    /// take is not a speaker anyone can hear.
    pub fn new(i2s: I2sTx<'static, Blocking>, codec: C) -> Result<Self, I2sError> {
        let mut ring = stream();
        // Zero the ring while it is still CPU-owned: the DMA starts the moment
        // `write` arms it, and a boot that handed the codec a garbage buffer
        // would pop the speaker. The silence this queues also covers the codec
        // while it settles into its locked clock.
        ring.push_with(|slot| {
            slot.fill(0);
            slot.len()
        });
        let transfer = i2s.write(ring).map_err(|(error, _, _)| error)?;
        Ok(Self {
            codec,
            transfer: Some(transfer),
            voice: None,
            tail: false,
            watchdog: PlayStreamWatchdog::new(RING_BYTES),
            restarts: 0,
            pending: None,
        })
    }

    /// Hands the DMA the next slice of the ring: up to [`PUSH_FRAMES`] frames of
    /// the voice, then silence to the end of what the DMA has freed. Reports
    /// whether any of that slice was sound.
    fn push(&mut self) -> bool {
        let Some(transfer) = self.transfer.as_mut() else {
            return false;
        };
        let voice = &mut self.voice;
        let mut produced_sound = false;
        transfer.push_with(|slot| render_slice(voice, slot, &mut produced_sound));
        produced_sound
    }

    /// Rebuilds the streaming transfer after the DMA has run the ring dry, which
    /// leaves it latched idle and sending nothing however much is pushed.
    ///
    /// Called with the ring already refilled, and both halves of that order are
    /// counterintuitive: [`feed`](Speaker::feed) pushes first because that is the only
    /// way to write into a live transfer, and the buffer `stop` returns must *not* be
    /// pushed into — it comes back whole with `pre_filled` set, so a push is offered the
    /// empty tail past the ring's end. `write` replays that whole ring, so priming first
    /// is what makes the replay this cadence's audio rather than the sound that stalled.
    fn restart(&mut self) {
        let Some(transfer) = self.transfer.take() else {
            return;
        };
        let (i2s_tx, ring) = transfer.stop();
        match i2s_tx.write(ring) {
            Ok(transfer) => {
                self.restarts += 1;
                log::warn!(
                    "[SPEAKER] restarted the outgoing DMA, recovery {}",
                    self.restarts
                );
                self.transfer = Some(transfer);
            }
            Err((error, _, _)) => {
                log::error!("[SPEAKER] the outgoing DMA refused the ring: {error:?}");
                self.voice = None;
                self.tail = false;
            }
        }
    }

    /// The repair half of a drain: rebuild the stream and report what the ring
    /// looked like when it was noticed.
    ///
    /// On the cooperative side because the rebuild allocates and the log takes the
    /// logger's lock; a feed that preempted a holder of either would deadlock with
    /// no reset to clear it. Nothing is lost by the deferral — the ring is already
    /// dry, so the stream is silent for a cadence either way.
    fn recover_drained(&mut self, recovery: Recovery) {
        let sound = if recovery.playing { "playing" } else { "idle" };
        log::warn!(
            "[SPEAKER] the outgoing DMA ran the ring dry after {} ms idle, {} of {RING_BYTES} bytes free, sound {sound}",
            self.watchdog.idle_limit_ms(),
            recovery.free_bytes,
        );
        self.restart();
    }
}
impl<C> Speaker for Es8311Tx<C>
where
    // `Send` because `Speaker` demands it: the feed that drives this driver runs
    // from an interrupt executor, and anything it owns has to be safe to reach
    // from interrupt context. Every codec here is — its I2C bus sits behind a
    // `CriticalSectionRawMutex`, whose contents are only ever reached with
    // interrupts masked.
    C: Mute + Send + 'static,
{
    /// Starts `sound` from the beginning, replacing anything playing.
    ///
    /// Nothing of the stream is touched: the sound is just what the next feeds
    /// put on the ring, so a tap lands mid-silence without interrupting the
    /// codec's lock — the pull of the draw, putting sound on a stream that has
    /// been running all along.
    fn play(&mut self, sound: Sound) -> Result<(), SpeakerFault> {
        self.voice = Some(match sound {
            Sound::Chime => Voice::Chime(Tone::chime()),
            Sound::Asset => Voice::Asset(Pcm::new(ASSET)),
        });
        self.tail = false;
        Ok(())
    }

    /// Latches the DAC quiet, or audible again. The stream is not touched, so a
    /// sound started while muted plays from its beginning rather than from
    /// wherever the output was left.
    fn set_muted(&mut self, muted: bool) -> Result<(), SpeakerFault> {
        self.codec.set_muted(muted).map_err(|_| {
            SpeakerFault::new(SpeakerOp::SetMuted, "the codec stopped answering on I2C")
        })
    }

    /// Advances the stream by one cadence and reports whether a sound is still
    /// going.
    ///
    /// Called even when nothing is playing: the idle stream carrying the capture's clocks
    /// must be fed silence too. A drain is only *recorded* here — repairing needs an
    /// allocation and the logger's lock, neither of which an interrupt may take (see
    /// [`recover`](Self::recover)) — so the priming push is load-bearing for
    /// [`restart`](Self::restart). A sound is done once its source is spent and the last
    /// frames it queued have played out.
    fn feed(&mut self, now_ms: u64) -> bool {
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
            // Recording is all this feed owes the stream. The push above is the
            // half that has to happen here, and it is enough: the ring is whole by
            // the time the cooperative side rebuilds, so what the restart replays
            // is this cadence's audio rather than stale.
            debug_assert!(self.pending.is_some());
        }
        if self.voice.as_ref().is_some_and(|voice| voice.done()) {
            self.voice = None;
        }
        let still = self.voice.is_some() || self.tail || pushed_sound;
        self.tail = pushed_sound;
        still
    }

    /// Rebuilds a stream the last [`feed`](Self::feed) found drained.
    ///
    /// Not a no-op on most hardware, and `None` whenever nothing is outstanding,
    /// so the caller can ask unconditionally from its own task.
    fn recover(&mut self) -> Option<Recovery> {
        let recovery = self.pending.take()?;
        self.recover_drained(recovery);
        Some(recovery)
    }
}

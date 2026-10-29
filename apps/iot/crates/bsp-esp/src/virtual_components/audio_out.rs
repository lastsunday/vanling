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

/// This is the distance between where a sample is *put on the ring* and where it
/// is *on the wire* — 120 ms on this board — and it is the whole reason the
/// reference is read through a play cursor rather than a write pointer. A
/// reference taken at the write pointer would sit thirty times outside the delay
/// search's window, and a filter handed one converges on whatever the correlation
/// grid happens to favour.
const TRANSMIT_RUNWAY_FRAMES: usize = RING_BYTES / BYTES_PER_FRAME;

/// Frames of microphone latency the reference has to reach back across to pair
/// with: this board's capture ring, three poll periods.
///
/// Stated here rather than read from `audio` so neither driver depends on the
/// other, and asserted against the real ring in both — a figure two modules each
/// spelled out is free to disagree, and a stale copy makes the pairing stop short
/// of the ring's real depth silently.
///
/// **Counted in capture frames, not transmit ones.** The receive ring holds one
/// sample per frame where the transmit ring holds two — see
/// [`CAPTURE_BYTES_PER_FRAME`] — so a capture ring three poll periods deep is
/// twice as many *frames* as the same byte count read as transmit frames.
const RECEIVE_LATENCY_FRAMES: usize = 3 * 1_920 / CAPTURE_BYTES_PER_FRAME;

/// The lag has to be *inside* the runway, or the log is sized for a ring shallower
/// than the one that exists and every pair past the difference is drawn from
/// nothing. The reverse — a log longer than needed — costs memory and nothing else.
const _: () = assert!(
    RECEIVE_LATENCY_FRAMES <= TRANSMIT_RUNWAY_FRAMES,
    "the capture ring cannot be deeper than the runway the log has to span"
);

/// Kept samples the shared log holds on this board.
pub const REFERENCE_SAMPLES: usize = samples_for(TRANSMIT_RUNWAY_FRAMES, RECEIVE_LATENCY_FRAMES);

/// The log covers the whole transmit runway, so it cannot lap while the feed keeps
/// up — asserted against the geometry rather than left to the arithmetic above
/// being edited correctly.
const _: () = assert!(
    REFERENCE_SAMPLES * DECIMATION as usize >= TRANSMIT_RUNWAY_FRAMES,
    "the reference log cannot hold this ring's runway"
);

/// Bytes per DMA descriptor: a 5 ms slice of the wire at [`SAMPLE_RATE_HZ`] —
/// one feed cadence, the sensible grain for a feed to take over as the DMA
/// finishes one.
const CHUNK_BYTES: usize = 960;

/// Descriptors in the ring. Twenty-four, so the whole ring is a generous slice
/// of runway that absorbs the kind of sub-120 ms delay a shared executor can
/// occasionally hand the feed loop.
const RING_CHUNKS: usize = 24;

/// The streaming ring: [`RING_CHUNKS`] descriptors, [`CHUNK_BYTES`] each, sized
/// as runway rather than against a cadence and large enough to take a feed's
/// whole push in one contiguous slice.
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

type Voice = Box<dyn SampleSource>;

/// Renders audio into a ring slice: up to [`PUSH_FRAMES`] frames of the voice's
/// next samples, or silence when there is none, each stereo-ised to the wire
/// format. Answers the bytes written, for a bounded hand-off.
///
/// Two steps, not one fused loop: the voice is asked for a whole slice of mono
/// frames, then [`interleave_mono`] widens them. Asking one frame at a time
/// meant re-entering the voice 240 times a feed, and — the reason the two are
/// separate here rather than the same thing — it left the interleaving itself
/// untested, because the version on this path was a copy of a function no test
/// could reach.
///
/// The frames are also copied into `rendered`, which is what makes the
/// transmitted stream readable back for a canceller. It is written on the *write*
/// side — the only place a transmitted sample exists — and read on the *play*
/// side, a ring's worth of runway behind; see [`crate::virtual_components::reference`].
fn render_slice(
    voice: &mut Option<Voice>,
    buf: &mut [u8],
    produced_sound: &mut bool,
    rendered: &mut [i16; PUSH_FRAMES],
) -> usize {
    // Whole frames only: a tail too short for one would be half a frame on the
    // wire, which is a click.
    let frames = (buf.len() / BYTES_PER_FRAME).min(PUSH_FRAMES);
    let mut produced = 0;
    if let Some(voice) = voice.as_mut() {
        produced = voice.fill(&mut rendered[..frames]);
    }
    *produced_sound |= produced != 0;
    // Whatever the voice did not reach is silence, and has to be said so: the
    // tail of a spent score is the gap between a sound's last note and its end.
    rendered[produced..frames].fill(0);

    interleave_mono(&rendered[..frames], buf)
}

/// What playback needs from a codec driver: the mute latch.
///
/// `C` is held for the mute latch and nothing else. The reference log is reached
/// through a `&'static` handle rather than owned, because the capture side reads
/// it from a different executor — see [`crate::virtual_components::reference`].
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
    /// The write end of the reference log the capture side reads, and the only
    /// reason this driver knows a microphone exists.
    ///
    /// Written from `render_slice`, which is the only place a transmitted sample
    /// exists, and read through the play cursor rather than the write pointer —
    /// which sits [`TRANSMIT_RUNWAY_FRAMES`] ahead of the wire.
    reference: Writer<REFERENCE_SAMPLES>,
    /// The frames the last push rendered, handed to the log afterwards. Driver
    /// state rather than a local because a push can render a whole ring and this
    /// is sized to that.
    rendered: [i16; PUSH_FRAMES],
    /// Frames handed to the ring since it was armed, **including the prefill**.
    ///
    /// The prefill counts because the ring is handed to the DMA already full: a
    /// whole ring of silence is queued and unsent at that moment, and leaving it
    /// out of the count is what makes the first play-cursor reading wrong rather
    /// than merely early.
    pushed_frames: usize,
    /// Frames the DMA had finished at the last cursor advance, so the delta
    /// between two readings is how far the wire moved. Monotonic across the
    /// stream's life and reset by a [`restart`](Self::restart), which replays
    /// the whole ring and so moves the wire back to its start.
    played_frames: usize,
}

impl<C> Es8311Tx<C> {
    /// Takes the transmit unit and the codec and arms the streaming transfer:
    /// the ring is prefilled with silence and handed to the DMA, which starts
    /// the codec's clocks with it. Left alone after that — a feed finding it
    /// momentarily idle is the ordinary state between feeds, not a fault.
    ///
    /// Fallible because the DMA can refuse the ring, and a ring that would not
    /// take is not a speaker anyone can hear.
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

    /// Hands the DMA the next slice of the ring: up to [`PUSH_FRAMES`] frames of
    /// the voice, then silence to the end of what the DMA has freed. Reports
    /// whether any of that slice was sound, and copies the frames it rendered
    /// into the reference log.
    ///
    /// The log is written **after** the push rather than inside it: the ring's
    /// free space bounds what the DMA will take, so the frames that reach the wire
    /// are the ones the push actually wrote — and only the push knows that.
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
        // Through the binding the destructuring opened, not `self`: the closure
        // above holds a borrow of the other fields for as long as it runs.
        *pushed_frames += frames;
        reference.write_frames(rendered[..frames].iter().copied());
        produced_sound
    }

    /// Moves the reference log's read cursor to where the wire actually is.
    ///
    /// Free space is the ring's **occupancy**, not the wire's position — the DMA
    /// frees bytes as it sends them, but the CPU refills them immediately, so a
    /// reading is only meaningful against how much has been pushed. That
    /// subtraction is [`wire_position`]'s, and it is in core so it can be tested
    /// against a ring this board cannot build.
    ///
    /// Cheap on purpose: this runs on the feed, which is the one loop that may not
    /// take a lock, allocate or log. It is arithmetic on a number the DMA already
    /// has, plus a saturating subtraction.
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
                // The wire is back at the start of the ring: `write` replays
                // every frame of it, so the pushed count is a whole ring again
                // and the cursor is at zero. Left where they were, the next
                // reading would be a saturating subtraction of a smaller number
                // and the log would never advance again — a pairing frozen at the
                // moment of the fault rather than reporting one.
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

    /// The repair half of a drain: rebuild the stream and report what the ring
    /// looked like when it was noticed.
    ///
    /// On the cooperative side because the rebuild allocates. Nothing is lost by
    /// the deferral — the ring is already dry, so the stream is silent for a
    /// cadence either way.
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
    // `Send` because `Speaker` demands it: the feed that drives this driver runs
    // from an interrupt executor, and anything it owns has to be safe to reach
    // from interrupt context. Every codec here is — its I2C bus sits behind a
    // `CriticalSectionRawMutex`, whose contents are only ever reached with
    // interrupts masked.
    C: Mute + Send + 'static,
{
    fn play(&mut self, source: Box<dyn SampleSource>) -> Result<(), SpeakerFault> {
        self.voice = Some(source);
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
    /// Called even when nothing is playing: the idle stream carrying the capture's
    /// clocks must be fed silence too. A drain is only *recorded* here, so the
    /// priming push is load-bearing for `restart`. A sound is
    /// done once its source is spent and the last frames it queued have played out.
    ///
    /// The play cursor is advanced *before* the push, from the state the DMA was
    /// left in by the previous cadence — which is the same information either
    /// order reports, and this one is right on the first feed, where the ring has
    /// been full since bring-up and a reading taken afterwards would count frames
    /// the DMA had not yet been given.
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

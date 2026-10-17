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
use iot_core::drivers::aec::{EchoOutcome, EchoStage, lag_in_samples};
use iot_core::drivers::audio::{
    Armed, AudioSample, AudioSource, BYTES_PER_POLL, BYTES_PER_SAMPLE, CAPTURE_BYTES_PER_FRAME,
    CaptureBacklog, CaptureWatchdog, Chain, CornerStep, CornerSweep, DECIMATION, Decimator,
    SAMPLE_RATE_HZ, SampleStream, dbfs, spl,
};

use crate::components::es7210::{HPF_CORNER, HPF_CORNER_CODES, HpfCorner, SPL_OFFSET_DECIBELS};
use crate::virtual_components::reference::{Reference, Side};

/// One-shot corner walk at boot, a field aid for telling a genuinely noisy room from
/// one that only sounds noisy below the filter's corner. A compile-time switch (not a
/// Cargo feature) because it measures rather than renders: a production build sets it to
/// `false` and the walk leaves the binary entirely.
///
/// Walks every corner then returns to [`HPF_CORNER`], so it costs one boot's settling
/// and nothing after.
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

/// The sizing rule the ring and the watchdog are both derived from, asserted
/// because a board that gets it wrong finds out from a capture that looks alive
/// and is not — the one symptom a diagnostic on this path cannot tell from a
/// quiet room. The backlog's levels are checked against the ring where they are
/// defined, in [`CaptureBacklog::new`].
const _: () = assert!(
    STREAM_BYTES >= BYTES_PER_POLL * 2,
    "the capture ring must hold two poll periods, or a late poll overruns it"
);

/// Bytes one drain may fold, so no single poll can hold the shared executor for
/// a whole backlog. A poll's worth arrives every [`CAPTURE_MS`], so two periods
/// of budget catches a backlog up at the rate it builds while capping the hold at
/// half the period it runs in. See [`Es7210Rx::drain`](super::Es7210Rx::drain).
const DRAIN_BYTES_PER_POLL: usize = BYTES_PER_POLL * 2;

/// Kept samples one poll's worst case produces, which is the scratch both the
/// decimator's output and the reference pairing are read into.
///
/// A poll drains at most [`DRAIN_BYTES_PER_POLL`], and the decimation drops two
/// frames in three, so this is the ceiling on both and it is derived rather than
/// quoted — a scratch one sample short would index past its end on the poll that
/// caught up a backlog.
const DECIMATED_PER_POLL: usize =
    DRAIN_BYTES_PER_POLL / CAPTURE_BYTES_PER_FRAME / DECIMATION as usize + 1;

/// The bound that makes the drain's write unconditional.
///
/// **This was a silent drop, and it cost more than the samples it threw away.**
/// The scratch was sized from the *transmit* frame width while the capture
/// delivers half that per frame, so a poll draining its whole budget produced
/// four hundred decimated samples into a buffer of three hundred and twenty-one:
/// the last seventy-nine of every poll were discarded with no counter, no log and
/// no effect on any reported number. `[实测]` the canceller was fed 12 400 of the
/// 16 000 samples a second it runs at, and the gap was not scattered — it was a
/// contiguous four-and-a-half milliseconds missing every twenty-five, which an
/// adaptive filter cannot converge across and a delay search cannot average over.
const _: () = assert!(
    DECIMATED_PER_POLL > DRAIN_BYTES_PER_POLL / CAPTURE_BYTES_PER_FRAME / DECIMATION as usize,
    "a poll's drain budget produces more decimated samples than the scratch holds"
);

/// Decimated samples of microphone latency the reference has to reach back across
/// before a pairing starts: a whole capture ring, the most the loop can ever leave
/// waiting.
///
/// This is the third thing that has to line up and the least obvious of them. The
/// newest frame a drain sees was captured however long ago the ring was laid down —
/// up to 60 ms — so the reference that goes with it is the frame the wire was
/// carrying *then*, not the one it is carrying when the poll runs. Reading from
/// the play cursor as it stands puts every pair out by that much, and the delay
/// search covers 4 ms.
const DECIMATED_RING_LATENCY: usize = STREAM_BYTES / CAPTURE_BYTES_PER_FRAME / DECIMATION as usize;

/// The reference scratch: the lag the pairing reaches back across, plus the run it
/// is pairing. Both, because the reference is read into one buffer and indexed at
/// the lag rather than into two.
const REFERENCE_SCRATCH: usize = DECIMATED_RING_LATENCY + DECIMATED_PER_POLL;

/// The pairing reaches back across this ring, and the shared log is sized from a
/// copy of this ring's depth written in `audio_out`.
///
/// Asserted here rather than trusted: the two are written in different modules,
/// and a stale copy makes the log shorter than the lag the pairing asks for — so
/// the log answers `None` past its end, which reads exactly like "the wire has not
/// started" and loses the pairing without saying so. That is the whole failure
/// mode of this path, so it gets a compile error rather than a runtime shrug.
const _: () = assert!(
    STREAM_BYTES == RECEIVE_RING_BYTES,
    "the shared reference log is sized for a different capture ring depth than \
     this one has"
);

/// The capture ring depth `audio_out` sized the shared log against. A copy, and a
/// copy is free to drift — which is what the assertion after it exists to catch.
const RECEIVE_RING_BYTES: usize = 3 * BYTES_PER_POLL;

/// The tap count this board runs the canceller at.
///
/// **Sixteen, and the number is a budget rather than a preference.** `[实测]` with
/// the canceller on and off, the product's worst single poll is 17–18 ms either
/// way and the capture source 11–13 ms either way, against `MAX_SINGLE_HOLD_MS`
/// = 30 — the two are not separable at this clock, so the tap count here is not
/// what is costing the budget. L=128 is a different matter: the probe put it at
/// 21 614 µs worst against a 17–25 ms input tick, which does not fit. Raising it
/// means either a slower tap count in the filter or a tick that stops folding
/// everything else — a design change, not a constant.
const AEC_TAPS: usize = 16;

const _: () = assert!(
    AEC_TAPS <= iot_core::drivers::aec::MAX_TAPS,
    "a tap count above the module's own maximum is not a choice"
);

/// Where the canceller stands before the demand has said otherwise.
///
/// **A level taken while the speaker is playing is not what a person in the room
/// hears**, unless the speaker's own output is taken back out of it first — so the
/// product's microphone switch owns this, and this is only the position before
/// that switch has been thrown. See [`Es7210Rx::set_collecting`].
///
/// A runtime flag rather than a feature because of what turning it off would
/// actually save: `features.md` rule 2's point is that a feature removes a
/// *module*, and here the ~7 KB only goes if a struct field does, which is the
/// consumer-side `#[cfg]` that rule names as a leak. The time is the part worth
/// having back.
const AEC_ENABLED: bool = true;

/// The capture ring this module sizes its watchdog against, stated where the
/// reference log's sizing can be checked against it.
///
/// The log has to reach back across everything this ring can still be holding
/// undrained, and it is sized from a figure in `reference.rs` that is a *copy* of
/// this one — a copy is free to drift, and a stale copy makes the log too short by
/// exactly the amount it drifted. Asserting here is what turns "someone edited a
/// number" into a compile error rather than a log that quietly laps.
const _: () = assert!(
    STREAM_BYTES == 3 * BYTES_PER_POLL,
    "the reference log is sized for a capture ring three poll periods deep"
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
        // **Two active channels, not one.** Slot 0 is MIC1 and slot 1 is MIC3,
        // which on this board carries the ES8311's output back as the echo
        // reference. `Channels::MONO` selects a single slot and esp-hal says of
        // an inactive channel that it "does not consume or write data in the DMA
        // buffer" in the receiver — so asking for one channel is exactly what
        // made the reference unavailable.
        .with_channels(Channels::MONO)
}

/// Slots in one capture frame, as the codec lays them out in TDM.
///
/// One, and the reason is measured rather than assumed. Two and four were both
/// tried with the codec genuinely in TDM — `init` reads its registers back and
/// they were right — and the part drove nothing at all. See
/// [`USE_MIC3_REFERENCE`] in the codec driver for what is known and what is not.
const CAPTURE_SLOTS: usize = 1;

/// Bytes one capture frame occupies.
///
/// **Derived from [`CAPTURE_SLOTS`], never written out.** This number and the
/// channel configuration have to be the same fact, and when they were two
/// separate literals the drain read four bytes at a time over a stream that
/// delivers two — reporting the microphone's next sample as if it were a
/// reference channel, which read plausibly and was wrong.
const CAPTURE_FRAME_BYTES: usize = CAPTURE_SLOTS * BYTES_PER_SAMPLE as usize;

/// Which slot of a capture frame is which. The board's channel order is not the
/// conventional one: the vendor's map has slot 0 as MIC1 and **slot 1 as MIC3**,
/// not MIC2 — so the reference is slot 1, never slot 2. Slots 2 and 3 are MIC2
/// and MIC4, which nothing on this board drives.
const SLOT_MIC: usize = 0;
const SLOT_REFERENCE: usize = 1;

/// The capture wrapped in something this crate owns.
///
/// `Armed` lives in the core crate so its states can be walked on the host, and that leaves
/// the orphan rule: a trait may only be implemented for a type the same crate defines. The
/// newtype adds no behaviour; it gives the peripheral a local name to hang the two
/// transitions on.
struct Chainable(Capture);

impl Chainable {
    /// Bytes the DMA has finished and nobody has folded.
    fn available_bytes(&mut self) -> usize {
        self.0.available_bytes()
    }

    fn peek(&mut self) -> &[u8] {
        self.0.peek()
    }

    fn consume(&mut self, len: usize) {
        self.0.consume(len);
    }
}

/// A chain that is off the wire but still assembled, so `read` can put the same ring back
/// without building anything new. The ring comes back as its `Final` view, the only one
/// `read` takes.
type Parked = (
    esp_hal::i2s::master::I2sRx<'static, Blocking>,
    DmaRxStreamBuf,
);

impl Armed for Chainable {
    type Parked = Parked;

    fn stop(self) -> Self::Parked {
        self.0.stop()
    }

    fn read(parked: Self::Parked) -> Result<Self, Self::Parked> {
        let (i2s_rx, buffer) = parked;
        i2s_rx
            .read(buffer)
            .map(Chainable)
            .map_err(|(error, i2s_rx, buffer)| {
                log::error!("[AUDIO] capture could not be armed, going silent: {error:?}");
                (i2s_rx, buffer)
            })
    }
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
/// `C` is held rather than dropped after bring-up: the corner walk needs the part
/// reachable for the half minute it lasts. Reached only through [`HpfCorner`], so no
/// I2C or register detail reaches the capture.
/// What each slot of a capture frame carried, and whether the two that matter
/// actually differ.
///
/// The gap is the diagnostic, not the peaks: a TDM part with one microphone live
/// duplicates its sample across left and right, so two slots can read the same
/// peak forever while carrying no second signal at all.
#[derive(Clone, Copy, Default)]
pub struct SlotPeaks {
    pub mic: u16,
    pub reference: u16,
    pub spare_two: u16,
    pub spare_three: u16,
    /// Largest difference between the microphone and the reference slot.
    pub gap: u16,
}

pub struct Es7210Rx<C, const R: usize> {
    /// What the speaker played, read at the play cursor so the canceller pairs
    /// against the frame on the wire rather than the frame the CPU last wrote.
    ///
    /// Held rather than passed per poll because the two halves of this path run on
    /// different executors and the reference crosses that boundary — see
    /// [`crate::virtual_components::reference`]. A `Side` rather than a trait
    /// object so a poll can borrow it without an allocation and without a
    /// self-referential borrow of `self`.
    reference: Side<R>,
    /// The chain and where it is, which is one question rather than two fields: a capture
    /// holding neither a running nor a parked chain has lost it, and that has to be
    /// answerable rather than inferred from silence.
    chain: Chain<Chainable>,
    /// Whether the lost-chain report has already gone out: terminal is terminal, so it is
    /// said once rather than on every poll of the next ten minutes.
    reported_lost: bool,
    stream: SampleStream,
    /// Notices the DMA stopping. The transfer cannot report it itself, so the
    /// capture is timed against the clock — see [`CaptureWatchdog`].
    watchdog: CaptureWatchdog,
    /// Judges how deep the ring is, so a starved input loop warns once instead of
    /// on every poll — and so the level it warns at is one the steady state
    /// cannot reach. See [`CaptureBacklog`].
    backlog: CaptureBacklog,
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
    /// Picks the frames the canceller works on, at the codec's rate. Phase 0
    /// because the transmit side's log is phase 0 and the two transfers share
    /// one clock — see [`crate::virtual_components::audio_out`].
    decimator: Decimator,
    /// The canceller itself, at [`AEC_TAPS`].
    echo: EchoStage<AEC_TAPS>,
    /// Whether to run the canceller at all. Follows the microphone; see
    /// [`AEC_ENABLED`] for why, and [`Es7210Rx::set_collecting`] for what moves it.
    ///
    /// **What this does not buy is the number.** The residual is computed and
    /// discarded — the envelope still folds the raw microphone, because folding
    /// the residual in means resampling 16 kHz back to the 48 kHz the envelope is
    /// specified on, and the A-weighting IIR's coefficients are those 48 kHz ones.
    /// So the panel's level still reads a microphone that hears the speaker.
    cancel_echo: bool,
    /// The capture's own decimated frames from this poll, read straight out of the
    /// DMA ring as it is drained. A poll's worth of microphone is what pairs with
    /// a poll's worth of reference, so this is scratch rather than state and
    /// nothing crosses a poll boundary in it.
    scratch_mic: [i16; DECIMATED_PER_POLL],
    /// The reference for those same frames, read from the transmit driver's log at
    /// the same point in the drain.
    scratch_reference: [i16; REFERENCE_SCRATCH],
    /// Frames folded since the last pairing, so the reference can be asked for
    /// exactly the stretch the microphone covers rather than an estimate of it.
    frames_this_pass: usize,
    /// Where in `scratch_reference` the paired run begins, and how long it is.
    ///
    /// A pair of fields rather than a slice, because the reference is read at an
    /// offset and cannot be indexed from zero: the lag the pairing reaches back
    /// across is the first thing it puts in that buffer.
    pair_lag: usize,
    paired_run: usize,
    /// What the last pairing did, so a probe can read the canceller's own numbers
    /// rather than infer them from an envelope. Default until the first pair.
    echo_report: EchoOutcome,
    /// Pairs that never happened because the reference could not be reached. A
    /// fault to report rather than a count to expect — see
    /// [`EchoStage::cancel_run`].
    echo_unpaired: u32,
    /// Bytes the DMA had finished when the last poll began, and how many the
    /// drain took.
    ///
    /// Reported as a pair because **either half alone cannot distinguish a
    /// half-rate link from a drain that stops early** — both show the same
    /// shortfall in what reaches the canceller, and the ratio is what tells
    /// them apart.
    arrived: usize,
    drained: usize,
    /// Loudest raw sample seen on each capture slot this poll: the microphone
    /// and the echo reference.
    ///
    /// Reported because **the reference channel is the one thing that has to be
    /// confirmed before anything can be built on it** — a board that wires
    /// nothing to MIC3 would make every later figure meaningless.
    slot_peaks: SlotPeaks,
}

pub struct EchoReport {
    outcome: EchoOutcome,
    /// Pairs that never happened over the capture's whole life, not just this
    /// poll: a fault that appears once is as much a fault as one that appears
    /// every poll, and a cumulative count cannot be averaged away.
    pub total_unpaired: u32,
    /// Kept samples the reference log holds unread.
    pub log_depth: usize,
    /// What the log can hold, from the ring geometry.
    pub log_capacity: usize,
    /// The loudest coefficient and which tap it sits on — what a converged
    /// canceller says the acoustic path runs through, and the one figure that can
    /// contradict the delay it chose.
    pub loudest_tap: (usize, i32),
    /// The loudest capture and reference sample of the last poll, as absolute
    /// LSB magnitudes. Read together they say which side of the path went quiet,
    /// which the ERLE cannot.
    pub levels: (u16, u16),
    /// Bytes the DMA had finished before the last poll's drain, and bytes the
    /// drain took. A shortfall in the first is the link; a shortfall only in the
    /// second is the drain.
    pub drain: (usize, usize),
    /// Loudest raw microphone and reference sample this poll.
    pub slot_peaks: SlotPeaks,
}

impl EchoReport {
    /// Pairs folded through the canceller on the last poll.
    pub const fn cancelled(&self) -> usize {
        self.outcome.cancelled
    }

    /// Pairs the last poll could not make, because the reference could not be
    /// reached that far back.
    pub const fn unpaired(&self) -> usize {
        self.outcome.unpaired
    }

    /// The enhancement over the stream so far, in decibels — **zero when nothing
    /// has been paired**, because a meter that has measured nothing must not read
    /// as perfect.
    pub const fn erle_decibels(&self) -> i32 {
        self.outcome.erle_decibels
    }

    pub const fn delay(&self) -> Option<usize> {
        self.outcome.delay
    }

    /// Whether the filter is adapting, so a reading can be read against the right
    /// state: frozen is barge-in, adapting is far-end only.
    pub const fn adapting(&self) -> bool {
        self.outcome.adapting
    }

    /// Whether the reference log is reading inside its own runway. `false` here
    /// with a large ERLE is the combination that means "the pairing is broken and
    /// the filter has not noticed yet".
    pub const fn log_inside_runway(&self) -> bool {
        self.log_depth <= self.log_capacity
    }
}

/// A capture a probe can poll directly, rather than through the product's
/// [`PollEntry`].
///
/// Only the canceller's own numbers need this: the product reads an envelope off
/// an intent, and a probe needs the driver itself so it can ask what the pairing
/// did. Taken by the board's `take_capture`, beside `take_motion_driver` and for
/// the same reason — the driver is the only thing that can reach the part.
pub trait EchoCapture {
    /// Whether the canceller is currently running. A probe has to see this: every
    /// figure it reports is zero when it is off, and a run of zero ERLE lines
    /// means nothing at all without knowing which of the two happened.
    fn cancel_echo(&self) -> bool;

    /// Turns the canceller on or off. See [`Es7210Rx::set_cancel_echo`].
    fn set_cancel_echo(&mut self, enabled: bool);

    fn poll(&mut self, now_ms: u64) -> AudioSample;

    /// What the canceller made of it, for a log or a probe to print.
    fn echo_report(&self) -> EchoReport;

    /// Where the echo path sits over the whole capture history. See
    /// [`Es7210Rx::scan_wide`] — on the trait because a probe reaches the
    /// capture through `dyn` and cannot reach an inherent method.
    fn scan_wide(&mut self) -> (usize, i64);

    /// The share of this room's echo path a filter of `taps` cannot reach, in
    /// hundredths. `-1` until a [`Self::scan_wide`] has run. See
    /// [`Es7210Rx::unreachable_by`].
    fn unreachable_by(&self, taps: usize) -> i32;
}

impl<C, const R: usize> EchoCapture for Es7210Rx<C, R>
where
    C: HpfCorner,
    C::CornerError: core::fmt::Debug,
{
    fn cancel_echo(&self) -> bool {
        Es7210Rx::cancel_echo(self)
    }

    fn set_cancel_echo(&mut self, enabled: bool) {
        Es7210Rx::set_cancel_echo(self, enabled);
    }

    fn scan_wide(&mut self) -> (usize, i64) {
        Es7210Rx::scan_wide(self)
    }

    fn unreachable_by(&self, taps: usize) -> i32 {
        Es7210Rx::unreachable_by(self, taps)
    }

    fn poll(&mut self, now_ms: u64) -> AudioSample {
        Es7210Rx::poll(self, now_ms)
    }

    fn echo_report(&self) -> EchoReport {
        Es7210Rx::echo_report(self)
    }
}

impl<C, const R: usize> Es7210Rx<C, R> {
    /// Takes a transfer armed on [`stream`]'s ring — the one the watchdog is
    /// sized against, so a ring built any other length would be judged by a
    /// deadline that does not describe it — the codec that captured it, and what
    /// the speaker played for it to pair against.
    pub fn new(transfer: Capture, codec: C, reference: Side<R>) -> Self {
        if HPF_CORNER_WALK {
            log::info!(
                "[AUDIO] HPF corner walk starting: {HPF_CORNER_CODES} codes, \
                 {} ms each, back on corner {HPF_CORNER} after",
                iot_core::drivers::audio::CORNER_SETTLE_MS
            );
        }
        Self {
            reference,
            chain: Chain::new(Chainable(transfer)),
            reported_lost: false,
            stream: SampleStream::new(),
            watchdog: CaptureWatchdog::new(STREAM_BYTES),
            backlog: CaptureBacklog::new(STREAM_BYTES),
            restarts: 0,
            walk: HPF_CORNER_WALK.then_some(CornerSweep::new(HPF_CORNER_CODES)),
            last_cal_log_ms: 0,
            codec,
            decimator: Decimator::new(),
            echo: EchoStage::new(),
            cancel_echo: AEC_ENABLED,
            scratch_mic: [0; DECIMATED_PER_POLL],
            scratch_reference: [0; REFERENCE_SCRATCH],
            frames_this_pass: 0,
            pair_lag: 0,
            paired_run: 0,
            echo_report: EchoOutcome::default(),
            echo_unpaired: 0,
            arrived: 0,
            drained: 0,
            slot_peaks: SlotPeaks::default(),
        }
    }

    /// Pairs the frames this pass drained against the speaker's play cursor.
    ///
    /// `lag_frames` is how deep the capture ring was when the drain started — **the
    /// single number that matters here.** The newest frame a drain sees was captured
    /// however long ago the ring was laid down, so the reference that goes with it is
    /// the frame the wire carried *then*; reading the play cursor as it stands at poll
    /// time pairs every sample against one up to 60 ms newer, fifteen times the delay
    /// search's window, and the search then answers "nothing found" and the filter
    /// converges on nothing, silently.
    ///
    /// Hence the play cursor rather than the write pointer: the transmit ring holds
    /// [`TRANSMIT_RUNWAY_FRAMES`](crate::virtual_components::audio_out) the DMA has not
    /// sent yet. It is also why a board with a microphone and no speaker cannot cancel
    /// at all — the transmit stream is what clocks the capture, and `Side::Silent` only
    /// guarantees the unpaired samples are zero, not that the two sides agree on time.
    /// Such a board gets the pairing and an empty run, not a quiet measurement of
    /// nothing.
    ///
    /// The two runs arrive at **opposite ends**: `scratch_mic[0]` is the oldest frame
    /// the drain saw, while the log is read backwards so `out[0]` is the frame on the
    /// wire now. Zipped as they stand every pair is wrong by the whole run, so the
    /// reference is reversed and both are fed oldest-to-newest — which is also the
    /// order the filter's own state has to be stepped in.
    ///
    /// **Fewer pairs than were asked for means the log has lapped or the wire has not
    /// started.** Pairing the prefix and reporting the rest is the whole answer:
    /// pairing the tail against frames the log does hold would cancel against a
    /// different stretch, which is the fault this type exists to keep visible.
    ///
    /// **Runs whether or not the canceller is on** — the pairing is one ring read and a
    /// reversal, the cancellation a filter per sample. A bench measurement of the room
    /// needs the pairing and has no use for the cancellation.
    fn pair(&mut self, lag_frames: usize, reference: &dyn Reference) {
        self.pair_lag = 0;
        self.paired_run = 0;
        let pairs = self.frames_this_pass.min(DECIMATED_PER_POLL);
        if pairs == 0 {
            return;
        }
        // The lag is a whole number of frames and the run is decimated, so the
        // lag rounds *up*: reaching one kept sample too far back leaves the pair
        // a fraction of a sample out, while reaching one short puts it a whole
        // sample out and in the wrong direction.
        let lag = lag_in_samples(lag_frames, DECIMATION as usize);
        let Some(window) = self.scratch_reference.get_mut(lag..lag + pairs) else {
            return;
        };
        let paired = reference.read_backwards(window);
        window[..paired].reverse();
        self.pair_lag = lag;
        self.paired_run = paired;
    }

    /// Hands the paired run to the canceller. See [`Self::pair`], which leaves it
    /// where this reads it.
    fn cancel_pairing(&mut self) {
        let paired = self.paired_run;
        if paired == 0 {
            return;
        }
        let window = &self.scratch_reference[self.pair_lag..self.pair_lag + paired];
        self.echo_report = self.echo.cancel_run(&self.scratch_mic[..paired], window);
        self.echo_unpaired += (self.frames_this_pass.min(DECIMATED_PER_POLL) - paired) as u32;
    }

    /// Turns the canceller on or off, for a bench build that wants its numbers.
    ///
    /// Not part of the capture's business: the product leaves it where
    /// [`AEC_ENABLED`] put it, and a probe asks for it explicitly. On, this costs
    /// [`AEC_TAPS`]-tap cancellation per decimated sample — see
    /// [`Es7210Rx::cancel_echo`] for why that is off by default. **The pairing it
    /// is fed does not stop with it**: see [`Self::pair`].
    ///
    /// Turning it off resets the decimator's phase, so a run that was stopped
    /// halfway does not leave the next one pairing from a different frame of every
    /// three than the one it thinks it is on.
    pub fn set_cancel_echo(&mut self, enabled: bool) {
        self.cancel_echo = enabled;
        if !enabled {
            self.decimator = Decimator::new();
            self.frames_this_pass = 0;
            self.pair_lag = 0;
            self.paired_run = 0;
            self.echo_report = EchoOutcome::default();
        }
    }

    /// Where the echo path sits over the whole capture history, and how strongly
    /// it matches there — the answer to "is there no path, or is it further back
    /// than the canceller's own search reaches?".
    ///
    /// Deliberately not folded into [`Self::echo_report`]: it costs four times a
    /// search, and a report that carried it would pay that per poll to answer a
    /// question whose answer does not change per poll.
    pub fn scan_wide(&mut self) -> (usize, i64) {
        self.echo.scan_wide()
    }

    /// The share of this room's echo path a filter of `taps` cannot reach, in
    /// hundredths. See [`EchoStage::unreachable_by`].
    pub fn unreachable_by(&self, taps: usize) -> i32 {
        self.echo.unreachable_by(taps)
    }

    /// Bytes the DMA had finished when the last poll started, and how many of
    /// them the drain actually took.
    pub const fn last_drain(&self) -> (usize, usize) {
        (self.arrived, self.drained)
    }

    /// Whether the canceller is running.
    pub const fn cancel_echo(&self) -> bool {
        self.cancel_echo
    }

    /// What the canceller has done, and how far it can be trusted.
    ///
    /// Bundled because the figures have to be read *together*: a large ERLE means
    /// nothing on its own if the pair count is zero, and a sensible pair count
    /// means nothing once the log has lapped. A probe that could read them one at
    /// a time would be able to report a healthy-looking number for a broken
    /// pairing, which is the one fault this path exists to keep visible.
    pub fn echo_report(&self) -> EchoReport {
        let (depth, capacity) = self.reference.depth();
        EchoReport {
            outcome: self.echo_report,
            total_unpaired: self.echo_unpaired,
            log_depth: depth,
            log_capacity: capacity,
            loudest_tap: self.echo.loudest_tap(),
            levels: (self.echo_report.mic_peak, self.echo_report.reference_peak),
            drain: self.last_drain(),
            slot_peaks: self.slot_peaks,
        }
    }

    /// Advances the corner walk by one poll and reports what the envelope says
    /// once a code has settled.
    ///
    /// Takes the walk out of `self` for the step, because a step both reads the walk and
    /// reaches the codec — fields of the same struct. A failed write abandons the walk: a
    /// codec that will not take a corner write will not take the next either, and a walk
    /// that limps on would report levels from an unnameable state. Either way the corner
    /// returns to [`HPF_CORNER`], so an early exit cannot leave the stage more open.
    fn step_walk(&mut self, now_ms: u64)
    where
        C: HpfCorner,
        C::CornerError: core::fmt::Debug,
    {
        let Some(mut walk) = self.walk.take() else {
            return;
        };
        let write = |codec: &mut C, corner: u8| codec.set_hpf_corner(corner).err();
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

    /// Folds what the DMA has finished into the envelope, in a bounded loop rather
    /// than one descriptor per poll: a late poll costs a column, not a widening gap.
    /// A partly filled descriptor folds as it stands, the trailing-byte carry in
    /// [`SampleStream`] making a mid-sample boundary free.
    ///
    /// The bound is what keeps the capture from starving the machine: folding a sample
    /// runs a sixth-order IIR, so draining a full [`STREAM_BYTES`] ring at once holds the
    /// shared executor for tens of milliseconds — long enough for the playback ring to
    /// empty underneath it. Two poll periods catches up while capping the hold.
    ///
    /// The same pass also decimates for the canceller, because the frames are only
    /// addressable here — they exist as DMA bytes and nowhere else — and pairing
    /// them against the reference later would mean copying the whole ring first.
    /// Every sample is read once for the envelope and, one frame in three, once
    /// more for the canceller.
    fn drain(&mut self, lag_frames: usize, reference: &dyn Reference) -> usize {
        // Destructured so the chain's borrow and the canceller's own fields are
        // disjoint: `running_mut` borrows the whole struct otherwise, and holding
        // that across the fold would forbid reading the samples at all. Folded in
        // place rather than copied out, because a descriptor is 480 bytes and
        // `architecture.md` budgets that stack.
        let cancelling = self.cancel_echo;
        let Self {
            chain,
            stream,
            decimator,
            scratch_mic,
            frames_this_pass,
            slot_peaks,
            ..
        } = self;
        let SlotPeaks {
            mic: mic_peak,
            reference: reference_peak,
            spare_two: slot2_peak,
            spare_three: slot3_peak,
            gap: slot_gap,
        } = slot_peaks;
        *mic_peak = 0;
        *reference_peak = 0;
        *slot2_peak = 0;
        *slot3_peak = 0;
        *slot_gap = 0;
        // Reset here rather than after the pairing, because what is in the
        // scratch when this returns is **this poll's decimated microphone** and
        // something else is allowed to want it: `EchoReport` is read by a probe
        // that turned the canceller on, and a caller asking only for the pairing
        // gets this run whether or not anything cancels it.
        *frames_this_pass = 0;
        let mut taken = 0usize;
        {
            let Some(transfer) = chain.running_mut() else {
                return 0;
            };
            let mut budget = DRAIN_BYTES_PER_POLL;
            while budget > 0 && transfer.available_bytes() > 0 {
                let descriptor = transfer.peek();
                let len = descriptor.len().min(budget);
                taken += len;
                stream.push_bytes(&descriptor[..len]);
                // One sample per **frame**, not per slot: both slots carry the same
                // stream — see [`tdm_config`] — so a sample per slot would halve the
                // canceller's rate. The transmit side pushes one per frame for the
                // same reason, and the two have to agree.
                //
                // Every slot is measured, and slot 1 is fed nothing: the point
                // of this pass is to learn whether the reference channel carries
                // a signal at all, before anything is built on it.
                for frame in descriptor[..len].chunks_exact(CAPTURE_FRAME_BYTES) {
                    // **A copy and a second channel look identical in a peak.**
                    // Both slots peaked within a count of each other in every
                    // frame width tried, which is equally what duplicated
                    // left/right data looks like, so the peak of the difference
                    // is measured too: zero means slot one is slot zero again,
                    // and no peak anywhere in the frame means the capture is not
                    // reading four slots at all.
                    for (slot, peak) in [
                        (SLOT_MIC, &mut *mic_peak),
                        (SLOT_REFERENCE, &mut *reference_peak),
                        (2, &mut *slot2_peak),
                        (3, &mut *slot3_peak),
                    ] {
                        if frame.len() > slot * BYTES_PER_SAMPLE as usize {
                            let raw = i16::from_le_bytes([frame[slot * 2], frame[slot * 2 + 1]]);
                            *peak = (*peak).max(raw.unsigned_abs());
                        }
                    }
                    if frame.len() > SLOT_REFERENCE * BYTES_PER_SAMPLE as usize {
                        let a = i16::from_le_bytes([frame[SLOT_MIC * 2], frame[SLOT_MIC * 2 + 1]]);
                        let b = i16::from_le_bytes([
                            frame[SLOT_REFERENCE * 2],
                            frame[SLOT_REFERENCE * 2 + 1],
                        ]);
                        *slot_gap = (*slot_gap).max((a as i32 - b as i32).unsigned_abs() as u16);
                    }
                }
                // Filled whether or not the canceller is running: a caller that
                // wants the microphone paired against the reference is not a
                // different path, it is this one with the cancellation switched
                // off.
                for frame in descriptor[..len].chunks_exact(CAPTURE_FRAME_BYTES) {
                    let sample = i16::from_le_bytes([frame[0], frame[1]]);
                    if let Some(kept) = decimator.push(sample) {
                        scratch_mic[*frames_this_pass] = kept;
                        *frames_this_pass += 1;
                    }
                }
                transfer.consume(len);
                budget -= len;
            }
        }
        self.pair(lag_frames, reference);
        if cancelling {
            self.cancel_pairing();
        }
        taken
    }

    /// Re-arms a capture whose DMA has stopped: the only way back is to take the
    /// transfer down and set a new one up, so the ring comes out of `stop` and
    /// goes straight into `read`, allocating nothing. It costs one descriptor of
    /// audio, which on a scope is a column with a notch in it.
    fn restart(&mut self, now_ms: u64) {
        if !self.chain.is_running() {
            // A parked plane is not a stalled one, and a lost chain has already said so.
            return;
        }
        self.restarts = self.restarts.saturating_add(1);
        if self.arm(now_ms) {
            log::info!(
                "[AUDIO] capture DMA had stopped, re-armed ({})",
                self.restarts
            );
        }
    }

    /// Puts a chain on the wire, from the one parked here or from a chain the caller has
    /// just taken down. The buffer that comes back is the one the ring was built on, so no
    /// allocation happens here.
    fn arm(&mut self, now_ms: u64) -> bool {
        // The one place a chain is stopped, whoever asked for it: a repair that took the
        // chain here itself left this to find nothing and dropped the ring with it.
        let Some(chain) = self.chain.take_to_arm() else {
            return false;
        };
        // The new stream starts on an empty ring, so it has to earn its own
        // deadline rather than be judged against the one the dead one left.
        self.watchdog.note_progress(now_ms);
        match Chainable::read(chain) {
            Ok(transfer) => {
                self.chain.put(transfer);
                true
            }
            // The error already said so, and the chain went with it.
            Err(_) => false,
        }
    }
}

impl<C, const R: usize> Es7210Rx<C, R>
where
    C: HpfCorner,
    C::CornerError: core::fmt::Debug,
{
    /// One poll: drain, pair against the reference, and report.
    ///
    /// Split out of [`AudioSource::sample`] for the two things the trait method
    /// cannot do: reach the codec for the corner walk, which needs the
    /// `HpfCorner` bound the trait does not carry, and be called by a probe that
    /// wants the canceller's own numbers on real hardware.
    pub fn poll(&mut self, now_ms: u64) -> AudioSample {
        // Copied out, not borrowed: a copy of a `&'static` handle is a pointer, and the
        // driver is borrowed mutably a few lines below.
        let reference = self.reference;
        let reference: &dyn Reference = &reference;
        let produced = match self.chain.running_mut() {
            Some(transfer) => transfer.available_bytes(),
            None => {
                // Off the wire is a legitimate answer — the demand parked this plane — but
                // so is having lost the chain, and the two look identical from here.
                if self.chain.is_lost() && !self.reported_lost {
                    self.reported_lost = true;
                    log::error!(
                        "[AUDIO] the capture chain is gone: a re-arm failed and dropped the ring. \
                         The microphone clock is not running and nothing will bring it back."
                    );
                }
                return self.summary(now_ms);
            }
        };
        if let Some(produced) = self.backlog.report(produced) {
            log::warn!(
                "[AUDIO] capture ring {produced} B deep, the input loop is behind the codec"
            );
        }
        // Measured before the drain, so it is the depth the newest frame came
        // from rather than the empty ring the drain leaves behind.
        let taken = self.drain(produced / CAPTURE_BYTES_PER_FRAME, reference);
        // The two together say whether the DMA is filling the ring or the drain
        // is walking away from it: half the pair each is what a half-rate link
        // looks like from either side, and only the ratio tells them apart.
        self.arrived = produced;
        self.drained = taken;
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

impl<C, const R: usize> AudioSource for Es7210Rx<C, R>
where
    C: HpfCorner,
    C::CornerError: core::fmt::Debug,
{
    fn accepts_slower_poll(&self) -> bool {
        // The ring is only emptied by `sample`, so a slower poll does not make the reading
        // staler — it lets the I2S engine halt on a full ring.
        false
    }

    fn set_collecting(&mut self, now_ms: u64, collecting: bool) {
        // The canceller follows the microphone, set here so no product call site can
        // drift from it. Before the guard below, which reads the chain's state
        // rather than the demand's — a chain lost to a failed re-arm would
        // otherwise leave the canceller on with nothing to pair — and only on a
        // change, because `set_cancel_echo` resets the pairing state and this runs
        // every tick.
        if collecting != self.cancel_echo {
            self.set_cancel_echo(collecting);
        }
        // The guard is the state the chain is in, not whether a field is empty: a chain
        // lost to a failed re-arm is not parked, so a turn-on still tries to arm it.
        if collecting == self.chain.is_running() {
            return;
        }
        if collecting {
            if !self.arm(now_ms) {
                log::error!("[AUDIO] the demand asked for the plane back and it did not come");
            }
        } else if self.chain.park() {
            // The chain comes down **here**, while someone asked for it. Left running, the
            // ring fills, the engine halts on its own, and the only thing that would notice
            // is the watchdog inside a `sample` this plane no longer receives.
            log::info!("[AUDIO] capture stopped: the demand turned the plane off");
        }
    }

    fn sample(&mut self, now_ms: u64) -> AudioSample {
        self.poll(now_ms)
    }
}

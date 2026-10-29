//! Continuous microphone capture for the Audio page.

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

const HPF_CORNER_WALK: bool = true;

const CAL_SPL_LOG: bool = false;
const CAL_LOG_MS: u64 = 5_000;

// Bytes per DMA descriptor. A quarter of the poll period's worth of audio, so
// the descriptors hand data over at a granularity a 20 ms poll does not notice
// while the ring still bounds how far behind the panel can fall.
const CHUNK_BYTES: usize = 480;

const STREAM_BYTES: usize = BYTES_PER_POLL * 3;

const _: () = assert!(
    STREAM_BYTES >= BYTES_PER_POLL * 2,
    "the capture ring must hold two poll periods, or a late poll overruns it"
);

const DRAIN_BYTES_PER_POLL: usize = BYTES_PER_POLL * 2;

const DECIMATED_PER_POLL: usize =
    DRAIN_BYTES_PER_POLL / CAPTURE_BYTES_PER_FRAME / DECIMATION as usize + 1;

// A short scratch drops a poll's tail with no counter and no log; the assertion is
// the only thing that says so.
const _: () = assert!(
    DECIMATED_PER_POLL > DRAIN_BYTES_PER_POLL / CAPTURE_BYTES_PER_FRAME / DECIMATION as usize,
    "a poll's drain budget produces more decimated samples than the scratch holds"
);

// Decimated microphone latency the reference must reach back across: one capture ring.
const DECIMATED_RING_LATENCY: usize = STREAM_BYTES / CAPTURE_BYTES_PER_FRAME / DECIMATION as usize;

const REFERENCE_SCRATCH: usize = DECIMATED_RING_LATENCY + DECIMATED_PER_POLL;

const _: () = assert!(
    STREAM_BYTES == RECEIVE_RING_BYTES,
    "the shared reference log is sized for a different capture ring depth than \
     this one has"
);

const RECEIVE_RING_BYTES: usize = 3 * BYTES_PER_POLL;

// The tap count is a budget, not a preference. `[实测]` 16 taps leaves the worst
// poll at 17–18 ms either way; 128 taps costs 21 614 µs and does not fit the input tick.
const AEC_TAPS: usize = 16;

const _: () = assert!(
    AEC_TAPS <= iot_core::drivers::aec::MAX_TAPS,
    "a tap count above the module's own maximum is not a choice"
);

const AEC_ENABLED: bool = true;

const _: () = assert!(
    STREAM_BYTES == 3 * BYTES_PER_POLL,
    "the reference log is sized for a capture ring three poll periods deep"
);

/// The transfer a capture runs on.
pub type Capture = I2sRxDmaTransfer<'static, Blocking, DmaRxStreamBuf>;

/// The codec-facing half of the wire format: Philips framing, 16 data bits, and a frame
/// left two slots wide, so both slots reach the DMA buffer.
pub fn tdm_config() -> TdmConfig {
    TdmConfig::new_tdm_philips()
        .with_sample_rate(Rate::from_hz(SAMPLE_RATE_HZ))
        .with_data_format(DataFormat::Data16Channel16)
        .with_channels(Channels::MONO)
}

const CAPTURE_SLOTS: usize = 1;

const CAPTURE_FRAME_BYTES: usize = CAPTURE_SLOTS * BYTES_PER_SAMPLE as usize;

const SLOT_MIC: usize = 0;
const SLOT_REFERENCE: usize = 1;

struct Chainable(Capture);

impl Chainable {
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
    reference: Side<R>,
    chain: Chain<Chainable>,
    reported_lost: bool,
    stream: SampleStream,
    watchdog: CaptureWatchdog,
    backlog: CaptureBacklog,
    restarts: u16,
    walk: Option<CornerSweep>,
    last_cal_log_ms: u64,
    codec: C,
    decimator: Decimator,
    echo: EchoStage<AEC_TAPS>,
    // Whether to run the canceller at all. Follows the microphone; see
    // [`AEC_ENABLED`] for why, and [`Es7210Rx::set_collecting`] for what moves it.
    //
    // **What this does not buy is the number.** The residual is computed and
    // discarded — the envelope still folds the raw microphone, because folding
    // the residual in means resampling 16 kHz back to the 48 kHz the envelope is
    // specified on, and the A-weighting IIR's coefficients are those 48 kHz ones.
    // So the panel's level still reads a microphone that hears the speaker.
    cancel_echo: bool,
    scratch_mic: [i16; DECIMATED_PER_POLL],
    scratch_reference: [i16; REFERENCE_SCRATCH],
    frames_this_pass: usize,
    pair_lag: usize,
    paired_run: usize,
    echo_report: EchoOutcome,
    echo_unpaired: u32,
    arrived: usize,
    drained: usize,
    slot_peaks: SlotPeaks,
}

pub struct EchoReport {
    outcome: EchoOutcome,
    /// Pairs that never happened over the capture's whole life, not just this poll: a fault
    /// that appears once is as much a fault as one that appears every poll, and a
    /// cumulative count cannot be averaged away.
    pub total_unpaired: u32,
    /// Kept samples the reference log holds unread.
    pub log_depth: usize,
    /// What the log can hold, from the ring geometry.
    pub log_capacity: usize,
    /// The loudest coefficient and which tap it sits on — what a converged canceller says
    /// the acoustic path runs through, and the one figure that can contradict the delay it
    /// chose.
    pub loudest_tap: (usize, i32),
    /// The loudest capture and reference sample of the last poll, as absolute LSB
    /// magnitudes.
    pub levels: (u16, u16),
    /// Bytes the DMA had finished before the last poll's drain, and bytes the drain took.
    pub drain: (usize, usize),
    /// Loudest raw microphone and reference sample this poll.
    pub slot_peaks: SlotPeaks,
}

impl EchoReport {
    /// Pairs folded through the canceller on the last poll.
    pub const fn cancelled(&self) -> usize {
        self.outcome.cancelled
    }

    /// Pairs the last poll could not make, because the reference could not be reached that
    /// far back.
    pub const fn unpaired(&self) -> usize {
        self.outcome.unpaired
    }

    /// The enhancement over the stream so far, in decibels — **zero when nothing has been
    /// paired**, because a meter that has measured nothing must not read as perfect.
    pub const fn erle_decibels(&self) -> i32 {
        self.outcome.erle_decibels
    }

    pub const fn delay(&self) -> Option<usize> {
        self.outcome.delay
    }

    /// Whether the filter is adapting, so a reading can be read against the right state:
    /// frozen is barge-in, adapting is far-end only.
    pub const fn adapting(&self) -> bool {
        self.outcome.adapting
    }

    /// Whether the reference log is reading inside its own runway.
    pub const fn log_inside_runway(&self) -> bool {
        self.log_depth <= self.log_capacity
    }
}

/// A capture a probe can poll directly, rather than through the product's
/// [`PollEntry`](iot_core::drivers::input::PollEntry).
pub trait EchoCapture {
    fn cancel_echo(&self) -> bool;

    fn set_cancel_echo(&mut self, enabled: bool);

    fn poll(&mut self, now_ms: u64) -> AudioSample;

    fn echo_report(&self) -> EchoReport;

    fn scan_wide(&mut self) -> (usize, i64);

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
    /// Takes a transfer armed on [`stream`]'s ring — the one the watchdog is sized against,
    /// so a ring built any other length would be judged by a deadline that does not
    /// describe it — the codec that captured it, and what the speaker played for it to pair
    /// against.
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

    // Pairs the frames this pass drained against the speaker's play cursor.
    //
    // `lag_frames` is how deep the capture ring was when the drain started — **the
    // single number that matters here.** The newest frame a drain sees was captured
    // however long ago the ring was laid down, so the reference that goes with it is
    // the frame the wire carried *then*; reading the play cursor as it stands at poll
    // time pairs every sample against one up to 60 ms newer, fifteen times the delay
    // search's window, and the search then answers "nothing found" and the filter
    // converges on nothing, silently.
    //
    // Hence the play cursor rather than the write pointer: the transmit ring holds
    // [`TRANSMIT_RUNWAY_FRAMES`](crate::virtual_components::audio_out) the DMA has not
    // sent yet. It is also why a board with a microphone and no speaker cannot cancel
    // at all — the transmit stream is what clocks the capture, and `Side::Silent` only
    // guarantees the unpaired samples are zero, not that the two sides agree on time.
    // Such a board gets the pairing and an empty run, not a quiet measurement of
    // nothing.
    //
    // The two runs arrive at **opposite ends**: `scratch_mic[0]` is the oldest frame
    // the drain saw, while the log is read backwards so `out[0]` is the frame on the
    // wire now. Zipped as they stand every pair is wrong by the whole run, so the
    // reference is reversed and both are fed oldest-to-newest — which is also the
    // order the filter's own state has to be stepped in.
    //
    // **Fewer pairs than were asked for means the log has lapped or the wire has not
    // started.** Pairing the prefix and reporting the rest is the whole answer:
    // pairing the tail against frames the log does hold would cancel against a
    // different stretch, which is the fault this type exists to keep visible.
    //
    // **Runs whether or not the canceller is on** — the pairing is one ring read and a
    // reversal, the cancellation a filter per sample. A bench measurement of the room
    // needs the pairing and has no use for the cancellation.
    fn pair(&mut self, lag_frames: usize, reference: &dyn Reference) {
        self.pair_lag = 0;
        self.paired_run = 0;
        let pairs = self.frames_this_pass.min(DECIMATED_PER_POLL);
        if pairs == 0 {
            return;
        }
        let lag = lag_in_samples(lag_frames, DECIMATION as usize);
        let Some(window) = self.scratch_reference.get_mut(lag..lag + pairs) else {
            return;
        };
        let paired = reference.read_backwards(window);
        window[..paired].reverse();
        self.pair_lag = lag;
        self.paired_run = paired;
    }

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

    /// Where the echo path sits over the whole capture history, and how strongly it matches
    /// there — the answer to "is there no path, or is it further back than the canceller's
    /// own search reaches?".
    pub fn scan_wide(&mut self) -> (usize, i64) {
        self.echo.scan_wide()
    }

    /// The share of this room's echo path a filter of `taps` cannot reach, in hundredths.
    pub fn unreachable_by(&self, taps: usize) -> i32 {
        self.echo.unreachable_by(taps)
    }

    /// Bytes the DMA had finished when the last poll started, and how many of them the
    /// drain actually took.
    pub const fn last_drain(&self) -> (usize, usize) {
        (self.arrived, self.drained)
    }

    /// Whether the canceller is running.
    pub const fn cancel_echo(&self) -> bool {
        self.cancel_echo
    }

    /// What the canceller has done, and how far it can be trusted.
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
                for frame in descriptor[..len].as_chunks::<CAPTURE_FRAME_BYTES>().0 {
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
                    if let Some(reference) = frame.get(SLOT_REFERENCE * 2..SLOT_REFERENCE * 2 + 2) {
                        let a = i16::from_le_bytes([frame[SLOT_MIC * 2], frame[SLOT_MIC * 2 + 1]]);
                        let b = i16::from_le_bytes([reference[0], reference[1]]);
                        *slot_gap = (*slot_gap).max((a as i32 - b as i32).unsigned_abs() as u16);
                    }
                }
                for frame in descriptor[..len].as_chunks::<CAPTURE_FRAME_BYTES>().0 {
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

    fn restart(&mut self, now_ms: u64) {
        if !self.chain.is_running() {
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

    fn arm(&mut self, now_ms: u64) -> bool {
        let Some(chain) = self.chain.take_to_arm() else {
            return false;
        };
        self.watchdog.note_progress(now_ms);
        match Chainable::read(chain) {
            Ok(transfer) => {
                self.chain.put(transfer);
                true
            }
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
    pub fn poll(&mut self, now_ms: u64) -> AudioSample {
        let reference = self.reference;
        let reference: &dyn Reference = &reference;
        let produced = match self.chain.running_mut() {
            Some(transfer) => transfer.available_bytes(),
            None => {
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
        let taken = self.drain(produced / CAPTURE_BYTES_PER_FRAME, reference);
        self.arrived = produced;
        self.drained = taken;
        if self.watchdog.stalled(now_ms, produced) {
            self.restart(now_ms);
        }
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

    fn summary(&self, now_ms: u64) -> AudioSample {
        AudioSample {
            envelope: self.stream.envelope(),
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
        false
    }

    fn set_collecting(&mut self, now_ms: u64, collecting: bool) {
        if collecting != self.cancel_echo {
            self.set_cancel_echo(collecting);
        }
        if collecting == self.chain.is_running() {
            return;
        }
        if collecting {
            if !self.arm(now_ms) {
                log::error!("[AUDIO] the demand asked for capture back and it did not come");
            }
        } else if self.chain.park() {
            log::info!("[AUDIO] capture stopped: the demand turned it off");
        }
    }

    fn sample(&mut self, now_ms: u64) -> AudioSample {
        self.poll(now_ms)
    }
}

impl<C: crate::hardware::SelfCheck, const R: usize> crate::hardware::SelfCheck for Es7210Rx<C, R> {
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

impl<C: crate::hardware::Named, const R: usize> crate::hardware::Named for Es7210Rx<C, R> {
    const NAME: &'static str = C::NAME;
}

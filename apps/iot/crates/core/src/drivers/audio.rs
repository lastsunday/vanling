use crate::drivers::input::{InputEvent, InputSource};

pub mod decimator;
mod weighting;

pub use decimator::{DECIMATION, Decimator};
use weighting::{AWeight, DbaMeter};

/// The codec's frame rate: one I2S frame per this many nanoseconds, which is what the
/// capture driver clocks the peripheral at and what a frame's worth of samples is counted
/// against.
pub const SAMPLE_RATE_HZ: u32 = 48_000;

/// 16-bit slots one I2S frame carries **on the transmit side**. The receive ring carries
/// one sample per frame: `Channels::MONO` selects a slot, it does not shorten the frame.
/// `[实测]` 8000 samples/s against the 16000 two slots would predict.
pub const SLOTS_PER_FRAME: u32 = 2;

/// Bytes one 16-bit sample occupies, little-endian like the wire.
pub const BYTES_PER_SAMPLE: u16 = 2;

/// Bytes one captured frame occupies: one sample. Not [`BYTES_PER_FRAME`], which is the
/// transmit side; see [`SLOTS_PER_FRAME`].
pub const CAPTURE_BYTES_PER_FRAME: usize = BYTES_PER_SAMPLE as usize;

/// Bytes one whole I2S frame occupies on the wire: its slots, each one sample wide.
pub const BYTES_PER_FRAME: usize = SLOTS_PER_FRAME as usize * BYTES_PER_SAMPLE as usize;

/// Capture cadence: how often the input loop polls the capture, and so how much audio can
/// be waiting when it gets there.
pub const CAPTURE_MS: u64 = 20;

/// Envelope columns one poll closes.
pub const COLUMNS_PER_POLL: u16 = 2;

/// Time one envelope column spans.
pub const COLUMN_MS: u64 = CAPTURE_MS / COLUMNS_PER_POLL as u64;

/// Samples one envelope column spans, counted in receive samples rather than transmit
/// slots. `[实测]` counting slots doubles the window and halves the sweep's speed.
pub const SAMPLES_PER_COLUMN: u16 = (SAMPLE_RATE_HZ as u64 * COLUMN_MS / 1_000) as u16;

/// Audio one capture poll hands over, in bytes.
pub const BYTES_PER_POLL: usize =
    SAMPLE_RATE_HZ as usize * CAPTURE_BYTES_PER_FRAME * CAPTURE_MS as usize / 1_000;

/// Envelope columns kept, sized to the pixel columns the panel's sweep is given, so one
/// column is one pixel column and nothing has to be decimated to draw it.
pub const ENVELOPE_COLUMNS: usize = 200;

const FULL_SCALE_LOG2_16: i32 = 16 * 15;

/// The same full scale as a peak magnitude, for comparisons that are about amplitude rather
/// than about a bar's position on the band.
pub const FULL_SCALE_LSB: u16 = 1 << 15;

const SCOPE_FLOOR_LOG2_16: i32 = 0;

const SCOPE_SPAN_LOG2_16: i32 = FULL_SCALE_LOG2_16 - SCOPE_FLOOR_LOG2_16;

/// The window's floor in decibels, which [`dbfs`] bottoms out at.
pub const SCOPE_FLOOR_DECIBELS: i32 = 90;

const LOG2_16_PER_OCTAVE: i32 = 16;

const MILLIDECIBELS_PER_OCTAVE: i64 = 6021;

const MILLIDECIBELS_PER_LOG2_16_UNIT: i64 = MILLIDECIBELS_PER_OCTAVE / 16;

const MANTISSA_BITS: u32 = 4;

const LOG2_16_CHORD_ERROR: [i8; 1 << MANTISSA_BITS] =
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0];

fn log2_16(value: u16) -> i32 {
    if value == 0 {
        return 0;
    }
    let value = u32::from(value);
    let octave = 31 - value.leading_zeros();
    let normalised = value << (31 - octave);
    let mantissa = ((normalised >> (31 - MANTISSA_BITS)) & ((1 << MANTISSA_BITS) - 1)) as usize;
    LOG2_16_PER_OCTAVE * octave as i32 + mantissa as i32 + LOG2_16_CHORD_ERROR[mantissa] as i32
}

/// A peak in LSB as a `0..=255` bar height, over `SCOPE_FLOOR_LOG2_16` up to
/// full scale. Here rather than beside the panel because the mapping has to be
/// checkable: the sweep is the one drawing that can be silently useless, since a
/// bar height that is always zero looks exactly like silence.
pub fn scope_height(peak_lsb: u16) -> u8 {
    let height =
        (log2_16(peak_lsb) - SCOPE_FLOOR_LOG2_16) * i32::from(u8::MAX) / SCOPE_SPAN_LOG2_16;
    height.clamp(0, i32::from(u8::MAX)) as u8
}

/// A peak in LSB as decibels below full scale, to the nearest decibel, never reading below
/// the window's [`SCOPE_FLOOR_DECIBELS`] and never above 0 — the numeric half of what a
/// meter shows, since 3000 LSB is not a level anyone can judge and −21 dBFS is.
pub fn dbfs(peak_lsb: u16) -> i16 {
    let below_rail = FULL_SCALE_LOG2_16 - log2_16(peak_lsb);
    let milli = i64::from(below_rail.max(0)) * MILLIDECIBELS_PER_LOG2_16_UNIT;
    let decibels = -(((milli + 500) / 1_000) as i32);
    decibels.clamp(-SCOPE_FLOOR_DECIBELS, 0) as i16
}

/// A [`dbfs`] reading as a sound pressure level: the same decibel count with the
/// microphone's own reference added back on, so 0 dBFS reads as whatever the hardware calls
/// full scale instead of as a number only this code can judge.
pub fn spl(dbfs: i16, offset_decibels: i16) -> i16 {
    dbfs + offset_decibels
}

/// How much of its height a bar may lose in one column, as a shift: 1/16 is
/// 0.561 dB per column, 56 dB/s at [`COLUMN_MS`], set by the window rather than by
/// meter convention — a needle-slow release leaves every transient 30 dB above the
/// floor for the whole sweep. 1/16 crosses [`SCOPE_FLOOR_DECIBELS`] in four-fifths
/// of [`ENVELOPE_COLUMNS`], and belongs to the column rate, so a dropped frame
/// costs a column of history.
pub const RELEASE_SHIFT: u32 = 4;

/// The peak a bar is drawn at this column: its own, or the one before it after one step of
/// the release ballistics, whichever is higher.
pub fn released_peak(measured: u16, previous: u16) -> u16 {
    let floor = previous.saturating_sub((previous >> RELEASE_SHIFT).max(1));
    measured.max(floor)
}

/// Peak envelope of a capture: one column per [`SAMPLES_PER_COLUMN`] samples, each the
/// loudest absolute sample in its window, wrapping once full.
#[derive(Debug, Clone, Copy)]
pub struct AudioEnvelope {
    columns: [u16; ENVELOPE_COLUMNS],
    weighted_columns: [u16; ENVELOPE_COLUMNS],
    rms: [u16; ENVELOPE_COLUMNS],
    weighted_rms: [u16; ENVELOPE_COLUMNS],
    cursor: u16,
    committed: u8,
    peak: u16,
    weighted_peak: u16,
    sum_squares: u64,
    weighted_sum_squares: u64,
    weight: AWeight,
    dba: DbaMeter,
    filled: u16,
    clipped: bool,
    columns_since_clip: Option<u16>,
}

impl Default for AudioEnvelope {
    fn default() -> Self {
        Self::ZERO
    }
}

impl PartialEq for AudioEnvelope {
    fn eq(&self, other: &Self) -> bool {
        self.columns == other.columns
            && self.weighted_columns == other.weighted_columns
            && self.rms == other.rms
            && self.weighted_rms == other.weighted_rms
            && self.committed == other.committed
            && self.clipped == other.clipped
            && self.columns_since_clip == other.columns_since_clip
    }
}

impl Eq for AudioEnvelope {}

impl AudioEnvelope {
    /// A blank envelope: nothing committed, every column zero.
    pub const ZERO: Self = Self {
        columns: [0; ENVELOPE_COLUMNS],
        weighted_columns: [0; ENVELOPE_COLUMNS],
        rms: [0; ENVELOPE_COLUMNS],
        weighted_rms: [0; ENVELOPE_COLUMNS],
        cursor: 0,
        committed: 0,
        peak: 0,
        weighted_peak: 0,
        sum_squares: 0,
        weighted_sum_squares: 0,
        weight: AWeight::new(),
        dba: DbaMeter::new(),
        filled: 0,
        clipped: false,
        columns_since_clip: None,
    };

    /// Folds samples in, closing and advancing a column every [`SAMPLES_PER_COLUMN`].
    pub fn push(&mut self, samples: &[i16]) {
        for &sample in samples {
            // `unsigned_abs` rather than `abs`, which overflows on `i16::MIN`:
            // that sample is the one true full-scale peak and must survive.
            let square = u32::from(sample.unsigned_abs());
            self.peak = self.peak.max(square as u16);
            self.sum_squares += u64::from(square * square);
            let weighted = self.weight.filter(sample);
            self.dba.update(weighted);
            let wsquare = u32::from(weighted.unsigned_abs());
            self.weighted_peak = self.weighted_peak.max(wsquare as u16);
            self.weighted_sum_squares += u64::from(wsquare * wsquare);
            self.filled += 1;
            if self.filled >= SAMPLES_PER_COLUMN {
                self.close_column();
            }
        }
    }

    fn close_column(&mut self) {
        let cursor = self.cursor as usize;
        self.columns[cursor] = self.peak;
        self.rms[cursor] = (self.sum_squares / u64::from(self.filled)).isqrt() as u16;
        self.weighted_columns[cursor] = self.weighted_peak;
        self.weighted_rms[cursor] =
            (self.weighted_sum_squares / u64::from(self.filled)).isqrt() as u16;
        if self.peak >= FULL_SCALE_LSB {
            self.clipped = true;
            self.columns_since_clip = Some(0);
        } else if let Some(age) = self.columns_since_clip {
            self.columns_since_clip = (age + 1 < ENVELOPE_COLUMNS as u16).then_some(age + 1);
        }
        self.cursor += 1;
        if self.cursor as usize == ENVELOPE_COLUMNS {
            self.cursor = 0;
        }
        self.committed = (self.committed + 1).min(ENVELOPE_COLUMNS as u8);
        self.peak = 0;
        self.sum_squares = 0;
        self.weighted_peak = 0;
        self.weighted_sum_squares = 0;
        self.filled = 0;
    }

    /// The A-weighted level as an RMS in LSB, saturating after the sample the meter last
    /// folded.
    pub const fn dba_lsb(&self) -> u16 {
        self.dba.level_lsb()
    }

    /// The whole window, oldest column first.
    #[cfg(test)]
    pub fn columns(&self) -> &[u16; ENVELOPE_COLUMNS] {
        &self.columns
    }

    /// The whole A-weighted window, raw peaks through the filter, same layout as
    /// [`columns`](Self::columns).
    #[cfg(test)]
    pub fn weighted_columns(&self) -> &[u16; ENVELOPE_COLUMNS] {
        &self.weighted_columns
    }

    fn column_by_age_from(&self, source: &[u16; ENVELOPE_COLUMNS], age: usize) -> u16 {
        if age >= usize::from(self.committed) {
            return 0;
        }
        let oldest = if usize::from(self.committed) == ENVELOPE_COLUMNS {
            self.cursor as usize
        } else {
            0
        };
        source[(oldest + age) % ENVELOPE_COLUMNS]
    }

    /// The bar heights the panel draws, oldest column first, with the release ballistics of
    /// [`released_peak`] threaded through in time order.
    pub fn released_peaks(&self) -> [u16; ENVELOPE_COLUMNS] {
        self.released_from(&self.columns)
    }

    /// [`released_peaks`](Self::released_peaks) over the A-weighted columns.
    pub fn weighted_released_peaks(&self) -> [u16; ENVELOPE_COLUMNS] {
        self.released_from(&self.weighted_columns)
    }

    fn released_from(&self, source: &[u16; ENVELOPE_COLUMNS]) -> [u16; ENVELOPE_COLUMNS] {
        let mut out = [0_u16; ENVELOPE_COLUMNS];
        let mut previous = 0;
        for (age, bar) in out.iter_mut().enumerate() {
            previous = released_peak(self.column_by_age_from(source, age), previous);
            *bar = previous;
        }
        out
    }

    /// Each column's RMS in LSB, oldest first, for the inner bar that tells a sustained
    /// level from a lone transient.
    pub fn rms_columns(&self) -> [u16; ENVELOPE_COLUMNS] {
        self.rms_by_age_from(&self.rms)
    }

    /// [`rms_columns`](Self::rms_columns) over the A-weighted RMS.
    pub fn weighted_rms_columns(&self) -> [u16; ENVELOPE_COLUMNS] {
        self.rms_by_age_from(&self.weighted_rms)
    }

    fn rms_by_age_from(&self, source: &[u16; ENVELOPE_COLUMNS]) -> [u16; ENVELOPE_COLUMNS] {
        let mut out = [0_u16; ENVELOPE_COLUMNS];
        for (age, level) in out.iter_mut().enumerate() {
            if age >= usize::from(self.committed) {
                break;
            }
            let oldest = if usize::from(self.committed) == ENVELOPE_COLUMNS {
                self.cursor as usize
            } else {
                0
            };
            *level = source[(oldest + age) % ENVELOPE_COLUMNS];
        }
        out
    }

    /// The window's loudest peak in LSB, the peak-hold a readout shows; zero until the
    /// first column commits.
    pub fn loudest(&self) -> u16 {
        self.columns[..usize::from(self.committed)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// The window's loudest RMS column, in LSB — the sustained level a panel reads as
    /// loudness.
    pub fn loudest_rms(&self) -> u16 {
        self.rms_columns()[..usize::from(self.committed)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// The window's median RMS column, in LSB — a floor rather than a peak.
    pub fn floor_rms(&self) -> u16 {
        let committed = usize::from(self.committed);
        if committed == 0 {
            return 0;
        }
        let mut window = self.rms_columns();
        window[..committed].sort_unstable();
        window[committed / 2]
    }

    /// Whether any column has reached full scale since this capture started.
    pub fn clipped(&self) -> bool {
        self.clipped
    }

    /// How many columns back the last full-scale column is, while it is still in the window
    /// — where on the sweep to mark it.
    pub fn clipped_age(&self) -> Option<usize> {
        let age = usize::from(self.columns_since_clip?);
        (age < usize::from(self.committed)).then_some(age)
    }

    /// Columns of history the window holds, clamped to [`ENVELOPE_COLUMNS`].
    pub fn committed(&self) -> u8 {
        self.committed
    }

    /// True while a window is partly filled and its column is still the previous value, so
    /// a panel drawing on poll rather than on sample count shows the newest bar a poll
    /// late.
    #[cfg(test)]
    pub fn mid_window(&self) -> bool {
        self.filled > 0
    }
}

const DECODE_BATCH: usize = 64;

/// Folds a captured byte stream into an envelope: a capture arrives as raw little-endian
/// 16-bit samples, and a transfer boundary can fall between the two bytes of one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleStream {
    envelope: AudioEnvelope,
    pending: Option<u8>,
}

impl SampleStream {
    pub const fn new() -> Self {
        Self {
            envelope: AudioEnvelope::ZERO,
            pending: None,
        }
    }

    /// Folds a run of little-endian 16-bit samples in, holding a trailing odd byte until
    /// the next call: a transfer hands out whatever its descriptor boundary happened to be,
    /// and that is rarely a sample boundary.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        if let Some(hi) = self.pending.take() {
            let Some((&lo, tail)) = rest.split_first() else {
                self.pending = Some(hi);
                return;
            };
            let sample = i16::from_le_bytes([hi, lo]);
            self.envelope.push(&[sample]);
            rest = tail;
        }
        let mut samples = [0_i16; DECODE_BATCH];
        let mut filled = 0;
        for pair in rest.as_chunks::<2>().0 {
            let sample = i16::from_le_bytes([pair[0], pair[1]]);
            samples[filled] = sample;
            filled += 1;
            if filled == DECODE_BATCH {
                self.envelope.push(&samples);
                filled = 0;
            }
        }
        if filled > 0 {
            self.envelope.push(&samples[..filled]);
        }
        if rest.len() % 2 == 1 {
            self.pending = Some(rest[rest.len() - 1]);
        }
    }

    /// The envelope as folded so far — the panel's scope sweep.
    pub const fn envelope(&self) -> AudioEnvelope {
        self.envelope
    }

    /// The A-weighted level as an RMS in LSB.
    pub const fn dba_lsb(&self) -> u16 {
        self.envelope.dba_lsb()
    }
}
impl Default for SampleStream {
    fn default() -> Self {
        Self::new()
    }
}

const fn ring_fill_ms(ring_bytes: usize) -> u64 {
    ring_bytes as u64 * CAPTURE_MS / BYTES_PER_POLL as u64
}

/// Notices a capture whose DMA has stopped, by the clock rather than the DMA: the
/// peripheral's own flag reports *its* completion, not the descriptor chain running dry, so
/// time is the only signal that holds either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureWatchdog {
    last_progress_ms: Option<u64>,
    silence_limit_ms: u64,
}

impl CaptureWatchdog {
    /// Watches a capture whose ring is `ring_bytes` long.
    pub const fn new(ring_bytes: usize) -> Self {
        Self {
            last_progress_ms: None,
            silence_limit_ms: ring_fill_ms(ring_bytes) + CAPTURE_MS,
        }
    }

    /// Whether the capture has gone quiet for too long, given what the last poll found
    /// waiting.
    pub fn stalled(&mut self, now_ms: u64, produced_bytes: usize) -> bool {
        if produced_bytes > 0 {
            self.last_progress_ms = Some(now_ms);
            return false;
        }
        let last = *self.last_progress_ms.get_or_insert(now_ms);
        now_ms.saturating_sub(last) > self.silence_limit_ms
    }

    /// Pushes the deadline out without a capture to show for it, for a capture that has
    /// just been re-armed and has not been polled since.
    pub fn note_progress(&mut self, now_ms: u64) {
        self.last_progress_ms = Some(now_ms);
    }
}

/// A capture chain that can be taken off the wire and put back.
pub trait Armed: Sized {
    type Parked;

    fn stop(self) -> Self::Parked;

    fn read(parked: Self::Parked) -> Result<Self, Self::Parked>;
}

/// Where a capture's chain is.
///
/// **The invariant: nothing stops a running chain except [`Self::take_to_arm`].**
/// That is one method rather than a convention because losing this chain is
/// unrecoverable and silent — the source keeps answering, with silence.
#[derive(Debug)]
pub struct Chain<A: Armed> {
    running: Option<A>,
    parked: Option<A::Parked>,
}

impl<A: Armed> Chain<A> {
    pub const fn new(running: A) -> Self {
        Self {
            running: Some(running),
            parked: None,
        }
    }

    /// Takes the chain off the wire, because the demand stopped wanting it.
    pub fn park(&mut self) -> bool {
        match self.running.take() {
            Some(running) => {
                self.parked = Some(running.stop());
                true
            }
            None => false,
        }
    }

    /// The chain to put back on the wire.
    pub fn take_to_arm(&mut self) -> Option<A::Parked> {
        match self.parked.take() {
            Some(parked) => Some(parked),
            None => self.running.take().map(Armed::stop),
        }
    }

    pub fn put(&mut self, armed: A) {
        self.running = Some(armed);
    }

    pub fn running(&self) -> Option<&A> {
        self.running.as_ref()
    }

    pub fn running_mut(&mut self) -> Option<&mut A> {
        self.running.as_mut()
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    #[cfg(test)]
    pub fn is_parked(&self) -> bool {
        self.parked.is_some()
    }

    /// Neither running nor parked: unrecoverable, and terminal.
    pub fn is_lost(&self) -> bool {
        self.running.is_none() && self.parked.is_none()
    }
}

/// Notices a capture whose input loop is not keeping up with the codec, by how deep the
/// ring is rather than by whether anything was lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureBacklog {
    warn_bytes: usize,
    clear_bytes: usize,
    backlogged: bool,
}

impl CaptureBacklog {
    /// Judges a capture whose ring is `ring_bytes` long.
    pub const fn new(ring_bytes: usize) -> Self {
        // A ring that cannot hold the levels is not one this can judge, and the
        // assert says so at compile time rather than warning about a backlog the
        // ring has no room to have.
        assert!(
            ring_bytes >= BYTES_PER_POLL * 5 / 2,
            "a ring has to hold a backlog's warn level and a poll of room"
        );
        Self {
            warn_bytes: BYTES_PER_POLL * 5 / 2,
            clear_bytes: BYTES_PER_POLL * 2,
            backlogged: false,
        }
    }

    /// The depth that opens a backlog, for a driver that has to describe it.
    #[cfg(test)]
    pub const fn warn_bytes(&self) -> usize {
        self.warn_bytes
    }

    /// The depth that closes one.
    #[cfg(test)]
    pub const fn clear_bytes(&self) -> usize {
        self.clear_bytes
    }

    /// Reports the depth a poll found, and gives back the depth only on the poll that opens
    /// a backlog — a driver logs the one, not the state.
    pub fn report(&mut self, produced_bytes: usize) -> Option<usize> {
        if produced_bytes < self.warn_bytes {
            if produced_bytes <= self.clear_bytes {
                self.backlogged = false;
            }
            return None;
        }
        if self.backlogged {
            return None;
        }
        self.backlogged = true;
        Some(produced_bytes)
    }

    #[cfg(test)]
    pub const fn backlogged(&self) -> bool {
        self.backlogged
    }
}

/// One capture poll: the peak envelope every consumer may read, plus how long the capture
/// has been running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioSample {
    pub envelope: AudioEnvelope,
    /// Capture wall time (ms) the source has been running, saturating; the panel reads it
    /// as elapsed time while a recording runs.
    pub elapsed_ms: u32,
    /// How many times the capture has had to be re-armed because its DMA stopped.
    pub restarts: u16,
    /// The A-weighted level of the same capture, as an RMS in LSB.
    pub dba_lsb: u16,
}

pub trait AudioSource {
    fn sample(&mut self, now_ms: u64) -> AudioSample;

    fn set_collecting(&mut self, _now_ms: u64, _collecting: bool) {}

    fn accepts_slower_poll(&self) -> bool {
        true
    }
}

pub trait CaptureSink {
    fn publish(&self, sample: AudioSample);
}

pub struct AudioInput<S> {
    source: S,
    sink: &'static dyn CaptureSink,
}

impl<S> AudioInput<S> {
    pub const fn new(source: S, sink: &'static dyn CaptureSink) -> Self {
        Self { source, sink }
    }
}

impl<S: AudioSource> InputSource for AudioInput<S> {
    fn name(&self) -> &'static str {
        "capture"
    }

    fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        self.sink.publish(self.source.sample(now_ms));
        None
    }

    fn set_demand_collecting(&mut self, now_ms: u64, collecting: bool) {
        self.source.set_collecting(now_ms, collecting);
    }

    fn accepts_slower_poll(&self) -> bool {
        self.source.accepts_slower_poll()
    }
}

/// How long a corner change is given to settle before the envelope covering it is read.
pub const CORNER_SETTLE_MS: u64 = 3_000;

/// What a corner walk wants the capture to do on this poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerStep {
    Settling,
    Write(u8),
    Read(u8),
    Done,
}

/// A one-shot walk of a filter's corner codes, so a board can report what each corner reads
/// on the real room rather than which of them it expected to.
#[derive(Debug, Clone, Copy)]
pub struct CornerSweep {
    codes: u8,
    next: u8,
    reading: Option<u8>,
    written_at_ms: u64,
}

impl CornerSweep {
    /// A walk of `codes` corner codes, numbered from zero.
    pub const fn new(codes: u8) -> Self {
        Self {
            codes,
            next: 0,
            reading: None,
            written_at_ms: 0,
        }
    }

    /// Advances the walk by one poll and says what to do.
    pub fn step(&mut self, now_ms: u64) -> CornerStep {
        if let Some(code) = self.reading {
            if now_ms.saturating_sub(self.written_at_ms) < CORNER_SETTLE_MS {
                return CornerStep::Settling;
            }
            self.reading = None;
            return CornerStep::Read(code);
        }
        if self.next >= self.codes {
            return CornerStep::Done;
        }
        let code = self.next;
        self.next += 1;
        self.reading = Some(code);
        self.written_at_ms = now_ms;
        CornerStep::Write(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use core::cell::RefCell;

    #[derive(Debug, PartialEq)]
    struct FakeChain(u32);

    #[derive(Debug)]
    struct FakeParked(u32);

    thread_local! {
        static READ_REFUSES: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static STOPS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    }

    impl Armed for FakeChain {
        type Parked = FakeParked;
        fn stop(self) -> FakeParked {
            STOPS.with(|n| n.set(n.get() + 1));
            FakeParked(self.0)
        }
        fn read(parked: FakeParked) -> Result<Self, FakeParked> {
            if READ_REFUSES.with(|r| r.get()) {
                return Err(parked);
            }
            Ok(FakeChain(parked.0))
        }
    }

    #[test]
    fn parking_and_arming_returns_the_same_chain() {
        let mut chain = Chain::new(FakeChain(7));
        assert!(chain.is_running());
        assert!(chain.park());

        let armed = chain.take_to_arm().expect("a parked chain is there to arm");
        chain.put(<FakeChain as Armed>::read(armed).expect("read takes"));
        assert_eq!(
            chain.running(),
            Some(&FakeChain(7)),
            "the same ring came back"
        );
        assert_eq!(
            STOPS.with(|n| n.get()),
            1,
            "stopped exactly once, not dropped"
        );
    }

    #[test]
    fn repairing_a_stalled_capture_never_drops_the_chain() {
        let mut chain = Chain::new(FakeChain(7));
        let armed = chain
            .take_to_arm()
            .expect("a running chain is there to arm");
        assert!(chain.is_lost(), "the chain is in hand, not in the struct");
        chain.put(<FakeChain as Armed>::read(armed).expect("read takes"));
        assert!(chain.is_running());
        assert!(!chain.is_lost());
    }

    #[test]
    fn a_capture_with_the_demand_off_is_parked_and_not_lost() {
        let mut chain = Chain::new(FakeChain(1));
        assert!(chain.park());
        assert!(!chain.is_running(), "off the wire");
        assert!(
            chain.is_parked(),
            "and still owned, which is not the same as gone"
        );
        assert!(!chain.is_lost());
    }

    #[test]
    fn a_re_arm_that_refuses_reports_itself_rather_than_going_quiet() {
        let mut chain = Chain::new(FakeChain(3));
        let armed = chain.take_to_arm().expect("there to arm");
        READ_REFUSES.with(|r| r.set(true));
        let refused = <FakeChain as Armed>::read(armed);
        READ_REFUSES.with(|r| r.set(false));

        assert!(refused.is_err(), "the peripheral said no");
        assert!(
            chain.is_lost(),
            "so there is nothing left to drive, and the caller has to be told"
        );
    }

    #[test]
    fn parking_nothing_is_not_a_second_park() {
        let mut chain = Chain::new(FakeChain(1));
        assert!(chain.park());
        assert!(!chain.park(), "already off the wire");
        assert!(
            chain.is_parked(),
            "and the one chain that was there is still parked"
        );
    }

    use alloc::vec::Vec;

    struct FakeSource {
        calls: u32,
        amplitude: i16,
        restarts: u16,
        collecting: Option<bool>,
        switched_at_ms: u64,
    }

    impl AudioSource for FakeSource {
        fn set_collecting(&mut self, now_ms: u64, collecting: bool) {
            self.collecting = Some(collecting);
            self.switched_at_ms = now_ms;
        }

        fn sample(&mut self, now_ms: u64) -> AudioSample {
            self.calls += 1;
            let mut samples = [0_i16; SAMPLES_PER_COLUMN as usize];
            samples[0] = self.amplitude;
            let mut envelope = AudioEnvelope::default();
            envelope.push(&samples);
            AudioSample {
                envelope,
                elapsed_ms: now_ms as u32,
                restarts: self.restarts,
                dba_lsb: 0,
            }
        }
    }

    #[derive(Default)]
    struct Recording {
        samples: RefCell<Vec<AudioSample>>,
    }

    impl CaptureSink for Recording {
        fn publish(&self, sample: AudioSample) {
            self.samples.borrow_mut().push(sample);
        }
    }

    fn sink() -> &'static Recording {
        Box::leak(Box::new(Recording::default()))
    }

    fn fake(amplitude: i16, restarts: u16) -> FakeSource {
        FakeSource {
            calls: 0,
            amplitude,
            restarts,
            collecting: None,
            switched_at_ms: 0,
        }
    }

    #[test]
    fn every_cadence_tick_publishes_a_capture_poll() {
        let sink = sink();
        let mut input = AudioInput::new(fake(8_000, 7), sink);
        for now_ms in [20, 40, 60] {
            assert!(
                input.poll(now_ms).is_none(),
                "a poll reports no event of its own; the window is the sink's"
            );
        }
        let samples = sink.samples.borrow();
        assert_eq!(samples.len(), 3, "one window per cadence tick");
        for (sample, now_ms) in samples.iter().zip([20, 40, 60]) {
            assert_eq!(sample.envelope.committed(), 1);
            assert_eq!(sample.elapsed_ms, now_ms as u32);
            assert_eq!(sample.restarts, 7);
        }
    }

    #[test]
    fn the_source_sees_the_clock_the_entry_was_polled_at() {
        let mut input = AudioInput::new(fake(0, 0), sink());
        input.poll(1_234);
        assert_eq!(
            input.source.calls, 1,
            "one poll, one read of the capture ring"
        );
    }

    #[test]
    fn turning_the_capture_off_reaches_the_source_without_a_poll() {
        let mut input = AudioInput::new(fake(0, 0), sink());
        assert_eq!(input.source.collecting, None);

        input.set_demand_collecting(30_000, false);
        assert_eq!(input.source.collecting, Some(false));
        assert_eq!(input.source.calls, 0, "and it took no poll to say so");

        input.set_demand_collecting(30_050, true);
        assert_eq!(input.source.collecting, Some(true));
    }

    // The ring the S3 capture runs on: three polls of audio, so the watchdog
    // tolerates 80 ms of silence — a full ring's fill plus a poll — before
    // calling a stall.
    const RING_BYTES: usize = BYTES_PER_POLL * 3;

    const A_POLL_OF_AUDIO: usize = BYTES_PER_POLL;

    #[test]
    fn a_capture_drained_every_poll_is_never_called_stalled() {
        let mut watchdog = CaptureWatchdog::new(RING_BYTES);
        for now_ms in (0..1_000).step_by(CAPTURE_MS as usize) {
            assert!(
                !watchdog.stalled(now_ms, A_POLL_OF_AUDIO),
                "a capture with {A_POLL_OF_AUDIO} B waiting at {now_ms} ms is running"
            );
        }
    }

    #[test]
    fn a_capture_that_filled_its_ring_once_and_stopped_is_called_stalled() {
        let mut watchdog = CaptureWatchdog::new(RING_BYTES);
        assert!(
            !watchdog.stalled(0, RING_BYTES),
            "the ring is full of audio"
        );
        for now_ms in [20, 60, 80] {
            assert!(
                !watchdog.stalled(now_ms, 0),
                "silence at {now_ms} ms is still within the ring's own fill time"
            );
        }
        assert!(
            watchdog.stalled(100, 0),
            "a full ring's worth of silence past the deadline means the engine stopped"
        );
    }

    #[test]
    fn a_rings_silence_limit_follows_the_rate_the_wire_moves_at() {
        // 60 ms of fill plus a 20 ms poll. Scored at the frame rate instead —
        // one slot per frame — the same ring buys 120 ms, which is how a stopped
        // capture ran for twice as long as it should have.
        assert_eq!(ring_fill_ms(RING_BYTES), 60);
        assert_eq!(
            CaptureWatchdog::new(RING_BYTES).silence_limit_ms,
            80,
            "a ring that holds three polls is given three polls plus one to fill"
        );
    }

    #[test]
    fn a_late_poll_within_the_silence_limit_is_not_a_stall() {
        let mut watchdog = CaptureWatchdog::new(RING_BYTES);
        watchdog.stalled(0, A_POLL_OF_AUDIO);
        assert!(
            !watchdog.stalled(40, 0),
            "a poll 40 ms late is behind, not dead — and must not cost the capture its ring"
        );
    }

    #[test]
    fn a_reatarmed_capture_gets_a_whole_deadline_again() {
        let mut watchdog = CaptureWatchdog::new(RING_BYTES);
        watchdog.stalled(0, A_POLL_OF_AUDIO);
        assert!(watchdog.stalled(200, 0));
        watchdog.note_progress(200);
        assert!(!watchdog.stalled(240, 0), "the new stream starts unarmed");
    }

    #[test]
    fn a_capture_that_never_produced_anything_is_still_given_its_grace() {
        let mut watchdog = CaptureWatchdog::new(RING_BYTES);
        assert!(!watchdog.stalled(500, 0), "the first poll only arms it");
        assert!(watchdog.stalled(600, 0), "and silence past that is a stall");
    }

    #[test]
    fn the_depth_a_healthy_capture_settles_at_is_not_called_a_backlog() {
        // A loop running a fixed beat behind the wire settles a whole poll
        // deeper than the steady state for ever, so the depths below are what
        // the panel reported while the ring stayed healthy.
        //
        // **Halved along with [`BYTES_PER_POLL`]**, and deliberately not
        // re-measured: these were recorded when a poll period was counted with
        // two slots per frame, so every figure in them was a poll count wearing
        // twice the bytes. What they assert is a *depth in polls* — 2.0 to 2.13
        // — and that is unchanged. Quoting them again in poll counts rather than
        // bytes is what stops the next frame-width correction from silently
        // invalidating them.
        let mut backlog = CaptureBacklog::new(RING_BYTES);
        for produced in [
            BYTES_PER_POLL * 2 + BYTES_PER_POLL / 51,
            BYTES_PER_POLL * 2 + BYTES_PER_POLL * 7 / 100,
            BYTES_PER_POLL * 2 + BYTES_PER_POLL * 3 / 32,
        ] {
            assert_eq!(
                backlog.report(produced),
                None,
                "a ring {produced} B deep is the steady state, not a backlog"
            );
            assert!(!backlog.backlogged());
        }
    }

    #[test]
    fn a_backlog_is_reported_once_and_stays_reported_until_the_ring_recovers() {
        let mut backlog = CaptureBacklog::new(RING_BYTES);
        let opened = backlog.report(BYTES_PER_POLL * 3);
        assert_eq!(
            opened,
            Some(BYTES_PER_POLL * 3),
            "the poll that opens a backlog gives its depth back to be logged"
        );
        assert!(backlog.backlogged());
        assert_eq!(
            backlog.report(BYTES_PER_POLL * 3),
            None,
            "a ring that stays deep is one backlog, not a new one every poll"
        );
        assert_eq!(backlog.report(BYTES_PER_POLL * 5 / 2 - 1), None);
        assert!(backlog.backlogged());
    }

    #[test]
    fn a_ring_back_at_its_steady_depth_has_recovered() {
        let mut backlog = CaptureBacklog::new(RING_BYTES);
        backlog.report(BYTES_PER_POLL * 3);
        backlog.report(BYTES_PER_POLL * 2);
        assert!(
            !backlog.backlogged(),
            "two polls is where a healthy loop sits, so the backlog is over"
        );
        assert_eq!(
            backlog.report(BYTES_PER_POLL * 2),
            None,
            "and a ring that stays there does not re-open it"
        );
    }

    #[test]
    fn the_backlog_levels_sit_clear_of_the_steady_state_and_inside_the_ring() {
        let backlog = CaptureBacklog::new(RING_BYTES);
        assert_eq!(backlog.warn_bytes(), 4_800);
        assert_eq!(backlog.clear_bytes(), 3_840);
        assert!(
            backlog.warn_bytes() > BYTES_PER_POLL * 2 + BYTES_PER_POLL * 3 / 32,
            "the deepest steady-state depth measured on hardware, 2.13 polls"
        );
        assert!(
            RING_BYTES > backlog.warn_bytes(),
            "and the ring has to hold the warn level with room to spare"
        );
        assert!(
            backlog.clear_bytes() < backlog.warn_bytes(),
            "the clear level is what stops a backlog being reported for ever"
        );
    }

    fn full_scale() -> Vec<i16> {
        alloc::vec![i16::MAX; SAMPLES_PER_COLUMN as usize]
    }

    const PEAK_FULL_SCALE: u16 = 32_768;

    #[test]
    fn a_fresh_envelope_commits_nothing() {
        let envelope = AudioEnvelope::default();
        assert_eq!(envelope.committed(), 0);
        assert!(!envelope.mid_window());
        assert!(envelope.columns().iter().all(|&column| column == 0));
        assert_eq!(envelope.loudest(), 0);
    }

    #[test]
    fn a_full_window_commits_the_peak_it_saw_in_lsb() {
        let mut samples = vec![0; SAMPLES_PER_COLUMN as usize];
        samples[SAMPLES_PER_COLUMN as usize / 2] = i16::MIN;
        let mut envelope = AudioEnvelope::default();
        envelope.push(&samples);
        assert_eq!(envelope.committed(), 1);
        assert_eq!(
            envelope.columns()[0],
            PEAK_FULL_SCALE,
            "the negative rail survives `unsigned_abs` and is the exact full scale"
        );
        assert_eq!(envelope.loudest(), PEAK_FULL_SCALE);
        assert!(!envelope.mid_window());
    }

    #[test]
    fn scope_height_is_flat_below_the_floor_and_full_at_the_rail() {
        assert_eq!(scope_height(0), 0, "silence draws no bar");
        assert_eq!(scope_height(1), 0, "and so does the floor itself");
        assert_eq!(
            scope_height(PEAK_FULL_SCALE),
            u8::MAX,
            "0 dBFS fills the band"
        );
        // The positive rail is half an LSB under the negative one, which the
        // decibel scale rounds away: 0.006 dB is not a visible bar.
        assert_eq!(scope_height(i16::MAX.unsigned_abs()), u8::MAX - 2);
    }

    #[test]
    fn scope_height_makes_conversational_speech_visible() {
        assert!(
            scope_height(3_000) > u8::MAX / 3,
            "−21 dBFS has to be plainly visible, not a sliver"
        );
        assert!(
            scope_height(1_000) > u8::MAX / 6,
            "−30 dBFS still has to read as a bar"
        );
        assert_eq!(scope_height(32), 85, "−60 dBFS, a third of the −90 dB band");
        assert_eq!(scope_height(64), 102);
    }

    #[test]
    fn scope_height_never_dips_as_the_signal_grows() {
        let mut previous = 0;
        for peak in 0..=PEAK_FULL_SCALE {
            let height = scope_height(peak);
            assert!(
                height >= previous,
                "peak {peak} drew {height}, under the {previous} before it"
            );
            previous = height;
        }
        assert_eq!(previous, u8::MAX, "and it ends at the rail");
    }

    #[test]
    fn dbfs_reads_a_level_anyone_can_judge() {
        assert_eq!(dbfs(PEAK_FULL_SCALE), 0, "full scale is the rail");
        assert_eq!(dbfs(0), -90, "silence reads the floor, not an infinity");
        assert_eq!(dbfs(1), -90, "and so does the floor itself");
        assert_eq!(dbfs(PEAK_FULL_SCALE / 10), -20, "a tenth of full scale");
        assert_eq!(dbfs(PEAK_FULL_SCALE / 2), -6, "half scale is −6 dB");
    }

    #[test]
    fn dbfs_agrees_with_the_band_it_is_drawn_on() {
        for peak in (0..=PEAK_FULL_SCALE).step_by(37) {
            let decibels = i32::from(dbfs(peak));
            if decibels < -SCOPE_FLOOR_DECIBELS {
                continue;
            }
            let expected =
                (decibels + SCOPE_FLOOR_DECIBELS) * i32::from(u8::MAX) / SCOPE_FLOOR_DECIBELS;
            let drawn = i32::from(scope_height(peak));
            assert!(
                (drawn - expected).abs() <= 4,
                "{peak} LSB read {decibels} dBFS and drew {drawn}, wanted about {expected}"
            );
        }
    }

    #[test]
    fn dbfs_never_runs_backwards_as_the_signal_grows() {
        let mut previous = i16::MIN;
        for peak in (0..=PEAK_FULL_SCALE).step_by(31) {
            let reading = dbfs(peak);
            assert!(
                reading >= previous,
                "{peak} LSB read {reading} dBFS, under the {previous} before it"
            );
            previous = reading;
        }
        assert_eq!(previous, 0, "and it ends at the rail");
    }

    #[test]
    fn spl_is_dbfs_with_the_microphones_own_reference_back() {
        const ZTS6216: i16 = 102;
        assert_eq!(spl(-40, ZTS6216), 62, "a −40 dBFS floor is 62 dB SPL");
        assert_eq!(spl(0, ZTS6216), 102, "and the rail is the offset itself");
        assert_eq!(
            spl(-60, ZTS6216),
            42,
            "the window floor lands 42, inside the range a room occupies"
        );
    }

    #[test]
    fn spl_never_runs_backwards_as_the_signal_grows() {
        const ZTS6216: i16 = 102;
        let mut previous = i16::MIN;
        for peak in (0..=PEAK_FULL_SCALE).step_by(31) {
            let reading = spl(dbfs(peak), ZTS6216);
            assert!(
                reading >= previous,
                "{peak} LSB read {reading} dB SPL, under the {previous} before it"
            );
            previous = reading;
        }
        assert_eq!(previous, 102, "and it ends at the microphone's full scale");
    }

    #[test]
    fn a_bar_falls_gradually_instead_of_tracking_the_signal() {
        let measured = 8_000;
        assert_eq!(released_peak(measured, 0), measured, "a first column");
        let after_one = released_peak(0, measured);
        assert!(after_one < measured, "a silent column falls");
        let fall = u32::from(measured - after_one);
        assert!(
            fall * 16 >= u32::from(measured) && fall * 10 < u32::from(measured),
            "fell {fall} of {measured} in one column, wanted between a tenth and a sixteenth"
        );
    }

    #[test]
    fn a_release_never_lifts_a_bar_above_its_own_column() {
        for previous in [1, 100, 4_000, 20_000, PEAK_FULL_SCALE] {
            for measured in [0, 1, 512, 16_000, PEAK_FULL_SCALE] {
                assert!(
                    released_peak(measured, previous) >= measured,
                    "{measured} under {previous}"
                );
            }
        }
    }

    #[test]
    fn a_release_crosses_the_whole_band_inside_the_window() {
        let mut drawn = PEAK_FULL_SCALE;
        let mut columns_to_floor = None;
        for column in 0..ENVELOPE_COLUMNS {
            drawn = released_peak(0, drawn);
            if scope_height(drawn) == 0 {
                columns_to_floor = Some(column + 1);
                break;
            }
        }
        let columns = columns_to_floor.expect("a release that never returns to the floor");
        assert!(
            columns < ENVELOPE_COLUMNS,
            "back to the floor in {columns} columns, which is longer than the \
             {ENVELOPE_COLUMNS}-column window it is drawn in"
        );
        assert!(
            scope_height(released_peak(0, PEAK_FULL_SCALE)) > 0,
            "the peak has to still be on the band one column after it was heard"
        );
    }

    #[test]
    fn the_rms_of_a_column_reads_the_level_not_the_loudest_sample() {
        let mut steady = AudioEnvelope::default();
        steady.push(&vec![4_000; SAMPLES_PER_COLUMN as usize]);
        assert_eq!(steady.columns()[0], 4_000);
        assert_eq!(steady.rms_columns()[0], 4_000, "a constant is its own RMS");

        let mut one_spike = AudioEnvelope::default();
        let mut samples = vec![0; SAMPLES_PER_COLUMN as usize];
        samples[0] = 16_000;
        one_spike.push(&samples);
        assert_eq!(
            one_spike.columns()[0],
            16_000,
            "the peak still sees the spike"
        );
        assert!(
            (0..1_200).contains(&one_spike.rms_columns()[0]),
            "one sample in {SAMPLES_PER_COLUMN} averages down to almost nothing, got {}",
            one_spike.rms_columns()[0]
        );
    }

    #[test]
    fn the_rms_of_a_two_way_signal_is_its_own_amplitude() {
        let mut envelope = AudioEnvelope::default();
        let alternating: Vec<i16> = (0..SAMPLES_PER_COLUMN as usize)
            .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
            .collect();
        envelope.push(&alternating);
        assert_eq!(envelope.columns()[0], PEAK_FULL_SCALE);
        assert!(
            envelope.rms_columns()[0] as u32 > i16::MAX.unsigned_abs() as u32 * 99 / 100,
            "an alternating rail reads within 1% of full scale, got {}",
            envelope.rms_columns()[0]
        );
    }

    #[test]
    fn a_clip_is_latched_and_says_when_it_happened() {
        let mut envelope = AudioEnvelope::default();
        envelope.push(&vec![1_000; SAMPLES_PER_COLUMN as usize * 3]);
        assert!(!envelope.clipped(), "a quiet window does not clip");
        assert_eq!(envelope.clipped_age(), None);

        let mut spike = vec![1_000; SAMPLES_PER_COLUMN as usize];
        spike[SAMPLES_PER_COLUMN as usize - 1] = i16::MIN;
        envelope.push(&spike);
        assert!(envelope.clipped(), "full scale latches");
        assert_eq!(
            envelope.clipped_age(),
            Some(0),
            "the mark sits on the column that clipped"
        );
        envelope.push(&vec![1_000; SAMPLES_PER_COLUMN as usize * 2]);
        assert_eq!(
            envelope.clipped_age(),
            Some(2),
            "and ages with the window, as a mark on a visible column has to"
        );
        assert!(
            envelope.clipped(),
            "the latch itself outlives the mark: the capture was distorted, and
             that is true however long the bar scrolls on"
        );
    }

    #[test]
    fn the_floor_is_the_median_and_not_the_loudest_column() {
        let mut envelope = AudioEnvelope::default();
        for amplitude in [1_000_i16, 1_000, 10_000, 1_000] {
            envelope.push(&vec![amplitude; SAMPLES_PER_COLUMN as usize]);
        }
        assert_eq!(envelope.committed(), 4);
        assert_eq!(envelope.loudest_rms(), 10_000, "the panel reads the event");
        assert_eq!(envelope.floor_rms(), 1_000, "the floor is the middle");
    }

    #[test]
    fn a_loud_column_cannot_move_the_floor_until_it_is_half_the_window() {
        let mut envelope = AudioEnvelope::default();
        for _ in 0..8 {
            envelope.push(&vec![1_000; SAMPLES_PER_COLUMN as usize]);
        }
        let baseline = envelope.floor_rms();
        envelope.push(&vec![16_000; SAMPLES_PER_COLUMN as usize]);
        assert_eq!(
            envelope.loudest_rms(),
            16_000,
            "the panel's own reading does jump"
        );
        assert_eq!(
            envelope.floor_rms(),
            baseline,
            "one loud column in nine moves nothing"
        );

        for _ in 0..7 {
            envelope.push(&vec![16_000; SAMPLES_PER_COLUMN as usize]);
        }
        assert_eq!(envelope.committed(), 16);
        assert_eq!(
            envelope.floor_rms(),
            16_000,
            "once the loud columns are half the window, the floor is loud"
        );
    }

    #[test]
    fn the_floor_ignores_the_columns_the_capture_has_not_heard_yet() {
        let mut envelope = AudioEnvelope::default();
        for _ in 0..3 {
            envelope.push(&vec![9_000; SAMPLES_PER_COLUMN as usize]);
        }
        assert_eq!(envelope.committed(), 3);
        assert_eq!(envelope.floor_rms(), 9_000, "the level of what was heard");
        assert_eq!(envelope.loudest_rms(), 9_000, "and the loudest agrees here");
    }

    #[test]
    fn a_uniform_window_has_a_floor_at_its_own_level() {
        let mut envelope = AudioEnvelope::default();
        for _ in 0..5 {
            envelope.push(&vec![2_000; SAMPLES_PER_COLUMN as usize]);
        }
        let floor = envelope.floor_rms();
        assert!((1_990..=2_000).contains(&floor), "floor {floor}");
    }

    #[test]
    fn a_window_that_has_committed_nothing_has_no_floor() {
        let envelope = AudioEnvelope::default();
        assert_eq!(envelope.committed(), 0);
        assert_eq!(
            envelope.floor_rms(),
            0,
            "nothing heard is nothing to take a middle of"
        );
    }

    #[test]
    fn a_window_of_silence_has_a_floor_of_zero() {
        let mut envelope = AudioEnvelope::default();
        for _ in 0..4 {
            envelope.push(&vec![0; SAMPLES_PER_COLUMN as usize]);
        }
        assert_eq!(envelope.committed(), 4);
        assert_eq!(envelope.floor_rms(), 0);
        assert_eq!(
            envelope.weighted_rms_columns()[3],
            0,
            "silence weights to silence"
        );
        assert_eq!(
            envelope.weighted_released_peaks()[0],
            0,
            "and draws no weighted bar"
        );
    }

    fn tone_buffer(freq_hz: f64, amplitude: i32, samples: usize) -> Vec<i16> {
        assert_eq!(samples % SAMPLES_PER_COLUMN as usize, 0, "whole columns");
        let step = 2.0 * core::f64::consts::PI * freq_hz / f64::from(SAMPLE_RATE_HZ);
        (0..samples)
            .map(|i| (f64::from(amplitude) * (step * i as f64).sin()) as i16)
            .collect()
    }

    // A-weighting cuts 62.5 Hz by about 26 dB, so a low tone must not draw a
    // solid band even when it saturates the raw columns.
    #[test]
    fn a_low_tone_drives_the_raw_columns_but_not_the_weighted_twin() {
        let low = tone_buffer(62.5, 12_000, SAMPLES_PER_COLUMN as usize * 4);
        let mut envelope = AudioEnvelope::default();
        envelope.push(&low);
        let raw = envelope.rms_columns()[2];
        let weighted = envelope.weighted_rms_columns()[2];
        assert!(
            weighted * 10 < raw,
            "62.5 Hz read {weighted} LSB weighted against {raw} LSB raw"
        );
        assert!(
            envelope.weighted_columns()[2] * 10 < envelope.columns()[2],
            "the separation holds for the peaks as well"
        );
    }

    // A-weighting is unity at 1 kHz, so a mid-band tone reads the same through
    // both column sets.
    #[test]
    fn a_mid_band_tone_reads_the_same_through_both_columns() {
        let mid = tone_buffer(1_000.0, 12_000, SAMPLES_PER_COLUMN as usize * 4);
        let mut envelope = AudioEnvelope::default();
        envelope.push(&mid);
        let raw = envelope.rms_columns()[2];
        let weighted = envelope.weighted_rms_columns()[2];
        assert!(
            (weighted as i32 - raw as i32).unsigned_abs() <= raw as u32 / 4,
            "1 kHz read {weighted} LSB weighted against {raw} LSB raw"
        );
    }

    #[test]
    fn a_clip_mark_ages_out_with_the_column_it_points_at() {
        let mut envelope = AudioEnvelope::default();
        let mut spike = vec![0; SAMPLES_PER_COLUMN as usize];
        spike[0] = i16::MIN;
        envelope.push(&spike);
        assert_eq!(envelope.clipped_age(), Some(0));
        envelope.push(&vec![0; SAMPLES_PER_COLUMN as usize * ENVELOPE_COLUMNS]);
        assert_eq!(envelope.clipped_age(), None, "the column is gone");
        assert!(envelope.clipped(), "but the capture still clipped");
    }

    #[test]
    fn the_release_falls_across_the_window_in_the_order_the_audio_arrived() {
        let mut envelope = AudioEnvelope::default();
        envelope.push(&vec![i16::MIN; SAMPLES_PER_COLUMN as usize]);
        envelope.push(&vec![
            0;
            SAMPLES_PER_COLUMN as usize * (ENVELOPE_COLUMNS - 1)
        ]);
        let bars = envelope.released_peaks();
        assert_eq!(
            bars[0], PEAK_FULL_SCALE,
            "the loudest column is drawn at its own level"
        );
        for age in 1..ENVELOPE_COLUMNS {
            assert!(
                bars[age] <= bars[age - 1],
                "bar {age} ({}) rose above the one before it ({})",
                bars[age],
                bars[age - 1]
            );
        }
        assert_eq!(
            bars[ENVELOPE_COLUMNS - 1],
            0,
            "the tail has fallen to the floor by the window's far edge"
        );
    }

    #[test]
    fn the_tallest_drawn_bar_is_the_peak_the_panel_reads_out() {
        let mut envelope = AudioEnvelope::default();
        for peak in [4_000, 0, 0, 25_000, 0, 0, 0, 0, 0, 0, 0, 0] {
            envelope.push(&vec![peak as i16; SAMPLES_PER_COLUMN as usize]);
        }
        let tallest = envelope.released_peaks();
        assert_eq!(
            scope_height(tallest.iter().copied().max().unwrap()),
            scope_height(envelope.loudest()),
            "the tallest bar and the peak readout must be the same level"
        );
        assert_eq!(envelope.loudest(), 25_000);
    }

    #[test]
    fn the_window_is_the_audio_two_seconds_worth() {
        let window_ms = ENVELOPE_COLUMNS as u64 * COLUMN_MS;
        assert_eq!(window_ms, 2_000, "two seconds of sweep");
        assert_eq!(
            COLUMNS_PER_POLL as u64 * COLUMN_MS,
            CAPTURE_MS,
            "a poll closes exactly its own columns and no more"
        );
    }

    #[test]
    fn a_partial_window_leaves_the_column_untouched() {
        let mut envelope = AudioEnvelope::default();
        envelope.push(&[i16::MAX; SAMPLES_PER_COLUMN as usize - 1]);
        assert_eq!(envelope.committed(), 0, "window not closed yet");
        assert!(envelope.mid_window());
        assert_eq!(envelope.columns()[0], 0);
    }

    #[test]
    fn a_column_is_never_written_before_its_window_closes() {
        let mut envelope = AudioEnvelope::default();
        for chunk in full_scale().chunks(97) {
            envelope.push(chunk);
        }
        assert_eq!(
            envelope.committed(),
            1,
            "960 samples in 97-sample chunks still closes one column"
        );
    }

    #[test]
    fn a_push_longer_than_a_window_carries_the_remainder_forward() {
        let mut samples = full_scale();
        samples.extend(full_scale());
        samples[SAMPLES_PER_COLUMN as usize + 4] = i16::MIN;
        let mut envelope = AudioEnvelope::default();
        envelope.push(&samples);
        assert_eq!(envelope.committed(), 2);
        assert_eq!(
            envelope.columns()[1],
            PEAK_FULL_SCALE,
            "the loud sample past the boundary landed in the next column"
        );
    }

    #[test]
    fn the_envelope_wraps_instead_of_growing() {
        let mut envelope = AudioEnvelope::default();
        envelope.push(&vec![5_000; SAMPLES_PER_COLUMN as usize * ENVELOPE_COLUMNS]);
        assert_eq!(envelope.committed(), ENVELOPE_COLUMNS as u8);
        let one_more = vec![i16::MIN; SAMPLES_PER_COLUMN as usize];
        envelope.push(&one_more);
        assert_eq!(
            envelope.committed(),
            ENVELOPE_COLUMNS as u8,
            "the count stops at one window's worth"
        );
        assert_eq!(
            envelope.columns()[0],
            PEAK_FULL_SCALE,
            "the window wrapped onto the oldest column"
        );
        assert_eq!(
            envelope.columns()[1],
            5_000,
            "its neighbour still holds the previous sweep"
        );
    }

    #[test]
    fn silence_keeps_committing_but_stays_flat() {
        let mut envelope = AudioEnvelope::default();
        envelope.push(&vec![0; SAMPLES_PER_COLUMN as usize * 3]);
        assert_eq!(envelope.committed(), 3);
        assert!(envelope.columns()[..3].iter().all(|&column| column == 0));
    }

    #[test]
    fn a_silent_capture_stops_reading_as_changed_once_the_ring_has_wrapped() {
        let silence = [0_i16; SAMPLES_PER_COLUMN as usize];
        let mut envelope = AudioEnvelope::default();
        for _ in 0..ENVELOPE_COLUMNS + 10 {
            envelope.push(&silence);
        }
        assert_eq!(
            envelope.committed(),
            ENVELOPE_COLUMNS as u8,
            "the ring has wrapped, so the cursor is no longer where it started",
        );

        let mut unchanged = 0;
        for _ in 0..600 {
            let before = envelope;
            envelope.push(&silence);
            if before == envelope {
                unchanged += 1;
            }
        }
        assert_eq!(
            unchanged, 600,
            "a silent capture draws the same sweep every poll, so it is not a change",
        );
    }

    #[test]
    fn a_capture_that_hears_something_is_still_read_as_changed() {
        let silence = [0_i16; SAMPLES_PER_COLUMN as usize];
        let mut envelope = AudioEnvelope::default();
        for _ in 0..ENVELOPE_COLUMNS + 10 {
            envelope.push(&silence);
        }
        let before = envelope;
        let mut loud = silence;
        loud[0] = 8_000;
        envelope.push(&loud);
        assert_ne!(
            before, envelope,
            "a column that moved is exactly what a repaint exists to draw",
        );
    }

    #[test]
    fn the_loudest_column_tells_a_silent_capture_from_an_empty_one() {
        let mut silent = AudioEnvelope::default();
        silent.push(&vec![0; SAMPLES_PER_COLUMN as usize * 4]);
        assert_eq!(silent.committed(), 4);
        assert_eq!(silent.loudest(), 0, "four columns of true silence");

        let mut quiet = AudioEnvelope::default();
        quiet.push(&vec![40; SAMPLES_PER_COLUMN as usize * 4]);
        assert_eq!(quiet.committed(), 4);
        assert_eq!(quiet.loudest(), 40, "the same columns carrying a whisper");

        let mut unwritten = AudioEnvelope::default();
        unwritten.push(&[900; 8]);
        assert_eq!(unwritten.committed(), 0);
        assert_eq!(
            unwritten.loudest(),
            0,
            "an uncommitted window has no columns to be loud in"
        );
    }

    #[test]
    fn the_loudest_column_only_reads_what_has_been_committed() {
        let mut envelope = AudioEnvelope::default();
        envelope.push(&vec![9_000; SAMPLES_PER_COLUMN as usize * 2]);
        assert_eq!(envelope.loudest(), 9_000);
        envelope.push(&vec![0; SAMPLES_PER_COLUMN as usize * ENVELOPE_COLUMNS]);
        assert_eq!(
            envelope.loudest(),
            0,
            "the loud column aged out of the window"
        );
    }

    fn to_le_bytes(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn a_byte_stream_folds_into_the_same_envelope_as_the_samples() {
        let samples = full_scale();
        let mut stream = SampleStream::new();
        stream.push_bytes(&to_le_bytes(&samples));
        assert_eq!(stream.envelope().committed(), 1);
        assert_eq!(
            stream.envelope().columns()[0],
            i16::MAX.unsigned_abs(),
            "the byte stream lands the same exact peak as the samples"
        );
    }

    #[test]
    fn a_transfer_boundary_mid_sample_costs_no_samples() {
        let samples = full_scale();
        let bytes = to_le_bytes(&samples);
        let mut whole = SampleStream::new();
        whole.push_bytes(&bytes);
        for offset in (1..bytes.len()).step_by(2) {
            let mut split = SampleStream::new();
            split.push_bytes(&bytes[..offset]);
            split.push_bytes(&bytes[offset..]);
            assert_eq!(
                split.envelope(),
                whole.envelope(),
                "a boundary {offset} bytes in must not change a single column"
            );
        }
    }

    #[test]
    fn a_trailing_odd_byte_waits_for_its_low_byte() {
        let mut stream = SampleStream::new();
        stream.push_bytes(&[0x01, 0x02, 0x03]);
        assert!(
            stream.envelope().mid_window(),
            "one whole sample plus a held high byte is a part-filled window"
        );
        stream.push_bytes(&[0x04]);
        let mut expected = AudioEnvelope::default();
        expected.push(&[
            i16::from_le_bytes([0x01, 0x02]),
            i16::from_le_bytes([0x03, 0x04]),
        ]);
        assert_eq!(stream.envelope(), expected);
    }

    #[test]
    fn a_pendant_byte_survives_an_empty_chunk() {
        let mut stream = SampleStream::new();
        stream.push_bytes(&[0x00, 0x40, 0x12]);
        stream.push_bytes(&[]);
        stream.push_bytes(&[0x34]);
        let mut expected = AudioEnvelope::default();
        expected.push(&[
            i16::from_le_bytes([0x00, 0x40]),
            i16::from_le_bytes([0x12, 0x34]),
        ]);
        assert_eq!(stream.envelope(), expected);
    }

    #[test]
    fn a_chunk_larger_than_a_decode_batch_still_folds() {
        let samples = vec![i16::MAX; SAMPLES_PER_COLUMN as usize * 3];
        let mut stream = SampleStream::new();
        stream.push_bytes(&to_le_bytes(&samples));
        assert_eq!(stream.envelope().committed(), 3);
        assert!(
            stream.envelope().columns()[..3]
                .iter()
                .all(|&c| c == i16::MAX.unsigned_abs())
        );
    }
}

#[cfg(test)]
mod corner_sweep_tests {
    use super::*;
    use alloc::vec::Vec;

    fn walk(codes: u8, poll_ms: u64) -> Vec<CornerStep> {
        let mut sweep = CornerSweep::new(codes);
        let mut steps = Vec::new();
        let mut now = 0;
        loop {
            let step = sweep.step(now);
            let done = step == CornerStep::Done;
            steps.push(step);
            if done {
                return steps;
            }
            now += poll_ms;
        }
    }

    #[test]
    fn every_code_is_written_and_read_in_order() {
        let steps = walk(8, CAPTURE_MS);
        let written: Vec<u8> = steps
            .iter()
            .filter_map(|s| match s {
                CornerStep::Write(code) => Some(*code),
                _ => None,
            })
            .collect();
        assert_eq!(written, alloc::vec![0, 1, 2, 3, 4, 5, 6, 7]);
        let read: Vec<u8> = steps
            .iter()
            .filter_map(|s| match s {
                CornerStep::Read(code) => Some(*code),
                _ => None,
            })
            .collect();
        assert_eq!(read, alloc::vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_code_is_read_only_after_it_has_settled() {
        let mut sweep = CornerSweep::new(2);
        assert_eq!(sweep.step(0), CornerStep::Write(0));
        assert_eq!(sweep.step(1), CornerStep::Settling);
        assert_eq!(sweep.step(CORNER_SETTLE_MS - 1), CornerStep::Settling);
        assert_eq!(sweep.step(CORNER_SETTLE_MS), CornerStep::Read(0));
    }

    #[test]
    fn a_read_is_followed_by_the_next_code_not_another_read() {
        let mut sweep = CornerSweep::new(3);
        assert_eq!(sweep.step(0), CornerStep::Write(0));
        assert_eq!(sweep.step(CORNER_SETTLE_MS), CornerStep::Read(0));
        assert_eq!(sweep.step(CORNER_SETTLE_MS), CornerStep::Write(1));
        assert_eq!(sweep.step(CORNER_SETTLE_MS * 2), CornerStep::Read(1));
        assert_eq!(sweep.step(CORNER_SETTLE_MS * 2), CornerStep::Write(2));
        assert_eq!(sweep.step(CORNER_SETTLE_MS * 3), CornerStep::Read(2));
        assert_eq!(sweep.step(CORNER_SETTLE_MS * 3), CornerStep::Done);
    }

    #[test]
    fn done_is_reported_once_and_never_repeats() {
        let mut sweep = CornerSweep::new(1);
        assert_eq!(sweep.step(0), CornerStep::Write(0));
        assert_eq!(sweep.step(CORNER_SETTLE_MS), CornerStep::Read(0));
        assert_eq!(sweep.step(CORNER_SETTLE_MS), CornerStep::Done);
        assert_eq!(sweep.step(CORNER_SETTLE_MS), CornerStep::Done);
    }

    #[test]
    fn a_part_with_no_codes_finishes_on_the_first_poll() {
        let mut sweep = CornerSweep::new(0);
        assert_eq!(sweep.step(0), CornerStep::Done);
    }

    #[test]
    fn a_clock_that_goes_backwards_waits_rather_than_reading_early() {
        let mut sweep = CornerSweep::new(2);
        assert_eq!(sweep.step(10_000), CornerStep::Write(0));
        assert_eq!(sweep.step(0), CornerStep::Settling);
        assert_eq!(sweep.step(12_999), CornerStep::Settling);
        assert_eq!(sweep.step(13_000), CornerStep::Read(0));
    }

    #[test]
    fn every_read_follows_its_own_write_by_a_full_settle() {
        let mut sweep = CornerSweep::new(8);
        let mut now = 0;
        let mut written_at: Option<u64> = None;
        let mut reads = 0;
        loop {
            match sweep.step(now) {
                CornerStep::Write(code) => {
                    assert_eq!(code, reads, "codes are written in order");
                    written_at = Some(now);
                }
                CornerStep::Read(code) => {
                    assert_eq!(code, reads, "codes are read in order");
                    let gap = now - written_at.expect("a code is written before it is read");
                    assert!(
                        gap >= CORNER_SETTLE_MS,
                        "code {code} read after only {gap} ms"
                    );
                    reads += 1;
                }
                CornerStep::Done => break,
                CornerStep::Settling => {}
            }
            now += CAPTURE_MS;
        }
        assert_eq!(reads, 8);
    }
}

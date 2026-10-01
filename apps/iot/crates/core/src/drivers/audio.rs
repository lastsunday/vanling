use crate::drivers::input::{InputEvent, InputSource};

mod weighting;

use weighting::{AWeight, DbaMeter};

/// The codec's frame rate: one I2S frame per this many nanoseconds, which is
/// what the capture driver clocks the peripheral at and what a frame's worth of
/// samples is counted against.
pub const SAMPLE_RATE_HZ: u32 = 48_000;

/// 16-bit slots one I2S frame carries. The ES7210 is clocked for a standard
/// two-slot frame, and the driver's `Channels::MONO` is a TDM slot *selection*
/// rather than a shorter frame — the peripheral's own mono mode is left off —
/// so the DMA is handed both slots. The capture therefore moves twice as many
/// samples per second as [`SAMPLE_RATE_HZ`] frames.
pub const SLOTS_PER_FRAME: u32 = 2;

/// Bytes one 16-bit sample occupies, little-endian like the wire.
pub const BYTES_PER_SAMPLE: u16 = 2;

/// Bytes one whole I2S frame occupies on the wire: its slots, each one sample
/// wide. The unit a playback ring is measured in, and the reason a mono source
/// has to be widened before it reaches the DMA rather than sized for one slot —
/// see `playback::interleave_mono`.
pub const BYTES_PER_FRAME: usize = SLOTS_PER_FRAME as usize * BYTES_PER_SAMPLE as usize;

/// Capture cadence: how often the input loop polls the capture, and so how much
/// audio can be waiting when it gets there. A whole multiple of the input
/// loop's base tick, like every other source's cadence.
pub const CAPTURE_MS: u64 = 20;

/// Envelope columns one poll closes. Two, so the window is half as long as the
/// poll count would make it: 200 columns at this rate is two seconds of audio,
/// which is long enough to read a phrase and short enough that a single vocal
/// event is not flattened into the noise around it.
pub const COLUMNS_PER_POLL: u16 = 2;

/// Time one envelope column spans.
pub const COLUMN_MS: u64 = CAPTURE_MS / COLUMNS_PER_POLL as u64;

/// Samples one envelope column spans at [`SAMPLE_RATE_HZ`] and
/// [`SLOTS_PER_FRAME`].
pub const SAMPLES_PER_COLUMN: u16 =
    (SAMPLE_RATE_HZ as u64 * SLOTS_PER_FRAME as u64 * COLUMN_MS / 1_000) as u16;

/// Audio one capture poll hands over, in bytes. The unit a capture ring and a
/// backlog are both measured in.
pub const BYTES_PER_POLL: usize = SAMPLE_RATE_HZ as usize
    * SLOTS_PER_FRAME as usize
    * BYTES_PER_SAMPLE as usize
    * CAPTURE_MS as usize
    / 1_000;

/// Envelope columns kept, sized to the pixel columns the panel's sweep is
/// given, so one column is one pixel column and nothing has to be decimated to
/// draw it. Long captures wrap and overwrite the oldest columns, which keeps
/// the type a fixed-size `Copy` payload able to ride an operation intent — the
/// samples themselves never leave the capture driver.
pub const ENVELOPE_COLUMNS: usize = 200;

/// Full scale, as a [`i16::MIN`] absolute: one LSB past the positive rail.
/// 2^15, so its log2 is fifteen whole octaves.
const FULL_SCALE_LOG2_16: i32 = 16 * 15;

/// The same full scale as a peak magnitude, for comparisons that are about
/// amplitude rather than about a bar's position on the band.
pub const FULL_SCALE_LSB: u16 = 1 << 15;

/// The quietest peak the scope draws a bar for, in the same 1/16-octave units:
/// 2^0 = 1 LSB, which is [`SCOPE_FLOOR_DECIBELS`] below the rail. A microphone
/// at conversational distance sits far above this, which is the point of
/// measuring in decibels — a bar scaled linearly to full scale spends ordinary
/// speech in the bottom two rows of the band and reads as a flat line.
const SCOPE_FLOOR_LOG2_16: i32 = 0;

/// The drawn span, in those units: fifteen octaves, i.e. −90 dBFS to 0 dBFS.
const SCOPE_SPAN_LOG2_16: i32 = FULL_SCALE_LOG2_16 - SCOPE_FLOOR_LOG2_16;

/// The window's floor in decibels, which [`dbfs`] bottoms out at. Public because
/// a scale has to be drawn as well as measured: a panel ruling lines at a floor
/// of its own would describe a different window from the one the numbers came
/// from. Ninety decibels is the whole 16-bit range, so a genuinely quiet room —
/// a microphone's own noise floor — lands at the bottom of the band, with the
/// headroom above it left to the levels that share the scale.
pub const SCOPE_FLOOR_DECIBELS: i32 = 90;

/// One doubling of amplitude is this many units.
const LOG2_16_PER_OCTAVE: i32 = 16;

/// Milli-decibels one doubling of amplitude is worth: 20·log₁₀2 rounded, which
/// is what the decibel is *defined* as.
const MILLIDECIBELS_PER_OCTAVE: i64 = 6021;

/// The same in the 1/16-octave units [`log2_16`] counts in.
const MILLIDECIBELS_PER_LOG2_16_UNIT: i64 = MILLIDECIBELS_PER_OCTAVE / 16;

/// Fractional-octave bits the mapping keeps, and so the size of the correction
/// table below.
const MANTISSA_BITS: u32 = 4;

/// The linear mantissa's error, in 1/16ths of an octave, indexed by the mantissa
/// it belongs to: how much `log₂(1 + m/16)` sits *above* the chord through
/// `m/16`, which is nearly half a decibel at the middle of every octave — worst
/// at −20 dBFS, where speech lives. Sixteen bytes of it bring the whole scale
/// within a working tenth of a decibel, and the same table serves the bar and
/// the number.
const LOG2_16_CHORD_ERROR: [i8; 1 << MANTISSA_BITS] =
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0];

/// A peak in LSB as 1/16ths of an octave above one LSB, so [`FULL_SCALE_LOG2_16`]
/// lands on the rail and one LSB — the scope floor,
/// [`SCOPE_FLOOR_LOG2_16`] — on zero. The single place the log scale is defined,
/// so a bar's height and its decibel reading are the same number read two ways
/// and cannot drift apart.
fn log2_16(value: u16) -> i32 {
    if value == 0 {
        // Below the floor rather than at it, which is what silence is: it has to
        // draw no bar and read as the bottom of the window, not as full scale.
        return 0;
    }
    let value = u32::from(value);
    let octave = 31 - value.leading_zeros();
    // Slide the leading one up to the top of the word, so the bits under it are
    // the fraction of the octave whatever the magnitude.
    let normalised = value << (31 - octave);
    let mantissa = ((normalised >> (31 - MANTISSA_BITS)) & ((1 << MANTISSA_BITS) - 1)) as usize;
    LOG2_16_PER_OCTAVE * octave as i32 + mantissa as i32 + LOG2_16_CHORD_ERROR[mantissa] as i32
}

/// A peak in LSB as a `0..=255` bar height, over [`SCOPE_FLOOR_LOG2_16`] up to
/// full scale. Here rather than beside the panel because the mapping has to be
/// checkable: the sweep is the one drawing that can be silently useless, since a
/// bar height that is always zero looks exactly like silence.
pub fn scope_height(peak_lsb: u16) -> u8 {
    let height =
        (log2_16(peak_lsb) - SCOPE_FLOOR_LOG2_16) * i32::from(u8::MAX) / SCOPE_SPAN_LOG2_16;
    height.clamp(0, i32::from(u8::MAX)) as u8
}

/// A peak in LSB as decibels below full scale, to the nearest decibel, never
/// reading below the window's [`SCOPE_FLOOR_DECIBELS`] and never above 0 — the
/// numeric half of what a meter shows, since 3000 LSB is not a level anyone can
/// judge and −21 dBFS is.
pub fn dbfs(peak_lsb: u16) -> i16 {
    let below_rail = FULL_SCALE_LOG2_16 - log2_16(peak_lsb);
    let milli = i64::from(below_rail.max(0)) * MILLIDECIBELS_PER_LOG2_16_UNIT;
    let decibels = -(((milli + 500) / 1_000) as i32);
    decibels.clamp(-SCOPE_FLOOR_DECIBELS, 0) as i16
}

/// A [`dbfs`] reading as a sound pressure level: the same decibel count with the
/// microphone's own reference added back on, so 0 dBFS reads as whatever the
/// hardware calls full scale instead of as a number only this code can judge.
/// The offset is the only part that varies between setups, so it is a parameter
/// rather than a constant here — a board knows its microphone and its gain, this
/// module does not, and the two cannot be derived from the window either.
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

/// The peak a bar is drawn at this column: its own, or the one before it after
/// one step of the release ballistics, whichever is higher. The falloff a meter
/// shows is never the signal's own — tracking it exactly would be a waveform,
/// and holding the bar a few columns is what makes a transient legible at all.
/// The step is at least one LSB, so a fall can clear a ~15-LSB fixed point and
/// a long silence reads as the bottom of the band rather than as a whisper the
/// window keeps on remembering.
pub fn released_peak(measured: u16, previous: u16) -> u16 {
    let floor = previous.saturating_sub((previous >> RELEASE_SHIFT).max(1));
    measured.max(floor)
}

/// Peak envelope of a capture: one column per [`SAMPLES_PER_COLUMN`] samples, each
/// the loudest absolute sample in its window, wrapping once full. Columns hold the
/// peak in LSB rather than a fraction of full scale, so a driver's readback is
/// comparable with its datasheet numbers; the loudness scale belongs to the panel
/// (see [`scope_height`]). The RMS of the same window rides beside the peak, and
/// sliding rather than growing keeps the envelope a fixed-size `Copy`.
///
/// Every column also keeps its A-weighted twin ([`weighting`]): the raw columns
/// stay the bench reading, the twin is the level a listener would call it, so a
/// low-frequency codec floor draws as a flat line instead of a solid band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioEnvelope {
    columns: [u16; ENVELOPE_COLUMNS],
    /// The same peaks A-weighted, per column.
    weighted_columns: [u16; ENVELOPE_COLUMNS],
    /// Root mean square of each column's samples, in LSB. Same window, same
    /// wrapping, same units — only the statistic differs.
    rms: [u16; ENVELOPE_COLUMNS],
    /// The same RMS A-weighted, per column.
    weighted_rms: [u16; ENVELOPE_COLUMNS],
    /// Index the next completed column writes to.
    cursor: u16,
    /// Columns of history available, clamped to [`ENVELOPE_COLUMNS`] so it
    /// answers "how much of the window is real" and stops counting once the
    /// first wrap has filled it.
    committed: u8,
    /// Loudest sample in the window being filled. The clip latch reads this raw
    /// value — a rail hit is about the codec, not the filter.
    peak: u16,
    /// Loudest A-weighted sample in the window being filled.
    weighted_peak: u16,
    /// Root mean square accumulator for the window being filled.
    sum_squares: u64,
    /// A-weighted RMS accumulator for the window being filled.
    weighted_sum_squares: u64,
    /// The A-weighting filter the weighted columns are folded through.
    weight: AWeight,
    /// The dBA number, riding the same weighted samples the weighted columns
    /// are folded from. It lives here because that is where the one cascade
    /// already runs — see [`DbaMeter`] for why it owns no filter of its own.
    dba: DbaMeter,
    /// Samples folded into the current window.
    filled: u16,
    /// Latched by any column that reached [`FULL_SCALE_LSB`]. A clip is an
    /// absolute statement that the signal was distorted somewhere, not a
    /// statement about the level right now, so it is kept for as long as this
    /// capture exists rather than scrolling away with the column that saw it.
    clipped: bool,
    /// Columns committed since the one that last clipped, so the panel can point
    /// at *when* it happened while the column is still in the window. A count
    /// rather than an index, which would have to be reasoned about against the
    /// wrap; cleared once the marked column is overwritten.
    columns_since_clip: Option<u16>,
}

impl Default for AudioEnvelope {
    fn default() -> Self {
        Self::ZERO
    }
}

impl AudioEnvelope {
    /// A blank envelope: nothing committed, every column zero. Named rather
    /// than derived because a 200-column array has no `Default` impl, and the
    /// state layer needs it in a `const` context.
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

    /// Folds samples in, closing and advancing a column every
    /// [`SAMPLES_PER_COLUMN`]. A push longer than one window carries the
    /// remainder into the next column instead of dropping it.
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

    /// Commits the window being filled: one column of peak and one of RMS, raw
    /// and A-weighted.
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
            // This column is one older than it was, and the one at the far end of
            // the window is about to be overwritten.
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

    /// The A-weighted level as an RMS in LSB, saturating after the sample the
    /// meter last folded. Zero until the first samples arrive, and it settles
    /// back toward it after the room goes quiet — the fall itself is the meter.
    pub const fn dba_lsb(&self) -> u16 {
        self.dba.level_lsb()
    }

    /// The whole window, oldest column first. Columns past [`Self::committed`]
    /// are still zero on a fresh envelope.
    pub fn columns(&self) -> &[u16; ENVELOPE_COLUMNS] {
        &self.columns
    }

    /// The whole A-weighted window, raw peaks through the filter, same layout as
    /// [`columns`](Self::columns).
    pub fn weighted_columns(&self) -> &[u16; ENVELOPE_COLUMNS] {
        &self.weighted_columns
    }

    /// One column by age, `0` being the oldest still in the window, from whichever
    /// column set the reader is drawing. Ages past what has been committed read
    /// as silence, which is what a column that has not been drawn yet should weigh.
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

    /// The bar heights the panel draws, oldest column first, with the release
    /// ballistics of [`released_peak`] threaded through in time order. Derived
    /// from the window rather than accumulated alongside it, so a repaint that
    /// happens twice, or not at all, cannot change what a bar reads.
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

    /// Each column's RMS in LSB, oldest first, for the inner bar that tells a
    /// sustained level from a lone transient.
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

    /// The window's loudest peak in LSB, the peak-hold a readout shows; zero
    /// until the first column commits. The column array alone cannot tell
    /// "nothing has arrived" from "silence arrived": both draw the same flat
    /// sweep.
    pub fn loudest(&self) -> u16 {
        self.columns[..usize::from(self.committed)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// The window's loudest RMS column, in LSB — the sustained level a panel
    /// reads as loudness. A maximum, so it is the wrong statistic for a floor:
    /// [`floor_rms`](Self::floor_rms) answers that.
    pub fn loudest_rms(&self) -> u16 {
        self.rms_columns()[..usize::from(self.committed)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// The window's median RMS column, in LSB — a floor rather than a peak.
    ///
    /// Median rather than a power mean on purpose: the mean is quadratic, so one
    /// transient column at sixteen times the room's level drags it up with 256×
    /// the energy, while the median ignores a lone loud column — and still moves
    /// when a low-frequency corner shifts every column at once.
    ///
    /// Zero for a window that has committed nothing.
    pub fn floor_rms(&self) -> u16 {
        let committed = usize::from(self.committed);
        if committed == 0 {
            return 0;
        }
        // Sorted on a copy: the window is still the capture's, and a diagnostic
        // that reordered it would change the very reading it came for. The copy
        // is the same one `rms_columns` already hands out.
        let mut window = self.rms_columns();
        window[..committed].sort_unstable();
        window[committed / 2]
    }

    /// Whether any column has reached full scale since this capture started.
    pub fn clipped(&self) -> bool {
        self.clipped
    }

    /// How many columns back the last full-scale column is, while it is still in
    /// the window — where on the sweep to mark it.
    pub fn clipped_age(&self) -> Option<usize> {
        let age = usize::from(self.columns_since_clip?);
        (age < usize::from(self.committed)).then_some(age)
    }

    /// Columns of history the window holds, clamped to [`ENVELOPE_COLUMNS`].
    pub fn committed(&self) -> u8 {
        self.committed
    }

    /// True while a window is partly filled and its column is still the
    /// previous value, so a panel drawing on poll rather than on sample count
    /// shows the newest bar a poll late.
    pub fn mid_window(&self) -> bool {
        self.filled > 0
    }
}

/// Samples decoded per [`SampleStream::push_bytes`] call, so folding a chunk
/// never needs a buffer sized to the chunk.
const DECODE_BATCH: usize = 64;

/// Folds a captured byte stream into an envelope: a capture arrives as raw
/// little-endian 16-bit samples, and a transfer boundary can fall between the two
/// bytes of one sample. Beside the envelope rather than next to the codec, so the
/// stitching is host-testable — the byte stream is the same whatever peripheral
/// produced it. The same samples also fold into the A-weighting meter, so a
/// dBA readout and the scope agree on what the room did, not just where two
/// windows happened to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleStream {
    envelope: AudioEnvelope,
    /// High byte of a sample whose low byte had not arrived yet.
    pending: Option<u8>,
}

impl SampleStream {
    pub const fn new() -> Self {
        Self {
            envelope: AudioEnvelope::ZERO,
            pending: None,
        }
    }

    /// Folds a run of little-endian 16-bit samples in, holding a trailing odd
    /// byte until the next call: a transfer hands out whatever its descriptor
    /// boundary happened to be, and that is rarely a sample boundary.
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
        for pair in rest.chunks_exact(2) {
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

    /// The envelope as folded so far — the panel's scope sweep. `committed()`
    /// on it doubles as a liveness signal, since it advances once per column
    /// whether or not anything was loud.
    pub const fn envelope(&self) -> AudioEnvelope {
        self.envelope
    }

    /// The A-weighted level as an RMS in LSB, saturating after the sample the
    /// meter last folded. Zero until the first bytes arrive, and it settles
    /// back toward it after the room goes quiet — the fall itself is the meter.
    pub const fn dba_lsb(&self) -> u16 {
        self.envelope.dba_lsb()
    }
}
impl Default for SampleStream {
    fn default() -> Self {
        Self::new()
    }
}

/// How long the DMA needs to fill a whole ring, and so the longest a healthy
/// capture can stay silent before it is fair to call it stalled. Measured in
/// [`BYTES_PER_POLL`] rather than in frames, because a ring is drained on the
/// poll and the rate the wire moves bytes at is two slots a frame, not one.
const fn ring_fill_ms(ring_bytes: usize) -> u64 {
    ring_bytes as u64 * CAPTURE_MS / BYTES_PER_POLL as u64
}

/// Notices a capture whose DMA has stopped, by the clock rather than the DMA: the
/// peripheral's own flag reports *its* completion, not the descriptor chain
/// running dry, so time is the only signal that holds either way. A ring is
/// drained every poll, so silence for longer than one ring's fill means the
/// engine is not running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureWatchdog {
    /// When the capture last handed over a byte, or the first poll that found
    /// nothing — the deadline a capture gets before it has to prove itself.
    last_progress_ms: Option<u64>,
    /// Silence tolerated before a stall is called: one ring's fill — the longest
    /// a capture can be late and still recoverable — plus the poll period.
    silence_limit_ms: u64,
}

impl CaptureWatchdog {
    /// Watches a capture whose ring is `ring_bytes` long. Unarmed until the
    /// first poll, so a driver can be built before it has a clock.
    pub const fn new(ring_bytes: usize) -> Self {
        Self {
            last_progress_ms: None,
            silence_limit_ms: ring_fill_ms(ring_bytes) + CAPTURE_MS,
        }
    }

    /// Whether the capture has gone quiet for too long, given what the last
    /// poll found waiting. Any byte at all counts as progress and pushes the
    /// deadline out again.
    pub fn stalled(&mut self, now_ms: u64, produced_bytes: usize) -> bool {
        if produced_bytes > 0 {
            self.last_progress_ms = Some(now_ms);
            return false;
        }
        let last = *self.last_progress_ms.get_or_insert(now_ms);
        now_ms.saturating_sub(last) > self.silence_limit_ms
    }

    /// Pushes the deadline out without a capture to show for it, for a capture
    /// that has just been re-armed and has not been polled since.
    pub fn note_progress(&mut self, now_ms: u64) {
        self.last_progress_ms = Some(now_ms);
    }
}

/// Notices a capture whose input loop is not keeping up with the codec, by how
/// deep the ring is rather than by whether anything was lost.
///
/// A healthy capture is not a ring that drains to nothing: the loop polls on a
/// cadence of its own, so a poll that arrives on time finds about one poll's
/// worth of audio waiting, and a loop that runs a fixed beat behind the wire
/// settles a whole poll deeper than that for ever. So the depth that matters is
/// not any particular number but where the ring sits between its steady state and
/// its limit, and a warning has to sit clear of the steady state or it fires
/// continuously and says nothing — which is what a line at two polls did, on a
/// ring that holds three.
///
/// Levels are in [`BYTES_PER_POLL`] for the same reason [`ring_fill_ms`] is:
/// a ring is drained on the poll, and the wire moves two slots a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureBacklog {
    /// Depth at which a backlog is first called, above the steady state and
    /// inside the ring: two and a half polls, so an input loop that has fallen a
    /// poll and a half behind is reported while there is still half a poll of
    /// ring left to absorb it.
    warn_bytes: usize,
    /// Depth at which the backlog is called over, back to the steady state's own
    /// two polls, so a ring that dips to a normal depth counts as recovered.
    clear_bytes: usize,
    /// Whether the last depth reported was over the line, so a ring that stays
    /// deep is reported once rather than on every poll.
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
    pub const fn warn_bytes(&self) -> usize {
        self.warn_bytes
    }

    /// The depth that closes one.
    pub const fn clear_bytes(&self) -> usize {
        self.clear_bytes
    }

    /// Reports the depth a poll found, and gives back the depth only on the poll
    /// that opens a backlog — a driver logs the one, not the state.
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

    /// Whether a backlog is currently called, for a driver that reports state
    /// rather than the moment it opened.
    pub const fn backlogged(&self) -> bool {
        self.backlogged
    }
}

/// One capture poll: the peak envelope every consumer may read, plus how long the
/// capture has been running. A summary, not a transcript — the raw samples stay
/// in the capture driver, so a snapshot stays small enough to broadcast like
/// every other diagnostics payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioSample {
    pub envelope: AudioEnvelope,
    /// Capture wall time (ms) the source has been running, saturating; the
    /// panel reads it as elapsed time while a recording runs.
    pub elapsed_ms: u32,
    /// How many times the capture has had to be re-armed because its DMA
    /// stopped. Read beside `committed`, which a stalled capture stops climbing
    /// while a silent one keeps climbing.
    pub restarts: u16,
    /// The A-weighted level of the same capture, as an RMS in LSB. Carried
    /// beside the envelope rather than derived from it, because an A-weighting
    /// is a filter — it has to see the samples, not the squares a window kept.
    pub dba_lsb: u16,
}

/// The capture plane a driver hands to core: whatever arrived since the last
/// poll, already folded into the peak envelope. Deliberately not fallible — a
/// short or missed read is silence as far as a scope is concerned, and a
/// condition with no correct response has no business in a retry path.
pub trait AudioSource {
    fn sample(&mut self, now_ms: u64) -> AudioSample;
}

/// Adapts an [`AudioSource`] onto the shared input interface, so a board hands
/// the app a ready-made poll entry and no codec type crosses into core. The
/// shape mirrors [`MotionInput`](crate::drivers::motion::MotionInput): the driver
/// stays in the bsp crate, only its summary comes through.
pub struct AudioInput<S> {
    source: S,
}

impl<S> AudioInput<S> {
    pub const fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: AudioSource> InputSource for AudioInput<S> {
    fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        // Every cadence tick publishes: a scope that only moved on new samples
        // would freeze between speech, and a flat line is the honest reading
        // during silence.
        Some(InputEvent::Audio(self.source.sample(now_ms)))
    }
}

/// How long a corner change is given to settle before the envelope covering it
/// is read. A DC-blocking filter's transient is a fraction of a millisecond, but
/// the reading that matters is a level the room keeps producing, so the wait is
/// set by the envelope rather than by the filter: three seconds is 150 capture
/// polls, far more columns than [`ENVELOPE_COLUMNS`] holds, so the whole window
/// behind the reading postdates the change.
pub const CORNER_SETTLE_MS: u64 = 3_000;

/// What a corner walk wants the capture to do on this poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerStep {
    /// Nothing yet: the corner written last is still settling.
    Settling,
    /// Write this corner code into the filter.
    Write(u8),
    /// The corner has been settled long enough; report the envelope's level as
    /// this code's reading.
    Read(u8),
    /// Every code has been read. The caller puts its own corner back and stops
    /// asking.
    Done,
}

/// A one-shot walk of a filter's corner codes, so a board can report what each
/// corner reads on the real room rather than which of them it expected to.
///
/// The sequencing is here and the corner codes are not, because the codes are a
/// property of the part and the walk is arithmetic on a clock — so the walk can
/// be exercised on the host, where the part's own driver is testable but this
/// capture path is not (it is built from DMA types no host can name). It is
/// driven one poll at a time by [`step`](Self::step) and holds no state the
/// caller has to keep in step with it.
#[derive(Debug, Clone, Copy)]
pub struct CornerSweep {
    /// How many codes the part has. A walk of none is finished on the first
    /// step, so a part that reports no codes is walked harmlessly.
    codes: u8,
    /// The next code to write; reaching `codes` ends the walk.
    next: u8,
    /// The code written and not yet read, if any.
    reading: Option<u8>,
    /// When that code was written.
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

    /// Advances the walk by one poll and says what to do. The caller writes on
    /// [`Write`](CornerStep::Write), reports `envelope`'s level on
    /// [`Read`](CornerStep::Read), and on [`Done`](CornerStep::Done) restores
    /// the corner it started from and drops the walk — it never reports
    /// [`Done`](CornerStep::Done) twice, so a caller that stops asking loses
    /// nothing.
    pub fn step(&mut self, now_ms: u64) -> CornerStep {
        if let Some(code) = self.reading {
            // Saturating, so a poll arriving with a clock that has gone backwards
            // waits rather than reading a level the corner had not reached yet.
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
    use alloc::vec::Vec;

    struct FakeSource {
        calls: u32,
        amplitude: i16,
        restarts: u16,
    }

    impl AudioSource for FakeSource {
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

    #[test]
    fn every_cadence_tick_publishes_a_capture_poll() {
        let mut input = AudioInput::new(FakeSource {
            calls: 0,
            amplitude: 8_000,
            restarts: 7,
        });
        // Silence has to keep publishing: a scope that only moved on new samples
        // would freeze between speech instead of showing a flat line.
        for now_ms in [20, 40, 60] {
            match input.poll(now_ms) {
                Some(InputEvent::Audio(sample)) => {
                    assert_eq!(sample.envelope.committed(), 1);
                    assert_eq!(sample.elapsed_ms, now_ms as u32);
                    // A stalled capture the driver repaired still has to say so,
                    // or the panel can only see its envelope stop advancing.
                    assert_eq!(sample.restarts, 7);
                }
                other => panic!("expected an audio poll, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_source_sees_the_clock_the_entry_was_polled_at() {
        let mut input = AudioInput::new(FakeSource {
            calls: 0,
            amplitude: 0,
            restarts: 0,
        });
        input.poll(1_234);
        assert_eq!(
            input.source.calls, 1,
            "one poll, one read of the capture ring"
        );
    }

    /// The ring the S3 capture runs on: three polls of audio, so the watchdog
    /// tolerates 80 ms of silence — a full ring's fill plus a poll — before
    /// calling a stall.
    const RING_BYTES: usize = BYTES_PER_POLL * 3;

    /// A whole poll period of audio, what a healthy capture hands over each time.
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
        // The failure this exists for: a DMA that filled the ring before the
        // consumer polled and stopped, which no amount of draining revives.
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

    /// The deadline a ring's size buys, derived rather than quoted: a driver that
    /// grows its ring has to get a proportionally longer grace period with it, or
    /// a perfectly healthy capture is called dead every few polls.
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
        // Re-arming hands back an empty ring, so the new stream has to be given
        // the same grace the first one had rather than judged against a
        // deadline the dead one left behind.
        watchdog.note_progress(200);
        assert!(!watchdog.stalled(240, 0), "the new stream starts unarmed");
    }

    #[test]
    fn a_capture_that_never_produced_anything_is_still_given_its_grace() {
        // A clock or a codec that never started reads the same as a stopped
        // DMA, and the watchdog is the only thing that can tell the panel — so
        // it has to start counting from the first poll, not from nothing.
        let mut watchdog = CaptureWatchdog::new(RING_BYTES);
        assert!(!watchdog.stalled(500, 0), "the first poll only arms it");
        assert!(watchdog.stalled(600, 0), "and silence past that is a stall");
    }

    #[test]
    fn the_depth_a_healthy_capture_settles_at_is_not_called_a_backlog() {
        // The regression this rule exists for. A poll that arrives on time finds
        // about one poll of audio waiting, and a loop running a fixed beat behind
        // the wire settles a whole poll deeper than that for ever — so a line at
        // two polls sits exactly on the steady state and fires on every poll, on
        // a ring that holds three and was never filling. The depths below are
        // what the panel actually reported while the ring stayed healthy.
        let mut backlog = CaptureBacklog::new(RING_BYTES);
        for produced in [7_708, 7_934, 8_188] {
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
        // Between the two levels the backlog stands: a ring dipping to just under
        // the warn line has not recovered, it is still behind.
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
        // Quoted rather than derived, because both numbers come from hardware
        // that is not here: the steady state is what the panel reported, and the
        // ring is what the driver sizes. A warn level at or under the steady
        // state would warn continuously again, and a level with no ring left above
        // it would warn too late to be worth anything.
        let backlog = CaptureBacklog::new(RING_BYTES);
        assert_eq!(backlog.warn_bytes(), 9_600);
        assert_eq!(backlog.clear_bytes(), 7_680);
        assert!(
            backlog.warn_bytes() > 8_188,
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

    /// One LSB past the positive rail — the absolute of [`i16::MIN`], and so the
    /// loudest peak a column can hold.
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
        // Why the scale is logarithmic: speech is a few thousand LSB, which a
        // linear bar draws in the bottom two rows of the half-band.
        assert!(
            scope_height(3_000) > u8::MAX / 3,
            "−21 dBFS has to be plainly visible, not a sliver"
        );
        assert!(
            scope_height(1_000) > u8::MAX / 6,
            "−30 dBFS still has to read as a bar"
        );
        assert_eq!(scope_height(32), 85, "−60 dBFS, a third of the −90 dB band");
        // 64 LSB is −54 dBFS, thirty-six decibels over a ninety-decibel window,
        // so two-fifths of the band: the floor is low enough to show a quiet room.
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
        // The reason a meter shows decibels: 3000 is a number only this code can
        // interpret. −21 dBFS is one every audio tool already reports.
        assert_eq!(dbfs(PEAK_FULL_SCALE), 0, "full scale is the rail");
        assert_eq!(dbfs(0), -90, "silence reads the floor, not an infinity");
        assert_eq!(dbfs(1), -90, "and so does the floor itself");
        assert_eq!(dbfs(PEAK_FULL_SCALE / 10), -20, "a tenth of full scale");
        assert_eq!(dbfs(PEAK_FULL_SCALE / 2), -6, "half scale is −6 dB");
    }

    #[test]
    fn dbfs_agrees_with_the_band_it_is_drawn_on() {
        // The reading and the bar beside it are one mapping, so they must never
        // put the level in two places.
        for peak in (0..=PEAK_FULL_SCALE).step_by(37) {
            let decibels = i32::from(dbfs(peak));
            if decibels < -SCOPE_FLOOR_DECIBELS {
                continue;
            }
            // Height is linear in decibels, so a reading and a bar must agree to
            // within the two decibel steps a 1/16-octave mantissa can move.
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
        // The two readings are the same signal counted from different zeros, so
        // the offset has to be the whole of the difference: anything else in
        // between means the two columns are no longer the same measurement. The
        // offset is this board's own (`SPL_OFFSET_DECIBELS` in `bsp-esp`), restated
        // here because the core cannot depend on the board that carries the part.
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
        // A level meter that fell as the room got louder would be a bug the
        // reading alone would not reveal, so the same walk dBFS makes is made
        // here rather than spot-checked at the ends.
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
        // The release is the difference between a level meter and a waveform: a
        // bar that fell as fast as the signal did would show only the attack and
        // lose the whole of every syllable after it.
        let measured = 8_000;
        assert_eq!(released_peak(measured, 0), measured, "a first column");
        let after_one = released_peak(0, measured);
        assert!(after_one < measured, "a silent column falls");
        // A sixteenth down, and no more than a tenth: the shift truncates, so the
        // fall rounds *down* and a bar that collapsed in one column would show
        // attacks and nothing else.
        let fall = u32::from(measured - after_one);
        assert!(
            fall * 16 >= u32::from(measured) && fall * 10 < u32::from(measured),
            "fell {fall} of {measured} in one column, wanted between a tenth and a sixteenth"
        );
    }

    #[test]
    fn a_release_never_lifts_a_bar_above_its_own_column() {
        // The falloff is a floor, not a value: a column that is louder than the
        // bar in front of it has to be drawn, or a rising signal would be
        // flattened into a flat line and the meter would stop being a meter.
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
        // The requirement the release rate was derived from: a transient must be
        // back at the floor before the window scrolls it off.
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
        // And not so fast that it is useless either: the attack it follows has to
        // survive a column or two to be seen at all.
        assert!(
            scope_height(released_peak(0, PEAK_FULL_SCALE)) > 0,
            "the peak has to still be on the band one column after it was heard"
        );
    }

    #[test]
    fn the_rms_of_a_column_reads_the_level_not_the_loudest_sample() {
        // The whole reason the two are drawn as separate bars: a column of one
        // loud sample and a column of steady sound at the same peak look identical
        // as a peak, and are not remotely the same level.
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
        // The capture folds both I2S slots, so a signal alternating between rails
        // must read as one full-scale column: not one LSB off full scale because
        // the two slots happen to be the two's-complement halves of each other.
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
        // Four columns: three at 1_000 and one at 10_000, so the loudest column
        // is ten times the rest. A maximum reports 10_000; the floor must report
        // 1_000, because the one loud column is an event and not the level.
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
        // The robustness the median is here for, and its exact limit: one loud
        // column among eight is ignored outright, because the window's own middle
        // does not change. Push past half the window and the floor has to follow,
        // or it would report a level the room mostly is not producing.
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

        // Seven more makes it eight loud against eight quiet, and the median
        // lands on the first loud column.
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
        // `committed` saturates, so a window that is not yet full still has its
        // uncommitted slots at zero. Taking the middle of the whole array rather
        // than of the committed part would put those zeros in the middle and
        // report a floor of nothing at all for the first two seconds of a
        // capture — which is exactly the window the panel shows first.
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
        // Every column the same, so the median is that column: the floor of a
        // steady room is the room.
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

    /// A few whole columns of a sine at `freq_hz`, in LSB.
    fn tone_buffer(freq_hz: f64, amplitude: i32, samples: usize) -> Vec<i16> {
        assert_eq!(samples % SAMPLES_PER_COLUMN as usize, 0, "whole columns");
        let step = 2.0 * core::f64::consts::PI * freq_hz / f64::from(SAMPLE_RATE_HZ);
        (0..samples)
            .map(|i| (f64::from(amplitude) * (step * i as f64).sin()) as i16)
            .collect()
    }

    /// A-weighting cuts 62.5 Hz by about 26 dB, so a low tone must not draw a
    /// solid band even when it saturates the raw columns.
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

    /// A-weighting is unity at 1 kHz, so a mid-band tone reads the same through
    /// both column sets.
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
        // Once the marked column has been overwritten there is nothing left to
        // point at, so the mark goes with it: a position in a window that no
        // longer contains it would be a claim about a bar that isn't there.
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
        // The sweep reads oldest at the left, so the release has to run from the
        // left of the window forward. A release applied in storage order would
        // run it the other way and draw a falling edge where the sound rose.
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
        // The number and the picture are one measurement: release only pulls a
        // bar down, so the tallest bar can neither exceed the loudest peak nor
        // be dragged below it.
        let mut envelope = AudioEnvelope::default();
        // Loud, quiet, loud again, then a run of silence long enough for the
        // release to fall most of the way: the cases where a released chain and
        // a plain maximum could come apart.
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
        // The reading that decides how long a phrase stays on screen: 200 columns
        // at ten milliseconds each. Derived from the constants that produce the
        // samples so it cannot drift from them.
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
    fn the_loudest_column_tells_a_silent_capture_from_an_empty_one() {
        // Both draw a flat sweep, which is why the panel reads this beside the
        // column count: committed still climbing with a loudest of zero means
        // the capture path is dead, not that the room is quiet.
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
        // Wrap the window so the loud column is overwritten, and the answer has
        // to follow the window rather than remember a peak from earlier.
        envelope.push(&vec![0; SAMPLES_PER_COLUMN as usize * ENVELOPE_COLUMNS]);
        assert_eq!(
            envelope.loudest(),
            0,
            "the loud column aged out of the window"
        );
    }

    /// The bytes a transfer hands out for `samples`, little-endian like the wire.
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
        // Every odd offset puts a boundary between the two bytes of one sample.
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
        // Two samples with two different values, so the comparison also pins
        // that the held byte was paired with *this* low byte rather than read
        // as a whole sample of its own.
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
        // Three windows' worth of bytes, arriving as one oversized chunk, so
        // the batch loop has to wrap rather than truncate.
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

    /// Runs the walk to completion the way a capture would, collecting what each
    /// poll asked for.
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
        // One millisecond short of the settle is still settling, and keeps
        // waiting across as many polls as it takes.
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
        // `saturating_sub` makes this a settle time of zero, so it waits —
        // reading here would report a level the corner had not reached. The read
        // lands a full settle *after the write* still, not after the old clock.
        assert_eq!(sweep.step(0), CornerStep::Settling);
        assert_eq!(sweep.step(12_999), CornerStep::Settling);
        assert_eq!(sweep.step(13_000), CornerStep::Read(0));
    }

    #[test]
    fn every_read_follows_its_own_write_by_a_full_settle() {
        // The whole point of the walk is that each reading is a settled room, so
        // this is the property worth pinning: the gap between a code being
        // written and being read is at least one settle window, measured on the
        // poll clock rather than assumed from the step count.
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

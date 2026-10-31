//! Playback: the sounds a device can make, the sample sources behind them, and the surface
//! a board hands the app to make them on.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::drivers::audio::{BYTES_PER_FRAME, SAMPLE_RATE_HZ};

/// A sound in the catalogue, in the order a tap cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    #[default]
    Tap,
    Notice,
    Warn,
    Error,
    Rise,
    Sweep,
}

impl Sound {
    /// The whole catalogue in cycle order, so a walker and a test cannot disagree about
    /// what the tap after the last sound does.
    pub const ALL: [Self; 5] = [Self::Tap, Self::Notice, Self::Warn, Self::Error, Self::Rise];

    /// The next sound in the catalogue, wrapping: what one tap on the Speaker page plays.
    pub const fn next(self) -> Self {
        match self {
            Self::Tap => Self::Notice,
            Self::Notice => Self::Warn,
            Self::Warn => Self::Error,
            Self::Error => Self::Rise,
            Self::Rise => Self::Tap,
            Self::Sweep => Self::Tap,
        }
    }
}

pub trait SampleSource: Send {
    fn fill(&mut self, out: &mut [i16]) -> usize;

    fn done(&self) -> bool;
}

/// Widens mono frames into the bytes the I2S DMA transmits: each frame is
/// [`crate::drivers::audio::SLOTS_PER_FRAME`] slots wide and carries the same sample in
/// every one, because a mono source into a two-slot frame has to look stereo to the codec
/// or only half the frame would be ours.
pub fn interleave_mono(frames: &[i16], out: &mut [u8]) -> usize {
    let mut written = 0;
    for (&frame, slots) in frames.iter().zip(out.as_chunks_mut::<BYTES_PER_FRAME>().0) {
        for slot in slots.as_chunks_mut::<2>().0 {
            slot.copy_from_slice(&frame.to_le_bytes());
        }
        written += BYTES_PER_FRAME;
    }
    written
}

/// One note as the synthesiser reads it: its frequency, how long it lasts, and the ramp
/// each end gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub hz: u16,
    pub ms: u16,
    pub ramp_ms: u16,
}

/// Longest ramp a note gets, however long it is: a note three seconds long ramped for a
/// quarter of itself spends most of its length fading rather than sounding.
pub const RAMP_MAX_MS: u16 = 40;

/// The ramp a parsed note gets at each end: a quarter of its length, capped.
pub const fn ramp_of(ms: u16) -> u16 {
    let quarter = ms / 4;
    if quarter > RAMP_MAX_MS {
        RAMP_MAX_MS
    } else {
        quarter
    }
}

const DEFAULT_US_PER_QUARTER: u32 = 500_000;

const A4_HZ: f64 = 440.0;
const A4_NOTE: i32 = 69;

fn hz_of(note: u8) -> u16 {
    let hz = A4_HZ * libm::pow(2.0, f64::from(i32::from(note) - A4_NOTE) / 12.0);
    (hz + 0.5).clamp(1.0, f64::from(u16::MAX)) as u16
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn done(&self) -> bool {
        self.bytes.is_empty()
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.bytes.len() < n {
            return None;
        }
        let (head, tail) = self.bytes.split_at(n);
        self.bytes = tail;
        Some(head)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn expect(&mut self, tag: &[u8; 4]) -> Option<()> {
        if self.take(4)? == tag { Some(()) } else { None }
    }

    // A variable-length quantity: seven bits per byte, high bit meaning another
    // byte follows. Four bytes is what the format allows, which is 28 bits —
    // far past any duration an earcon can express.
    fn vlq(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.take(1)?[0];
            value = (value << 7) | u32::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
}

struct Sounding {
    start: u32,
    note: u8,
    length: u32,
}

struct Tempo {
    tick: u32,
    us_per_quarter: u32,
}

struct Score {
    notes: Vec<Sounding>,
    tempos: Vec<Tempo>,
}

impl Score {
    fn ms_of(tempos: &[Tempo], tick: u32, ticks: u32, division: u32) -> u32 {
        let us_per_quarter = tempos
            .iter()
            .rev()
            .find(|tempo| tempo.tick <= tick)
            .map_or(DEFAULT_US_PER_QUARTER, |tempo| tempo.us_per_quarter);
        ticks.saturating_mul(us_per_quarter) / division / 1_000
    }

    fn read_track(&mut self, track: &mut Reader<'_>, base: u32) -> Option<u32> {
        let mut tick = 0u32;
        let mut running = 0u8;
        let mut open: Vec<(u8, u32)> = Vec::new();
        while !track.done() {
            tick = tick.saturating_add(track.vlq()?);
            let mut status = track.take(1)?[0];
            if status < 0x80 {
                if running == 0 {
                    return None;
                }
                status = running;
            }
            match status & 0xf0 {
                0x80 | 0x90 => {
                    let note = track.take(1)?[0];
                    let velocity = track.take(1)?[0];
                    running = status;
                    if status & 0xf0 == 0x90 && velocity != 0 {
                        open.push((note, tick));
                    } else if let Some(index) = open.iter().position(|&(n, _)| n == note) {
                        let (_, start) = open.remove(index);
                        self.notes.push(Sounding {
                            start,
                            note,
                            length: tick.saturating_sub(start),
                        });
                    }
                }
                0xa0 | 0xb0 | 0xe0 => {
                    running = status;
                    track.take(2)?;
                }
                0xc0 | 0xd0 => {
                    running = status;
                    track.take(1)?;
                }
                0xf0 => {
                    running = 0;
                    match status {
                        0xff => {
                            let kind = track.take(1)?[0];
                            let len = track.vlq()? as usize;
                            let data = track.take(len)?;
                            if kind == TEMPO_META && data.len() == 3 {
                                let us = (u32::from(data[0]) << 16)
                                    | (u32::from(data[1]) << 8)
                                    | u32::from(data[2]);
                                self.tempos.push(Tempo {
                                    tick: base.saturating_add(tick),
                                    us_per_quarter: us.max(1),
                                });
                            }
                        }
                        _ => {
                            let len = track.vlq()? as usize;
                            track.take(len)?;
                        }
                    }
                }
                _ => return None,
            }
        }
        Some(tick)
    }

    fn into_notes(self, division: u32) -> Vec<Note> {
        let Self {
            notes: mut sounded,
            tempos,
        } = self;
        sounded.sort_by_key(|note| note.start);
        let mut notes = Vec::with_capacity(sounded.len() * 2);
        let mut cursor = 0u32;
        for note in sounded {
            if note.start > cursor {
                let rest = Self::ms_of(&tempos, cursor, note.start - cursor, division);
                push_rest(&mut notes, rest);
            }
            let ms = Self::ms_of(&tempos, note.start, note.length, division);
            push_note(&mut notes, hz_of(note.note), ms);
            cursor = note.start.saturating_add(note.length);
        }
        notes
    }
}

const TEMPO_META: u8 = 0x51;

fn push_rest(notes: &mut Vec<Note>, ms: u32) {
    if ms != 0 {
        notes.push(Note {
            hz: 0,
            ms: to_ms(ms),
            ramp_ms: 0,
        });
    }
}

fn push_note(notes: &mut Vec<Note>, hz: u16, ms: u32) {
    let ms = to_ms(ms);
    if ms != 0 {
        notes.push(Note {
            hz,
            ms,
            ramp_ms: ramp_of(ms),
        });
    }
}

fn to_ms(ms: u32) -> u16 {
    u16::try_from(ms).unwrap_or(u16::MAX)
}

/// Reads a Standard MIDI File into the table [`Tone`] walks.
///
/// Runs on the cooperative side — the feed that drives the synthesiser has no
/// allocator — and hands the finished table over, so nothing on that path ever
/// touches a byte of the file.
///
/// Tracks are concatenated in file order: a piece split across tracks is one
/// sound, not several, and the alternative would silently play only the first.
pub fn from_midi(bytes: &[u8]) -> Option<Vec<Note>> {
    let mut reader = Reader::new(bytes);
    reader.expect(b"MThd")?;
    if reader.u32()? != 6 {
        return None;
    }
    reader.u16()?;
    reader.u16()?;
    let division = u32::from(reader.u16()?);
    if division == 0 {
        return None;
    }

    let mut score = Score {
        notes: Vec::new(),
        tempos: Vec::new(),
    };
    let mut base = 0u32;
    while !reader.done() {
        let tag = reader.take(4)?;
        let len = reader.u32()? as usize;
        let body = reader.take(len)?;
        if tag != *b"MTrk" {
            continue;
        }
        let mut track = Reader::new(body);
        let spanned = score.read_track(&mut track, base)?;
        base = base.saturating_add(spanned);
    }
    Some(score.into_notes(division))
}

const TONE_PEAK_LSB: f32 = 23_000.0;

const fn samples_of_ms(ms: u16) -> u32 {
    ms as u32 * SAMPLE_RATE_HZ / 1_000
}

// Entries in the sine table, a power of two so the phase can be masked rather
// than divided. More entries than the score's highest note can cycle through
// between samples: at 1,320 Hz a 48 kHz sample lands 0.0275 turns apart, so
// [`WAVE_ENTRIES`] entries give a coarse spacing of 0.0039 turns and the
// largest possible step skips about seven of them.
const WAVE_ENTRIES: usize = 1024;

const WAVE_END: usize = WAVE_ENTRIES + 1;

const PHASE_BITS: u32 = 8;

const PHASE_MASK: u32 = (1 << PHASE_BITS) - 1;

const WAVE_TURN: u32 = (WAVE_ENTRIES as u32) << PHASE_BITS;

const Q16_BITS: u32 = 16;
const Q16_ONE: i32 = 1 << Q16_BITS;

const GAIN_BITS: u32 = 8;

#[cfg(test)]
const GAIN_ONE: u16 = 1 << GAIN_BITS;

const MAX_PARTIALS: usize = 4;

const TIMBRE: [(u16, u16); 2] = [(100, 180), (276, 76)];

/// The single-partial timbre, kept so the additive path can be held to the oscillator it
/// matches — see `a_lone_partial_is_the_oscillator_it_replaced`.
#[cfg(test)]
pub const PLAIN: [(u16, u16); 1] = [(100, GAIN_ONE)];

// One turn of a sine, sampled at [`WAVE_ENTRIES`] even points and scaled to the
// rail.
//
// Built at compile time: `libm::sinf` is not a `const fn`, so the entries come
// from the polynomial in [`sine`], which its test holds to within one LSB of
// the `sinf` this replaced.
//
// `i16` rather than `f32` because the read side is what runs in an interrupt,
// where an FPU instruction is not slow but fatal: see [`Tone::next_sample`].
// Keeping the interpolation in the integer domain leaves the per-sample path
// with nothing but integer arithmetic.
static WAVE: [i16; WAVE_END] = {
    let mut wave = [0_i16; WAVE_END];
    let mut i = 0;
    while i < WAVE_END {
        let turns = i as f32 / WAVE_ENTRIES as f32;
        let phase = core::f32::consts::TAU * turns;
        wave[i] = rounded(sine(phase) * TONE_PEAK_LSB);
        i += 1;
    }
    wave[WAVE_ENTRIES] = wave[0];
    wave
};

const fn rounded(scaled: f32) -> i16 {
    let biased = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    biased as i16
}

const fn sine(x: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let (half, negative) = if x > pi { (x - pi, true) } else { (x, false) };
    let quarter = if half > pi / 2.0 { pi - half } else { half };
    let squared = quarter * quarter;
    let series = 1.0
        - squared
            * (1.0 / 6.0
                - squared
                    * (1.0 / 120.0
                        - squared
                            * (1.0 / 5040.0
                                - squared * (1.0 / 362880.0 - squared * (1.0 / 39_916_800.0)))));
    let value = quarter * series;
    if negative { -value } else { value }
}

/// A generated sound: one oscillator per partial walking a note table.
#[derive(Debug, Clone)]
pub struct Tone {
    score: Vec<Note>,
    timbre: &'static [(u16, u16)],
    note: usize,
    phase: [u32; MAX_PARTIALS],
    phase_step: [u32; MAX_PARTIALS],
    samples: u32,
    ramp: u32,
    into_note: u32,
    inv_ramp: u32,
}

impl Tone {
    /// A voice sounding `score` on the family `TIMBRE`.
    pub fn playing(score: Vec<Note>) -> Self {
        Self::on(score, &TIMBRE)
    }

    /// A voice sounding `score` on `timbre`.
    ///
    /// # Panics
    ///
    /// On a timbre that is empty or longer than `MAX_PARTIALS`: the first
    /// would make a silent voice the second would silently drop partials, and a
    /// sound quieter than the score asked for is the harder of the two to hear
    /// coming.
    pub fn on(score: Vec<Note>, timbre: &'static [(u16, u16)]) -> Self {
        assert!(
            !timbre.is_empty() && timbre.len() <= MAX_PARTIALS,
            "a voice needs one to four partials, not {}",
            timbre.len(),
        );
        let mut tone = Self {
            score,
            timbre,
            note: 0,
            phase: [0; MAX_PARTIALS],
            phase_step: [0; MAX_PARTIALS],
            samples: 0,
            ramp: 0,
            into_note: 0,
            inv_ramp: 0,
        };
        tone.enter(0);
        tone
    }

    fn enter(&mut self, index: usize) {
        self.note = index;
        self.into_note = 0;
        self.phase = [0; MAX_PARTIALS];
        let Some(note) = self.score.get(index) else {
            self.samples = 0;
            self.ramp = 0;
            self.phase_step = [0; MAX_PARTIALS];
            self.inv_ramp = 0;
            return;
        };
        self.samples = samples_of_ms(note.ms);
        self.ramp = samples_of_ms(note.ramp_ms);
        // Phase units rather than radians, so the sample path wraps by
        // comparison instead of dividing. Widened for the multiply because a
        // table turn times a frequency times a ratio overflows `u32` well below
        // Nyquist.
        for (partial, &(ratio, _)) in self.timbre.iter().enumerate() {
            self.phase_step[partial] =
                (u64::from(WAVE_TURN) * u64::from(note.hz) * u64::from(ratio)
                    / (100 * u64::from(SAMPLE_RATE_HZ))) as u32;
            debug_assert!(
                self.phase_step[partial] < WAVE_TURN,
                "a note cannot turn a sample"
            );
        }
        self.inv_ramp = if self.ramp == 0 {
            0
        } else {
            (u64::from(Q16_ONE as u32) * u64::from(Q16_ONE as u32) / u64::from(self.ramp)) as u32
        };
    }

    fn envelope(&self) -> i32 {
        if self.ramp == 0 {
            return Q16_ONE;
        }
        let peak = self
            .into_note
            .min(self.samples - self.into_note)
            .min(self.ramp);
        ((u64::from(peak) * u64::from(self.inv_ramp)) >> 16) as i32
    }

    // One sample: every partial summed and weighted, and the oscillators and
    // note position stepped past it.
    //
    // Integer from end to end, because this runs in the feed interrupt and an
    // FPU instruction there raises `Cp0Disabled`: the board stops on the first
    // tap frame with no reset, from a path with no handler to survive it. It
    // is a property of the interrupt context, not of the arithmetic — the same
    // code on the cooperative executor is fine.
    //
    // The gains are taken off **before** the envelope, not after: at full
    // envelope a weighted partial is `peak × 256`, and multiplying that by a
    // 16-bit envelope overflows `i32` part-way up. Shifting the gain off first
    // divides the whole sum by the fixed-point one exactly, which is also why a
    // lone partial at that gain lands back on the unweighted oscillator — see
    // `a_lone_partial_is_the_oscillator_it_replaced`.
    fn next_sample(&mut self) -> i16 {
        let envelope = self.envelope();
        let mut sum = 0i32;
        for (partial, &(_, gain)) in self.timbre.iter().enumerate() {
            let at = self.phase[partial];
            let whole = (at >> PHASE_BITS) as usize;
            let fraction = (at & PHASE_MASK) as i32;
            let (low, high) = (WAVE[whole] as i32, WAVE[whole + 1] as i32);
            let interpolated =
                low + (((high - low) * fraction + (1 << (PHASE_BITS - 1))) >> PHASE_BITS);
            sum += interpolated * i32::from(gain);
            self.phase[partial] = at + self.phase_step[partial];
            if self.phase[partial] >= WAVE_TURN {
                self.phase[partial] -= WAVE_TURN;
            }
        }
        let sample = ((sum >> GAIN_BITS) * envelope + (1 << (Q16_BITS - 1))) >> Q16_BITS;
        self.into_note += 1;
        sample as i16
    }
}

/// A linear chirp: the excitation a room is **measured** with rather than one it
/// is heard with.
///
/// **Chosen over noise and over a single impulse** for two reasons that decide
/// whether the measurement is worth anything. Its inverse is itself up to a
/// scale, so a recording of one deconvolves into the path that made it instead
/// of into an estimate; and how far apart two reflections can be told apart is
/// set by the *sweep rate* rather than by the length of the excitation, so a
/// sweep that crosses the band in 200 ms separates reflections a millisecond
/// apart. An impulse would put all its energy at one instant and leave that
/// choice to the room.
///
/// The phase is carried in a [`u64`] at `Q16_BITS` of extra resolution because
/// the step this sweeps is far below one table entry per sample: over the band
/// below, a whole number of phase units per sample truncates to zero and the
/// sweep becomes a tone.
///
/// # A sweep needs the two ends to share a clock, and this board's do
///
/// A chirp decorrelates the moment the playback and recording clocks differ by
/// even one sample, which at the top of the band is a tenth of a cycle — that is
/// the usual reason a sweep is not the excitation for measuring a path through a
/// device's own speakers and microphone. **That failure needs two unsynchronised
/// clocks, and there are not two here:** both codecs are programmed as slaves of
/// one host MCLK out of one `I2S0` peripheral, so a sample handed to the DAC and
/// the sample the ADC returns are the same sample index in the same clock.
///
/// A single tone dodges that requirement, and paying for it is worse than it
/// looks: a tone's energy sits at one frequency, so it measures the transfer
/// function at that frequency and nothing else — no path length, no tail, and no
/// answer to "how many taps does this room need".
pub struct Sweep {
    phase: u64,
    step: u64,
    step_delta: u64,
    index: u32,
    length: u32,
    from_step: u64,
    step_delta_base: u64,
}

const SWEEP_BITS: u32 = Q16_BITS;

const SWEEP_TURN: u64 = (WAVE_TURN as u64) << SWEEP_BITS;

// The level the sweep runs at, and **a measurement parameter rather than a
// taste one**.
//
// `[实测]` at half the rail the ADC clipped (32 767 peak); a fifth of the rail leaves
// headroom for a louder room than the sweep.
const SWEEP_PEAK_LSB: i32 = 2_500;

impl Sweep {
    /// A sweep from `from_hz` to `to_hz` over `milliseconds`, looping without a step — see
    /// [`SampleSource::fill`] for what makes the join clean.
    pub const fn new(from_hz: u32, to_hz: u32, milliseconds: u32) -> Self {
        let samples = (SAMPLE_RATE_HZ / 1_000) * milliseconds;
        let turn = WAVE_TURN as u64;
        let from_step = from_hz as u64 * turn * (1 << SWEEP_BITS) / SAMPLE_RATE_HZ as u64;
        let step_delta = ((to_hz - from_hz) as u64 * turn * (1 << SWEEP_BITS))
            / (SAMPLE_RATE_HZ as u64 * samples as u64);
        Self {
            phase: 0,
            step: from_step,
            step_delta,
            index: 0,
            length: samples,
            from_step,
            step_delta_base: step_delta,
        }
    }

    // Back to the bottom of the band for the next pass, **keeping the phase
    // where it is**.
    //
    // `[实测]` resetting the phase put a 10 567 LSB step at the join; carrying it keeps
    // the join one ordinary step.
    fn rewind(&mut self) {
        self.step = self.from_step;
        self.step_delta = self.step_delta_base;
        self.index = 0;
    }

    fn next_sample(&mut self) -> i16 {
        let at = self.phase;
        self.phase += self.step;
        self.step += self.step_delta;
        if self.phase >= SWEEP_TURN {
            self.phase -= SWEEP_TURN;
        }
        self.index += 1;

        let whole = (at >> (SWEEP_BITS + PHASE_BITS)) as usize;
        let fraction = ((at >> SWEEP_BITS) & u64::from(PHASE_MASK)) as i32;
        let (low, high) = (WAVE[whole] as i32, WAVE[whole + 1] as i32);
        let interpolated =
            low + (((high - low) * fraction + (1 << (PHASE_BITS - 1))) >> PHASE_BITS);
        (interpolated * SWEEP_PEAK_LSB / 23_000) as i16
    }
}

impl SampleSource for Sweep {
    // **Never runs out.** A sweep is the excitation a room is measured with, and
    // a driver that has to be re-armed mid-measurement puts a click and a gap
    // in the recording for every re-arm — which is the measurement being ruined
    // by the act of taking it. Looping here also closes: 300 Hz to 6 kHz over
    // 200 ms is exactly six hundred and thirty cycles, so the phase comes back
    // to where it started and the waveform joins itself.
    fn fill(&mut self, out: &mut [i16]) -> usize {
        for (n, slot) in out.iter_mut().enumerate() {
            if self.index >= self.length {
                if n != 0 {
                    break;
                }
                self.rewind();
            }
            *slot = self.next_sample();
        }
        out.len()
    }

    fn done(&self) -> bool {
        false
    }
}

impl SampleSource for Tone {
    fn fill(&mut self, out: &mut [i16]) -> usize {
        let mut filled = 0;
        for slot in out.iter_mut() {
            if self.done() {
                break;
            }
            *slot = self.next_sample();
            if self.into_note >= self.samples {
                self.enter(self.note + 1);
            }
            filled += 1;
        }
        filled
    }

    fn done(&self) -> bool {
        self.note >= self.score.len()
    }
}

/// Whether a sound is still audible, from whether its source has anything left and what the
/// arm in flight is carrying.
#[cfg(test)]
pub const fn still_sounding(source_done: bool, arm_sounded: bool, arm_in_flight: bool) -> bool {
    !source_done || (arm_sounded && arm_in_flight)
}

/// How often a streaming ring is fed.
pub const FEED_MS: u32 = 5;

const WIRE_BYTES_PER_SEC: usize = BYTES_PER_FRAME * SAMPLE_RATE_HZ as usize;

const fn ring_play_ms(ring_bytes: usize) -> u64 {
    ring_bytes as u64 * 1_000 / WIRE_BYTES_PER_SEC as u64
}

/// Notices a transmit stream the DMA has genuinely run dry, by the clock rather than by the
/// DMA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayStreamWatchdog {
    last_busy_ms: Option<u64>,
    idle_limit_ms: u64,
    reported: bool,
}

impl PlayStreamWatchdog {
    /// Watches a stream whose ring is `ring_bytes` long.
    pub const fn new(ring_bytes: usize) -> Self {
        Self {
            last_busy_ms: None,
            idle_limit_ms: ring_play_ms(ring_bytes) + FEED_MS as u64,
            reported: false,
        }
    }

    /// Whether the stream has been idle too long, given whether the DMA had anything to
    /// send this feed.
    pub fn drained(&mut self, now_ms: u64, tx_idle: bool) -> bool {
        if !tx_idle {
            self.last_busy_ms = Some(now_ms);
            self.reported = false;
            return false;
        }
        let last = *self.last_busy_ms.get_or_insert(now_ms);
        now_ms.saturating_sub(last) > self.idle_limit_ms
    }

    /// Reports the feed on which the stream was first found drained since it was
    /// last seen working, so a driver logs the one rather than the state — a
    /// stream left dry would otherwise log on every feed for ever, which on a
    /// 5 ms cadence is a log line the reader cannot outrun.
    pub fn report_drained(&mut self, now_ms: u64, tx_idle: bool) -> bool {
        if !self.drained(now_ms, tx_idle) || self.reported {
            return false;
        }
        self.reported = true;
        true
    }

    /// The whole of a stream's runway, for a driver that has to describe what it is
    /// watching.
    pub const fn idle_limit_ms(&self) -> u64 {
        self.idle_limit_ms
    }
}

/// Which of a speaker's two fallible calls was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerOp {
    Play,
    SetMuted,
}

impl core::fmt::Display for SpeakerOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Play => "play",
            Self::SetMuted => "set_muted",
        })
    }
}

/// Why a speaker could not carry out a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerFault {
    /// Which command was refused.
    pub op: SpeakerOp,
    /// The board's own words for why, so a log names the real cause rather than a generic
    /// "the codec did not answer".
    pub reason: &'static str,
}

impl SpeakerFault {
    /// A refusal of `op`, in the board's own words.
    pub const fn new(op: SpeakerOp, reason: &'static str) -> Self {
        Self { op, reason }
    }
}

impl core::fmt::Display for SpeakerFault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} refused: {}", self.op, self.reason)
    }
}

/// What a driver had to repair after its last [`feed`](Speaker::feed) found the output
/// stream drained, handed back so the repair can be reported rather than only performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recovery {
    /// Writable bytes left in the ring at the moment the drain was noticed.
    pub free_bytes: usize,
    /// The ring's whole writable length, `free_bytes`'s denominator.
    pub ring_bytes: usize,
    /// How long the stream may sit idle before the drain is called — the runway it overran.
    pub runway_ms: u64,
    /// Whether a sound was on the ring when it drained — what the stall cost the listener.
    pub playing: bool,
    /// Whether [`recover`](Speaker::recover) could rebuild the stream. The caller reports
    /// this; the driver itself must stay silent.
    pub repaired: bool,
    /// How many times this driver has rebuilt its stream since boot.
    pub restarts: u32,
}

pub trait Speaker: Send + 'static {
    fn play(&mut self, source: Box<dyn SampleSource>) -> Result<(), SpeakerFault>;

    fn set_muted(&mut self, muted: bool) -> Result<(), SpeakerFault>;

    fn feed(&mut self, now_ms: u64) -> bool;

    /// Whether [`feed`](Speaker::feed) recorded a drain that still needs
    /// [`recover`](Speaker::recover). The feed loop reads this to wake the repair side
    /// instead of polling for it.
    fn needs_recovery(&self) -> bool {
        false
    }

    /// Repairs a drain recorded by [`feed`](Speaker::feed). This runs on the cooperative
    /// side: it must not log, allocate, or wait, because the caller holds the lock the
    /// feed interrupt also takes.
    fn recover(&mut self) -> Option<Recovery>;
}

/// A speaker that is not wired, for a board that has an output surface in the product but
/// nothing driving it on this hardware.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnwiredSpeaker;

impl Speaker for UnwiredSpeaker {
    fn play(&mut self, _source: Box<dyn SampleSource>) -> Result<(), SpeakerFault> {
        Ok(())
    }

    fn set_muted(&mut self, _muted: bool) -> Result<(), SpeakerFault> {
        Ok(())
    }

    fn feed(&mut self, _now_ms: u64) -> bool {
        false
    }

    fn recover(&mut self) -> Option<Recovery> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::audio::SLOTS_PER_FRAME;
    use alloc::vec;
    use alloc::vec::Vec;
    use libm::sinf;

    const REST: u8 = 0;

    const TAP: &[(u8, u16)] = &[(81, 120), (REST, 40), (88, 180)];

    fn mid(notes: &[(u8, u16)]) -> Vec<u8> {
        const DIV: u32 = 480;
        const QUARTER_MS: u32 = 500;

        let vlq = |n: u32| {
            let mut out = vec![(n & 0x7f) as u8];
            let mut rest = n >> 7;
            while rest > 0 {
                out.insert(0, (rest & 0x7f) as u8 | 0x80);
                rest >>= 7;
            }
            out
        };
        // Rounded to nearest rather than truncated, so a note written as 180 ms
        // comes back as 180 ms and not as 179 — the half added back is the same
        // half the files shipped in `assets/` were rounded with.
        let ticks = |ms: u16| (u32::from(ms) * DIV + QUARTER_MS / 2) / QUARTER_MS;

        let mut track = vec![0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20];
        let mut pending = 0u32;
        for &(note, ms) in notes {
            if note == REST {
                pending += ticks(ms);
                continue;
            }
            track.extend(vlq(pending));
            track.extend([0x90, note, 0x64]);
            pending = 0;
            track.extend(vlq(ticks(ms)));
            track.extend([0x80, note, 0x40]);
        }
        track.extend([0x00, 0xff, 0x2f, 0x00]);

        let mut file = b"MThd".to_vec();
        file.extend(6u32.to_be_bytes());
        file.extend(0u16.to_be_bytes()); // format 0
        file.extend(1u16.to_be_bytes());
        file.extend((DIV as u16).to_be_bytes());
        file.extend(b"MTrk");
        file.extend((track.len() as u32).to_be_bytes());
        file.extend(track);
        file
    }

    fn playing(notes: &[(u8, u16)]) -> (Tone, Vec<i16>) {
        let score = from_midi(&mid(notes)).expect("a fixture it just built parses");
        let samples = samples_of(score.iter());
        let mut tone = Tone::playing(score);
        let mut out = vec![0_i16; samples];
        let written = tone.fill(&mut out);
        assert_eq!(written, samples, "the score is the length it states");
        assert!(tone.done(), "a filled score has run out");
        (tone, out)
    }

    fn tap() -> (Tone, Vec<i16>) {
        playing(TAP)
    }

    fn samples_of<'a>(notes: impl Iterator<Item = &'a Note>) -> usize {
        notes.map(|note| samples_of_ms(note.ms) as usize).sum()
    }

    #[test]
    fn the_table_holds_the_oscillator_it_replaced() {
        // Checked against libm's `sinf` rather than an ideal nobody can build; one LSB at 16
        // bits is far below the rail's resolution through a DAC.
        let mut worst = 0_i32;
        for (index, &table) in WAVE.iter().enumerate().take(WAVE_ENTRIES) {
            let turns = index as f32 / WAVE_ENTRIES as f32;
            let expected = (sinf(core::f32::consts::TAU * turns) * TONE_PEAK_LSB) as i16;
            worst = worst
                .max(i32::from(table) - i32::from(expected))
                .max(i32::from(expected) - i32::from(table));
        }
        assert!(
            worst <= 1,
            "the table is {worst} LSB from the oscillator it replaced",
        );
        assert_eq!(
            WAVE[WAVE_ENTRIES], WAVE[0],
            "a turn closes on the value it opened with",
        );
    }

    #[test]
    fn the_interpolated_oscillator_tracks_sinf_within_an_lsb() {
        let mut worst = 0_i32;
        for step in 0..48_000_u32 {
            let phase = u64::from(step) * u64::from(WAVE_TURN) / 48_000;
            let phase = phase as u32;
            let whole = (phase >> PHASE_BITS) as usize;
            let fraction = (phase & PHASE_MASK) as i32;
            let (low, high) = (WAVE[whole] as i32, WAVE[whole + 1] as i32);
            let got = low + (((high - low) * fraction) >> PHASE_BITS);

            let turns = phase as f64 / f64::from(WAVE_TURN);
            let want = (core::f64::consts::TAU * turns).sin() * f64::from(TONE_PEAK_LSB);
            worst = worst.max(((f64::from(got) - want).round() as i32).abs());
        }
        assert!(
            worst <= 1,
            "interpolated oscillator is {worst} LSB from the oscillator it replaced",
        );
    }

    #[test]
    fn every_partial_stays_inside_the_table_it_indexes() {
        let score = from_midi(&mid(TAP)).expect("parses");
        let total = samples_of(score.iter());
        let mut tone = Tone::playing(score);
        let mut frame = [0_i16; 1];
        for _ in 0..total {
            for (partial, &phase) in tone.phase.iter().enumerate() {
                assert!(
                    phase < WAVE_TURN,
                    "partial {partial} phase {phase} left the table",
                );
            }
            assert_eq!(tone.fill(&mut frame), 1);
        }
        assert!(tone.done(), "the tap ran out rather than spinning");
    }

    #[test]
    fn the_catalogue_cycles_and_wraps() {
        let mut lap = [Sound::Tap; Sound::ALL.len()];
        let mut walked = Sound::ALL[0];
        for slot in lap.iter_mut() {
            walked = walked.next();
            *slot = walked;
        }
        let mut expected = Sound::ALL;
        expected.rotate_left(1);
        assert_eq!(lap, expected, "a lap visits the catalogue in order");
        assert_eq!(walked, Sound::ALL[0], "and comes back to the head of it",);
        for sound in Sound::ALL {
            assert!(
                lap.contains(&sound),
                "{sound:?} is catalogued but unreachable",
            );
        }
        assert!(
            !Sound::ALL.contains(&Sound::Sweep),
            "the bench excitation stays off the tap cycle",
        );
    }

    #[test]
    fn a_tap_runs_for_exactly_the_length_its_score_states() {
        let score = from_midi(&mid(TAP)).expect("parses");
        let stated = samples_of(score.iter());
        let (_, samples) = tap();
        assert_eq!(samples.len(), stated);
        let sounded: Vec<_> = score
            .iter()
            .filter(|note| note.hz != 0)
            .map(|note| (note.hz, note.ms))
            .collect();
        assert_eq!(sounded, [(880, 119), (1319, 180)], "A5 then E6, a fifth up");
    }

    #[test]
    fn a_tap_opens_and_closes_on_silence() {
        let (_, samples) = tap();
        assert_eq!(samples[0], 0, "and opens on the oscillator's own zero");
        assert!(
            samples[samples.len() - 1].unsigned_abs() < TONE_PEAK_LSB as u16 / 100,
            "and ends inside 1% of the rail, which is the release ramp and not a cut"
        );
        let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
        assert!(
            (TONE_PEAK_LSB as u16 - 128..=TONE_PEAK_LSB as u16).contains(&loudest),
            "{loudest} LSB is not the nominal peak"
        );
    }

    #[test]
    fn a_note_following_another_is_joined_through_silence() {
        const JOINED: &[(u8, u16)] = &[(81, 60), (88, 60)];
        let score = from_midi(&mid(JOINED)).expect("parses");
        let mut tone = Tone::playing(score.clone());
        let mut samples = [0_i16; 6_000];
        tone.fill(&mut samples);
        let boundary = samples_of_ms(score[0].ms) as usize;
        assert!(
            samples[boundary - 1..=boundary]
                .iter()
                .all(|&s| s.abs() < 500),
            "the join is a crossing, not a step"
        );
    }

    #[test]
    fn a_tap_never_reaches_the_rail() {
        let (_, samples) = tap();
        let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
        assert!(
            loudest < 32_768,
            "{loudest} LSB hit the rail, which is the headroom the DAC volume needs"
        );
    }

    const BAND: (u32, u32, u32) = (300, 6_000, 200);

    fn sweep_of(band: (u32, u32, u32)) -> Vec<i16> {
        let length = (SAMPLE_RATE_HZ / 1_000 * band.2) as usize;
        let mut out = vec![0_i16; length];
        assert_eq!(Sweep::new(band.0, band.1, band.2).fill(&mut out), length);
        out
    }

    #[test]
    fn a_sweep_is_reproducible_bit_for_bit() {
        assert_eq!(sweep_of(BAND), sweep_of(BAND));
    }

    #[test]
    fn a_sweep_actually_sweeps_instead_of_sitting_on_one_tone() {
        let (from, to, milliseconds) = BAND;
        let sweep = sweep_of(BAND);

        let sweep_state = Sweep::new(from, to, milliseconds);
        assert!(
            sweep_state.step > 0 && sweep_state.step < SWEEP_TURN,
            "the step is one whole tone's worth or none: {}",
            sweep_state.step
        );
        assert!(
            sweep_state.step_delta > 0,
            "the frequency has to move at all, and over this band it moves by less \
             than one phase unit per sample — which is what needs the extra resolution"
        );

        let crossings = sweep
            .windows(2)
            .filter(|p| (p[0] < 0) != (p[1] < 0))
            .count();
        let seconds = f64::from(milliseconds) / 1_000.0;
        let twice_cycles = 2.0 * (f64::from(from) + f64::from(to)) / 2.0 * seconds;
        assert!(
            (crossings as f64) > twice_cycles * 0.9,
            "{crossings} zero crossings against about {twice_cycles} — too few for a sweep"
        );
        assert!(
            (crossings as f64) < twice_cycles * 1.1,
            "{crossings} zero crossings against about {twice_cycles} — too many for a sweep"
        );
    }

    #[test]
    fn a_sweep_loops_without_a_step_and_runs_below_the_rail() {
        let (from, to, milliseconds) = BAND;
        let length = (SAMPLE_RATE_HZ / 1_000 * milliseconds) as usize;

        let mut sweep = Sweep::new(from, to, milliseconds);
        let mut short = vec![0_i16; length / 4];
        assert_eq!(sweep.fill(&mut short), length / 4, "a partial fill is full");
        assert!(!sweep.done(), "and there is more of it");

        let mut two = vec![0_i16; length * 2];
        let mut long = Sweep::new(from, to, milliseconds);
        assert_eq!(
            long.fill(&mut two),
            length * 2,
            "a looping sweep never runs out"
        );
        let step_across = (i32::from(two[length - 1]) - i32::from(two[length])).abs();
        let mut steps: Vec<i32> = (1..length - 1)
            .map(|n| (i32::from(two[n]) - i32::from(two[n + 1])).abs())
            .collect();
        steps.sort_unstable();
        let p99 = steps[steps.len() * 99 / 100];
        let worst = steps[steps.len() - 1];
        let _ = p99;
        assert!(
            step_across <= worst + worst / 2,
            "the join steps by {step_across} where the steepest ordinary step is {worst}"
        );

        let peak = two.iter().map(|s| i32::from(*s).abs()).max().unwrap();
        assert!(peak > SWEEP_PEAK_LSB / 2, "the sweep is at {peak} LSB");
        assert!(peak <= SWEEP_PEAK_LSB, "and never past {SWEEP_PEAK_LSB}");
    }

    #[test]
    fn filling_a_tap_in_pieces_is_the_same_stream_as_filling_it_at_once() {
        let score = || from_midi(&mid(TAP)).expect("parses");
        let mut whole = Tone::playing(score());
        let mut at_once = vec![0_i16; 4_800];
        whole.fill(&mut at_once);

        let mut piecemeal = Tone::playing(score());
        let mut in_pieces = Vec::new();
        for size in [256, 1, 1024, 7, 3_512] {
            let mut chunk = vec![0_i16; size];
            let written = piecemeal.fill(&mut chunk);
            in_pieces.extend_from_slice(&chunk[..written]);
        }
        assert_eq!(in_pieces, at_once);
    }

    #[test]
    fn a_tone_reports_done_only_once_the_score_has_played_out() {
        let score = from_midi(&mid(TAP)).expect("parses");
        let stated = samples_of(score.iter());
        let mut tone = Tone::playing(score);
        let mut samples = [0_i16; 512];
        assert!(!tone.done(), "a fresh tone has its whole score ahead of it");
        let mut total = 0;
        while !tone.done() {
            total += tone.fill(&mut samples);
        }
        assert_eq!(total, stated);
        assert_eq!(tone.fill(&mut samples), 0, "and stays empty afterwards");
    }

    #[test]
    fn an_empty_score_is_harmless() {
        let mut tone = Tone::playing(Vec::new());
        assert!(tone.done());
        assert_eq!(tone.fill(&mut [0_i16; 16]), 0);
    }

    #[test]
    fn a_file_with_nothing_in_it_is_harmless() {
        let notes = from_midi(&mid(&[])).expect("an empty track is still a file");
        let mut tone = Tone::playing(notes);
        assert!(tone.done());
        assert_eq!(tone.fill(&mut [0_i16; 16]), 0);
    }

    #[test]
    fn a_rest_is_silence_rather_than_a_hum() {
        let (_, samples) = tap();
        let rest_start = samples_of_ms(119) as usize;
        let rest_end = rest_start + samples_of_ms(39) as usize;
        assert!(
            samples[rest_start..rest_end].iter().all(|&s| s == 0),
            "the gap between the two notes is silence"
        );
    }

    #[test]
    fn a_lone_partial_is_the_oscillator_it_replaced() {
        let score = from_midi(&mid(TAP)).expect("parses");
        let mut tone = Tone::on(score.clone(), &PLAIN);

        let mut phase = 0u32;
        let mut index = 0usize;
        let mut into = 0u32;
        let mut wanted = vec![0_i16; samples_of(score.iter())];
        for slot in &mut wanted {
            if index >= score.len() {
                break;
            }
            let note = score[index];
            let samples = samples_of_ms(note.ms);
            let ramp = samples_of_ms(note.ramp_ms);
            let step =
                (u64::from(WAVE_TURN) * u64::from(note.hz) / u64::from(SAMPLE_RATE_HZ)) as u32;
            let inv_ramp = if ramp == 0 {
                0
            } else {
                (u64::from(Q16_ONE as u32) * u64::from(Q16_ONE as u32) / u64::from(ramp)) as u32
            };
            let envelope = if ramp == 0 {
                Q16_ONE
            } else {
                let peak = into.min(samples - into).min(ramp);
                ((u64::from(peak) * u64::from(inv_ramp)) >> 16) as i32
            };
            let whole = (phase >> PHASE_BITS) as usize;
            let fraction = (phase & PHASE_MASK) as i32;
            let (low, high) = (WAVE[whole] as i32, WAVE[whole + 1] as i32);
            let interpolated =
                low + (((high - low) * fraction + (1 << (PHASE_BITS - 1))) >> PHASE_BITS);
            *slot = ((interpolated * envelope + (1 << (Q16_BITS - 1))) >> Q16_BITS) as i16;

            phase += step;
            if phase >= WAVE_TURN {
                phase -= WAVE_TURN;
            }
            into += 1;
            if into >= samples {
                index += 1;
                phase = 0;
                into = 0;
            }
        }

        let mut got = vec![0_i16; wanted.len()];
        assert_eq!(tone.fill(&mut got), wanted.len());
        assert_eq!(
            got, wanted,
            "one partial at full gain is not the oscillator it replaced",
        );
    }

    #[test]
    fn a_lone_partial_still_peaks_where_the_rail_is() {
        let score = from_midi(&mid(TAP)).expect("parses");
        let mut tone = Tone::on(score, &PLAIN);
        let mut samples = vec![0_i16; 4_800];
        tone.fill(&mut samples);
        let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
        assert_eq!(
            loudest, TONE_PEAK_LSB as u16,
            "a lone partial reaches exactly the rail the table is built at",
        );
    }

    #[test]
    fn the_family_timbres_partials_are_inharmonic() {
        assert_eq!(TIMBRE.len(), 2, "a fundamental and one partial above it");
        let &(fundament, _) = &TIMBRE[0];
        assert_eq!(fundament, 100, "the fundamental is the note itself");
        let &(partial, _) = &TIMBRE[1];
        assert_eq!(partial, 276, "the partial keeps the bell's ratio");
        assert_ne!(
            partial % fundament,
            0,
            "a partial at a whole multiple of the fundamental is a harmonic",
        );
    }

    #[test]
    fn the_family_timbre_cannot_outrun_the_rail() {
        let total: u32 = TIMBRE.iter().map(|&(_, gain)| u32::from(gain)).sum();
        assert_eq!(
            total,
            u32::from(GAIN_ONE),
            "the partials' gains have to sum to one",
        );
    }

    #[test]
    fn a_note_shorter_than_its_ramp_is_ramped_not_overshot() {
        let score = from_midi(&mid(&[(81, 1)])).expect("parses");
        let mut tone = Tone::playing(score);
        let mut samples = vec![0_i16; samples_of_ms(1) as usize];
        tone.fill(&mut samples);
        assert!(
            samples.iter().all(|&s| s.unsigned_abs() < 32_768),
            "a note shorter than its ramp still has to stay inside the rail",
        );
    }

    const FRAMES: [i16; 4] = [i16::MIN, 0, i16::MAX, -1];

    #[test]
    fn every_frame_reaches_both_slots_little_endian() {
        let frames = FRAMES;
        let mut bytes = [0_u8; BYTES_PER_FRAME * FRAMES.len()];
        let written = interleave_mono(&frames, &mut bytes);
        assert_eq!(written, bytes.len());
        for (index, &frame) in frames.iter().enumerate() {
            let frame_bytes = &bytes[index * BYTES_PER_FRAME..][..BYTES_PER_FRAME];
            for slot in frame_bytes.as_chunks::<2>().0 {
                assert_eq!(
                    i16::from_le_bytes([slot[0], slot[1]]),
                    frame,
                    "a mono frame has to arrive whole in every slot"
                );
            }
        }
    }

    #[test]
    fn a_buffer_that_cannot_hold_a_whole_frame_takes_nothing() {
        let mut bytes = [0xAA_u8; BYTES_PER_FRAME - 1];
        assert_eq!(interleave_mono(&[1, 2, 3], &mut bytes), 0);
        assert!(
            bytes.iter().all(|&b| b == 0xAA),
            "half a frame on the wire is a click, so nothing is written"
        );
    }

    #[test]
    fn interleaving_stops_at_the_buffer_edge() {
        let mut bytes = [0_u8; BYTES_PER_FRAME * 2];
        let written = interleave_mono(&[7, 8, 9], &mut bytes);
        assert_eq!(written, BYTES_PER_FRAME * 2, "two frames of three");
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), 7);
        assert_eq!(
            i16::from_le_bytes([bytes[BYTES_PER_FRAME], bytes[BYTES_PER_FRAME + 1]]),
            8
        );
    }

    #[test]
    fn a_frame_is_as_wide_as_the_capture_takes_it_to_be() {
        assert_eq!(BYTES_PER_FRAME, 4);
        assert_eq!(BYTES_PER_FRAME, SLOTS_PER_FRAME as usize * 2);
    }

    #[test]
    fn an_unwired_speaker_never_claims_to_be_playing() {
        let mut speaker = UnwiredSpeaker;
        speaker
            .play(Box::new(Tone::playing(vec![])))
            .expect("takes a source");
        speaker.set_muted(true).expect("takes a latch");
        assert!(
            !speaker.feed(0),
            "a sound that was never made is never done playing"
        );
    }

    #[test]
    fn a_sound_ends_when_its_source_is_spent_and_nothing_audible_is_in_flight() {
        assert!(
            still_sounding(false, true, true),
            "a source with samples left"
        );
        assert!(
            still_sounding(false, false, false),
            "and it does not need an arm"
        );
        assert!(
            still_sounding(true, true, true),
            "a sounded arm in flight is the tail of a spent source"
        );
        assert!(
            !still_sounding(true, false, true),
            "a silent arm in flight means the sounded one has drained"
        );
        assert!(
            !still_sounding(true, true, false),
            "nothing in flight at all"
        );
        assert!(!still_sounding(true, false, false));
    }

    #[test]
    fn a_spent_source_never_reports_playing_again() {
        assert!(
            !still_sounding(true, false, true),
            "the silent arm a spent source arms must not read as a sound in progress"
        );
        assert!(!still_sounding(true, false, false));
        assert!(!still_sounding(true, true, false));
    }

    const RING_BYTES: usize = 960 * 24;

    #[test]
    fn a_ring_of_runway_is_a_hundred_and_twenty_milliseconds() {
        // 960 B is one feed of wire at 48 kHz two slots wide, and 24 of them is
        // the ring. Everything the watchdog decides is "is the idle longer than
        // this", so the number itself is the test.
        assert_eq!(ring_play_ms(RING_BYTES), 120);
        assert_eq!(
            ring_play_ms(RING_BYTES / 2),
            ring_play_ms(RING_BYTES) / 2,
            "runway is measured in bytes, so it halves with the ring"
        );
    }

    #[test]
    fn a_brief_idle_between_feeds_is_not_a_drained_stream() {
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let mut now_ms = 0_u64;
        while now_ms <= ring_play_ms(RING_BYTES) {
            assert!(
                !watchdog.drained(now_ms, true),
                "a stream idle for {now_ms} ms is still inside its ring's runway"
            );
            now_ms += FEED_MS as u64;
        }
    }

    #[test]
    fn a_stream_idle_past_a_whole_ring_is_drained() {
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let limit = watchdog.idle_limit_ms();
        assert_eq!(limit, 120 + FEED_MS as u64);
        assert!(
            !watchdog.drained(0, true),
            "the first idle only starts the clock"
        );
        assert!(
            !watchdog.drained(limit, true),
            "a ring's worth of idle is exactly the limit, not past it"
        );
        assert!(watchdog.drained(limit + 1, true), "a feed later is");
    }

    #[test]
    fn one_busy_feed_pushes_the_deadline_out_again() {
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let limit = watchdog.idle_limit_ms();
        let mut now_ms = 0_u64;
        while now_ms < 10 * limit {
            assert!(
                !watchdog.drained(now_ms, false),
                "a busy feed is never a drain"
            );
            now_ms += limit - 1;
            assert!(
                !watchdog.drained(now_ms, true),
                "an idle shorter than the limit is not a drain"
            );
            now_ms += 1;
            assert!(
                !watchdog.drained(now_ms, false),
                "and the feed that finds it busy resets the deadline"
            );
            now_ms += FEED_MS as u64;
        }
    }

    #[test]
    fn a_drain_is_reported_once_however_long_the_stream_stays_dry() {
        // A stream left dry reports on every feed it is asked about, and a feed is
        // every 5 ms — so reporting the state rather than the moment would be a
        // log line the reader cannot outrun, which is no way to be told about a
        // fault that has stopped all sound.
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let limit = watchdog.idle_limit_ms();
        assert!(
            !watchdog.report_drained(0, true),
            "the first idle only starts the clock"
        );
        let mut now_ms = limit + 1;
        assert!(
            watchdog.report_drained(now_ms, true),
            "the feed that passes the limit is the one to report"
        );
        while now_ms < 10 * limit {
            now_ms += FEED_MS as u64;
            assert!(
                !watchdog.report_drained(now_ms, true),
                "and the feeds after it are the same drain, not new ones"
            );
        }
        assert!(
            !watchdog.report_drained(now_ms, false),
            "a busy feed is not a drain"
        );
        now_ms += 2 * limit;
        assert!(
            watchdog.report_drained(now_ms, true),
            "a second dry spell is its own report"
        );
    }

    #[test]
    fn a_watchdog_built_before_the_loop_measures_from_its_first_feed() {
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        assert!(
            !watchdog.drained(0, true),
            "the first idle it is shown starts the clock, it is not a drain"
        );
        assert!(
            watchdog.drained(300, true),
            "300 ms later is well past the limit, measured from that first feed"
        );
    }

    #[test]
    fn repeated_drains_report_once_each_and_never_drift() {
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let limit = watchdog.idle_limit_ms();
        let mut now_ms = 0_u64;
        let mut reports = 0_u32;
        for _ in 0..50 {
            while now_ms < 2 * limit {
                now_ms += FEED_MS as u64;
                assert!(
                    !watchdog.drained(now_ms, true),
                    "a one-cadence idle is not a drain"
                );
                now_ms += FEED_MS as u64;
                assert!(
                    !watchdog.drained(now_ms, false),
                    "a busy feed is never a drain"
                );
            }
            let dry_from = now_ms;
            while now_ms < dry_from + limit {
                now_ms += FEED_MS as u64;
                assert!(
                    !watchdog.report_drained(now_ms, true),
                    "{now_ms} ms is inside the ring's runway"
                );
            }
            now_ms += 1;
            assert!(
                watchdog.report_drained(now_ms, true),
                "the first feed past the limit is the one to report"
            );
            reports += 1;
            let dry_until = now_ms + 10 * limit;
            while now_ms < dry_until {
                now_ms += FEED_MS as u64;
                assert!(
                    !watchdog.report_drained(now_ms, true),
                    "the feeds after a report are the same drain, not new ones"
                );
            }
            now_ms += FEED_MS as u64;
            assert!(!watchdog.report_drained(now_ms, false));
        }
        assert_eq!(reports, 50, "every spell reported, and only once each");
    }
}

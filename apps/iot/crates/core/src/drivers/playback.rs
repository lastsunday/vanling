//! Playback: the sounds a device can make, the sample sources behind them, and
//! the surface a board hands the app to make them on.
//!
//! Everything here is arithmetic on a buffer, so it builds and is tested on the
//! host — the peripheral that carries it away (a DMA ring) is the bsp crate's
//! problem, not this one's.
//!
//! # Where the sounds come from
//!
//! Every sound in the catalogue is a **Standard MIDI File**, opened and edited
//! by any DAW — a sound nobody can play, adjust or attribute is a sound nobody
//! can replace. The synthesiser is unchanged in kind, so MIDI buys no timbre
//! here. It buys provenance.

use alloc::vec::Vec;

use crate::drivers::audio::{BYTES_PER_FRAME, SAMPLE_RATE_HZ};

/// A sound in the catalogue, in the order a tap cycles them.
///
/// The tap-played sounds share **one timbre** and differ only in rhythm and
/// pitch, which is the structure a family of earcons is meant to have. Giving
/// each sound its own instrument would spend the family layer on a distinction
/// nothing here needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    /// Two rising notes and a rest between them, over in a third of a second:
    /// an answer to a tap, not a tune to sit through. A rising two-note figure
    /// is the learned convention for "that worked".
    #[default]
    Tap,
    /// One note — the shortest thing still heard as a discrete event, and the
    /// convention for "something happened".
    Notice,
    /// The same note three times. Repetition is what says *again*, which a
    /// contour cannot because the contour is identical to a single note's.
    Warn,
    /// Two falling notes: the only sound in the catalogue that falls.
    Error,
    /// Two notes an octave apart, and the lowest pair in the catalogue: the
    /// ascent without the turn. Placed low so it reads as its own figure rather
    /// than as the tap pitched down.
    Rise,
    /// A linear chirp, played by a bench measurement rather than offered to a
    /// tap — so it is **not in [`Sound::ALL`]**, and the tap cycle never reaches
    /// it.
    ///
    /// It is a member of the catalogue rather than a separate entry point
    /// because a sound is what [`Speaker::play`](crate::drivers::playback::SpeakerOp)
    /// takes, and a second way to name one would be a second thing that can be
    /// started and forgotten.
    Sweep,
}

impl Sound {
    /// The whole catalogue in cycle order, so a walker and a test cannot
    /// disagree about what the tap after the last sound does.
    pub const ALL: [Self; 5] = [Self::Tap, Self::Notice, Self::Warn, Self::Error, Self::Rise];

    /// The next sound in the catalogue, wrapping: what one tap on the Speaker
    /// page plays. `const` because the page's state is a `const` too.
    /// [`Self::Sweep`] is not in the cycle, so nothing walks to it and it answers
    /// [`Self::Tap`].
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

/// A supply of mono 16-bit frames at [`SAMPLE_RATE_HZ`] — the format a source
/// hands the driver, which is what widens them into the codec's frame.
///
/// `Copy` on the sources, not `Clone`: a play is a whole sound started from the
/// beginning, and there is never a second copy of one in flight to keep in step.
pub trait SampleSource {
    /// Fills `out` with frames and returns how many it wrote. Fewer than `out`
    /// means the source ran out, which [`done`](Self::done) then reports.
    fn fill(&mut self, out: &mut [i16]) -> usize;

    /// Whether there is nothing left to give.
    fn done(&self) -> bool;
}

/// Widens mono frames into the bytes the I2S DMA transmits: each frame is
/// [`SLOTS_PER_FRAME`] slots wide and carries the same sample in every one,
/// because a mono source into a two-slot frame has to look stereo to the codec
/// or only half the frame would be ours. Returns the bytes written, so a driver
/// can hand this straight to a ring's fill callback.
///
/// A buffer too short for a whole frame writes nothing rather than half a frame:
/// a partial frame on the wire is a click.
pub fn interleave_mono(frames: &[i16], out: &mut [u8]) -> usize {
    let mut written = 0;
    for (&frame, slots) in frames.iter().zip(out.chunks_exact_mut(BYTES_PER_FRAME)) {
        for slot in slots.chunks_exact_mut(2) {
            slot.copy_from_slice(&frame.to_le_bytes());
        }
        written += BYTES_PER_FRAME;
    }
    written
}

/// One note as the synthesiser reads it: its frequency, how long it lasts, and
/// the ramp each end gets. `hz == 0` is a rest, and a rest needs no ramp.
///
/// **Not a format anything is written in.** The catalogue is MIDI files, and
/// [`from_midi`] is the only thing that builds one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub hz: u16,
    pub ms: u16,
    pub ramp_ms: u16,
}

/// Longest ramp a note gets, however long it is: a note three seconds long
/// ramped for a quarter of itself spends most of its length fading rather than
/// sounding.
pub const RAMP_MAX_MS: u16 = 40;

/// The ramp a parsed note gets at each end: a quarter of its length, capped.
///
/// Neither version of MIDI carries an envelope, so this is inferred rather than
/// declared — and a note starting or ending on a step edge is a click.
pub const fn ramp_of(ms: u16) -> u16 {
    let quarter = ms / 4;
    if quarter > RAMP_MAX_MS {
        RAMP_MAX_MS
    } else {
        quarter
    }
}

/// Tempo assumed for a file that states none: 120 BPM, which is what the format
/// specifies for a file that does not say.
const DEFAULT_US_PER_QUARTER: u32 = 500_000;

/// A4 as equal temperament states it, and the note it is stated for.
const A4_HZ: f64 = 440.0;
const A4_NOTE: i32 = 69;

/// Hz for a MIDI note number, in equal temperament.
///
/// A float here and nowhere else on the path: parsing runs on the cooperative
/// side, where the FPU is on. The synthesiser that walks the result cannot.
fn hz_of(note: u8) -> u16 {
    let hz = A4_HZ * libm::pow(2.0, f64::from(i32::from(note) - A4_NOTE) / 12.0);
    (hz + 0.5).clamp(1.0, f64::from(u16::MAX)) as u16
}

/// A cursor over a byte slice that refuses to run off the end, so a malformed
/// file ends the parse rather than reading whatever follows it.
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

    /// A variable-length quantity: seven bits per byte, high bit meaning another
    /// byte follows. Four bytes is what the format allows, which is 28 bits —
    /// far past any duration an earcon can express.
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

/// A note that has started sounding and has not yet stopped.
struct Sounding {
    start: u32,
    note: u8,
    length: u32,
}

/// A tempo in force from `tick` onwards.
struct Tempo {
    tick: u32,
    us_per_quarter: u32,
}

/// The whole of a file, read.
struct Score {
    notes: Vec<Sounding>,
    tempos: Vec<Tempo>,
}

impl Score {
    /// Milliseconds of `ticks`, at the tempo in force when they began.
    ///
    /// Free-standing rather than a method so [`into_notes`](Self::into_notes)
    /// can consume the notes it walks without borrowing itself to ask about them.
    fn ms_of(tempos: &[Tempo], tick: u32, ticks: u32, division: u32) -> u32 {
        let us_per_quarter = tempos
            .iter()
            .rev()
            .find(|tempo| tempo.tick <= tick)
            .map_or(DEFAULT_US_PER_QUARTER, |tempo| tempo.us_per_quarter);
        ticks.saturating_mul(us_per_quarter) / division / 1_000
    }

    /// Reads one track's events, appending what they say. `base` is where this
    /// track's ticks begin in the file's whole timeline — tracks are
    /// concatenated, so without it a tempo change in the second track lands on
    /// top of the first track's ticks.
    ///
    /// Everything an earcon does not use is skipped rather than refused: a DAW
    /// writes track names, program changes, controllers and sysex by default, so
    /// rejecting a file for carrying them would make the one authoring tool in
    /// use the one that does not work.
    fn read_track(&mut self, track: &mut Reader<'_>, base: u32) -> Option<u32> {
        let mut tick = 0u32;
        let mut running = 0u8;
        let mut open: Vec<(u8, u32)> = Vec::new();
        while !track.done() {
            tick = tick.saturating_add(track.vlq()?);
            let mut status = track.take(1)?[0];
            if status < 0x80 {
                // Running status: the event repeats the last channel status with
                // no byte of its own. There has to be one, or this byte is not
                // a status byte at all.
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
                    // A note on with no velocity is a note off.
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
                        // Sysex: a length and then bytes, with no running status
                        // carried across it.
                        _ => {
                            let len = track.vlq()? as usize;
                            track.take(len)?;
                        }
                    }
                }
                // System-realtime bytes never reach a file's event stream.
                _ => return None,
            }
        }
        Some(tick)
    }

    /// The table the synthesiser walks: notes in the order they begin, with the
    /// gaps between them carried as rests.
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

/// Meta-event kind carrying the tempo.
const TEMPO_META: u8 = 0x51;

/// A zero-length rest costs nothing and says nothing, so it is dropped: a
/// sound whose first note starts late would otherwise open with a silent note.
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

/// Saturates rather than wrapping: a file claiming a note longer than the
/// field could hold would otherwise come out as a click.
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
            // Any other chunk is skipped whole rather than parsed.
            continue;
        }
        let mut track = Reader::new(body);
        let spanned = score.read_track(&mut track, base)?;
        base = base.saturating_add(spanned);
    }
    Some(score.into_notes(division))
}

/// The loudest sample a note is generated at, in the 16-bit wire's own LSB.
///
/// Below the rail on purpose: the DAC volume register is the level control and
/// this is only the headroom left to it, so a synthesis artefact or a loud asset
/// arriving later has somewhere to go before it turns into clipping on the
/// speaker rather than in the samples.
const TONE_PEAK_LSB: f32 = 23_000.0;

/// Samples one millisecond spans, so a score can be written in the time a person
/// would say it out in.
const fn samples_of_ms(ms: u16) -> u32 {
    ms as u32 * SAMPLE_RATE_HZ / 1_000
}

/// Entries in the sine table, a power of two so the phase can be masked rather
/// than divided. More entries than the score's highest note can cycle through
/// between samples: at 1,320 Hz a 48 kHz sample lands 0.0275 turns apart, so
/// [`WAVE_ENTRIES`] entries give a coarse spacing of 0.0039 turns and the
/// largest possible step skips about seven of them.
const WAVE_ENTRIES: usize = 1024;

/// The last table entry, which repeats the first, so the interpolator can read
/// one entry past any index it lands on without wrapping.
const WAVE_END: usize = WAVE_ENTRIES + 1;

/// Fractional bits a phase carries, so a step can land between two table entries
/// and be interpolated rather than rounded to the nearest and heard as a
/// staircase.
const PHASE_BITS: u32 = 8;

const PHASE_MASK: u32 = (1 << PHASE_BITS) - 1;

/// A whole turn of the table, in phase units: what the oscillator wraps at.
const WAVE_TURN: u32 = (WAVE_ENTRIES as u32) << PHASE_BITS;

const Q16_BITS: u32 = 16;
const Q16_ONE: i32 = 1 << Q16_BITS;

/// Fractional bits a partial's gain carries, so gains are integers that
/// accumulate exactly and a lone partial at full gain reproduces the
/// unweighted oscillator bit for bit.
const GAIN_BITS: u32 = 8;

/// Full-scale gain: one, in [`GAIN_BITS`] fractional bits.
const GAIN_ONE: u16 = 1 << GAIN_BITS;

/// Partials a voice can sound at once.
///
/// An array rather than a `Vec` because the feed allocates nothing, so this is
/// the voice's state. Two is what the family timbre uses; four leaves room
/// without costing anything a partial does not already cost.
const MAX_PARTIALS: usize = 4;

/// The family timbre every generated sound shares: a fundamental and one
/// inharmonic partial 2.76x above it, the ratio a struck bell's second partial
/// sits at.
///
/// **Inharmonic on purpose.** A bell's partials are not integer multiples of its
/// fundamental, and integer multiples are what makes a synthesised tone a
/// buzzer — which is why the sound this replaced had to be stored as samples
/// rather than generated at all.
///
/// Gains are [`GAIN_ONE`] across the pair, so the partials together reach no
/// further than one alone would. That cap is what stops a note whose partials
/// happen to line up from clipping into the click the ramps exist to prevent.
const TIMBRE: [(u16, u16); 2] = [(100, 180), (276, 76)];

/// The single-partial timbre the synthesiser started as, kept so the additive
/// path can be held to producing exactly what it replaced — see
/// `a_lone_partial_is_the_oscillator_it_replaced`.
pub const PLAIN: [(u16, u16); 1] = [(100, GAIN_ONE)];

/// One turn of a sine, sampled at [`WAVE_ENTRIES`] even points and scaled to the
/// rail.
///
/// Built at compile time: `libm::sinf` is not a `const fn`, so the entries come
/// from the polynomial in [`sine`], which its test holds to within one LSB of
/// the `sinf` this replaced.
///
/// `i16` rather than `f32` because the read side is what runs in an interrupt,
/// where an FPU instruction is not slow but fatal: see [`Tone::next_sample`].
/// Keeping the interpolation in the integer domain leaves the per-sample path
/// with nothing but integer arithmetic.
static WAVE: [i16; WAVE_END] = {
    let mut wave = [0_i16; WAVE_END];
    let mut i = 0;
    while i < WAVE_END {
        let turns = i as f32 / WAVE_ENTRIES as f32;
        let phase = core::f32::consts::TAU * turns;
        // Scaling by the rail here means `WAVE[i]` already carries the intended
        // level and the envelope is the only thing left to scale it by.
        wave[i] = rounded(sine(phase) * TONE_PEAK_LSB);
        i += 1;
    }
    // The first entry's interpolation partner, so the last segment of a turn
    // lands back on the value the turn started from.
    wave[WAVE_ENTRIES] = wave[0];
    wave
};

/// A sample at the rail, rounded rather than truncated.
///
/// `as i16` truncates, and a table built that way is a full LSB low everywhere
/// before interpolation even starts — which is most of the error the table's
/// accuracy claim has to leave room for.
const fn rounded(scaled: f32) -> i16 {
    let biased = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    biased as i16
}

/// `sinf` for a compile-time table: a whole turn folded into a quarter of one,
/// then Taylor over that.
///
/// Folding first is what keeps the series short — Taylor diverges towards `2π`,
/// and over the quarter turn the truncation is around 4e-8 relative, a small
/// fraction of the one LSB [`WAVE`] is held to. Compile time only: [`WAVE`] is
/// its only caller.
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
    /// The notes to walk, parsed from the sound's MIDI file.
    ///
    /// Owned rather than borrowed because the driver holds the note table it
    /// parsed and the voice that walks it in one struct — a borrowed table would
    /// be a field pointing into another field of the same value.
    score: Vec<Note>,
    /// The partials this voice sounds: a frequency ratio in hundredths and a
    /// [`GAIN_ONE`]-scale gain.
    timbre: &'static [(u16, u16)],
    note: usize,
    /// Oscillator position per partial, in [`PHASE_BITS`]-fractional units of a
    /// table entry.
    phase: [u32; MAX_PARTIALS],
    /// Phase units one sample advances per partial — the note's frequency times
    /// its ratio, and zero for a rest.
    phase_step: [u32; MAX_PARTIALS],
    samples: u32,
    /// Ramp length in samples; zero means the note is played flat out.
    ramp: u32,
    into_note: u32,
    /// The ramp as a Q16 multiplier per sample, divided out once per note rather
    /// than per sample. Recomputed on every note would cost a 64-bit divide in
    /// the interrupt, which is the same cost this type exists to avoid.
    inv_ramp: u32,
}

impl Tone {
    /// A voice sounding `score` on the family [`TIMBRE`].
    pub fn playing(score: Vec<Note>) -> Self {
        Self::on(score, &TIMBRE)
    }

    /// A voice sounding `score` on `timbre`.
    ///
    /// # Panics
    ///
    /// On a timbre that is empty or longer than [`MAX_PARTIALS`]: the first
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

    /// Moves to note `index`, zeroing the phases so every note starts at the same
    /// point of its cycles — a note that began mid-cycle would open on whatever
    /// the last one happened to leave in the oscillators.
    fn enter(&mut self, index: usize) {
        self.note = index;
        self.into_note = 0;
        self.phase = [0; MAX_PARTIALS];
        let Some(note) = self.score.get(index) else {
            // Past the last note: a zero-length one, so the fill loop stops on
            // `done` rather than spinning on a score that ran out.
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
            // A single subtract on wrap is only enough while a step stays inside
            // a turn; a note at or above the sample rate times its ratio would
            // wrap more than once, and is a score that cannot be played rather
            // than one this has to survive.
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

    /// The note's amplitude right now, as a Q16 multiplier: a ramp in over the
    /// first `ramp_ms` and out over the last.
    fn envelope(&self) -> i32 {
        if self.ramp == 0 {
            return Q16_ONE;
        }
        // The nearer of the two edges is what the note is under, and the ramp is
        // the ceiling on both — which bounds this at `Q16_ONE` and so bounds the
        // sample below it, with no saturation needed anywhere.
        let peak = self
            .into_note
            .min(self.samples - self.into_note)
            .min(self.ramp);
        ((u64::from(peak) * u64::from(self.inv_ramp)) >> 16) as i32
    }

    /// One sample: every partial summed and weighted, and the oscillators and
    /// note position stepped past it.
    ///
    /// Integer from end to end, because this runs in the feed interrupt and an
    /// FPU instruction there raises `Cp0Disabled`: the board stops on the first
    /// tap frame with no reset, from a path with no handler to survive it. It
    /// is a property of the interrupt context, not of the arithmetic — the same
    /// code on the cooperative executor is fine.
    ///
    /// The gains are taken off **before** the envelope, not after: at full
    /// envelope a weighted partial is `peak × 256`, and multiplying that by a
    /// 16-bit envelope overflows `i32` part-way up. Shifting the gain off first
    /// divides the whole sum by [`GAIN_ONE`] exactly, which is also why a lone
    /// partial at [`GAIN_ONE`] lands back on the unweighted oscillator — see
    /// `a_lone_partial_is_the_oscillator_it_replaced`.
    fn next_sample(&mut self) -> i16 {
        let envelope = self.envelope();
        let mut sum = 0i32;
        for (partial, &(_, gain)) in self.timbre.iter().enumerate() {
            let at = self.phase[partial];
            let whole = (at >> PHASE_BITS) as usize;
            let fraction = (at & PHASE_MASK) as i32;
            let (low, high) = (WAVE[whole] as i32, WAVE[whole + 1] as i32);
            // Interpolating between entries is what makes a coarse table sound
            // like a fine one: it costs one multiply and a shift, and lands
            // within an LSB of the oscillator the table was built from.
            let interpolated =
                low + (((high - low) * fraction + (1 << (PHASE_BITS - 1))) >> PHASE_BITS);
            sum += interpolated * i32::from(gain);
            self.phase[partial] = at + self.phase_step[partial];
            if self.phase[partial] >= WAVE_TURN {
                self.phase[partial] -= WAVE_TURN;
            }
        }
        // Rounded rather than truncated: the two fixed-point steps here would
        // otherwise each throw away up to an LSB, and together they are the
        // difference between a table that reproduces the oscillator it was built
        // from and one that audibly does not. One add is cheaper than that.
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

/// Fractional bits the sweep's phase carries beyond [`WAVE_TURN`].
const SWEEP_BITS: u32 = Q16_BITS;

/// One whole turn of the table at sweep resolution.
const SWEEP_TURN: u64 = (WAVE_TURN as u64) << SWEEP_BITS;

/// The level the sweep runs at, and **a measurement parameter rather than a
/// taste one**.
///
/// `[实测]` at half the rail the microphone's recording peaked at 32 767: the ADC
/// was clipping. A clipped signal carries the amplifier's harmonics instead of
/// the room's response, so it neither correlates with the sweep that produced it
/// nor deconvolves into anything — deconvolution cannot undo it, because the
/// harmonics are *in* the recording. A fifth of the rail is what left headroom,
/// and the margin is deliberate: a recorder aimed at a peak between −6 and −3
/// dBFS has room for the room to be louder than the sweep.
const SWEEP_PEAK_LSB: i32 = 2_500;

impl Sweep {
    /// A sweep from `from_hz` to `to_hz` over `milliseconds`, looping without a
    /// step — see [`SampleSource::fill`] for what makes the join clean.
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

    /// Back to the bottom of the band for the next pass, **keeping the phase
    /// where it is**.
    ///
    /// `[实测]` resetting the phase as well put a 10 567 LSB step at the join,
    /// against a steepest ordinary step of 8 797 — a click, because the phase a
    /// whole sweep accumulates is only *nearly* a whole number of turns (630
    /// cycles, plus whatever the two truncated divisions leave behind), and
    /// zeroing it drops that remainder into the waveform as a discontinuity.
    /// Carrying it costs nothing and makes the join one ordinary step: the
    /// frequency jumps, the value does not.
    fn rewind(&mut self) {
        self.step = self.from_step;
        self.step_delta = self.step_delta_base;
        self.index = 0;
    }

    /// One sample, and the oscillator and note position stepped past it.
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
    /// **Never runs out.** A sweep is the excitation a room is measured with, and
    /// a driver that has to be re-armed mid-measurement puts a click and a gap
    /// in the recording for every re-arm — which is the measurement being ruined
    /// by the act of taking it. Looping here also closes: 300 Hz to 6 kHz over
    /// 200 ms is exactly six hundred and thirty cycles, so the phase comes back
    /// to where it started and the waveform joins itself.
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

/// Whether a sound is still audible, from whether its source has anything left
/// and what the arm in flight is carrying.
///
/// A source that has run out is not the end of the sound: the arm holding its
/// last frames has to play out first, which is what `arm_sounded` is for. The
/// sound ends once the source is spent *and* nothing audible is in flight.
///
/// A pure function on purpose. This is the one decision in the transport that is
/// easy to invert and invisible until hardware, because nothing about it can be
/// seen from the host — the arm only exists on the board. Getting it backwards
/// strands the page in `Playing` for ever: a spent source keeps arming silent
/// rings, and every one of them answers "still going".
#[cfg(test)]
pub const fn still_sounding(source_done: bool, arm_sounded: bool, arm_in_flight: bool) -> bool {
    !source_done || (arm_sounded && arm_in_flight)
}

/// How often a streaming ring is fed.
///
/// A board's driver keeps one streaming transfer alive by refilling its ring on
/// this cadence, silence when nothing plays and the sound when one does — so
/// this is the rate the capture's clocks run at, not a latency path. A feed is a
/// memory write bounded by what the DMA has already finished, so a feed a little
/// late costs headroom in the ring and nothing audible; only a gap wider than
/// [`PlayStreamWatchdog`]'s limit can run a ring dry.
pub const FEED_MS: u32 = 5;

const WIRE_BYTES_PER_SEC: usize = BYTES_PER_FRAME * SAMPLE_RATE_HZ as usize;

/// How long the DMA needs to play a whole ring out, and so the longest a healthy
/// stream can leave the wire idle before it is fair to call it drained. Measured
/// in bytes rather than in frames, because a ring is drained a descriptor at a
/// time and the rate the wire moves bytes at is two slots a frame, not one.
const fn ring_play_ms(ring_bytes: usize) -> u64 {
    ring_bytes as u64 * 1_000 / WIRE_BYTES_PER_SEC as u64
}

/// Notices a transmit stream the DMA has genuinely run dry, by the clock rather
/// than by the DMA.
///
/// The peripheral's own flag cannot answer this: `tx_idle` reports that the FIFO
/// is empty and no descriptor is in flight *at this instant*, which for a stream
/// fed a cadence is the ordinary state between two feeds, and says nothing about
/// whether the ring still has runway. So the flag is only ever taken as "busy or
/// idle this feed", and the verdict is left to time — the same signal
/// [`crate::drivers::audio::CaptureWatchdog`] trusts on the capture side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayStreamWatchdog {
    /// When the DMA was last seen with something to send, or the first feed that
    /// found it idle — the deadline a stream gets before it has to prove itself.
    last_busy_ms: Option<u64>,
    /// Idleness tolerated before a drain is called: one ring's playthrough — the
    /// longest a stream can be late and still be whole — plus one feed, so a feed
    /// that is merely due is never mistaken for a dry one.
    idle_limit_ms: u64,
    /// Whether the drain this stream is in has already been reported, so a stream
    /// that stays dry is one report rather than one per feed for ever.
    reported: bool,
}

impl PlayStreamWatchdog {
    /// Watches a stream whose ring is `ring_bytes` long. Unarmed until the first
    /// feed, so a driver can be built before it has a clock.
    pub const fn new(ring_bytes: usize) -> Self {
        Self {
            last_busy_ms: None,
            idle_limit_ms: ring_play_ms(ring_bytes) + FEED_MS as u64,
            reported: false,
        }
    }

    /// Whether the stream has been idle too long, given whether the DMA had
    /// anything to send this feed. A busy feed counts as progress and pushes the
    /// deadline out again.
    ///
    /// The state, and true for as long as the stream stays dry. A driver that
    /// wants to say so once wants [`report_drained`](Self::report_drained).
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

    /// The whole of a stream's runway, for a driver that has to describe what it
    /// is watching.
    pub const fn idle_limit_ms(&self) -> u64 {
        self.idle_limit_ms
    }
}

/// Which of a speaker's two fallible calls was refused.
///
/// Named rather than inferred so a log says what was being attempted when the
/// codec fell silent, instead of only that something did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerOp {
    /// A [`Speaker::play`] the codec refused to start.
    Play,
    /// A [`Speaker::set_muted`] the codec refused to latch.
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
///
/// One named type for the whole trait rather than an associated type per
/// implementation: the app holds `Box<dyn Speaker>`, and a trait object cannot
/// be written down without naming its error. A board keeps its own rich error and
/// converts here, because the app only reports a refusal and never branches on
/// which failure it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerFault {
    /// Which command was refused.
    pub op: SpeakerOp,
    /// The board's own words for why, so a log names the real cause rather than
    /// a generic "the codec did not answer".
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

/// What a driver had to repair after its last [`feed`](Speaker::feed) found the
/// output stream drained, handed back so the repair can be reported rather than
/// only performed.
///
/// Named for the same reason [`SpeakerFault`] is: the feed runs as a `#[task]`, a
/// task cannot be generic, so the driver reaches it as a trait object — and a
/// trait object cannot spell an unnamed type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recovery {
    /// Writable bytes left in the ring at the moment the drain was noticed. A
    /// whole ring free means the DMA played everything and then found nothing;
    /// anything less means it stopped with audio still queued behind it. Which of
    /// the two it was is what localises the fault.
    pub free_bytes: usize,
    /// Whether a sound was on the ring when it drained — what the stall cost the
    /// listener.
    pub playing: bool,
}

/// The playback surface a board hands the app: make a catalogue sound and latch
/// the output quiet. Fallible because both reach I2C, and a codec that stops
/// answering has to be able to say so.
///
/// `Send + 'static` because the feed may run from a higher-priority interrupt
/// than the rest of the app, and nothing that does not cross that boundary can be
/// fed from there.
///
/// Three steps, not one: [`play`](Self::play) starts a sound,
/// [`feed`](Self::feed) advances it, and the state waits for the driver to say it
/// is done. Only `feed` is bounded and infallible, so only it may run where a
/// delay is a fault — which is why the app owns the cadence and the board the
/// ring.
pub trait Speaker: Send + 'static {
    /// Starts `sound` from the beginning, replacing anything playing.
    fn play(&mut self, sound: Sound) -> Result<(), SpeakerFault>;

    /// Latches the output quiet, or audible again.
    fn set_muted(&mut self, muted: bool) -> Result<(), SpeakerFault>;

    /// Advances the sound by one cadence and reports whether it is still going; a
    /// no-op while nothing plays, so the caller can ask unconditionally.
    ///
    /// `now_ms` is the caller's own millisecond count, which is what a driver needs
    /// to tell a stream that has run dry from one merely between two feeds. The app
    /// owns the cadence and so owns the clock.
    ///
    /// Infallible and bounded on purpose: this writes only to the memory the DMA is
    /// reading, so there is no bus to fail on, and it may not log, allocate or take
    /// a lock — which is what makes it the one method allowed to run from an
    /// interrupt, and why a drained stream is *recorded* here and repaired by
    /// [`recover`](Self::recover).
    fn feed(&mut self, now_ms: u64) -> bool;

    /// Performs and reports the repair a previous [`feed`](Self::feed) found
    /// necessary, and `None` when there was nothing to do.
    ///
    /// Split from `feed` because the repair is the opposite of what `feed` is
    /// allowed to be: rebuilding a DMA ring allocates, and the log channel does
    /// not serialise writers, so a line printed from a feed would interleave with
    /// whatever the preempting task had half-sent. The allocation is the hard
    /// constraint — a cooperative executor can allocate, an interrupt handler
    /// cannot — and the output is the reason the repair reports from here rather
    /// than from where it is noticed.
    ///
    /// So the app calls this from the cooperative side, next to the I2C calls it
    /// already keeps there.
    fn recover(&mut self) -> Option<Recovery>;
}

/// A speaker that is not wired, for a board that has an output surface in the
/// product but nothing driving it on this hardware. `HasPlayback` names its
/// speaker as a type rather than an `Option` of a trait object, so a board with
/// no output still has to name something — this is that name, not a silent
/// fallback that would let a page offer a sound nothing can make.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnwiredSpeaker;

impl Speaker for UnwiredSpeaker {
    fn play(&mut self, _sound: Sound) -> Result<(), SpeakerFault> {
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
    // Only the tests compare against it: the oscillator itself reads the table.
    use crate::drivers::audio::SLOTS_PER_FRAME;
    use alloc::vec;
    use alloc::vec::Vec;
    use libm::sinf;

    /// Stands for a rest in a fixture's note list. MIDI note 0 is a real pitch
    /// a fifth of an octave under the lowest earcon anyone would write, so the
    /// sentinel costs nothing and saves carrying an `Option` through the
    /// fixtures.
    const REST: u8 = 0;

    /// The tap as it ships: a fifth, then a rest, then a fifth and a half.
    const TAP: &[(u8, u16)] = &[(81, 120), (REST, 40), (88, 180)];

    /// Builds a one-track MIDI file from note numbers and milliseconds.
    ///
    /// A fixture that hand-builds a [`Note`] table would exercise the
    /// synthesiser while skipping the parser — and the parser is now on the path
    /// every sound takes, so a test suite written that way could stay green
    /// against a parser that reads nothing at all.
    fn mid(notes: &[(u8, u16)]) -> Vec<u8> {
        const DIV: u32 = 480;
        /// One quarter note at 120 BPM, which is what the tempo event below says.
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

        // The leading zero is a delta time like any other: every event in a track is
        // preceded by one, a meta event included, and a file that omits it reads
        // its own status byte as part of a duration.
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
        // And the same at the end of the track: the end-of-track event is an
        // event, and events are preceded by delta times.
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

    /// A voice on `notes`, and the whole of it in one buffer.
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

    /// The whole tap in one buffer, so a case can look at the whole sound.
    fn tap() -> (Tone, Vec<i16>) {
        playing(TAP)
    }

    /// Samples a score states, which is the length a fill of it must write.
    fn samples_of<'a>(notes: impl Iterator<Item = &'a Note>) -> usize {
        notes.map(|note| samples_of_ms(note.ms) as usize).sum()
    }

    #[test]
    fn the_table_holds_the_oscillator_it_replaced() {
        // Checked against the very `sinf` it replaced rather than an ideal
        // nobody can build; one LSB at 16 bits is far below the rail's resolution
        // through a DAC.
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
        // Interpolating between entries is what makes a coarse table sound like a
        // fine one, so the claim is measured over a whole turn at a step finer
        // than any note in the score — the worst place a table's error shows.
        //
        // Driven through integer phase units exactly as `next_sample` does,
        // rather than through a float of its own: a test that recomputes the
        // arithmetic differently from the code would pass while the code drifted,
        // and the one thing worth pinning here is that this path — which cannot
        // touch the FPU, and so has no float in it at all — is still the
        // oscillator.
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
        // Driven through `fill` one frame at a time, because that is how the
        // voice is called and because `fill` is what advances the note.
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
        // A lap of the catalogue visits every sound exactly once and comes back
        // to where it started, so a sound added to `ALL` later cannot be left
        // stranded outside the cycle a tap walks. The expectation is the
        // catalogue itself rotated, not a list written out here — the two are the
        // same claim, but only one of them survives the next sound being added.
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
        // The round trip is the whole point of the fixture, so the parsed table
        // has to be the notes the file was given — not merely a table of the
        // right length.
        let sounded: Vec<_> = score
            .iter()
            .filter(|note| note.hz != 0)
            .map(|note| (note.hz, note.ms))
            .collect();
        assert_eq!(sounded, [(880, 119), (1319, 180)], "A5 then E6, a fifth up");
    }

    #[test]
    fn a_tap_opens_and_closes_on_silence() {
        // The whole reason notes carry a ramp: a sound that stopped on whatever
        // its last sample happened to be leaves a step on the wire, and a step
        // is a click — louder than the tap it ends.
        let (_, samples) = tap();
        assert_eq!(samples[0], 0, "and opens on the oscillator's own zero");
        assert!(
            samples[samples.len() - 1].unsigned_abs() < TONE_PEAK_LSB as u16 / 100,
            "and ends inside 1% of the rail, which is the release ramp and not a cut"
        );
        // The middle is the sound itself: a tap nobody can hear is a broken
        // one, and asserting only the ends would not notice. The crest lands
        // within half a sample of the nominal peak, so the level asked for is
        // the level got.
        let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
        assert!(
            (TONE_PEAK_LSB as u16 - 128..=TONE_PEAK_LSB as u16).contains(&loudest),
            "{loudest} LSB is not the nominal peak"
        );
    }

    #[test]
    fn a_note_following_another_is_joined_through_silence() {
        // The outgoing note ends on its release and the incoming one starts on
        // its attack, so the join is a crossing rather than a jump from one
        // wave to another.
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

    /// The sweep the room measurement runs on, and the numbers that describe it.
    const BAND: (u32, u32, u32) = (300, 6_000, 200);

    /// A whole sweep as the board would play it.
    fn sweep_of(band: (u32, u32, u32)) -> Vec<i16> {
        let length = (SAMPLE_RATE_HZ / 1_000 * band.2) as usize;
        let mut out = vec![0_i16; length];
        assert_eq!(Sweep::new(band.0, band.1, band.2).fill(&mut out), length);
        out
    }

    #[test]
    fn a_sweep_is_reproducible_bit_for_bit() {
        // The whole measurement rests on this: the host reconstructs the sweep
        // from the same arithmetic to deconvolve against it, so two sweeps that
        // differ by an LSB would deconvolve into a room with the difference in
        // it.
        assert_eq!(sweep_of(BAND), sweep_of(BAND));
    }

    #[test]
    fn a_sweep_actually_sweeps_instead_of_sitting_on_one_tone() {
        // **The failure this guards is a truncated step.** The frequency moves
        // by a fraction of one phase unit per sample over this band, so a step
        // kept at whole units rounds the increment to zero and the sweep becomes
        // a single tone — which would deconvolve into one sharp reflection and
        // read as a room with no tail at all.
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
        // A sine crosses zero twice a cycle, and a linear sweep's cycle count is
        // the mean frequency times the run — so the count to expect is twice
        // that, and comparing against the cycle count alone would read a
        // perfectly good sweep as running at double speed.
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

        // **The loop is the point**, so it is checked by what it produces rather
        // than by what it reports: a sweep that restarts on a step would show a
        // jump between the last sample of one pass and the first of the next.
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
        // **Half again the steepest ordinary step, and `[measured]` why.** A
        // chirp that repeats has to change frequency somewhere, and where it does
        // one sample advances by a step at the top of the band and the next by a
        // step at the bottom — so the join is bounded by the band's own
        // steepest slope, not by noise or by a rounding remainder. 10 567 against
        // 8 797 is that, and 23 000 would be a click.
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
        // The driver refills a DMA ring in whatever sizes it has room for, so a
        // source that only sounded right when handed a whole sound at once
        // would be useless — and would only show up as a note boundary glitch.
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
        // A board that offers a generated sound must not be able to hang the
        // playback task on a score with nothing in it.
        let mut tone = Tone::playing(Vec::new());
        assert!(tone.done());
        assert_eq!(tone.fill(&mut [0_i16; 16]), 0);
    }

    #[test]
    fn a_file_with_nothing_in_it_is_harmless() {
        // The empty table's way in: a file a DAW exported with every note
        // deleted. Playing it must not hang the playback task either.
        let notes = from_midi(&mid(&[])).expect("an empty track is still a file");
        let mut tone = Tone::playing(notes);
        assert!(tone.done());
        assert_eq!(tone.fill(&mut [0_i16; 16]), 0);
    }

    #[test]
    fn a_rest_is_silence_rather_than_a_hum() {
        // The tap's middle is a rest, and a rest that hummed would be an
        // interval the score never asked for.
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
        // **The additive path is only trustworthy if reducing it to one partial
        // at full gain gives back the synthesis it replaced.**
        //
        // The reference below is that synthesis written out — one table read, one
        // envelope, nothing summed — rather than a block of numbers pasted in
        // earlier. A pasted block pins sixteen samples and says nothing about the
        // rest of a 16 000-sample sound; this compares two implementations over
        // all of it, and it would notice a ramp or an interpolation that drifted
        // anywhere in the score.
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
        // The other half of the same claim: the level a single partial reaches
        // is unchanged by the weighting, so the gain shift is not quietly
        // halving everything.
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
        // Integer multiples of a fundamental are what makes a synthesised tone a
        // buzzer. The second partial is the ratio a struck bell carries, and
        // rounding it to 3 would put it on top of an octave and two — the exact
        // thing this timbre exists to avoid.
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
        // The gains have to sum to one, or two partials that happen to line up
        // would carry the sum past a single one and clip — producing exactly the
        // click at the end of a sound that the ramps exist to prevent.
        let total: u32 = TIMBRE.iter().map(|&(_, gain)| u32::from(gain)).sum();
        assert_eq!(
            total,
            u32::from(GAIN_ONE),
            "the partials' gains have to sum to one",
        );
    }

    #[test]
    fn a_note_shorter_than_its_ramp_is_ramped_not_overshot() {
        // The envelope clamps the ramp to the note's own length on both edges,
        // so a note shorter than the ramp does not divide by a length past its
        // end — which would make the multiplier overshoot a full one.
        let score = from_midi(&mid(&[(81, 1)])).expect("parses");
        let mut tone = Tone::playing(score);
        let mut samples = vec![0_i16; samples_of_ms(1) as usize];
        tone.fill(&mut samples);
        assert!(
            samples.iter().all(|&s| s.unsigned_abs() < 32_768),
            "a note shorter than its ramp still has to stay inside the rail",
        );
    }

    /// A few frames to spread across the slots: the extremes, the zero that
    /// would hide a sign bug, and a small value.
    const FRAMES: [i16; 4] = [i16::MIN, 0, i16::MAX, -1];

    #[test]
    fn every_frame_reaches_both_slots_little_endian() {
        let frames = FRAMES;
        let mut bytes = [0_u8; BYTES_PER_FRAME * FRAMES.len()];
        let written = interleave_mono(&frames, &mut bytes);
        assert_eq!(written, bytes.len());
        for (index, &frame) in frames.iter().enumerate() {
            let frame_bytes = &bytes[index * BYTES_PER_FRAME..][..BYTES_PER_FRAME];
            for slot in frame_bytes.chunks_exact(2) {
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
        // Both directions of audio go over the same wire, so a playback frame
        // sized differently from a capture frame would put playback at twice the
        // capture's rate on the same DMA.
        assert_eq!(BYTES_PER_FRAME, 4);
        assert_eq!(BYTES_PER_FRAME, SLOTS_PER_FRAME as usize * 2);
    }

    #[test]
    fn an_unwired_speaker_never_claims_to_be_playing() {
        let mut speaker = UnwiredSpeaker;
        // A board with no output takes this type, so the one copy of the app runs
        // on it — but a `play` that reported a sound in progress would leave the
        // page in `Playing` with nothing that can ever finish it.
        speaker.play(Sound::Tap).expect("takes a sound");
        speaker.set_muted(true).expect("takes a latch");
        assert!(
            !speaker.feed(0),
            "a sound that was never made is never done playing"
        );
    }

    #[test]
    fn a_sound_ends_when_its_source_is_spent_and_nothing_audible_is_in_flight() {
        // The whole truth table, because this is the decision an inverted `!`
        // hides: with the source spent, a sounded arm in flight is the tail and
        // keeps the sound going, and anything else ends it.
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
        // Asserted over the states a driver actually reaches while winding
        // down, where a silent arm must not read as a sound in progress.
        assert!(
            !still_sounding(true, false, true),
            "the silent arm a spent source arms must not read as a sound in progress"
        );
        assert!(!still_sounding(true, false, false));
        assert!(!still_sounding(true, true, false));
    }

    /// The ring the bsp driver builds, so these cases measure the real geometry
    /// rather than a shape picked to make an assertion pass.
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
        // `tx_idle` goes high whenever the FIFO is momentarily empty, which is
        // the ordinary state between two feeds, so the walk has to span a whole
        // ring's worth for no unlucky cadence to hide it.
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
        // One feed cadence past the ring's own playthrough: by then the CPU has
        // had the time it would need to fill the ring, so an idle this long is
        // the DMA not running rather than a feed merely due.
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
        // A stream that is busy on and off all evening is healthy, and a watchdog
        // that only remembered the first idle would eventually call it drained
        // mid-sound — the same destructive false positive in slower motion.
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let limit = watchdog.idle_limit_ms();
        let mut now_ms = 0_u64;
        while now_ms < 10 * limit {
            assert!(
                !watchdog.drained(now_ms, false),
                "a busy feed is never a drain"
            );
            // An idle stretch, then the DMA is seen working again.
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
        // A stream that comes back and dries again is a new thing to report.
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
        // A driver is built before the playback loop has a clock, so the first
        // feed it ever sees is the baseline: it must not come up already failing,
        // and it must measure from there rather than from zero.
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
        // The clock is walked feed by feed across many spells rather than
        // jumping, so a deadline that crept would show up as a false positive
        // inside a spell it should have stayed quiet through.
        let mut watchdog = PlayStreamWatchdog::new(RING_BYTES);
        let limit = watchdog.idle_limit_ms();
        let mut now_ms = 0_u64;
        let mut reports = 0_u32;
        for _ in 0..50 {
            // A healthy spell: the stream is busy, with a one-cadence idle between
            // feeds, which is the ordinary shape. It ends busy, because that is
            // what moves the deadline the dry spell is measured from.
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
            // A dry spell: past the limit the first feed reports, and the ones after
            // it are the same spell — including the feed that recovers the ring.
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
            // The recovery: the stream is busy again, which both closes the spell
            // and arms the next one.
            now_ms += FEED_MS as u64;
            assert!(!watchdog.report_drained(now_ms, false));
        }
        assert_eq!(reports, 50, "every spell reported, and only once each");
    }
}

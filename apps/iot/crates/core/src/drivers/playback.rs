//! Playback: the sounds a device can make, the sample sources behind them, and
//! the surface a board hands the app to make them on.
//!
//! Everything here is arithmetic on a buffer, so it builds and is tested on the
//! host — the peripheral that carries it away (a DMA ring) is the bsp crate's
//! problem, not this one's.

use crate::drivers::audio::{BYTES_PER_FRAME, SAMPLE_RATE_HZ};

/// A sound in the catalogue, in the order a tap cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    /// A tone synthesised as it plays: no asset, nothing in flash, and
    /// retunable by editing a score.
    #[default]
    Chime,
    /// A PCM blob linked into the firmware and read straight out of flash.
    Asset,
}

impl Sound {
    /// The whole catalogue in cycle order, so a walker and a test cannot
    /// disagree about what the tap after the last sound does.
    pub const ALL: [Self; 2] = [Self::Chime, Self::Asset];

    /// The next sound in the catalogue, wrapping: what one tap on the Speaker
    /// page plays. `const` because the page's state is a `const` too.
    pub const fn next(self) -> Self {
        match self {
            Self::Chime => Self::Asset,
            Self::Asset => Self::Chime,
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

/// One note of a generated score: its frequency, how long it lasts, and the
/// ramp each end gets. `hz == 0` is a rest, and a rest needs no ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub hz: u16,
    pub ms: u16,
    pub ramp_ms: u16,
}

/// The chime: two notes with a rest between them, so it reads as one sound
/// rather than as a beep, and is over in a third of a second — a one-shot
/// answer to a tap, not a tune to sit through.
pub const CHIME: &[Note] = &[
    Note {
        hz: 880,
        ms: 120,
        ramp_ms: 12,
    },
    Note {
        hz: 0,
        ms: 40,
        ramp_ms: 0,
    },
    Note {
        hz: 1320,
        ms: 180,
        ramp_ms: 40,
    },
];

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

/// A generated sound: one oscillator walking a score, one note at a time.
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    score: &'static [Note],
    note: usize,
    /// Oscillator position, in [`PHASE_BITS`]-fractional units of a table entry.
    phase: u32,
    /// Phase units one sample advances — the note's frequency, and zero for a
    /// rest.
    phase_step: u32,
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
    /// The chime, the sound [`Sound::Chime`] means.
    pub fn chime() -> Self {
        Self::playing(CHIME)
    }

    /// A tone playing `score` from its first note.
    pub fn playing(score: &'static [Note]) -> Self {
        let mut tone = Self {
            score,
            note: 0,
            phase: 0,
            phase_step: 0,
            samples: 0,
            ramp: 0,
            into_note: 0,
            inv_ramp: 0,
        };
        tone.enter(0);
        tone
    }

    /// Moves to note `index`, zeroing the phase so every note starts at the same
    /// point of its cycle — a note that began mid-cycle would open on whatever
    /// the last one happened to leave in the oscillator.
    fn enter(&mut self, index: usize) {
        self.note = index;
        self.phase = 0;
        self.into_note = 0;
        let Some(note) = self.score.get(index) else {
            // Past the last note: a zero-length one, so the fill loop stops on
            // `done` rather than spinning on a score that ran out.
            self.samples = 0;
            self.ramp = 0;
            self.phase_step = 0;
            self.inv_ramp = 0;
            return;
        };
        self.samples = samples_of_ms(note.ms);
        self.ramp = samples_of_ms(note.ramp_ms);
        // Phase units rather than radians, so the sample path wraps by
        // comparison instead of dividing. Widened for the multiply because a
        // table turn times a frequency overflows `u32` well below Nyquist.
        self.phase_step =
            (u64::from(WAVE_TURN) * u64::from(note.hz) / u64::from(SAMPLE_RATE_HZ)) as u32;
        // A single subtract on wrap is only enough while a step stays inside a
        // turn; a note at or above the sample rate would wrap more than once and
        // is a score that cannot be played, not one this has to survive.
        debug_assert!(self.phase_step < WAVE_TURN, "a note cannot turn a sample");
        self.inv_ramp = if self.ramp == 0 {
            0
        } else {
            (u64::from(Q16_ONE as u32) * u64::from(Q16_ONE as u32) / u64::from(self.ramp)) as u32
        };
    }

    /// The note's amplitude right now, as a Q16 multiplier: a ramp in over the
    /// first `ramp_ms` and out over the last. A note that started or stopped on
    /// a step edge would click, and a click is louder than a chime.
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

    /// One sample, and the oscillator and note position stepped past it.
    ///
    /// Integer from end to end, because this runs in the feed interrupt and an
    /// FPU instruction there raises `Cp0Disabled`: the board stops on the first
    /// chime frame with no reset, from a path with no handler to survive it. It
    /// is a property of the interrupt context, not of the arithmetic — the same
    /// code on the cooperative executor is fine.
    fn next_sample(&mut self) -> i16 {
        let whole = (self.phase >> PHASE_BITS) as usize;
        let fraction = (self.phase & PHASE_MASK) as i32;
        let (low, high) = (WAVE[whole] as i32, WAVE[whole + 1] as i32);
        // Interpolating between entries is what makes a coarse table sound like a
        // fine one: it costs one multiply and a shift, and lands within an LSB of
        // the oscillator the table was built from.
        let interpolated =
            low + (((high - low) * fraction + (1 << (PHASE_BITS - 1))) >> PHASE_BITS);
        // Rounded rather than truncated: the two fixed-point steps here would
        // otherwise each throw away up to an LSB, and together they are the
        // difference between a table that reproduces the oscillator it was built
        // from and one that audibly does not. One add is cheaper than that.
        let sample = (interpolated * self.envelope() + (1 << (Q16_BITS - 1))) >> Q16_BITS;
        self.phase += self.phase_step;
        if self.phase >= WAVE_TURN {
            self.phase -= WAVE_TURN;
        }
        self.into_note += 1;
        sample as i16
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

/// A PCM blob read straight from flash: mono 16-bit little-endian samples at
/// [`SAMPLE_RATE_HZ`], the format [`Sound::Asset`] ships. A blob of the wrong
/// rate or width is not detected here — the codec would play it at this rate,
/// and the format is a property of the file that only the file's author knows.
#[derive(Debug, Clone, Copy)]
pub struct Pcm<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Pcm<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }
}

impl SampleSource for Pcm<'_> {
    fn fill(&mut self, out: &mut [i16]) -> usize {
        let mut filled = 0;
        for slot in out.iter_mut() {
            let Some(pair) = self.bytes.get(self.cursor..self.cursor + 2) else {
                break;
            };
            *slot = i16::from_le_bytes([pair[0], pair[1]]);
            self.cursor += 2;
            filled += 1;
        }
        filled
    }

    fn done(&self) -> bool {
        // A trailing odd byte is a malformed blob, and it is left unread rather
        // than played as half a sample: one wrong sample at the end of a sound
        // is a click, which is worse than a byte nobody notices is missing.
        self.cursor + 1 >= self.bytes.len()
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

/// Bytes the wire carries a second: a frame is [`BYTES_PER_FRAME`] wide and
/// [`SAMPLE_RATE_HZ`] of them go out a second.
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

    /// The whole chime in one buffer, so a case can look at the whole sound.
    fn chime() -> (Tone, Vec<i16>) {
        let samples = CHIME
            .iter()
            .map(|note| samples_of_ms(note.ms) as usize)
            .sum();
        let mut tone = Tone::chime();
        let mut out = vec![0_i16; samples];
        let written = tone.fill(&mut out);
        assert_eq!(written, samples, "the score is the length it states");
        assert!(tone.done(), "a filled chime has run out");
        (tone, out)
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
    fn a_chime_stays_inside_the_table_it_indexes() {
        // Driven through `fill` one frame at a time, because that is how the
        // voice is called and because `fill` is what advances the note.
        let mut tone = Tone::chime();
        let mut frame = [0_i16; 1];
        for _ in 0..16_320 {
            assert!(
                tone.phase < WAVE_TURN,
                "phase {} left the table",
                tone.phase,
            );
            assert_eq!(tone.fill(&mut frame), 1);
        }
        assert!(tone.done(), "the chime ran out rather than spinning");
    }

    #[test]
    fn the_catalogue_cycles_and_wraps() {
        assert_eq!(Sound::Chime.next(), Sound::Asset);
        assert_eq!(Sound::Asset.next(), Sound::Chime);
        // A lap of the catalogue visits every sound exactly once and comes back
        // to where it started, so a sound added to `ALL` later cannot be left
        // stranded outside the cycle a tap walks.
        let mut lap = [Sound::Chime; Sound::ALL.len()];
        let mut walked = Sound::Chime;
        for slot in &mut lap {
            walked = walked.next();
            *slot = walked;
        }
        assert_eq!(lap, [Sound::Asset, Sound::Chime]);
        assert_eq!(walked, Sound::Chime, "a lap returns to the head");
        for sound in Sound::ALL {
            assert!(
                lap.contains(&sound),
                "{sound:?} is catalogued but unreachable"
            );
        }
    }

    #[test]
    fn a_chime_runs_for_exactly_the_length_its_score_states() {
        let stated: u32 = CHIME.iter().map(|note| samples_of_ms(note.ms)).sum();
        assert_eq!(stated, 16_320, "340 ms of 48 kHz");
        let (_, samples) = chime();
        assert_eq!(samples.len() as u32, stated);
    }

    #[test]
    fn a_chime_opens_and_closes_on_silence() {
        // The whole reason notes carry a ramp: a sound that stopped on whatever
        // its last sample happened to be leaves a step on the wire, and a step
        // is a click — louder than the chime it ends.
        let (_, samples) = chime();
        assert_eq!(samples[0], 0, "and opens on the oscillator's own zero");
        assert!(
            samples[samples.len() - 1].unsigned_abs() < TONE_PEAK_LSB as u16 / 100,
            "and ends inside 1% of the rail, which is the release ramp and not a cut"
        );
        // The middle is the sound itself: a chime nobody can hear is a broken
        // one, and asserting only the ends would not notice. The crest lands
        // within half a sample of the nominal peak, so the level asked for is
        // the level got.
        let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
        assert!(
            (TONE_PEAK_LSB as u16 - 128..=TONE_PEAK_LSB as u16).contains(&loudest),
            "{loudest} LSB is not the nominal peak"
        );
    }

    /// Two notes with no rest between them, which is the case a step edge would
    /// actually appear in. A `const` rather than a `let` because a score is
    /// borrowed for `'static`: it has to outlive the tone reading it.
    const JOINED: [Note; 2] = [
        Note {
            hz: 880,
            ms: 60,
            ramp_ms: 12,
        },
        Note {
            hz: 1320,
            ms: 60,
            ramp_ms: 12,
        },
    ];

    #[test]
    fn a_note_following_another_is_joined_through_silence() {
        // The outgoing note ends on its release and the incoming one starts on
        // its attack, so the join is a crossing rather than a jump from one
        // wave to another.
        let mut tone = Tone::playing(&JOINED);
        let mut samples = [0_i16; 6_000];
        tone.fill(&mut samples);
        let boundary = samples_of_ms(JOINED[0].ms) as usize;
        assert!(
            samples[boundary - 1..=boundary]
                .iter()
                .all(|&s| s.abs() < 500),
            "the join is a crossing, not a step"
        );
    }

    #[test]
    fn a_chime_never_reaches_the_rail() {
        let (_, samples) = chime();
        let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
        assert!(
            loudest < 32_768,
            "{loudest} LSB hit the rail, which is the headroom the DAC volume needs"
        );
    }

    #[test]
    fn filling_a_chime_in_pieces_is_the_same_stream_as_filling_it_at_once() {
        // The driver refills a DMA ring in whatever sizes it has room for, so a
        // source that only sounded right when handed a whole sound at once
        // would be useless — and would only show up as a note boundary glitch.
        let mut whole = Tone::chime();
        let mut at_once = vec![0_i16; 4_800];
        whole.fill(&mut at_once);

        let mut piecemeal = Tone::chime();
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
        let mut tone = Tone::chime();
        let mut samples = [0_i16; 512];
        assert!(!tone.done(), "a fresh tone has its whole score ahead of it");
        let mut total = 0;
        while !tone.done() {
            total += tone.fill(&mut samples);
        }
        assert_eq!(total, 16_320);
        assert_eq!(tone.fill(&mut samples), 0, "and stays empty afterwards");
    }

    #[test]
    fn an_empty_score_is_harmless() {
        // A board that offers a generated sound must not be able to hang the
        // playback task on a score with nothing in it.
        let mut tone = Tone::playing(&[]);
        assert!(tone.done());
        assert_eq!(tone.fill(&mut [0_i16; 16]), 0);
    }

    #[test]
    fn a_rest_is_silence_rather_than_a_hum() {
        let mut tone = Tone::playing(&[Note {
            hz: 0,
            ms: 100,
            ramp_ms: 0,
        }]);
        let mut samples = [1_i16; 1_000];
        assert_eq!(tone.fill(&mut samples), 1_000);
        assert!(
            samples.iter().all(|&s| s == 0),
            "a zero-frequency note is the gap between two that are not"
        );
    }

    #[test]
    fn a_pcm_blob_is_decoded_little_endian_and_stops_at_its_end() {
        let blob = [0x00, 0x80, 0xFF, 0x7F, 0x01, 0x00];
        let mut pcm = Pcm::new(&blob);
        let mut out = [0_i16; 8];
        assert_eq!(pcm.fill(&mut out), 3, "three whole samples");
        assert_eq!(&out[..3], &[i16::MIN, i16::MAX, 1]);
        assert!(pcm.done());
        assert_eq!(pcm.fill(&mut out), 0, "and no more");
    }

    #[test]
    fn a_trailing_byte_is_left_unread_rather_than_halved() {
        // A blob one byte short of a whole sample would otherwise put half a
        // sample on the wire, which is a click at the end of every play.
        let blob = [0x00, 0x80, 0x11];
        let mut pcm = Pcm::new(&blob);
        let mut out = [0_i16; 4];
        assert_eq!(pcm.fill(&mut out), 1);
        assert!(pcm.done());
    }

    #[test]
    fn an_empty_blob_plays_nothing_at_all() {
        let mut pcm = Pcm::new(&[]);
        assert!(pcm.done());
        assert_eq!(pcm.fill(&mut [0_i16; 4]), 0);
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
        speaker.play(Sound::Chime).expect("takes a sound");
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

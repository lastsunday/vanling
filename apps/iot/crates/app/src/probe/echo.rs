//! What the linear AEC costs per audio tick, on this chip, at the clock this
//! image brings the chip up at.
//!
//! **This probe touches no peripheral.** The canceller is arithmetic on two
//! streams, so its cost depends on the tap count and the clock and on nothing
//! about the board — which is what makes it safe to reserve time for it before
//! wiring it in.
//!
//! Per tap count it reports the **first** tick (the delay search against an empty
//! history, standing for no later tick), the **worst** tick (the bound in
//! `architecture.md` is a worst tick, and an average is not one), **the share of a
//! 20 ms tick**, and **where the time goes** — delay search, convolution,
//! metering, which differ in cost and in what changing them costs.
//!
//! Two stream states, and the label matters. An **adapting** filter pays for the
//! convolution *and* the coefficient update; a **frozen** one pays only for the
//! convolution. A converged filter with someone talking over it is frozen
//! (`gate.rs`), so frozen is what barge-in costs and adapting is what far-end-only
//! playback costs; the larger of the two is what the budget covers.
//!
//! Ticks run back to back rather than on a 20 ms schedule, so it measures the work
//! and nothing else — no wake latency, no other task's interference — and the log
//! says so. Each measurement follows an untouched warm-up pass, because a filter
//! that has never run is also paying to bring its own pages in.
use alloc::boxed::Box;
use alloc::vec::Vec;

use embassy_time::Instant;
use iot_core::drivers::aec::{
    AdaptationGate, AdaptiveFilter, Canceller, DelayEstimator, ErleMeter, MAX_STEP_Q30,
    SAMPLE_RATE_HZ, state_bytes,
};

/// The capture path's own tick, in milliseconds. `CAPTURE_MS` in `input.rs`; a tick of
/// anything else would be measuring a product that does not exist.
const TICK_MS: u64 = 20;

/// Samples in one tick at [`SAMPLE_RATE_HZ`].
const TICK_SAMPLES: usize = SAMPLE_RATE_HZ as usize * TICK_MS as usize / 1_000;

/// Ticks per pass over the buffer below. The buffer is a *source* of samples, not the
/// filter's history — the estimator and the filter carry their own state across a pass
/// boundary — so it only has to be long enough to refill between ticks.
const TICKS_PER_PASS: usize = 4;

/// Passes run per tap count: 256 ticks, 5.1 s of audio. The convergence transient takes
/// seconds, and a window shorter than it would report the transient rather than the steady
/// state — which is a different quantity with a very different number.
const PASSES: usize = 32;

/// Peak amplitude of the reference. Loud on purpose: the delay search's power gate skips the
/// search below a level, so a quiet reference measures the cheap case and a loud one the
/// expensive case. The expensive case is the budget.
const REFERENCE_AMPLITUDE: i32 = 9_000;

/// Peak amplitude of the near-end talker, enough to pass the freeze floor so the frozen
/// figures are measured on a filter that has actually stopped.
const SPEECH_AMPLITUDE: i32 = 8_000;

/// The echo path, in ppm per tap: a bulk delay then a decaying series of reflections, the
/// same shape as the acceptance test's room.
const PATH: [(usize, i32); 5] = [
    (12, 700_000),
    (17, -320_000),
    (23, 180_000),
    (31, -90_000),
    (44, 45_000),
];

/// A room: what went to the speaker, and what the microphone heard.
///
/// Heap-allocated rather than a local, and that is not a style preference: a local of this
/// size sits in the main task's stack, and the stack is the shared resource
/// `architecture.md` budgets and the linked ELF measures. A probe spending ten kilobytes
/// of it to measure something that will later run *in* that task is the wrong way round.
struct Room {
    played: [i16; TICK_SAMPLES * TICKS_PER_PASS],
    heard: [i16; TICK_SAMPLES * TICKS_PER_PASS],
}

impl Room {
    const fn new() -> Self {
        Self {
            played: [0; TICK_SAMPLES * TICKS_PER_PASS],
            heard: [0; TICK_SAMPLES * TICKS_PER_PASS],
        }
    }
}

/// A deterministic broadband source. The same one the acceptance test uses, reproduced here
/// because `signal` is test-only and a bench probe is not a test. Broadbandness is the
/// property that matters: a near-periodic reference correlates with itself at nearly every
/// lag, which would make the delay search's confidence gate a matter of luck.
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Self {
        Self(if seed == 0 { 0x9e37_79b9 } else { seed })
    }

    fn next(&mut self, amplitude: i32) -> i16 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        ((self.0 >> 8) as i64 * 2 * i64::from(amplitude) / (1 << 24) - i64::from(amplitude)) as i16
    }
}

/// Refills the room for one pass. `speech` at 0 leaves nobody in it; otherwise the near end
/// starts at the half-way point, which is past convergence, so the talker arrives into a
/// filter that already knows the room — barge-in rather than a startup.
fn fill(room: &mut Room, seed: u32, speech: bool) {
    let mut played = Noise::new(seed);
    let mut talking = Noise::new(!seed);
    let samples = room.played.len();
    for n in 0..samples {
        room.played[n] = played.next(REFERENCE_AMPLITUDE);
        let echo = PATH
            .iter()
            .filter(|(tap, _)| n >= *tap)
            .map(|(tap, ppm)| i64::from(room.played[n - tap]) * i64::from(*ppm) / 1_000_000)
            .sum::<i64>();
        let near = if speech && n >= samples / 2 {
            i64::from(talking.next(SPEECH_AMPLITUDE))
        } else {
            0
        };
        room.heard[n] = (echo + near) as i16;
    }
}

/// What one configuration cost, in microseconds per tick.
#[derive(Default)]
struct Cost {
    first: u64,
    worst: u64,
    total: u64,
    ticks: u32,
}

impl Cost {
    fn record(&mut self, micros: u64) {
        if self.ticks == 0 {
            self.first = micros;
        }
        self.worst = self.worst.max(micros);
        self.total += micros;
        self.ticks += 1;
    }

    fn mean(&self) -> u64 {
        self.total / u64::from(self.ticks.max(1))
    }

    /// Measured occupancy as a share of the tick it has to fit inside. The window rather than
    /// the tick count, so the figure stays right if the two drift.
    fn share(&self) -> u64 {
        self.total * 100 / (u64::from(self.ticks.max(1)) * TICK_MS * 1_000)
    }

    fn mean_as_signed(&self) -> i64 {
        self.mean() as i64
    }
}

/// Refills the room between passes and times every tick `body` performs.
fn timed<F>(room: &mut Room, speech: bool, mut body: F) -> Cost
where
    F: FnMut(&Room, usize),
{
    let mut cost = Cost::default();
    for pass in 0..PASSES {
        fill(room, 1 + pass as u32, speech);
        for tick in 0..TICKS_PER_PASS {
            let at = tick * TICK_SAMPLES;
            let started = Instant::now();
            body(room, at);
            cost.record(started.elapsed().as_micros());
        }
    }
    cost
}

/// Measures one tap count end to end, then attributes the time.
fn measure_taps<const L: usize>(room: &mut Room) {
    log::info!("[ECHO] --- L={L}, {} B of state ---", state_bytes::<L>());

    let mut canceller = Canceller::<L>::new();
    let total = timed(room, false, |room, at| {
        for n in at..at + TICK_SAMPLES {
            canceller.process(room.heard[n], room.played[n]);
        }
    });
    log::info!(
        "[ECHO] L={L:<4} far end only (adapting): first {} us, mean {} us, worst {} us, \
         {}% of a {TICK_MS} ms tick",
        total.first,
        total.mean(),
        total.worst,
        total.share()
    );
    log::info!(
        "[ECHO] L={L:<4}   settled at delay {:?}, ERLE {} dB",
        canceller.delay(),
        canceller.erle_decibels()
    );

    let mut barging = Canceller::<L>::new();
    let frozen = timed(room, true, |room, at| {
        for n in at..at + TICK_SAMPLES {
            barging.process(room.heard[n], room.played[n]);
        }
    });
    log::info!(
        "[ECHO] L={L:<4} with a talker  (frozen):   mean {} us, worst {} us, {}% of a tick, \
         adapting now {}",
        frozen.mean(),
        frozen.worst,
        frozen.share(),
        barging.adapting()
    );

    let mut search = DelayEstimator::new();
    let searched = timed(room, false, |room, at| {
        for n in at..at + TICK_SAMPLES {
            search.update(room.heard[n], room.played[n]);
        }
    });
    log::info!(
        "[ECHO] L={L:<4} delay search alone:       mean {} us, worst {} us, {}% of a tick, \
         {} runs",
        searched.mean(),
        searched.worst,
        searched.share(),
        search.searches()
    );

    // At the module's own maximum step rather than through `AdaptationGate`, so this is
    // the filter's worst case by construction.
    let mut filter = AdaptiveFilter::<L>::new();
    let convolved = timed(room, false, |room, at| {
        for n in at..at + TICK_SAMPLES {
            filter.process(room.heard[n], room.played[n], MAX_STEP_Q30);
        }
    });
    log::info!(
        "[ECHO] L={L:<4} filter alone (adapting): mean {} us, worst {} us, {}% of a tick",
        convolved.mean(),
        convolved.worst,
        convolved.share()
    );

    // The same call with the step at zero, which is the filter frozen: the
    // convolution runs, the coefficient update does not. The difference from the
    // line above is the update loop on its own.
    let mut still = AdaptiveFilter::<L>::new();
    let held = timed(room, false, |room, at| {
        for n in at..at + TICK_SAMPLES {
            still.process(room.heard[n], room.played[n], 0);
        }
    });
    log::info!(
        "[ECHO] L={L:<4} filter alone (frozen):   mean {} us, {}% of a tick; the \
         coefficient update is {} us",
        held.mean(),
        held.share(),
        convolved.mean_as_signed() - held.mean_as_signed()
    );

    // The three meters and the coherence computation: the price of knowing whether adaptation
    // should be running at all.
    let mut gate = AdaptationGate::new();
    let mut meter = ErleMeter::new();
    let overhead = timed(room, false, |room, at| {
        for n in at..at + TICK_SAMPLES {
            gate.update(room.heard[n], room.played[n]);
            meter.update(room.heard[n], room.heard[n]);
        }
    });
    log::info!(
        "[ECHO] L={L:<4} meters alone:            mean {} us, worst {} us, {}% of a tick",
        overhead.mean(),
        overhead.worst,
        overhead.share()
    );
    log::info!(
        "[ECHO] L={L:<4} unattributed remainder:  mean {} us",
        total.mean_as_signed() - searched.mean_as_signed() - convolved.mean_as_signed()
    );
}

/// A deliberately plain FIR, written the way a fast implementation writes one.
///
/// This is the calibration this probe needed and could not have. Its first reports
/// put the canceller at 219 cycles per MAC while Espressif publish `dsps_fir_f32`
/// — 1024 samples through 256 coefficients — at 443,671 cycles on the same
/// ESP32S3, and that gap is not something bounds checks and a reload or two
/// explain. The way out is not a better estimate but a reference run: **the same
/// shape of work, in the same image, on the same board, under the same build**,
/// which turns an absolute claim into a ratio that cannot be wrong about the
/// platform.
///
/// The ratio is what settled it. A loop *simpler* than the canceller's inner
/// loop cannot be much more than a few times slower per MAC, and measuring it at
/// 193× slower is not a property of the loop — it is the clock having been read
/// off the datasheet. See the caller's comment on where the clock comes from.
///
/// Written with iterators over slices on purpose: that is what lets the
/// compiler drop the bounds checks, and the absence of an `&mut self` receiver
/// is the other half of what the canceller's inner loop gets wrong.
fn reference_fir(samples: &[i32], coefficients: &[i32]) -> Vec<i32> {
    samples
        .iter()
        .enumerate()
        .map(|(n, _)| {
            let window = samples.get(..=n).unwrap_or(samples);
            let take = window.len().min(coefficients.len());
            window
                .get(window.len() - take..)
                .unwrap_or_default()
                .iter()
                .rev()
                .zip(coefficients)
                .fold(0i32, |acc, (a, b)| acc.wrapping_add(a.wrapping_mul(*b)))
        })
        .collect()
}

/// Reference work in microseconds per second of audio, and the MAC rate it implies.
struct Reference {
    micros: u64,
    macs: u64,
}

fn measure_reference(room: &mut Room) -> Reference {
    const TAPS: usize = 256;
    const SAMPLES: usize = 1024;

    let coefficients: Vec<i32> = (0..TAPS).map(|k| (k as i32 % 17) - 8).collect();
    // The same window a FIR sees, taken from the room's own reference so the
    // data is the broadband signal the canceller is measured on.
    let samples: Vec<i32> = room.played[..SAMPLES.min(room.played.len())]
        .iter()
        .map(|s| i32::from(*s))
        .collect();

    // One untimed pass so the data is warm and the cost is not first-touch.
    core::hint::black_box(reference_fir(&samples, &coefficients));

    // Each round is logged as it lands. A reference loop that is *also* slow is
    // the finding, and it must not be hidden behind a minute of silence.
    let rounds = 4u64;
    let mut total = 0u64;
    for round in 0..rounds {
        let started = Instant::now();
        let out = reference_fir(&samples, &coefficients);
        let micros = started.elapsed().as_micros();
        total += micros;
        core::hint::black_box(out);
        log::info!("[ECHO]   reference round {round}: {micros} us");
    }
    // The real count, not an approximation of it: the first `TAPS` outputs run
    // short windows, so the total is the triangle plus the full-length remainder.
    // Counting only the full windows overstates the rate by about a sixth.
    let macs = (TAPS as u64 * (TAPS as u64 + 1) / 2) + (SAMPLES - TAPS) as u64 * TAPS as u64;
    Reference {
        micros: total / rounds,
        macs,
    }
}

/// Walks the tap counts and reports what each costs, against `cpu_hz`.
///
/// Returns rather than parking, so a caller can run this on one core and then
/// hand the second core the same job: two runs of the same measurement on the
/// same board are the only comparison that can say whether a second core is
/// worth anything.
pub async fn measure(cpu_hz: u64) {
    let mut room = Box::new(Room::new());
    log::info!("[ECHO] room buffer on the heap, not on the task stack");

    log::info!(
        "[ECHO] linear AEC cost on this chip: {} ticks of {} samples per tap count",
        PASSES * TICKS_PER_PASS,
        TICK_SAMPLES
    );
    log::info!(
        "[ECHO] ticks run back to back, so this is the work and nothing else: a floor for \
         what the product would see, not the loaded figure"
    );
    log::info!(
        "[ECHO] the budget to fit inside is a worst tick of 17-25 ms against a 30 ms bound, \
         per docs/content/development/iot/architecture.md"
    );
    log::info!("[ECHO] no peripheral is touched: the cost belongs to the tap count and the clock");
    log::info!("[ECHO] CPU clock as programmed: {cpu_hz} Hz");

    let reference = measure_reference(room.as_mut());
    // cycles = micros * hz / 1e6, so cycles/MAC divides that by the MAC count.
    let cycles_per_mac = reference.micros * cpu_hz / 1_000_000 / reference.macs;
    log::info!(
        "[ECHO] reference FIR, 256 taps x 1024 samples (the ESP-DSP case): {} us per pass \
         = {cycles_per_mac} cycles/MAC over {} MACs, in i32",
        reference.micros,
        reference.macs,
    );
    // 443,671 cycles over 262,144 MACs at 240 MHz, rescaled to this clock. Kept in
    // tenths because at this clock the number lands near 5 and an integer would
    // round it to a figure that reads like a different claim.
    let published_tenths = 443_671u64 * 240_000_000 * 10 / (cpu_hz * 262_144);
    log::info!(
        "[ECHO]   Espressif publish 443,671 cycles for this case as f32 on the ESP32S3 at \
         240 MHz, which is {}/10 cycles/MAC folded to {cpu_hz} Hz -- but as f32, and \
         this target runs with the FPU off, so that figure is not reachable from here \
         without turning it on. This line is the floor the canceller is measured against.",
        published_tenths
    );

    measure_taps::<16>(room.as_mut());
    measure_taps::<32>(room.as_mut());
    measure_taps::<64>(room.as_mut());
    measure_taps::<128>(room.as_mut());

    log::info!("[ECHO] done");
}

//! What the linear AEC costs per audio tick, on this chip, at this image's clock.
//!
//! **No peripheral**: the canceller is arithmetic on two streams, so the cost belongs to the
//! tap count and the clock. Per tap count it reports the first tick, the worst tick (the
//! bound is a worst tick), the share of a 20 ms tick, and where the time goes. **Frozen** is
//! what barge-in costs, **adapting** what far-end-only playback costs; the budget covers the
//! larger. Ticks run back to back after a warm-up pass, so this is the work and nothing else.
use alloc::boxed::Box;
use alloc::vec::Vec;

use embassy_time::Instant;
use iot_core::drivers::aec::{
    AdaptationGate, AdaptiveFilter, Canceller, DelayEstimator, ErleMeter, MAX_STEP_Q30,
    SAMPLE_RATE_HZ, state_bytes,
};

const TICK_MS: u64 = 20;

const TICK_SAMPLES: usize = SAMPLE_RATE_HZ as usize * TICK_MS as usize / 1_000;

const TICKS_PER_PASS: usize = 4;

const PASSES: usize = 32;

const REFERENCE_AMPLITUDE: i32 = 9_000;

const SPEECH_AMPLITUDE: i32 = 8_000;

const PATH: [(usize, i32); 5] = [
    (12, 700_000),
    (17, -320_000),
    (23, 180_000),
    (31, -90_000),
    (44, 45_000),
];

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

    fn share(&self) -> u64 {
        self.total * 100 / (u64::from(self.ticks.max(1)) * TICK_MS * 1_000)
    }

    fn mean_as_signed(&self) -> i64 {
        self.mean() as i64
    }
}

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

struct Reference {
    micros: u64,
    macs: u64,
}

fn measure_reference(room: &mut Room) -> Reference {
    const TAPS: usize = 256;
    const SAMPLES: usize = 1024;

    let coefficients: Vec<i32> = (0..TAPS).map(|k| (k as i32 % 17) - 8).collect();
    let samples: Vec<i32> = room.played[..SAMPLES.min(room.played.len())]
        .iter()
        .map(|s| i32::from(*s))
        .collect();

    core::hint::black_box(reference_fir(&samples, &coefficients));

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
    let macs = (TAPS as u64 * (TAPS as u64 + 1) / 2) + (SAMPLES - TAPS) as u64 * TAPS as u64;
    Reference {
        micros: total / rounds,
        macs,
    }
}

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
    let cycles_per_mac = reference.micros * cpu_hz / 1_000_000 / reference.macs;
    log::info!(
        "[ECHO] reference FIR, 256 taps x 1024 samples (the ESP-DSP case): {} us per pass \
         = {cycles_per_mac} cycles/MAC over {} MACs, in i32",
        reference.micros,
        reference.macs,
    );
    // 443,671 cycles over 262,144 MACs at 240 MHz, rescaled to this clock. Tenths, because
    // an integer here would round a number near 5 into a different-looking claim.
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

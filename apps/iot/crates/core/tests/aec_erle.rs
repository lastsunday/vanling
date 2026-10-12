//! What the linear canceller in `drivers/aec` achieves, measured against the best
//! answer that could exist for the path it was given.
//!
//! # Why not a borrowed recording
//!
//! Every public vector for this was tried; all four are unusable.
//!
//! - **Espressif `aec_in_far.wav` / `aec_in_near.wav`** — not a synchronous pair:
//!   best normalised cross-correlation over the whole 53 s at any lag is **0.137**,
//!   a single-tap fit accounts for **0.08 dB**. This canceller reports
//!   **−14.4 dB** on them, which is what subtracting an unrelated signal looks
//!   like.
//! - **Espressif `audio_mic_file` / `audio_ref_file`** — what Espressif's own
//!   `test_afe.cpp` feeds `aec_process`. Synchronous (0.743) but holds no echo
//!   path: an offline least-squares fit bounds it at −1.04 dB at 32 taps through
//!   −4.97 dB at 256, with the kernel peaking on the *last* tap.
//! - **Microsoft AEC-Challenge `real_doubled`** — unusable *by design*. Cutler et
//!   al., arXiv:2309.12553 §4.1, note the PC may run AEC on the **send** signal;
//!   a stack that already cancels leaves a linear model nothing to predict. Best
//!   offline Wiener bound over 40 GUIDs at 128 taps: **−0.42 dB**.
//! - **ITU-T G.168 (04/2015)** — the right place to look, and its Annex D really
//!   does carry explicit impulse responses needing no audio files. Not used: the
//!   transcription cannot be verified. Extraction loses the first element of some
//!   rows, the column order is ambiguous, and under *both* readings the
//!   standard's own 0 dB invariant fails by 1–11 dB. Data that cannot be checked
//!   is not data a test should be built on. Its Test 12 note settles the question
//!   anyway: *"This test may be enhanced by the future addition of test vectors
//!   that represent real acoustic echo signals. [...] for further study."* The
//!   standards body says they do not exist either.
//!
//! # What the reference is instead
//!
//! For each path, [`offline_bound`] solves the least-squares problem exactly and
//! reports the enhancement the best possible filter of the same length could
//! reach. Computed here rather than transcribed, so it cannot be wrong, and it
//! answers the question a linear adaptive filter exists to answer: **did it find
//! the best linear solution, and how close?** Beating the bound is overfitting
//! noise; falling far below it is not converging.
//!
//! # The limits of the number
//!
//! The paths are generated, so they test the contract rather than a room. The
//! loop gain, the delay and the thresholds need this board in front of it. And
//! the 15 dB bar is **necessary, not sufficient** — ERLE's correlation with how
//! the device actually sounds is **PCC 0.31**.

use iot_core::drivers::aec::{Canceller, Coef, MAX_TAPS, SAMPLE_RATE_HZ};

/// The acceptance threshold from the plan: enough enhancement for a talker to be
/// heard over the speaker, which needs the echo about a decade down.
const BARGE_IN_DECIBELS: i32 = 15;

/// How close to the offline optimum a converged filter is expected to get. The
/// adaptive filter sees the signal one sample at a time and never sees the whole
/// record, so it should land near but below the offline answer rather than on it.
const CLOSE_TO_OPTIMUM_DB: f64 = 3.0;

/// Samples per analysis window, 20 ms at [`SAMPLE_RATE_HZ`] — the capture's own
/// cadence, so a window here is one `input_task` tick of audio.
const WINDOW: usize = SAMPLE_RATE_HZ as usize * 20 / 1_000;

/// Far-end activity per [`WINDOW`]: a window counts when its reference power is
/// within a factor of [`FAR_ACTIVE_FLOOR`] of the loudest window's.
const FAR_ACTIVE_FLOOR: i64 = 4;

/// The direct reflection, before the per-tap envelope — large enough that it is
/// the loudest tap in the path, which is what makes the path have an onset.
const DIRECT_REFLECTION: i32 = 30_000;

/// A reflection after it. Kept well under [`DIRECT_REFLECTION`] for the same
/// reason: a first tap no louder than the ones after it is not a direct reflection.
const REFLECTION_AMPLITUDE: i32 = 8_000;

/// One channel of a room: what the microphone heard, and what was sent to the
/// speaker.
struct Room {
    mic: Vec<i16>,
    reference: Vec<i16>,
}

/// A deterministic broadband source.
///
/// Broadbandness is not decoration: a near-periodic reference correlates with
/// itself at nearly every lag, which makes the delay search's confidence gate a
/// matter of luck and lets a filter sit a tap off and still look converged.
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

/// An echo path: `length` taps, a bulk `delay`, and a decay per tap.
///
/// Generated rather than transcribed, for the reason in the module comment: a
/// number that cannot be checked is not a number a test should rest on. What these
/// buy is the one thing a borrowed recording cannot — an offline optimum computed
/// on the *same* path, so the gap between them is meaningful.
struct Path {
    name: &'static str,
    delay: usize,
    length: usize,
    /// Multiplied by this per tap, so a longer path is also a quieter one.
    decay: f64,
    seed: u32,
}

impl Path {
    /// The path as a tap sequence.
    ///
    /// The first tap is a **direct reflection** and the rest are random in sign,
    /// which is the shape a room actually has: one loud arrival that gives the path
    /// an onset, then reflections that are individually weaker and arrive with no
    /// pattern to them. A path whose very first tap is also random has no onset at
    /// all — every lag correlates about equally — and the delay search then has
    /// nothing to lock onto and reports wherever the noise happened to peak. That
    /// was measured, not assumed: with the first tap no louder than the rest, a
    /// 12-sample path reported 15 and a 4-sample one reported 22, cross-checked
    /// offline where the peak genuinely was. The filter reached 0.4 dB because it
    /// was aiming at the wrong place.
    fn taps(&self) -> Vec<f64> {
        let mut noise = Noise::new(self.seed);
        (0..self.length)
            .map(|k| {
                let envelope = self.decay.powi(k as i32);
                if k == 0 {
                    envelope * f64::from(DIRECT_REFLECTION)
                } else {
                    f64::from(noise.next(REFLECTION_AMPLITUDE)) * envelope
                }
            })
            .collect()
    }

    /// The room this path produces, as a digital reference and a microphone.
    fn room(&self, samples: usize, near_end_from: usize, near_amplitude: i32) -> Room {
        let taps = self.taps();
        let peak = taps.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1.0);
        // Scaled so the path's strongest tap is a round fraction of full scale, which
        // keeps the microphone and the reference both inside i16.
        let gain = 9_000.0 / peak;
        let scaled: Vec<i16> = taps.iter().map(|t| (t * gain) as i16).collect();

        let mut played = Noise::new(0x1234_5678);
        let reference: Vec<i16> = (0..samples).map(|_| played.next(12_000)).collect();
        let mut talking = Noise::new(0x0bad_c0de);
        let mic = (0..samples)
            .map(|n| {
                let echo: i64 = (0..self.length)
                    .filter(|k| n >= self.delay + k)
                    .map(|k| {
                        i64::from(reference[n - self.delay - k]) * i64::from(scaled[k]) / 32_768
                    })
                    .sum();
                let near = if n >= near_end_from {
                    i64::from(talking.next(near_amplitude))
                } else {
                    0
                };
                (echo + near) as i16
            })
            .collect();
        Room { mic, reference }
    }
}

/// A near-end talker loud enough to cross the learning rate's freeze floor.
///
/// `LEAKAGE_FREEZE` sits at about 4096 LSB RMS, and a uniform source of amplitude A
/// has an RMS of roughly A/3 — so anything below about 12 000 never freezes the
/// filter, by design: a person quieter than that is exactly the person barge-in must
/// not be interrupted for. This is that case deliberately excluded, and tested
/// instead.
const NEAR_END_ABOVE_FREEZE: i32 = 9_000;

/// Samples the offline optimum is solved over. See [`offline_bound`].
const SAMPLES_FOR_OPTIMUM: usize = 16_384;

/// The paths under test.
///
/// Varied on the axes that matter for an adaptive filter: how long the tail is
/// relative to the taps it has, how quickly it decays, and where the bulk delay
/// sits. [`PATH_TAIL_OVERRUN`] is the interesting one — its tail is longer than
/// the filter, so the offline bound is genuinely below what a perfect fit could
/// otherwise reach, and the gap is the filter's truncation and nothing else.
const PATHS: [Path; 4] = [
    Path {
        name: "short, fast decay",
        delay: 8,
        length: 16,
        decay: 0.62,
        seed: 0x00a1_0001,
    },
    Path {
        name: "medium, moderate decay",
        delay: 12,
        length: 48,
        decay: 0.86,
        seed: 0x00a1_0002,
    },
    Path {
        name: "long tail, slow decay",
        delay: 6,
        length: 192,
        decay: 0.97,
        seed: 0x00a1_0003,
    },
    Path {
        name: "tail longer than the filter",
        delay: 4,
        length: 2 * MAX_TAPS,
        decay: 0.99,
        seed: 0x00a1_0004,
    },
];

fn power(samples: &[i16]) -> i64 {
    samples.iter().map(|s| i64::from(*s) * i64::from(*s)).sum()
}

fn decibels(above: i64, below: i64) -> f64 {
    10.0 * (above as f64 / below.max(1) as f64).log10()
}

/// Far-end activity per [`WINDOW`].
fn far_active(reference: &[i16]) -> Vec<bool> {
    let windows: Vec<i64> = reference.chunks(WINDOW).map(power).collect();
    let loudest = windows.iter().copied().max().unwrap_or(1).max(1);
    windows
        .iter()
        .map(|p| p * FAR_ACTIVE_FLOOR >= loudest)
        .collect()
}

/// The enhancement the best possible linear filter could reach on this room.
///
/// Solves the normal equations for an `order`-tap FIR by ordinary elimination
/// with a ridge term, so the answer is the offline optimum rather than a bound on
/// it. Returns `None` when the system is too ill-conditioned to mean anything,
/// because a reference that silently returns a wrong number is worse than none.
fn offline_bound(room: &Room, order: usize) -> Option<f64> {
    let samples = order + 1;
    // Bounded on purpose. The solve is O(n·L²) in f64 and the answer converges long
    // before the record ends — at L = 128 the whole run was 173 s, which is a test
    // nobody runs twice. What is needed is far more samples than the filter has
    // unknowns, and this is comfortably that.
    let n = room
        .reference
        .len()
        .min(room.mic.len())
        .min(SAMPLES_FOR_OPTIMUM);
    if n < samples * 4 {
        return None;
    }
    // Autocorrelation of the reference, and its cross-correlation with the mic.
    let mut normal = vec![vec![0.0f64; samples]; samples];
    let mut cross = vec![0.0f64; samples];
    for n0 in order..n {
        let window: Vec<f64> = (0..samples)
            .map(|k| f64::from(room.reference[n0 - k]))
            .collect();
        let mic = f64::from(room.mic[n0]);
        for i in 0..samples {
            cross[i] += window[i] * mic;
            for j in 0..samples {
                normal[i][j] += window[i] * window[j];
            }
        }
    }
    // A ridge scaled to the diagonal: enough to make the solve well defined for a
    // reference that is momentarily quiet, small enough not to move the answer.
    let trace: f64 = (0..samples).map(|i| normal[i][i]).sum();
    for (i, row) in normal.iter_mut().enumerate() {
        row[i] += trace * 1e-12;
    }
    // Gaussian elimination with partial pivoting, in place.
    for col in 0..samples {
        let (pivot, _) = (col..samples)
            .map(|r| (r, normal[r][col].abs()))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("col < samples");
        if normal[pivot][col].abs() < 1e-12 {
            return None;
        }
        normal.swap(col, pivot);
        cross.swap(col, pivot);
        let head = normal[col][col];
        for r in col + 1..samples {
            let factor = normal[r][col] / head;
            if factor == 0.0 {
                continue;
            }
            let head: Vec<f64> = normal[col][col..].to_vec();
            for (c, cell) in normal[r].iter_mut().enumerate().skip(col) {
                *cell -= factor * head[c - col];
            }
            cross[r] -= factor * cross[col];
        }
    }
    let mut solution = vec![0.0f64; samples];
    for i in (0..samples).rev() {
        let mut acc = cross[i];
        for j in i + 1..samples {
            acc -= normal[i][j] * solution[j];
        }
        solution[i] = acc / normal[i][i];
    }

    let mut mic_power = 0.0f64;
    let mut residual_power = 0.0f64;
    for n0 in order..n {
        let mut estimate = 0.0f64;
        for (k, weight) in solution.iter().enumerate() {
            estimate += weight * f64::from(room.reference[n0 - k]);
        }
        let mic = f64::from(room.mic[n0]);
        mic_power += mic * mic;
        let residual = mic - estimate;
        residual_power += residual * residual;
    }
    (mic_power > 0.0 && residual_power > 0.0).then(|| 10.0 * (mic_power / residual_power).log10())
}

/// What one pass over a room produced.
///
/// `steady` and `whole_file` are different quantities and conflating them is how
/// a canceller ends up looking better than it is. `steady` is the enhancement of a
/// filter that has already learned, which is what ERLE means; `whole_file` also
/// charges it for what it owed while converging. The AEC Challenge evaluates the
/// second half for exactly this reason (Cutler et al., §7).
struct Report {
    steady: f64,
    whole_file: f64,
    met: f64,
    per_second: Vec<f64>,
    delay: Option<usize>,
    coefficients: Vec<Coef>,
}

fn measure(room: &Room) -> Report {
    let active = far_active(&room.reference);
    let steady_from = room.mic.len() / 2;
    let mut canceller = Canceller::<MAX_TAPS>::new();
    let (mut mic_all, mut left_all) = (0i64, 0i64);
    let (mut mic_late, mut left_late) = (0i64, 0i64);
    let mut per_window = Vec::new();

    for (n, (&m, &r)) in room.mic.iter().zip(&room.reference).enumerate() {
        let out = canceller.process(m, r);
        if active[n / WINDOW] {
            let square = |v: i16| i64::from(v) * i64::from(v);
            let (mic_power, residual_power) = (square(m), square(out.residual));
            mic_all += mic_power;
            left_all += residual_power;
            if n >= steady_from {
                mic_late += mic_power;
                left_late += residual_power;
            }
        }
        if (n + 1) % WINDOW == 0 {
            per_window.push((mic_all, left_all));
        }
    }
    // `per_window` is indexed by `WINDOW`, so a second of it is this many entries.
    let windows_per_second = (SAMPLE_RATE_HZ as usize / WINDOW).max(1);
    Report {
        steady: decibels(mic_late, left_late),
        whole_file: decibels(mic_all, left_all),
        met: f64::from(canceller.erle_decibels()),
        per_second: per_window
            .chunks(windows_per_second)
            .map(|chunk| {
                let last = chunk.last().unwrap();
                decibels(last.0, last.1)
            })
            .collect(),
        delay: canceller.delay(),
        coefficients: canceller.filter().coefficients().to_vec(),
    }
}

#[test]
fn the_canceller_lands_near_the_offline_optimum_on_every_path() {
    // The acceptance gate, and it is stated against the optimum rather than against
    // a fixed number of decibels.
    //
    // A fixed bar would be a statement about the *path*, not about the algorithm: on
    // the last path below, the offline optimum itself is 14.3 dB, so no 128-tap
    // linear filter can reach 15 and asking one to is asking it to be wrong. The
    // question a linear adaptive filter can be held to is whether it found the best
    // linear solution for the path it was shown, and that is what this asks.
    let samples = 64_000;
    println!(
        "{:30} {:>6} {:>9} {:>9} {:>7} {:>6}",
        "path", "taps", "offline", "canceller", "gap", "delay"
    );
    let mut cleared_bar = 0;
    let mut reachable = 0;
    for path in &PATHS {
        let room = path.room(samples, usize::MAX, 0);
        let optimum =
            offline_bound(&room, MAX_TAPS).unwrap_or_else(|| panic!("{}: singular", path.name));
        let report = measure(&room);
        let gap = optimum - report.steady;
        println!(
            "{:30} {:>6} {:>9.1} {:>9.1} {:>7.1} {:>6?}",
            path.name, path.length, optimum, report.steady, gap, report.delay
        );
        assert!(
            gap <= CLOSE_TO_OPTIMUM_DB,
            "{}: the offline optimum is {optimum:.1} dB and the adaptive filter reached \
             {:.1} dB, a gap of {gap:.1} dB — it is not converging to the linear solution",
            path.name,
            report.steady
        );
        if path.length <= MAX_TAPS {
            // A path the filter can hold in full: here the bar is meaningful, and
            // missing it is the canceller's fault rather than the path's.
            reachable += 1;
            if report.steady as i32 >= BARGE_IN_DECIBELS {
                cleared_bar += 1;
            }
            assert!(
                report.steady as i32 >= BARGE_IN_DECIBELS,
                "{}: a canceller with {} taps for a {}-tap path reached {:.1} dB, against \
                 a {BARGE_IN_DECIBELS} dB bar — and the path is short enough that the \
                 filter could hold all of it",
                path.name,
                MAX_TAPS,
                path.length,
                report.steady
            );
        }
    }
    assert_eq!(
        cleared_bar, reachable,
        "not every path the filter could hold cleared the bar"
    );
}

#[test]
fn a_path_longer_than_the_filter_caps_everybody() {
    // The finding that reframes the bar: on a path longer than the filter, the
    // offline optimum is itself below the threshold. This is a property of the path
    // and the tap count, not of the implementation, and it is why the gate above is
    // stated as a gap.
    let path = PATHS
        .iter()
        .max_by_key(|p| p.length)
        .expect("the paths are not empty");
    let room = path.room(64_000, usize::MAX, 0);
    let optimum = offline_bound(&room, MAX_TAPS).expect("a singular solve");
    let report = measure(&room);
    println!(
        "  a {}-tap path with {} taps available: offline {optimum:.1} dB, \
         canceller {:.1} dB",
        path.length, MAX_TAPS, report.steady
    );
    assert!(
        optimum < BARGE_IN_DECIBELS as f64,
        "the offline optimum is {optimum:.1} dB, which is above the bar, so this path \
         no longer demonstrates that the path and not the algorithm sets the limit"
    );
    assert!(
        optimum - report.steady <= CLOSE_TO_OPTIMUM_DB,
        "even capped, the canceller should get within {CLOSE_TO_OPTIMUM_DB} dB of it"
    );
}

#[test]
fn the_canceller_does_not_beat_the_offline_optimum() {
    // The other direction, and the one that catches a test rigged in the
    // canceller's favour: a filter cannot exceed the best linear solution for the
    // path it was shown, so a number above it means the reference is wrong, not
    // that the implementation is good.
    let room = PATHS[1].room(64_000, usize::MAX, 0);
    let optimum = offline_bound(&room, MAX_TAPS).expect("a singular solve");
    let report = measure(&room);
    println!(
        "  offline {optimum:.1} dB, adaptive {:.1} dB",
        report.steady
    );
    assert!(
        report.steady <= optimum + 0.5,
        "the adaptive filter reports {:.1} dB against an offline optimum of {optimum:.1} \\
         dB, so either the measurement or the reference is wrong",
        report.steady
    );
}

#[test]
fn a_filter_longer_than_the_path_is_not_needed() {
    // A path of 16 taps cannot need 128 of them. The filter's energy outside the
    // path's own length is what it would have to be inventing.
    let path = &PATHS[0];
    let room = path.room(32_000, usize::MAX, 0);
    let report = measure(&room);
    let outside = report.coefficients[path.length..]
        .iter()
        .map(|c| {
            let raw = i64::from(c.to_q30());
            raw * raw
        })
        .sum::<i64>();
    let total: i64 = report
        .coefficients
        .iter()
        .map(|c| {
            let raw = i64::from(c.to_q30());
            raw * raw
        })
        .sum();
    println!(
        "  filter energy past tap {}: {:.2}%",
        path.length,
        100.0 * outside as f64 / total.max(1) as f64
    );
    assert!(
        outside * 50 < total,
        "the filter put {:.1}% of its energy past the end of a {}-tap path",
        100.0 * outside as f64 / total.max(1) as f64,
        path.length
    );
}

#[test]
fn the_delay_estimate_finds_each_paths_bulk_delay() {
    // The window has to land where the path starts, or every tap after it is
    // spent on silence.
    let samples = 24_000;
    for path in &PATHS {
        let room = path.room(samples, usize::MAX, 0);
        let report = measure(&room);
        let found = report.delay.expect("a path in the window has a delay");
        assert!(
            found.abs_diff(path.delay) <= 2,
            "{}: the path starts at {} samples, the estimator said {found}",
            path.name,
            path.delay
        );
    }
}

#[test]
fn the_canceller_tracks_a_path_that_moves() {
    // A board gets picked up and put down somewhere else.
    //
    // What follows the path is the *delay*, not the loudest tap: the estimator
    // slides the window onto the bulk delay, so once it has moved the echo arrives
    // at the head of the window again and the peak is back at tap 0. That is the
    // whole reason the estimator exists — the distance stops costing taps.
    let path = &PATHS[1];
    let before_room = path.room(24_000, usize::MAX, 0);
    let mut canceller = Canceller::<MAX_TAPS>::new();
    for (&m, &r) in before_room.mic.iter().zip(&before_room.reference) {
        canceller.process(m, r);
    }
    let before = canceller.delay().expect("a path in the window has a delay");

    let moved = Path {
        name: path.name,
        delay: path.delay + 6,
        length: path.length,
        decay: path.decay,
        seed: path.seed,
    };
    let after_room = moved.room(24_000, usize::MAX, 0);
    for (&m, &r) in after_room.mic.iter().zip(&after_room.reference) {
        canceller.process(m, r);
    }
    let now = canceller
        .delay()
        .expect("the estimator does not give the delay back");
    println!("  delay {before} -> {now} after the path moved six samples");
    assert_eq!(now, moved.delay, "the window did not follow the path");
}

#[test]
fn a_talker_over_the_speaker_does_not_get_learned_as_echo() {
    // The barge-in case, and the reason the learning rate exists: with the near end
    // present the error is speech, and a filter still adapting at full speed would
    // model the speech as a room.
    //
    // **This measures what the design does, including the part it does not do well.**
    // The freeze engages and holds, and the damage stops — but it does not start
    // clean: for roughly half a second after a talker begins, the coherence estimate
    // is still describing the room, and the filter adapts at full speed through the
    // onset. What gets baked in during that window is measured here and bounded,
    // not waved at.
    //
    // Growth is the quantity, not the share: a converged filter on a path shorter
    // than itself carries coefficients past the end of that path, and on this path
    // they hold 46% of the total energy before anyone speaks. An absolute share would
    // report a healthy filter as a broken one.
    let path = &PATHS[1];
    let near_end_from = 32_000;
    let room = path.room(64_000, near_end_from, NEAR_END_ABOVE_FREEZE);
    let mut canceller = Canceller::<MAX_TAPS>::new();

    let energy_past = |c: &[Coef]| -> f64 {
        c[path.length..]
            .iter()
            .map(|c| {
                let raw = i64::from(c.to_q30());
                (raw * raw) as f64
            })
            .sum()
    };

    let mut quiet = 0.0f64;
    let mut at_freeze = 0.0f64;
    let mut end = 0.0f64;
    let mut frozen = 0usize;
    let mut present = 0usize;
    for (n, (&m, &r)) in room.mic.iter().zip(&room.reference).enumerate() {
        canceller.process(m, r);
        if n + 1 == near_end_from {
            quiet = energy_past(canceller.filter().coefficients());
        }
        if n >= near_end_from {
            present += 1;
            if !canceller.adapting() {
                frozen += 1;
                if at_freeze == 0.0 {
                    at_freeze = energy_past(canceller.filter().coefficients());
                }
            }
            end = energy_past(canceller.filter().coefficients());
        }
    }

    println!(
        "  frozen for {frozen}/{present} samples; filter energy past tap {}: \
         {quiet:.3e} quiet -> {at_freeze:.3e} when the freeze took hold -> {end:.3e} at \
         the end",
        path.length
    );

    assert!(
        frozen > present / 2,
        "a talker this loud should stop adaptation for most of its duration, and it \
         stopped for {frozen} of {present}"
    );
    assert!(
        at_freeze > quiet * 100.0,
        "the onset burst was supposed to bake the talker in, and it only went \
         {quiet:.3e} -> {at_freeze:.3e}. Either the freeze is faster than this test \
         thought, or the freeze is not what is stopping it, and those are different \
         designs to fix"
    );
    assert!(
        end <= at_freeze * 1.01,
        "the filter kept adapting while frozen: energy past the path went \
         {at_freeze:.3e} -> {end:.3e} across {frozen} frozen samples"
    );
}

#[test]
fn silence_produces_no_echo_and_no_filter() {
    // Nothing to cancel: the reference is silent, so no amount of the microphone's
    // own content is echo. This is the case a canceller that forgets to normalise
    // invents an echo out of.
    let mut canceller = Canceller::<MAX_TAPS>::new();
    let mut noise = Noise::new(0x0dd0_0dd0);
    for _ in 0..16_000 {
        canceller.process(noise.next(6_000), 0);
    }
    let (_, peak) = canceller.loudest_tap();
    assert!(
        peak < (1 << 30) / 64,
        "a silent reference must not teach the filter a path, peak {peak}"
    );
}

#[test]
fn the_meter_reads_what_the_samples_say() {
    // The meter folds an exponential average over a ~30 ms window; the measurement
    // sums whole windows. Two accountings of the same stream, and a meter that
    // disagrees with its own instrument is worse than no meter. The comparison is
    // against the steady state, because that is the stretch the meter's window is
    // looking at.
    let room = PATHS[1].room(48_000, usize::MAX, 0);
    let report = measure(&room);
    println!(
        "  steady {:.1} dB, meter {:.1} dB, whole file {:.1} dB",
        report.steady, report.met, report.whole_file
    );
    assert!(
        (report.steady - report.met).abs() <= 8.0,
        "the meter reads {:.1} dB where the samples say {:.1} dB",
        report.met,
        report.steady
    );
}

#[test]
fn convergence_is_reported_as_a_curve_rather_than_only_its_end_point() {
    // A single number cannot distinguish "converged fast" from "converged after
    // twenty seconds", and the difference decides whether the first half of a call
    // is usable.
    let room = PATHS[2].room(48_000, usize::MAX, 0);
    let report = measure(&room);
    let curve = report
        .per_second
        .iter()
        .map(|d| format!("{d:.0}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("  per second  {curve}");
    assert!(
        report.per_second.len() >= 3,
        "a 3 s room should have a curve, got {curve:?}"
    );
    let last = *report.per_second.last().unwrap();
    assert!(
        last > report.per_second[0],
        "the curve went from {:.1} to {last:.1} dB, which is not convergence",
        report.per_second[0]
    );
}

//! What the linear AEC does on a **real room**, through the product's own pairing.
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::sounds::SWEEP_MS;
use embassy_executor::SendSpawner;
use embassy_time::{Duration, Instant, Timer};
use iot_bsp_esp::virtual_components::EchoCapture;
use iot_core::demand::MAX_SINGLE_HOLD_MS;
use iot_core::drivers::aec::SAMPLE_RATE_HZ;
use iot_core::drivers::audio::{AudioSample, CAPTURE_MS, ENVELOPE_COLUMNS, dbfs};
use iot_core::drivers::board::HasPlayback;
use iot_core::drivers::playback::{FEED_MS, Sound, Speaker};

use crate::playback::{Shared, feed_task, share, take_stats};

// Whether the probe alternates the amplifier at all.
//
// **Off measures the microphone's floor, and that is all it measures.** A
// canceller handed a silent reference has nothing to correlate against, so the
// delay search cannot return an estimate however good the room is — `[实测]` it
// sat at `delay None` for every run of a muted-speaker probe, which reads
// exactly like a board with no echo path. Alternating also keeps the one thing
// only the muted run can give: the microphone's level with no echo present, so
// "the path is weak" stays distinguishable from "the speaker disturbs the
// microphone". Both halves are wanted, so the alternation is the default and
// the muted-only run is the one to switch to deliberately.
const A_B_RUN: bool = true;

const PATH_RUN: bool = false;

// How long the path run holds the amplifier in each state before switching.
//
// **`[实测]` this has to be several times the correlation the analysis reads.**
// The switch itself is a step on the wire, and a window that contains one carries
// that step's energy into the very band being measured.
const PATH_AB_MS: u64 = 2_000;

const REPORT_MS: u64 = 1_000;

const BARREN_REPORTS: u32 = 3;

const A_B_MS: u64 = ENVELOPE_COLUMNS as u64 * CAPTURE_MS + 2_000;

const A_B_WARMUP_MS: u64 = ENVELOPE_COLUMNS as u64 * CAPTURE_MS;

static AUDIBLE: AtomicBool = AtomicBool::new(true);

/// Runs the product's feed, keeps a sound on it, and polls the product's capture.
pub async fn run<B>(mut board: B, mut capture: Box<dyn EchoCapture>, feed: SendSpawner) -> !
where
    B: HasPlayback,
{
    let Some(speaker) = board.take_playback() else {
        log::error!("[ECHO] no speaker wired, there is no echo path to measure");
        park().await
    };

    let shared = share(speaker as Box<dyn Speaker>);
    log::info!(
        "[ECHO] alternating the amplifier every report window: an ERLE of 0 dB cannot tell \
         a mispaired reference from no echo at all, and the microphone's level under a silent \
         speaker does"
    );
    log::info!(
        "[ECHO] playing and capturing together; a canceller handed a silent reference \
         measures nothing, so one sound is held on the ring for the whole run"
    );
    log::info!(
        "[ECHO] the budget to fit inside is a single poll of {MAX_SINGLE_HOLD_MS} ms, and \
         worst hold is reported below — that figure, not the ERLE, decides whether it fits"
    );

    if !A_B_RUN {
        match shared.lock(|shared| shared.speaker.borrow_mut().set_muted(true)) {
            Ok(()) => AUDIBLE.store(false, Ordering::Relaxed),
            Err(error) => log::error!("[ECHO] the amplifier would not mute: {error}"),
        }
        log::info!(
            "[ECHO] A/B off: the amplifier is muted from boot, so the levels below are \
             the microphone on its own with no echo present"
        );
    }
    feed.spawn(feed_task(shared).expect("the feed executor has room for the one task on it"));
    if !PATH_RUN {
        capture.set_cancel_echo(true);
        log::info!(
            "[ECHO] canceller running: {}",
            if capture.cancel_echo() { "yes" } else { "no" }
        );
    }
    embassy_futures::join::join(keep_sounding(shared), poll_cancellers(capture.as_mut())).await;
    park().await
}

// Keeps a sound going so the reference is never silent. It alternates between the
// synthesised pair and the stored PCM, and each sound lasts longer than the delay
// estimator's 256 ms correlation window — shorter and the reported delay chases.
async fn keep_sounding(shared: &'static Shared) -> ! {
    let mut sound = if PATH_RUN {
        Sound::Sweep
    } else {
        Sound::ALL[0]
    };
    let mut last_report = Instant::now();
    let mut held_since = Instant::now();
    if PATH_RUN {
        if let Err(error) =
            shared.lock(|shared| crate::sounds::play(&mut **shared.speaker.borrow_mut(), sound))
        {
            log::error!("[PATH] the sweep would not start: {error}");
            park().await;
        }
        log::info!(
            "[PATH] a broadband sweep on the wire for the whole run; a single tone would \
             measure one frequency and no path at all"
        );
    }
    loop {
        if !PATH_RUN
            && let Err(error) =
                shared.lock(|shared| crate::sounds::play(&mut **shared.speaker.borrow_mut(), sound))
        {
            log::error!("[ECHO] {sound:?} refused: {error}");
        }
        Timer::after(Duration::from_millis(if PATH_RUN {
            SWEEP_MS as u64 + 20
        } else {
            u64::from(FEED_MS) * 8
        }))
        .await;

        let now = Instant::now();
        let window_ms = now.saturating_duration_since(last_report).as_millis();
        if window_ms >= REPORT_MS {
            let (feeds, worst_gap_ms, late_gaps, worst_work_us) = take_stats(shared);
            log::info!(
                "[ECHO] feed: {feeds} in {window_ms} ms, worst gap {worst_gap_ms} ms, \
                 {late_gaps} late, worst work {worst_work_us} us"
            );
            last_report = now;
        }

        let ab_due = if PATH_RUN {
            held_since.elapsed().as_millis() >= PATH_AB_MS
        } else {
            A_B_RUN && held_since.elapsed().as_millis() >= A_B_MS
        };
        if ab_due {
            let next = !AUDIBLE.load(Ordering::Relaxed);
            if let Err(error) = shared.lock(|shared| shared.speaker.borrow_mut().set_muted(next)) {
                log::error!("[ECHO] mute would not latch, the A/B is over: {error}");
            }
            AUDIBLE.store(next, Ordering::Relaxed);
            held_since = Instant::now();
            if PATH_RUN {
                log::info!("[PATH] amplifier now {}", if next { "on" } else { "off" });
            } else {
                sound = sound.next();
                log::info!(
                    "[ECHO] amplifier now {}, {sound:?} playing, holding for {A_B_MS} ms",
                    if next { "on" } else { "off" }
                );
            }
        }
    }
}

fn report_path(capture: &mut dyn EchoCapture) {
    let shares: [i32; 4] = [16, 64, 128, 256].map(|taps| capture.unreachable_by(taps));
    if shares[0] < 0 {
        log::info!(
            "[PATH] no settled delay, so there is nowhere to place a filter and the \
             shares below are not a measurement"
        );
        return;
    }
    log::info!(
        "[PATH] out of reach of a filter of — 16 taps {0}%, 64 taps {1}%, 128 taps {2}%, \
         256 taps {3}%. {4}",
        shares[0],
        shares[1],
        shares[2],
        shares[3],
        if shares[0] == 0 {
            "16 taps hold the whole correlated path on this board"
        } else {
            "whatever is left is what survives, however well the rest converges"
        },
    );
}

async fn poll_cancellers(capture: &mut dyn EchoCapture) -> ! {
    let mut now_ms: u64 = 0;
    let mut last_report = Instant::now();
    let mut worst_hold_us: u32 = 0;
    let mut barren: u32 = 0;
    let mut seen_audible = AUDIBLE.load(Ordering::Relaxed);
    let mut audible_since = Instant::now();
    let mut polls: u32 = 0;
    let mut paired_total: usize = 0;

    loop {
        Timer::after(Duration::from_millis(CAPTURE_MS)).await;
        now_ms += CAPTURE_MS;
        let started = Instant::now();
        let sample: AudioSample = capture.poll(now_ms);
        worst_hold_us = worst_hold_us.max(started.elapsed().as_micros() as u32);
        polls += 1;
        paired_total += capture.echo_report().cancelled();

        let now = Instant::now();
        let window_ms = now.saturating_duration_since(last_report).as_millis();
        if window_ms < REPORT_MS {
            continue;
        }
        let report = capture.echo_report();
        if !capture.cancel_echo() {
            log::error!("[ECHO] the canceller is not running, so nothing below is a measurement");
        }
        let audible = AUDIBLE.load(Ordering::Relaxed);
        if audible != seen_audible {
            seen_audible = audible;
            audible_since = Instant::now();
        }
        let settled = audible_since.elapsed().as_millis() >= A_B_WARMUP_MS;
        report_line(&report, worst_hold_us, &sample, audible, settled);
        if worst_hold_us > MAX_SINGLE_HOLD_MS * 1_000 {
            log::error!(
                "[ECHO] a poll held {worst_hold_us} us, past the {MAX_SINGLE_HOLD_MS} ms \
                 bound: the canceller does not fit where it is"
            );
        }
        if report.delay().is_none() {
            let (lag, peak) = capture.scan_wide();
            let at_ms = lag as u64 * 1_000 / SAMPLE_RATE_HZ as u64;
            log::info!(
                "[PATH] wide scan: best match {peak} per mille at {lag} samples ({at_ms} ms) \
                 against a search that reaches {} samples — {}",
                64,
                if peak >= 300 {
                    "the path is there and the filter is too short to hold it"
                } else {
                    "no path anywhere in the history"
                },
            );
            if peak >= 300 {
                report_path(capture);
            }
        }

        if !report.log_inside_runway() {
            log::error!(
                "[ECHO] the reference log holds {} of {} samples, past its runway: the \
                 pairing is reading from a lap and the ERLE above does not mean what it says",
                report.log_depth,
                report.log_capacity,
            );
        }

        barren = if report.cancelled() == 0 {
            barren + 1
        } else {
            0
        };
        if barren >= BARREN_REPORTS {
            log::error!(
                "[ECHO] {barren} reports with nothing paired: the canceller is running on \
                 silence, so the ERLE above is measuring nothing at all"
            );
        }

        if window_ms > 0 && polls > 0 {
            log::info!(
                "[ECHO] capture rate: {polls} polls in {window_ms} ms, {paired_total} paired \
                 = {} samples/s of the 16000 the canceller runs, {} per poll",
                paired_total as u64 * 1000 / window_ms,
                paired_total / polls as usize,
            );
        }
        polls = 0;
        paired_total = 0;
        worst_hold_us = 0;
        last_report = now;
    }
}

// One report, as a line a serial log can be read from.
//
// The microphone's own level is reported beside the ERLE because **an ERLE of
// 0 dB says two completely different things**: a filter handed a reference that
// does not predict the echo, and a filter handed no echo at all because the
// speaker is silent or the amplifier is off. Both are 0, both converge quietly,
// and the ERLE alone cannot tell them apart — but the microphone's level can.
fn report_line(
    report: &iot_bsp_esp::virtual_components::EchoReport,
    worst_hold_us: u32,
    sample: &AudioSample,
    audible: bool,
    settled: bool,
) {
    log::info!(
        "[ECHO] ring {} B arrived, {} B drained — a shortfall in the first is the \
         link, in the second alone it is the drain",
        report.drain.0,
        report.drain.1,
    );
    let (tap, peak) = report.loudest_tap;
    log::info!(
        "[ECHO] {} paired this poll, {} unpaired in total | ERLE {} dB, delay {:?}, \
         tap {tap} at {peak} q30, {}",
        report.cancelled(),
        report.total_unpaired,
        report.erle_decibels(),
        report.delay(),
        if report.adapting() {
            "adapting"
        } else {
            "frozen"
        },
    );
    let slots = report.slot_peaks;
    log::info!(
        "[ECHO] capture slots: MIC1 {} | slot2 {} | slot3 {} | slot4 {} LSB, MIC1-vs-MIC3 gap {}",
        slots.mic,
        slots.reference,
        slots.spare_two,
        slots.spare_three,
        slots.gap,
    );
    log::info!(
        "[ECHO] reference log {} of {} kept samples, worst single poll {worst_hold_us} us \
         of a {MAX_SINGLE_HOLD_MS} ms bound",
        report.log_depth,
        report.log_capacity,
    );
    log::info!(
        "[ECHO] amplifier {} — reference peaked {} LSB, microphone peaked {} LSB",
        if audible { "on " } else { "off" },
        report.levels.1,
        report.levels.0,
    );
    if settled {
        log::info!(
            "[ECHO] amplifier settled {} — A-weighted {} dBFS over a floor of {}",
            if audible { "on " } else { "off" },
            dbfs(sample.dba_lsb),
            sample.envelope.floor_rms(),
        );
    }
}

async fn park() -> ! {
    loop {
        core::future::pending::<()>().await
    }
}

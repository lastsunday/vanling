//! Playback isolation probe: the product's own feed loop, plus a walker that keeps a
//! sound going so the loop has something to carry, and the product's own counters.
//!
//! Sharing [`feed_loop`] rather than writing a second schedule is what makes the
//! probe evidence about the product: same absolute deadline, same unconditional
//! refill, same counters. A loop of its own would only report its own behaviour,
//! and a subtle difference — feeding while a sound is on the ring rather than
//! always — would make a green probe mean nothing.

use alloc::boxed::Box;
use core::sync::atomic::Ordering;

use embassy_futures::join::join;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::board::HasPlayback;
use iot_core::drivers::playback::{FEED_MS, Sound, Speaker};

use crate::playback::{FEED_LATE_MS, Shared, feed_loop, share, take_stats};

pub async fn run<B>(mut board: B) -> !
where
    B: HasPlayback,
{
    let Some(speaker) = board.take_playback() else {
        log::error!("[PROBE] no speaker wired, nothing to drive");
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    };

    // The erased handle the product's playback task takes, so the probe drives
    // that path rather than a concrete driver reached directly.
    let shared = share(speaker as Box<dyn Speaker>);
    // Both futures are `!`, so `join` returning at all is unreachable; parking keeps
    // the signature honest rather than reaching for a panic the compiler can see
    // is dead.
    join(feed_loop(shared), walk_catalogue(shared)).await;
    loop {
        core::future::pending::<()>().await
    }
}

/// Walks the sound catalogue for as long as the feed runs, alternating the two
/// because they are built differently — a synthesised pair against a stored PCM —
/// so a stall only one of them provokes cannot hide behind whichever was playing.
///
/// The catalogue order comes from [`Sound::next`] rather than a second `match`, so
/// this walker and the product's page cannot disagree about what follows a sound,
/// and it starts from [`Sound::ALL`]'s head rather than a literal.
async fn walk_catalogue(shared: &'static Shared) -> ! {
    let mut sound = Sound::ALL[0];
    let mut plays: u32 = 0;
    let mut last_report = Instant::now();

    loop {
        match shared.lock(|shared| shared.speaker.borrow_mut().play(sound)) {
            Ok(()) => {
                plays += 1;
                log::info!("[PROBE] playing {sound:?}, play {plays}");
            }
            Err(error) => log::error!("[PROBE] {sound:?} refused: {error:?}"),
        }
        sound = sound.next();

        // Wait for this sound to finish, then take the next. The driver's report
        // is the only statement of completion — `false` from `feed` means nothing
        // in flight — so this reads the same flag the product's phase does rather
        // than timing a sound out.
        while !shared.lock(|shared| shared.done.swap(false, Ordering::Relaxed)) {
            Timer::after(Duration::from_millis(FEED_MS.into())).await;
        }

        // The repair the feed found necessary, on the cooperative side as the
        // product does it, so the probe covers that path instead of leaving it to
        // the product build to discover.
        if let Some(recovery) = shared.lock(|shared| shared.speaker.borrow_mut().recover()) {
            log::info!(
                "[PROBE] recovered the outgoing DMA, {} bytes free, sound was {}",
                recovery.free_bytes,
                if recovery.playing { "playing" } else { "idle" },
            );
        }

        let now = Instant::now();
        let window_ms = now.saturating_duration_since(last_report).as_millis();
        if window_ms >= PROBE_REPORT_MS {
            let (feeds, worst_gap_ms, late_gaps, worst_work_us) = take_stats(shared);
            log::info!(
                "[PROBE] {feeds} feeds in {window_ms} ms = {} mHz, worst gap {worst_gap_ms} ms, {late_gaps} gaps over {FEED_LATE_MS} ms, worst work {worst_work_us} us",
                u64::from(feeds) * 1_000_000 / window_ms,
            );
            last_report = now;
        }
    }
}

/// Window the probe's cadence report closes on, matching the product's so the two
/// lines can be read against each other.
const PROBE_REPORT_MS: u64 = 2_000;

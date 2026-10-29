//! Playback isolation probe: the product's own feed loop, plus a walker that keeps a sound
//! going so the loop has something to carry, and the product's own counters.

use alloc::boxed::Box;
use core::sync::atomic::Ordering;

use embassy_futures::join::join;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::board::HasPlayback;
use iot_core::drivers::playback::{FEED_MS, Sound, Speaker};

use crate::playback::{FEED_LATE_MS, Shared, feed_loop, share, take_stats};

/// Board-independent: the AEC touches no peripheral, so its cost belongs to the tap count
/// and the clock rather than to any wiring.
pub mod echo;

/// The same canceller against a real room, through the product's own pairing.
#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod echo_room;

/// S3 only: the switch itself is a register write, which is S3-only the moment esp-hal
/// refuses to do it.
#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod clock;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod motion;

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

    let shared = share(speaker as Box<dyn Speaker>);
    // Both futures are `!`, so `join` returning at all is unreachable; parking keeps
    // the signature honest rather than reaching for a panic the compiler can see
    // is dead.
    join(feed_loop(shared), walk_catalogue(shared)).await;
    loop {
        core::future::pending::<()>().await
    }
}

async fn walk_catalogue(shared: &'static Shared) -> ! {
    let mut sound = Sound::ALL[0];
    let mut plays: u32 = 0;
    let mut last_report = Instant::now();

    loop {
        match shared.lock(|shared| crate::sounds::play(&mut **shared.speaker.borrow_mut(), sound)) {
            Ok(()) => {
                plays += 1;
                log::info!("[PROBE] playing {sound:?}, play {plays}");
            }
            Err(error) => log::error!("[PROBE] {sound:?} refused: {error:?}"),
        }
        sound = sound.next();

        while !shared.lock(|shared| shared.done.swap(false, Ordering::Relaxed)) {
            Timer::after(Duration::from_millis(FEED_MS.into())).await;
        }

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

const PROBE_REPORT_MS: u64 = 2_000;

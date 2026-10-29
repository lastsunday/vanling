//! Playback task: the one place a sound actually happens.

use crate::input::IntentBus;
use alloc::boxed::Box;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::playback::{FEED_MS, Sound, Speaker};
use iot_core::intent::{BusinessIntent, Intent, PlaybackAction};

/// Cross-task playback commands: the render loop's diff against what the driver has been
/// told, so a dropped play is asked for again next pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommand {
    Play(Sound),
    SetMuted(bool),
}

/// Playback channel: the render loop writes, the playback task drains.
pub type PlaybackBus = Channel<CriticalSectionRawMutex, PlaybackCommand, 4>;

/// Passed as `&'static` into the tasks, so no consumer reaches this by name.
pub static PLAYBACK_BUS: PlaybackBus = PlaybackBus::new();

const CADENCE_REPORT_MS: u64 = 2_000;

pub(crate) const FEED_LATE_MS: u64 = 20;

/// What the feed loop has to publish for the control loop to report.
#[derive(Default)]
pub struct FeedStats {
    feeds: AtomicU32,
    worst_gap_ms: AtomicU32,
    late_gaps: AtomicU32,
    worst_work_us: AtomicU32,
}

/// The one speaker, plus the bookkeeping the two loops share.
pub struct Playback {
    pub(crate) speaker: RefCell<Box<dyn Speaker>>,
    stats: FeedStats,
    pub(crate) done: AtomicBool,
}

impl Playback {
    fn new(speaker: Box<dyn Speaker>) -> Self {
        Self {
            speaker: RefCell::new(speaker),
            stats: FeedStats::default(),
            done: AtomicBool::new(false),
        }
    }
}

pub type Shared = Mutex<CriticalSectionRawMutex, Playback>;

/// Wraps the board's speaker in the shared state the two loops use, and hands back a handle
/// that outlives them.
pub fn share(speaker: Box<dyn Speaker>) -> &'static Shared {
    &*Box::leak(Box::new(Mutex::new(Playback::new(speaker))))
}

pub(crate) fn take_stats(shared: &Shared) -> (u32, u32, u32, u32) {
    shared.lock(|shared| {
        (
            shared.stats.feeds.swap(0, Ordering::Relaxed),
            shared.stats.worst_gap_ms.swap(0, Ordering::Relaxed),
            shared.stats.late_gaps.swap(0, Ordering::Relaxed),
            shared.stats.worst_work_us.swap(0, Ordering::Relaxed),
        )
    })
}

/// Drives the ring: one feed every [`FEED_MS`], and nothing else.
pub async fn feed_loop(shared: &'static Shared) -> ! {
    let mut next_feed: Instant = Instant::now();
    let mut now_ms: u64 = 0;
    let mut last_arrival = Instant::now();
    loop {
        let arrived = Instant::now();
        let gap_ms = arrived.saturating_duration_since(last_arrival).as_millis() as u32;
        last_arrival = arrived;
        shared.lock(|shared| {
            let still = shared.speaker.borrow_mut().feed(now_ms);
            if !still {
                shared.done.store(true, Ordering::Relaxed);
            }
            shared.stats.feeds.fetch_add(1, Ordering::Relaxed);
            shared
                .stats
                .worst_gap_ms
                .fetch_max(gap_ms, Ordering::Relaxed);
            if u64::from(gap_ms) > FEED_LATE_MS {
                shared.stats.late_gaps.fetch_add(1, Ordering::Relaxed);
            }
            shared.stats.worst_work_us.fetch_max(
                Instant::now()
                    .saturating_duration_since(arrived)
                    .as_micros() as u32,
                Ordering::Relaxed,
            );
        });
        next_feed += Duration::from_millis(FEED_MS.into());
        now_ms = now_ms.wrapping_add(FEED_MS.into());
        Timer::at(next_feed).await;
    }
}

/// The feed loop as a task, so it can be spawned onto an executor that preempts the
/// cooperative one.
#[embassy_executor::task]
pub async fn feed_task(shared: &'static Shared) {
    feed_loop(shared).await
}

/// The other half of playback: applies commands and reports a sound's end.
pub async fn control_loop(
    shared: &'static Shared,
    playback_bus: &'static PlaybackBus,
    intent_bus: &'static IntentBus,
) -> ! {
    let mut sounding = false;
    let mut last_report = Instant::now();
    loop {
        while let Ok(command) = playback_bus.try_receive() {
            match command {
                PlaybackCommand::Play(sound) => {
                    log::info!("[PLAY] {sound:?}");
                    let accepted = shared.lock(|s| {
                        let accepted = crate::sounds::play(&mut **s.speaker.borrow_mut(), sound);
                        if accepted.is_ok() {
                            s.done.store(false, Ordering::Relaxed);
                        }
                        accepted
                    });
                    match accepted {
                        Ok(()) => sounding = true,
                        Err(error) => {
                            log::error!("[PLAY] {sound:?} refused: {error}");
                            sounding = false;
                            finished(intent_bus).await;
                        }
                    }
                }
                PlaybackCommand::SetMuted(muted) => {
                    log::info!("[PLAY] mute {}", u8::from(muted));
                    if let Err(error) = shared.lock(|s| s.speaker.borrow_mut().set_muted(muted)) {
                        log::error!("[PLAY] mute refused: {error}");
                    }
                }
            }
        }
        if shared.lock(|s| s.done.swap(false, Ordering::Relaxed)) && sounding {
            sounding = false;
            finished(intent_bus).await;
        }
        if let Some(recovery) = shared.lock(|s| s.speaker.borrow_mut().recover()) {
            log::info!(
                "[PLAY] recovered the outgoing DMA, {} bytes free, sound was {}",
                recovery.free_bytes,
                if recovery.playing { "playing" } else { "idle" },
            );
        }
        // In millihertz so the division stays in integers, read as "2.4 feeds short
        // of 200 Hz". `feeds` counts the window alone: a running total over a
        // wall-clock window would drift upward and report an unreachable rate.
        let now = Instant::now();
        let window_ms = now.saturating_duration_since(last_report).as_millis();
        if window_ms >= CADENCE_REPORT_MS {
            let (feeds, worst_gap_ms, late_gaps, worst_work_us) = take_stats(shared);
            let expected = (window_ms / u64::from(FEED_MS)).max(1) as u32;
            let percent = u64::from(feeds) * 100 / u64::from(expected);
            let late = late_gaps > 0 || percent < 95;
            log::log!(
                if late {
                    log::Level::Warn
                } else {
                    log::Level::Debug
                },
                "[PLAY] cadence {feeds} feeds in {window_ms} ms ({percent}% of nominal), \
                 {late_gaps} gaps over {} ms, worst gap {worst_gap_ms} ms, worst work {worst_work_us} us",
                FEED_LATE_MS,
            );
            last_report = now;
        }
        Timer::after(Duration::from_millis(FEED_MS.into())).await;
    }
}

async fn finished(intent_bus: &'static IntentBus) {
    intent_bus
        .send(Intent::Business(BusinessIntent::Playback(
            PlaybackAction::Finished,
        )))
        .await;
}

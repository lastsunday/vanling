//! Playback task: the one place a sound actually happens.
//!
//! The state layer decides *what* should be heard, the board's [`Speaker`] knows
//! *how*, and this is the loop between them. The split that matters: [`feed_loop`]
//! may not block, [`control_loop`] may.

use crate::input::IntentBus;
use alloc::boxed::Box;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::playback::{FEED_MS, Sound, Speaker};
use iot_core::intent::{BusinessIntent, Intent};

/// Cross-task playback commands: the render loop's diff of the playback state,
/// as absolute targets like every other business move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommand {
    /// Start this sound from the beginning, replacing anything playing.
    Play(Sound),
    /// Latch the output quiet, or audible again.
    SetMuted(bool),
}

/// Playback channel: the render loop writes, the playback task drains.
pub type PlaybackBus = Channel<CriticalSectionRawMutex, PlaybackCommand, 4>;

/// Passed as `&'static` into the tasks, so no consumer reaches this by name.
pub static PLAYBACK_BUS: PlaybackBus = PlaybackBus::new();

/// Wall-clock window between cadence reports. Deliberately not counted in feeds:
/// a feed-counted window only closes on a loop that is keeping up, so it goes
/// silent exactly when the schedule is being missed, which is the one case the
/// report exists to explain.
const CADENCE_REPORT_MS: u64 = 2_000;

/// A feed gap longer than this is a hold-up rather than jitter — four [`FEED_MS`]
/// slots back to back. Counted per window, and counted in the loop that measures the
/// gap rather than the one that prints it, because the loop that measures it is the
/// one that cannot afford to print.
pub(crate) const FEED_LATE_MS: u64 = 20;

/// What the feed loop has to publish for the control loop to report. Counters rather
/// than lines: a UART write from the feed would hold it for the line's time, which is
/// the hold-up the feed exists to eliminate.
#[derive(Default)]
pub struct FeedStats {
    feeds: AtomicU32,
    /// Longest interval between two feeds in the window, in ms.
    worst_gap_ms: AtomicU32,
    /// Gaps in this window over [`FEED_LATE_MS`].
    late_gaps: AtomicU32,
    /// Longest single `feed` call, in microseconds. `worst_gap_ms` measures how
    /// late a feed *arrived*; this measures how long it *stayed*, which is what
    /// decides whether anything below this priority got to run at all.
    worst_work_us: AtomicU32,
}

/// The one speaker, plus the bookkeeping the two loops share.
///
/// Behind one critical section so a feed and a mute cannot interleave inside the
/// driver. `CriticalSectionRawMutex` fits exactly this pair: it never spins, so the
/// feed takes it from an interrupt without a bounded wait, and a control loop inside
/// it is never interrupted partway through a codec write.
pub struct Playback {
    /// Boxed so the trait object — not the board's driver type — is what both loops
    /// name, which is what lets the feed task be spawned.
    pub(crate) speaker: RefCell<Box<dyn Speaker>>,
    stats: FeedStats,
    /// Set by the feed on a tick where the driver reported nothing in flight. A
    /// flag rather than a send, because the feed cannot wait for room on a channel.
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

/// Wraps the board's speaker in the shared state the two loops use, and hands back a
/// handle that outlives them.
///
/// Leaked rather than put in a `static`: the handle is `&'static` because both loops
/// are, and a `Box<dyn Speaker>` has no const constructor — but it is written once,
/// before either loop starts, so it is one never-freed handle rather than a leak.
pub fn share(speaker: Box<dyn Speaker>) -> &'static Shared {
    &*Box::leak(Box::new(Mutex::new(Playback::new(speaker))))
}

/// Reads the feed loop's counters and resets them, for one window's report.
///
/// `pub(crate)` because the playback probe reports the same four figures as the
/// product, and a probe that published its own would be reporting a second
/// schedule's behaviour rather than this one's.
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
///
/// The only part of playback that may not block, and it carries the capture's clock
/// as well as the sound. Publishes rather than logs, so see [`FeedStats`].
pub async fn feed_loop(shared: &'static Shared) -> ! {
    let mut next_feed: Instant = Instant::now();
    // The loop's own millisecond count, advanced with the schedule. A driver needs
    // it to tell a stream that has run dry from one merely between two feeds, and
    // this loop owns the cadence — so it owns the clock too.
    let mut now_ms: u64 = 0;
    // Real elapsed time, which `now_ms` cannot see: it advances one cadence per
    // iteration, so a loop held up elsewhere looks here like an on-time feed.
    let mut last_arrival = Instant::now();
    loop {
        let arrived = Instant::now();
        let gap_ms = arrived.saturating_duration_since(last_arrival).as_millis() as u32;
        last_arrival = arrived;
        shared.lock(|shared| {
            // Fed unconditionally: the idle stream carries the capture's clocks, so
            // it is refilled whether or not a sound is on it.
            let still = shared.speaker.borrow_mut().feed(now_ms);
            // Safe to read without knowing whether a sound was claimed: the driver
            // arms its voice before `play` returns, so `false` means nothing in
            // flight rather than nothing armed.
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
        // Absolute deadline: work between feeds never piles drift onto the ring.
        next_feed += Duration::from_millis(FEED_MS.into());
        now_ms = now_ms.wrapping_add(FEED_MS.into());
        Timer::at(next_feed).await;
    }
}

/// The feed loop as a task, so it can be spawned onto an executor that preempts the
/// cooperative one. Monomorphic because a `#[task]` cannot be generic — which is why
/// the speaker arrives as a trait object and why [`Speaker`]'s error is one named type.
#[embassy_executor::task]
pub async fn feed_task(shared: &'static Shared) {
    feed_loop(shared).await
}

/// The other half of playback: applies commands and reports a sound's end. Everything
/// here may block — the codec's registers are behind I2C, the finish report waits for
/// room on the intent bus — which is what makes it the safe side of the split.
pub async fn control_loop(
    shared: &'static Shared,
    playback_bus: &'static PlaybackBus,
    intent_bus: &'static IntentBus,
) -> ! {
    let mut sounding = false;
    let mut last_report = Instant::now();
    loop {
        // Commands first: a play that arrived while a sound was sounding must be
        // seen before the finish that would otherwise end the old one.
        while let Ok(command) = playback_bus.try_receive() {
            match command {
                PlaybackCommand::Play(sound) => {
                    log::info!("[PLAY] {sound:?}");
                    // `done` means *this* sound ended, so the report the feed left
                    // standing between two sounds goes with the `play` that arms the
                    // driver — read as this sound's end, it would finish the sound
                    // the instant it started. Both halves under one lock.
                    let accepted = shared.lock(|s| {
                        let accepted = s.speaker.borrow_mut().play(sound);
                        if accepted.is_ok() {
                            s.done.store(false, Ordering::Relaxed);
                        }
                        accepted
                    });
                    match accepted {
                        Ok(()) => sounding = true,
                        Err(error) => {
                            // Already `Playing`; it would drop every later tap
                            // against a sound that never started, so close the phase.
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
            // The driver's word is the phase's word — the only way `Playing` clears.
            sounding = false;
            finished(intent_bus).await;
        }
        // The repair the last feed found necessary, done here rather than there:
        // rebuilding a ring allocates and logging takes locks a feed must never
        // wait on. A cadence late is free — the ring is already dry.
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
            log::info!(
                "[PLAY] {} feeds in {} ms = {} mHz, worst gap {} ms, {} gaps over {} ms, worst work {} us",
                feeds,
                window_ms,
                u64::from(feeds) * 1_000_000 / window_ms,
                worst_gap_ms,
                late_gaps,
                FEED_LATE_MS,
                worst_work_us,
            );
            last_report = now;
        }
        Timer::after(Duration::from_millis(FEED_MS.into())).await;
    }
}

/// Hand the page its sound's end, waiting for room rather than dropping it.
async fn finished(intent_bus: &'static IntentBus) {
    intent_bus
        .send(Intent::Business(BusinessIntent::PlaybackFinished))
        .await;
}

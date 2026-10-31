//! Playback task: the one place a sound actually happens.

mod capture;
mod dispatch;
pub mod sounds;

pub(crate) use dispatch::PlaybackDispatch;

use crate::input::IntentBus;
use crate::workers;
use alloc::boxed::Box;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_futures::join::join;
use embassy_futures::select::select3;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::playback::{FEED_MS, Recovery, SampleSource, Sound, Speaker, SpeakerFault};
use iot_core::intent::{BusinessIntent, Intent, PlaybackAction};
use iot_core::state::PlaybackCommand;

/// Wakes the control side only when it has something to do: a sound ended, or the driver
/// recorded a drain to repair. The feed loop signals it after releasing the speaker lock.
static FEED_WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

const CADENCE_REPORT_MS: u64 = 2_000;

pub(crate) const FEED_LATE_MS: u64 = 20;

#[derive(Default)]
struct FeedStats {
    feeds: AtomicU32,
    worst_gap_ms: AtomicU32,
    late_gaps: AtomicU32,
    worst_work_us: AtomicU32,
}

pub struct Playback {
    pub(crate) speaker: RefCell<Box<dyn Speaker>>,
    stats: FeedStats,
    pub(crate) done: AtomicBool,
    /// Whether the control side believes a sound is on the ring. The feed side reads this
    /// to decide whether a drained ring means a sound ended or just idle silence.
    sounding: AtomicBool,
}

impl Playback {
    fn new(speaker: Box<dyn Speaker>) -> Self {
        Self {
            speaker: RefCell::new(speaker),
            stats: FeedStats::default(),
            done: AtomicBool::new(false),
            sounding: AtomicBool::new(false),
        }
    }
}

pub type Shared = Mutex<CriticalSectionRawMutex, Playback>;

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

/// Starts a sound. Parsing and allocating stay outside the lock the feed interrupt takes;
/// the lock covers one `play` and the bookkeeping it implies.
pub fn start(shared: &'static Shared, sound: Sound) -> Result<(), SpeakerFault> {
    start_source(shared, sounds::source_of(sound)?)
}

/// Starts a source the caller prepared, for an excitation no catalogue entry names.
pub fn start_source(
    shared: &'static Shared,
    source: Box<dyn SampleSource>,
) -> Result<(), SpeakerFault> {
    shared.lock(|s| {
        let accepted = s.speaker.borrow_mut().play(source);
        if accepted.is_ok() {
            s.done.store(false, Ordering::Relaxed);
            s.sounding.store(true, Ordering::Relaxed);
        } else {
            s.sounding.store(false, Ordering::Relaxed);
        }
        accepted
    })
}

/// Records one feed's outcome and answers whether the control side must be woken. Only an
/// edge does: idle silence returns `still == false` on every feed, and waking on that would
/// put the old 5 ms poll straight back.
fn feed_edge(playback: &Playback, still: bool, needs_recovery: bool) -> bool {
    let ended = !still
        && playback.sounding.load(Ordering::Relaxed)
        && !playback.done.swap(true, Ordering::Relaxed);
    ended || needs_recovery
}

/// Drives the ring: one feed every `FEED_MS`, and nothing else.
pub async fn feed_loop(shared: &'static Shared) -> ! {
    let mut next_feed: Instant = Instant::now();
    let mut now_ms: u64 = 0;
    let mut last_arrival = Instant::now();
    loop {
        let arrived = Instant::now();
        let gap_ms = arrived.saturating_duration_since(last_arrival).as_millis() as u32;
        last_arrival = arrived;
        let mut wake = false;
        shared.lock(|shared| {
            let still = shared.speaker.borrow_mut().feed(now_ms);
            let needs_recovery = shared.speaker.borrow().needs_recovery();
            wake = feed_edge(shared, still, needs_recovery);
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
        if wake {
            FEED_WAKE.signal(());
        }
        next_feed += Duration::from_millis(FEED_MS.into());
        now_ms = now_ms.wrapping_add(FEED_MS.into());
        Timer::at(next_feed).await;
    }
}

/// The feed loop as a task: it must run on an executor that preempts the cooperative one.
#[embassy_executor::task]
pub async fn feed_task(shared: &'static Shared) {
    feed_loop(shared).await
}

/// No speaker: nothing to play or feed. A joined future that never resolves keeps the tasks
/// beside it running.
async fn silent() -> ! {
    match core::future::pending::<core::convert::Infallible>().await {}
}

/// Owns the audio-side task topology: who feeds the ring, and on which executor.
pub async fn playback_tasks(
    shared: Option<&'static Shared>,
    feed: crate::Feed,
    speaker: workers::Speaker,
    intent_bus: &'static IntentBus,
) -> ! {
    let Some(shared) = shared else { silent().await };
    match feed {
        crate::Feed::Interrupt(spawner) => {
            // Spawned, not joined: joining a future on the executor this one does not run
            // waits for ever.
            spawner.spawn(
                feed_task(shared).expect("the feed executor has room for the one task on it"),
            );
            control_loop(shared, speaker, intent_bus).await
        }
        crate::Feed::Inline => {
            match join(control_loop(shared, speaker, intent_bus), feed_loop(shared)).await {}
        }
    }
}

pub async fn control_loop(
    shared: &'static Shared,
    speaker: workers::Speaker,
    intent_bus: &'static IntentBus,
) -> ! {
    let mut last_report = Instant::now();
    loop {
        while let Ok(command) = speaker.to.try_receive() {
            match command {
                PlaybackCommand::Play(sound) => {
                    log::info!("[PLAY] {sound:?}");
                    if let Err(error) = start(shared, sound) {
                        log::error!("[PLAY] {sound:?} refused: {error}");
                        finished(intent_bus).await;
                    }
                }
                PlaybackCommand::SetMuted(muted) => {
                    log::info!("[PLAY] mute {}", u8::from(muted));
                    // The codec and the DMA live in one object, so this cannot leave the
                    // lock; it must stay one bounded register write, with no log or alloc.
                    if let Err(error) = shared.lock(|s| s.speaker.borrow_mut().set_muted(muted)) {
                        log::error!("[PLAY] mute refused: {error}");
                    }
                }
            }
        }
        if shared.lock(|s| {
            s.done.swap(false, Ordering::Relaxed) && s.sounding.swap(false, Ordering::Relaxed)
        }) {
            finished(intent_bus).await;
        }
        if let Some(recovery) = shared.lock(|s| s.speaker.borrow_mut().recover()) {
            report_recovery("PLAY", &recovery);
        }
        // Millihertz keeps the division in integers. `feeds` counts this window alone; a
        // running total would drift and report an unreachable rate.
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
        let deadline = last_report + Duration::from_millis(CADENCE_REPORT_MS);
        select3(
            speaker.to.ready_to_receive(),
            FEED_WAKE.wait(),
            Timer::at(deadline),
        )
        .await;
    }
}

/// Reports one recovery outcome under the caller's tag. The product and the bench probe ask
/// the driver the same question, so they must read the answer the same way.
pub fn report_recovery(tag: &str, recovery: &Recovery) {
    if recovery.repaired {
        log::info!(
            "[{tag}] recovered the outgoing DMA ({}), ring dry after {} ms idle, {} of {} bytes free, sound was {}",
            recovery.restarts,
            recovery.runway_ms,
            recovery.free_bytes,
            recovery.ring_bytes,
            if recovery.playing { "playing" } else { "idle" },
        );
    } else {
        log::error!(
            "[{tag}] the outgoing DMA refused the rebuild, {} of {} bytes free, sound was {}",
            recovery.free_bytes,
            recovery.ring_bytes,
            if recovery.playing { "playing" } else { "idle" },
        );
    }
}

async fn finished(intent_bus: &'static IntentBus) {
    intent_bus
        .send(Intent::Business(BusinessIntent::Playback(
            PlaybackAction::Finished,
        )))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::playback::SampleSource;

    struct Silent;

    impl Speaker for Silent {
        fn play(&mut self, _source: Box<dyn SampleSource>) -> Result<(), SpeakerFault> {
            Ok(())
        }

        fn set_muted(&mut self, _muted: bool) -> Result<(), SpeakerFault> {
            Ok(())
        }

        fn feed(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn recover(&mut self) -> Option<iot_core::drivers::playback::Recovery> {
            None
        }
    }

    #[test]
    fn idle_silence_wakes_nobody_but_one_ended_sound_wakes_once() {
        let playback = Playback::new(Box::new(Silent));

        assert!(
            !feed_edge(&playback, false, false),
            "an idle silent ring woke the control side on every feed"
        );

        playback.sounding.store(true, Ordering::Relaxed);
        assert!(
            feed_edge(&playback, false, false),
            "the feed that ended a sound did not wake the control side"
        );
        assert!(
            !feed_edge(&playback, false, false),
            "the same ended sound woke the control side twice"
        );
    }

    #[test]
    fn a_recorded_drain_wakes_the_control_side_even_when_nothing_sounds() {
        let playback = Playback::new(Box::new(Silent));

        assert!(
            feed_edge(&playback, true, true),
            "a drain waiting to be repaired did not wake the control side"
        );
    }
}

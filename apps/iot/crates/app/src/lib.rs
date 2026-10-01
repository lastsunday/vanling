#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::Ordering;

use embassy_executor::SendSpawner;
use embassy_futures::join::{join, join3, join4};
use embassy_time::{Duration, Instant, Timer};
use iot_core::drivers::board::{
    Board as BoardTrait, HasAudio, HasInput, HasLight, HasMotion, HasPlayback,
};
use iot_core::drivers::playback::{FEED_MS, Sound, Speaker};
use iot_core::state::DeviceManager;

pub mod input;
pub mod playback;
pub mod render;

use input::{INTENT_BUS, input_task};
use playback::{
    FEED_LATE_MS, PLAYBACK_BUS, Shared, control_loop, feed_loop, feed_task, share, take_stats,
};
use render::{LightRenderer, RENDER_BUS, Render, render_loop};

/// Where the feed loop runs, because the feed has a deadline the rest of the app can
/// break and the board may or may not be able to defend it.
///
/// The feed writes the DMA ring every [`FEED_MS`], and since this board hangs the
/// capture's clock off the transmit unit, that write also keeps the microphone's clock
/// alive — a late feed is a gap in the speaker *and* a stalled capture. Input, render
/// and the control loop share one cooperative executor where a task that does not
/// `.await` holds the others, long enough to overrun the ring's whole runway. An
/// [`InterruptExecutor`] preempts it. One that has none — the host, a board with no
/// speaker — takes [`Inline`](Self::Inline).
///
/// The measurements behind that choice are in the record, not here.
pub enum SpeakerRunner {
    /// Run the feed on a higher-priority interrupt executor, so a long
    /// cooperative holder cannot delay it.
    Interrupt(SendSpawner),
    /// Run the feed on the caller's own executor, joined with everything else.
    Inline,
}

/// Application composition: takes the board's light and input sources,
/// registers one light renderer per wired surface, and runs the persistent tasks.
/// Generic over capabilities, so every board wiring the same capabilities is
/// served by this single copy.
pub async fn run<B>(mut board: B, speaker_runner: SpeakerRunner) -> !
where
    B: BoardTrait + HasLight + HasInput + HasMotion + HasAudio + HasPlayback + 'static,
{
    let lights = board.take_lights().expect("board has a wired light");
    let mut sources = board.take_input().unwrap_or_default();
    let motion = board.take_motion();
    let motion_enabled = motion.is_some();
    let motion_caps = board.motion_capabilities();
    log::info!(
        "[MOTION] declared capabilities 0x{:X}, source {}",
        motion_caps.bits(),
        if motion_enabled { "wired" } else { "absent" }
    );
    // The capture is just another polled source: it joins the same tick rather
    // than a task of its own, so a slow poll costs envelope columns and nothing
    // else.
    let audio = board.take_audio();
    let audio_enabled = audio.is_some();
    sources.extend(motion);
    sources.extend(audio);

    // Unlike the capture, playback is a task of its own: a sound has to keep
    // being fed while it plays, and a task is what makes that independent of how
    // often anything else is polled. A board with no speaker contributes no
    // task — the page is gated off and the bus stays empty.
    let speaker = board.take_playback();
    let playback_enabled = speaker.is_some();
    if !playback_enabled {
        log::info!("[PLAY] no speaker wired");
    }

    let mut render = Render::new();
    for (instance, light) in lights.into_iter().enumerate() {
        let _ = render.register(Box::new(LightRenderer::new(instance as u8, light)), 0);
    }
    let mut manager = Box::new(
        DeviceManager::with_motion(motion_enabled, motion_caps)
            .with_audio(audio_enabled)
            .with_playback(playback_enabled),
    );
    // Both playback loops reach the driver through one shared handle rather than
    // owning it, because they run on different executors: the feed has to be
    // preemptible and the codec's I2C calls must not be. The board's driver is
    // erased to a trait object on the way in — that erasure is what lets the feed
    // loop be a task, and the erasure needs a nameable error type, which is why
    // `Speaker::Error` is one named type rather than an implementation's own.
    let shared = speaker.map(|speaker| share(speaker as Box<dyn Speaker>));

    // A board without a speaker still has to run the tasks it has, so the playback
    // loops are only ever started when there is one: parking on buses nothing
    // writes would be two more tasks doing nothing. Three arms rather than one, so
    // the tuples the joins return never have to agree.
    match (shared, speaker_runner) {
        (Some(shared), SpeakerRunner::Interrupt(spawner)) => {
            // Spawned, not joined: it is on the other executor, and joining a
            // future that lives on an executor this one does not run would wait
            // for ever.
            spawner.spawn(
                feed_task(shared).expect("the feed executor has room for the one task on it"),
            );
            match join3(
                input_task(&INTENT_BUS, sources),
                render_loop(
                    &INTENT_BUS,
                    &RENDER_BUS,
                    &PLAYBACK_BUS,
                    render,
                    &mut manager,
                ),
                control_loop(shared, &PLAYBACK_BUS, &INTENT_BUS),
            )
            .await {}
        }
        (Some(shared), SpeakerRunner::Inline) => match join4(
            input_task(&INTENT_BUS, sources),
            render_loop(
                &INTENT_BUS,
                &RENDER_BUS,
                &PLAYBACK_BUS,
                render,
                &mut manager,
            ),
            control_loop(shared, &PLAYBACK_BUS, &INTENT_BUS),
            playback::feed_loop(shared),
        )
        .await {},
        (None, _) => match join(
            input_task(&INTENT_BUS, sources),
            render_loop(
                &INTENT_BUS,
                &RENDER_BUS,
                &PLAYBACK_BUS,
                render,
                &mut manager,
            ),
        )
        .await {},
    }
}

/// Playback isolation probe: the product's own feed loop, plus a walker that keeps a
/// sound going so the loop has something to carry, and the product's own counters.
///
/// See the `audio-probe` binary's own header for why this exists and what it cannot
/// catch.
///
/// Sharing [`feed_loop`] rather than writing a second schedule is what makes the
/// probe evidence about the product: same absolute deadline, same unconditional
/// refill, same counters. A loop of its own would only report its own behaviour,
/// and a subtle difference — feeding while a sound is on the ring rather than
/// always — would make a green probe mean nothing.
pub async fn run_audio_only<B>(mut board: B) -> !
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

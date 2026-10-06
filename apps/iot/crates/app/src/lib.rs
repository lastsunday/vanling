#![no_std]

extern crate alloc;

use alloc::boxed::Box;

use embassy_executor::SendSpawner;
use embassy_futures::join::{join, join3, join4};
use iot_core::drivers::board::{
    Board as BoardTrait, HasAudio, HasCamera, HasInput, HasLight, HasMotion, HasPlayback,
};
use iot_core::drivers::playback::Speaker;
use iot_core::state::{DeviceManager, DisplayPage};

/// The page the device comes up on, named by `IOT_BOOT_PAGE` at build time.
///
/// A build-time convenience, so a page that would otherwise need a triple-tap on the hardware
/// is reachable by flashing a build that names it. It says nothing about what the board can do
/// — that comes from the capabilities declared below, and [`DeviceManager::with_boot_page`]
/// drops the request if the wiring is not there.
///
/// Parsed from a string rather than a feature because the value is an enum, and a feature can
/// only be present or absent: five pages would be five mutually exclusive features, and a build
/// naming two of them would be resolved by the order of the `cfg` arms rather than by anything
/// the caller wrote. One name with several values is the same shape the chip crates use for
/// their own configuration in `.cargo/config.toml`.
fn boot_page() -> DisplayPage {
    boot_page_from_env(option_env!("IOT_BOOT_PAGE"))
}

/// The variant name each page answers to, which is what `IOT_BOOT_PAGE` spells.
const BOOT_PAGE_NAMES: [(&str, DisplayPage); 5] = [
    ("ambient", DisplayPage::Ambient),
    ("attitude", DisplayPage::Attitude),
    ("audio", DisplayPage::Audio),
    ("speaker", DisplayPage::Speaker),
    ("camera", DisplayPage::Camera),
];

/// Resolves a boot page name, falling back to [`DisplayPage::Ambient`].
///
/// Every way this can fail falls back rather than fails the build: unset, empty, or a name that
/// is not a page. A mistyped variable is a build-time mistake, and a firmware that refuses to
/// link over it is harder to diagnose than one that comes up on the default page and says so.
///
/// Case-insensitive and space-tolerant, because a variable set from a shell or a CI job arrives
/// in whatever case the author typed, and a page that only booted when spelled exactly right
/// would be a page nobody reaches. Neither check can be `const`: both walk the name, and a
/// `const` cannot.
fn boot_page_from_env(name: Option<&str>) -> DisplayPage {
    let Some(name) = name else {
        return DisplayPage::Ambient;
    };
    let wanted = name.trim().to_ascii_lowercase();
    if let Some((_, page)) = BOOT_PAGE_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == wanted)
    {
        return *page;
    }
    log::warn!("[IOT] IOT_BOOT_PAGE={name:?} is not a page, booting on Ambient");
    DisplayPage::Ambient
}

#[cfg(test)]
mod boot_page_tests {
    use super::{BOOT_PAGE_NAMES, boot_page_from_env};
    use iot_core::state::DisplayPage;

    #[test]
    fn every_page_is_reachable_by_name() {
        // The table is the whole mechanism, so a page missing from it is a page no build can
        // start on -- and nothing else would say so.
        for (name, expected) in BOOT_PAGE_NAMES {
            assert_eq!(
                boot_page_from_env(Some(name)),
                expected,
                "{name:?} does not resolve to the page it names"
            );
        }
    }

    #[test]
    fn a_name_survives_case_and_stray_whitespace() {
        // A variable set from a shell or a CI job arrives in whatever case was typed, and a
        // page reachable only by exact spelling is a page nobody reaches.
        for name in ["CAMERA", "Camera", "  camera  ", "\tcamera\n"] {
            assert_eq!(
                boot_page_from_env(Some(name)),
                DisplayPage::Camera,
                "{name:?} should resolve"
            );
        }
    }

    #[test]
    fn anything_unusable_lands_on_ambient() {
        // Every way this can be wrong falls back rather than failing the link: a firmware that
        // refuses to build over a mistyped variable is harder to diagnose than one that says so
        // and comes up on the default page.
        assert_eq!(boot_page_from_env(None), DisplayPage::Ambient, "unset");
        assert_eq!(boot_page_from_env(Some("")), DisplayPage::Ambient, "empty");
        assert_eq!(
            boot_page_from_env(Some("   ")),
            DisplayPage::Ambient,
            "blank"
        );
        assert_eq!(
            boot_page_from_env(Some("photograph")),
            DisplayPage::Ambient,
            "a name that is not a page"
        );
    }

    #[test]
    fn the_default_and_the_named_page_are_different_pages() {
        // Guards the reason this exists: if every name resolved to Ambient the feature would
        // compile and quietly do nothing, which is the failure a bench would report as "the
        // page is not there".
        assert_eq!(boot_page_from_env(None), DisplayPage::Ambient);
        assert_ne!(boot_page_from_env(Some("camera")), DisplayPage::Ambient);
    }
}

pub mod input;
pub mod playback;
pub mod probe;
pub mod render;

use input::{INTENT_BUS, input_task};
use playback::{PLAYBACK_BUS, control_loop, feed_task, share};
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
    B: BoardTrait + HasLight + HasInput + HasMotion + HasAudio + HasPlayback + HasCamera + 'static,
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

    // The camera goes to the panel surface rather than to a renderer of its own: a frame has
    // nowhere to be seen except the panel, so a separate renderer would have to share the one
    // buffer with the surface that already owns it. The surface is also the only thing that
    // knows how to park the chain before painting a colour under it.
    let mut camera = board.take_camera();
    let camera_enabled = camera.is_some();
    if !camera_enabled {
        log::info!("[CAM] no camera wired, the Camera page stays dark");
    }

    let mut render = Render::new();
    for (instance, light) in lights.into_iter().enumerate() {
        let renderer = LightRenderer::new(instance as u8, light);
        // Instance 0 is the panel — the one surface with a frame buffer to fill. A strip on a
        // board that also mounts a sensor takes nothing, and the camera is dropped rather than
        // kept alive on a surface that cannot draw it.
        match camera.take() {
            Some(taken) if instance == 0 => {
                let _ = render.register(Box::new(renderer.with_camera(taken)), 0);
            }
            Some(taken) => {
                log::info!("[CAM] surface {instance} has no frame buffer, dropping the camera");
                drop(taken);
                let _ = render.register(Box::new(renderer), 0);
            }
            None => {
                let _ = render.register(Box::new(renderer), 0);
            }
        }
    }
    let mut manager = Box::new(
        DeviceManager::with_motion(motion_enabled, motion_caps)
            .with_audio(audio_enabled)
            .with_playback(playback_enabled)
            .with_camera(camera_enabled)
            .with_boot_page(boot_page()),
    );
    if manager.state().page != DisplayPage::Ambient {
        log::info!("[IOT] boot page is {:?}", manager.state().page);
    }
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

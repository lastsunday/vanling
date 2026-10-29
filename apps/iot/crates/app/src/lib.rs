#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::RefCell;

use embassy_executor::SendSpawner;
use embassy_futures::join::{join, join3, join4};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use iot_core::config::Configurator;
use iot_core::demand::{ClockSink, DemandKey, EffectiveDemand, Override};
use iot_core::drivers::board::{
    Board as BoardTrait, HasAudio, HasCamera, HasInput, HasLight, HasMotion, HasPanel, HasPlayback,
};
use iot_core::drivers::playback::Speaker;
use iot_core::state::{DeviceManager, DisplayPage};

/// The demand a running device is under, read by the tasks that schedule against it and written
/// by the one that derives it. Shared state, because a demand is idempotent and only its latest
/// value matters. The [`RefCell`] inside the mutex is never held across an await.
pub type SharedDemand = Mutex<CriticalSectionRawMutex, RefCell<EffectiveDemand>>;

/// The page the device comes up on, named by `IOT_BOOT_PAGE` at build time. It says nothing about
/// what the board can do; [`DeviceManager::with_boot_page`] drops a page the wiring lacks.
///
/// A string rather than a feature, because the value is an enum and mutually exclusive features
/// would be resolved by `cfg` order. This is the shape the chip crates use in `.cargo/config.toml`.
fn boot_page() -> DisplayPage {
    // Resolved while the build runs, not at boot: the run-time form moved the main task's frame
    // by four kilobytes, enough to put the Wi-Fi join below the stack floor.
    if let Some(name) = UNKNOWN_BOOT_PAGE {
        log::warn!("[IOT] IOT_BOOT_PAGE={name:?} is not a page, booting on Ambient");
    }
    BOOT_PAGE
}

fn boot_override() -> Override {
    let parsed = iot_core::demand::parse_override(option_env!("IOT_DEMAND").unwrap_or(""));
    if option_env!("IOT_DEMAND").is_some() && !parsed.names_anything() {
        log::warn!("[IOT] IOT_DEMAND named nothing this firmware recognises: ignored");
    }
    parsed
}

/// The variant name each page answers to, which is what `IOT_BOOT_PAGE` spells.
const BOOT_PAGE_NAMES: [(&str, DisplayPage); 6] = [
    ("ambient", DisplayPage::Ambient),
    ("attitude", DisplayPage::Attitude),
    ("audio", DisplayPage::Audio),
    ("speaker", DisplayPage::Speaker),
    ("camera", DisplayPage::Camera),
    ("network", DisplayPage::Network),
];

const BOOT_PAGE: DisplayPage = boot_page_from_env(option_env!("IOT_BOOT_PAGE"));

const UNKNOWN_BOOT_PAGE: Option<&str> = unknown_page(option_env!("IOT_BOOT_PAGE"));

const fn boot_page_from_env(name: Option<&str>) -> DisplayPage {
    let Some(name) = name else {
        return DisplayPage::Ambient;
    };
    match page_named(name.trim_ascii()) {
        Some(page) => page,
        None => DisplayPage::Ambient,
    }
}

const fn page_named(wanted: &str) -> Option<DisplayPage> {
    let wanted = wanted.as_bytes();
    let mut index = 0;
    while index < BOOT_PAGE_NAMES.len() {
        if eq_ignore_ascii_case(BOOT_PAGE_NAMES[index].0.as_bytes(), wanted) {
            return Some(BOOT_PAGE_NAMES[index].1);
        }
        index += 1;
    }
    None
}

const fn unknown_page(name: Option<&str>) -> Option<&str> {
    let Some(name) = name else {
        return None;
    };
    let trimmed = name.trim_ascii();
    if trimmed.is_empty() || page_named(trimmed).is_some() {
        return None;
    }
    Some(name)
}

const fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if !left[index].eq_ignore_ascii_case(&right[index]) {
            return false;
        }
        index += 1;
    }
    true
}

/// The wiring a build can get wrong without failing to build, held by a test instead.
///
/// Every entry here is a dependency feature that changes behaviour rather than the API: the
/// crate compiles, the code runs, and the mistake shows up only against hardware. Reading the
/// manifest is the one place a host test can see the feature list, since Cargo does not expose
/// a dependency's features to the crate that depends on it.
#[cfg(test)]
mod wiring_tests {
    const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");

    /// Whether one dependency's entry names a feature. An entry may be wrapped over several
    /// lines by `cargo fmt`, so this reads from the name through the line that closes the
    /// inline table rather than looking at a single line.
    fn dependency_names(name: &str, feature: &str) -> bool {
        let mut inside = false;
        for line in WORKSPACE_MANIFEST.lines() {
            inside |= line.starts_with(name);
            if !inside {
                continue;
            }
            if line.contains(feature) {
                return true;
            }
            if line.contains('}') {
                return false;
            }
        }
        panic!("the workspace declares {name}");
    }

    #[test]
    fn the_dhcp_server_can_address_an_ipv4_reply() {
        // Sending the reply is the one thing setup mode's DHCP server does, and the address it
        // sends to is where `edge-nal-embassy` gates the conversion on **its own** `proto-ipv4`,
        // not on the `embassy-net` feature of the same name. With it off, `to_emb_addr` answers
        // `None` for every IPv4 address and the reply comes back `UnsupportedProto` — while the
        // build, the bind and the receive are all silent. A phone that joined the access point
        // then never gets an address and sits on "connecting".
        assert!(
            dependency_names("edge-nal-embassy", "\"proto-ipv4\""),
            "edge-nal-embassy without its own proto-ipv4 cannot address a DHCP reply"
        );
    }
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
    fn only_a_name_no_page_answers_to_is_reported() {
        for name in ["AMBIENT", "  attitude  ", "\tnetwork\n"] {
            assert_eq!(super::unknown_page(Some(name)), None, "{name:?}");
        }
        assert_eq!(super::unknown_page(None), None, "unset");
        assert_eq!(super::unknown_page(Some("   ")), None, "blank");
        assert_eq!(
            super::unknown_page(Some("photograph")),
            Some("photograph"),
            "an unknown name"
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

pub mod frame;
pub mod input;
/// The link lives behind the `wifi` feature rather than inside `run`: it is the one part of the
/// product that needs the radio's own types, and a build without a radio still compiles the
/// rest of the app.
#[cfg(feature = "wifi")]
pub mod network;
/// The bench that replays a credential through the panel's own path. Always compiled, because the
/// render loop steps it unconditionally and [`panel::BENCH`] is what makes it nothing: a build
/// named for the bench types, and every other build's loop has the call folded away.
pub mod panel;
pub mod playback;
pub mod probe;
/// Setup mode's device side: the access point, the page on it, and the services behind both.
/// Behind `wifi` with the link it belongs to — a build with no radio has no access point to
/// put up, and the protocol half of it ([`iot_core::provision`]) is the part a host test drives.
#[cfg(feature = "wifi")]
pub mod provision;
pub mod render;
pub mod sounds;

use input::{CAPTURE, INTENT_BUS, input_task};
use playback::{PLAYBACK_BUS, control_loop, feed_task, share};
use render::{INBOUND, LightRenderer, OUTBOUND, Render, render_loop};

/// Where the feed loop runs. It writes the DMA ring every
/// [`FEED_MS`](iot_core::drivers::playback::FEED_MS), which also keeps the microphone's clock
/// alive; a late feed is a gap in the speaker and a stalled capture. An interrupt executor
/// preempts the cooperative one, where a task that does not `.await` can overrun the runway. A
/// board with no interrupt executor takes [`Inline`](Self::Inline).
pub enum Feed {
    /// Run the feed on a higher-priority interrupt executor, so a long
    /// cooperative holder cannot delay it.
    Interrupt(SendSpawner),
    /// Run the feed on the caller's own executor, joined with everything else.
    Inline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Network {
    /// A station came up: the page belongs in the cycle.
    Present,
    /// No radio, or none that started: the page is skipped.
    Absent,
}

impl Network {
    /// What the entry point found when it took the board's radio.
    pub const fn of(present: bool) -> Self {
        if present { Self::Present } else { Self::Absent }
    }

    /// Whether a page showing the link belongs in the cycle.
    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

pub struct Wiring<C> {
    pub clock: C,
    pub feed: Feed,
    pub network: Network,
}

/// Application composition: take the board's light and input sources, register one renderer per
/// wired surface, and run the persistent tasks. Generic over capabilities, so every board with the
/// same wiring shares this copy.
pub async fn run<B, C>(mut board: B, wiring: Wiring<C>) -> !
where
    B: BoardTrait
        + HasLight
        + HasInput
        + HasMotion
        + HasAudio
        + HasPlayback
        + HasCamera
        + HasPanel
        + 'static,
    C: ClockSink,
{
    let Wiring {
        mut clock,
        feed,
        network,
    } = wiring;
    let lights = board.take_lights().expect("board has a wired light");
    let mut sources = board.take_input().unwrap_or_default();
    let motion = board.take_motion();
    let motion_enabled = motion.is_some();
    let motion_caps = board.motion_capabilities();
    log::info!(
        "[MOTION] {}, capabilities 0x{:X}",
        if motion_enabled { "wired" } else { "absent" },
        motion_caps.bits(),
    );
    let audio = board.take_audio(&CAPTURE);
    let audio_enabled = audio.is_some();
    let mut demand_keys: Vec<DemandKey> = alloc::vec![DemandKey::DISPLAY];
    if let Some(motion) = motion {
        sources.push(motion);
        demand_keys.push(DemandKey::MOTION);
    }
    if let Some(audio) = audio {
        sources.push(audio);
        demand_keys.push(DemandKey::AUDIO);
    }

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
    if camera_enabled {
        demand_keys.push(DemandKey::CAMERA);
    }
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
                render.register(Box::new(renderer.with_camera(taken)), 0);
            }
            Some(taken) => {
                log::info!("[CAM] surface {instance} has no frame buffer, dropping the camera");
                drop(taken);
                render.register(Box::new(renderer), 0);
            }
            None => {
                render.register(Box::new(renderer), 0);
            }
        }
    }
    let override_ = boot_override();
    let mut manager = Box::new(
        DeviceManager::with_motion(motion_enabled, motion_caps)
            .with_demand_keys(demand_keys)
            .with_audio(audio_enabled)
            .with_playback(playback_enabled)
            .with_camera(camera_enabled)
            .with_network(network.is_present())
            .with_panel(board.panel())
            .with_boot_page(boot_page())
            .with_override(override_),
    );
    if manager.boot_page() != DisplayPage::Ambient {
        log::info!("[IOT] boot page is {:?}", manager.boot_page());
    }
    // What the board's parts can be left in. The configurator holds this so the decision
    // has the parts' answer at the moment a demand is made, rather than needing a lookup
    // threaded down to whoever polls them.
    let configurator = Configurator::new(board.motion_configurations());
    // Both playback loops reach the driver through one shared handle rather than
    // owning it, because they run on different executors: the feed has to be
    // preemptible and the codec's I2C calls must not be. The board's driver is
    // erased to a trait object on the way in — that erasure is what lets the feed
    // loop be a task, and the erasure needs a nameable error type, which is why
    // `Speaker::Error` is one named type rather than an implementation's own.
    let shared = speaker.map(|speaker| share(speaker as Box<dyn Speaker>));

    // A local rather than a `static`: this is the function that creates both tasks that
    // read it, so a process-wide value would let a second `run` in one process answer
    // for the first. Opened with the page's demand already in it, which makes the
    // polling loop's first tick schedule against it by construction rather than by a
    // line above saying so. The feed loop is deliberately not given it: that loop's
    // existence is what supplies the microphone's clock, so handing it a gate would
    // make the one thing nothing may switch off the one thing a page could switch.
    let demand = SharedDemand::new(RefCell::new(manager.demand().clone()));
    log::info!("[IOT] demand {}", manager.demand());

    let input_future = input_task(&INTENT_BUS, sources, &configurator, &demand);
    let render_future = render_loop(
        &INTENT_BUS,
        &CAPTURE,
        &OUTBOUND,
        &INBOUND,
        &PLAYBACK_BUS,
        render,
        &mut manager,
        &mut clock,
        &demand,
    );
    match (shared, feed) {
        (Some(shared), Feed::Interrupt(spawner)) => {
            // Spawned, not joined: it is on the other executor, and joining a
            // future that lives on an executor this one does not run would wait
            // for ever.
            spawner.spawn(
                feed_task(shared).expect("the feed executor has room for the one task on it"),
            );
            match join3(
                input_future,
                render_future,
                control_loop(shared, &PLAYBACK_BUS, &INTENT_BUS),
            )
            .await {}
        }
        (Some(shared), Feed::Inline) => match join4(
            input_future,
            render_future,
            control_loop(shared, &PLAYBACK_BUS, &INTENT_BUS),
            playback::feed_loop(shared),
        )
        .await {},
        (None, _) => match join(input_future, render_future).await {},
    }
}

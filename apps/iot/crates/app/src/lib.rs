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
use iot_core::demand::{ClockSink, DemandKey, EffectiveDemand};
use iot_core::diagnostics::DiagnosticsSink;
use iot_core::drivers::board::{
    Board as BoardTrait, HasAudio, HasCamera, HasInput, HasMotion, HasPlayback, WiredSurface,
};
use iot_core::drivers::camera::CameraTarget;
use iot_core::drivers::light::{MAX_LIGHTS, RgbLight};
use iot_core::drivers::playback::Speaker;
use iot_core::state::{DeviceManager, DisplayPage, SurfaceRole};

pub type SharedDemand = Mutex<CriticalSectionRawMutex, RefCell<EffectiveDemand>>;

mod audio;
pub mod bench;
pub mod boot;
pub mod bridge;
mod frame;
pub mod input;
#[cfg(feature = "wifi")]
pub mod net;
mod pipeline;
mod surface;

use audio::{CAPTURE, PLAYBACK_BUS, control_loop, feed_task, share};
use boot::{boot_override, boot_page};
use bridge::{INBOUND, OUTBOUND};
use input::{INTENT_BUS, input_task};
use pipeline::run_loop;
use surface::wired;

pub enum Feed {
    Interrupt(SendSpawner),
    Inline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Network {
    Present,
    Absent,
}

impl Network {
    pub const fn of(present: bool) -> Self {
        if present { Self::Present } else { Self::Absent }
    }

    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

pub struct Wiring<C, S> {
    pub clock: C,
    pub feed: Feed,
    pub network: Network,
    /// The wired output surfaces, each carrying what the board says it is for.
    pub surfaces: Vec<WiredSurface<S>>,
}

/// Application composition: one renderer per wired surface, then the persistent tasks.
pub async fn run<B, C, S>(mut board: B, wiring: Wiring<C, S>) -> !
where
    B: BoardTrait + HasInput + HasMotion + HasAudio + HasPlayback + HasCamera + 'static,
    C: ClockSink,
    S: RgbLight + DiagnosticsSink + CameraTarget + 'static,
{
    let Wiring {
        mut clock,
        feed,
        network,
        surfaces,
    } = wiring;
    assert!(
        surfaces.len() <= MAX_LIGHTS,
        "more wired surfaces than slots"
    );
    let mut surface_roles = [SurfaceRole::Absent; MAX_LIGHTS];
    for (role, wired) in surface_roles.iter_mut().zip(&surfaces) {
        *role = wired.role();
    }
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

    let speaker = board.take_playback();
    let playback_enabled = speaker.is_some();
    if !playback_enabled {
        log::info!("[PLAY] no speaker wired");
    }

    let camera = board.take_camera();
    let camera_enabled = camera.is_some();
    if camera_enabled {
        demand_keys.push(DemandKey::CAMERA);
    }
    if !camera_enabled {
        log::info!("[CAM] no camera wired, the Camera page stays dark");
    }

    let render = wired(
        surfaces
            .into_iter()
            .map(WiredSurface::into_surface)
            .collect(),
        camera,
    );
    let override_ = boot_override();
    let mut manager = Box::new(
        DeviceManager::with_motion(motion_enabled, motion_caps)
            .with_demand_keys(demand_keys)
            .with_audio(audio_enabled)
            .with_playback(playback_enabled)
            .with_camera(camera_enabled)
            .with_network(network.is_present())
            .with_surface_roles(surface_roles)
            .with_boot_page(boot_page())
            .with_override(override_),
    );
    if manager.boot_page() != DisplayPage::Ambient {
        log::info!("[IOT] boot page is {:?}", manager.boot_page());
    }
    let configurator = Configurator::new(board.motion_configurations());
    // One shared handle: the feed runs on the preemptible executor, and the codec's I2C calls must not.
    let shared = speaker.map(|speaker| share(speaker as Box<dyn Speaker>));

    // The feed loop gets no demand gate: its existence is what clocks the microphone.
    let demand = SharedDemand::new(RefCell::new(manager.demand().clone()));
    log::info!("[IOT] demand {}", manager.demand());

    let input_future = input_task(&INTENT_BUS, sources, &configurator, &demand);
    let pipeline_future = run_loop(
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
            // Spawned, not joined: joining a future on the executor this one does not run waits for ever.
            spawner.spawn(
                feed_task(shared).expect("the feed executor has room for the one task on it"),
            );
            match join3(
                input_future,
                pipeline_future,
                control_loop(shared, &PLAYBACK_BUS, &INTENT_BUS),
            )
            .await {}
        }
        (Some(shared), Feed::Inline) => match join4(
            input_future,
            pipeline_future,
            control_loop(shared, &PLAYBACK_BUS, &INTENT_BUS),
            audio::feed_loop(shared),
        )
        .await {},
        (None, _) => match join(input_future, pipeline_future).await {},
    }
}

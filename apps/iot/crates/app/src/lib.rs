#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::RefCell;

use embassy_executor::SendSpawner;
use embassy_futures::join::join3;
use iot_core::config::Configurator;
use iot_core::demand::ClockSink;
use iot_core::drivers::board::{HasAudio, HasCamera, HasInput, HasMotion, HasPlayback};
use iot_core::drivers::playback::Speaker as SpeakerDriver;
use iot_core::drivers::surface::WiredSurface;
use iot_core::state::{Backed, DeviceManager, DisplayPage, Setup};

mod audio;
pub mod bench;
pub mod boot;
mod frame;
pub mod input;
#[cfg(feature = "wifi")]
pub mod net;
mod pipeline;
mod surface;
pub mod workers;

use audio::{playback_tasks, share};
use boot::{boot_override, boot_page};
use input::{INTENT_BUS, input_task};
use pipeline::run_loop;
use surface::wired;
use workers::{CAPTURE, Input, NET, SPEAKER, SharedDemand, Workers};

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

pub struct Wiring<C> {
    pub clock: C,
    pub feed: Feed,
    pub network: Network,
    /// The wired output surfaces, each carrying what the board says it is for.
    pub surfaces: Vec<WiredSurface>,
}

/// Application composition: one renderer per wired surface, then the persistent tasks.
pub async fn run<B, C>(mut board: B, wiring: Wiring<C>) -> !
where
    B: HasInput + HasMotion + HasAudio + HasPlayback + HasCamera,
    C: ClockSink,
{
    let Wiring {
        mut clock,
        feed,
        network,
        surfaces,
    } = wiring;

    // Take everything off the board once; nothing below asks it anything again.
    let mut sources = board.take_input().unwrap_or_default();
    let motion = board.take_motion();
    let audio = board.take_audio(&CAPTURE);
    let speaker = board.take_playback();
    let camera = board.take_camera();
    let motion_caps = board.motion_capabilities();
    let backed = Backed {
        motion: motion.is_some().then_some(motion_caps),
        audio: audio.is_some(),
        playback: speaker.is_some(),
        camera: camera.is_some(),
        network: network.is_present(),
    };
    let configurator = Configurator::new(board.motion_configurations());
    sources.extend(motion);
    sources.extend(audio);

    log::info!(
        "[MOTION] {}, capabilities 0x{:X}",
        if backed.motion.is_some() {
            "wired"
        } else {
            "absent"
        },
        motion_caps.bits(),
    );
    if !backed.playback {
        log::info!("[PLAY] no speaker wired");
    }
    if !backed.camera {
        log::info!("[CAM] no camera wired, the Camera page stays dark");
    }

    let (render, surface_roles) = wired(surfaces, camera);
    let mut manager = Box::new(DeviceManager::boot(Setup {
        backed,
        demand_keys: backed.demand_keys(),
        surface_roles,
        boot_page: boot_page(),
        overrides: boot_override(),
    }));
    let boot = manager.boot_page();
    if boot != DisplayPage::Ambient {
        log::info!("[IOT] boot page is {boot:?}");
    }

    // One shared handle: the feed runs on the preemptible executor, and the codec's I2C calls must not.
    let shared = speaker.map(|speaker| share(speaker as Box<dyn SpeakerDriver>));
    let demand = SharedDemand::new(RefCell::new(manager.demand().clone()));
    log::info!("[IOT] demand {}", manager.demand());

    let playback = playback_tasks(shared, feed, SPEAKER, &INTENT_BUS);
    let workers = Workers {
        input: Input {
            to: &demand,
            from: &CAPTURE,
        },
        speaker: SPEAKER,
        net: NET,
    };
    match join3(
        input_task(&INTENT_BUS, sources, &configurator, workers.input),
        run_loop(&INTENT_BUS, workers, render, &mut manager, &mut clock),
        playback,
    )
    .await {}
}

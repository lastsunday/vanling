#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::RefCell;

use embassy_executor::SendSpawner;
use embassy_futures::join::join;
use iot_core::config::Configurator;
use iot_core::demand::ClockSink;
use iot_core::drivers::board::{HasAudio, HasCamera, HasInput, HasMotion, HasPlayback};
use iot_core::drivers::input::PollEntry;
use iot_core::drivers::playback::Speaker;
use iot_core::drivers::surface::WiredSurface;
use iot_core::state::{Backed, Boot, DeviceManager, DisplayPage};

pub mod bench;
pub mod boot;
mod executor_stack;
mod frame;
pub mod input;
#[cfg(feature = "wifi")]
pub mod net;
mod pipeline;
mod playback;
mod surface;
pub mod workers;

use boot::{boot_override, boot_page};
use input::{INTENT_BUS, input_task};
use pipeline::run_loop;
use playback::{playback_tasks, share};
use surface::wired;
use workers::{CAPTURE, Input, NET, PLAYBACK, SharedDemand, Workers};

pub enum Feed {
    Interrupt(SendSpawner),
    Inline,
}

/// Where the I/O half runs. `Inline` spawns it on the caller's executor, which is what a
/// single-core chip has; `Elsewhere` parks it on `queue` for whatever executor the caller
/// started on another core. The task is the same either way — a placement changes timing,
/// never meaning.
#[derive(Clone, Copy)]
pub enum IoPlacement {
    Inline(embassy_executor::Spawner),
    Elsewhere(&'static IoQueue),
}

/// One slot: the driver half is sent exactly once, at boot.
pub type IoQueue = embassy_sync::channel::Channel<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    Io,
    1,
>;

/// The half of the composition that touches drivers: input polling, the codec, the speaker
/// and the panel bus. Everything in here blocks, so it is the part that must not share an
/// executor with the frame loop.
pub struct Io {
    pub sources: Vec<PollEntry>,
    pub configurator: Configurator,
    pub demand: &'static SharedDemand,
    pub speaker: Option<&'static playback::Shared>,
    pub feed: Feed,
}

pub struct Wiring<C> {
    pub clock: C,
    pub feed: Feed,
    /// Whether a radio came up. `run` never sees the station — `serve` takes it — so the
    /// caller states the bit that gates the Network page.
    pub network: bool,
    /// The wired output surfaces, each carrying what the board says it is for.
    pub surfaces: Vec<WiredSurface>,
    /// Where the driver half runs; the caller is the only one that knows how many cores it
    /// has.
    pub io: IoPlacement,
}

/// The driver half: poll the sources, keep the codec fed, answer playback commands.
async fn io_loop(io: Io) {
    let Io {
        sources,
        configurator,
        demand,
        speaker,
        feed,
    } = io;
    let workers = Workers {
        input: Input {
            to: demand,
            from: &CAPTURE,
        },
        playback: PLAYBACK,
        net: NET,
    };
    match join(
        input_task(&INTENT_BUS, sources, &configurator, workers.input),
        playback_tasks(speaker, feed, PLAYBACK, &INTENT_BUS),
    )
    .await {}
}

#[embassy_executor::task]
pub async fn run_io(io: Io) {
    io_loop(io).await
}

/// The same task, picked up from the queue the coordinator fills after boot: on a second
/// core it is spawned before the coordinator has built the bundle.
#[embassy_executor::task]
pub async fn run_io_from(queue: &'static IoQueue) {
    let io = queue.receive().await;
    io_loop(io).await
}

/// The coordinator half: one renderer per wired surface, then the frame loop.
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
        io,
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
        network,
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

    let (renderers, surface_roles) = wired(surfaces, camera);
    let mut manager = Box::new(DeviceManager::boot(Boot {
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
    let shared = speaker.map(|speaker| share(speaker as Box<dyn Speaker>));
    // Leaked because the driver half outlives this future: on a second core it is another
    // task, and a task cannot borrow the frame loop's stack.
    let demand: &'static SharedDemand = Box::leak(Box::new(SharedDemand::new(RefCell::new(
        manager.demand().clone(),
    ))));
    log::info!("[IOT] demand {}", manager.demand());

    let drivers = Io {
        sources,
        configurator,
        demand,
        speaker: shared,
        feed,
    };
    match io {
        IoPlacement::Inline(spawner) => {
            spawner.spawn(run_io(drivers).expect("the driver task fits its slot"));
        }
        IoPlacement::Elsewhere(queue) => queue.send(drivers).await,
    }

    let workers = Workers {
        input: Input {
            to: demand,
            from: &CAPTURE,
        },
        playback: PLAYBACK,
        net: NET,
    };
    run_loop(&INTENT_BUS, workers, renderers, &mut manager, &mut clock).await
}

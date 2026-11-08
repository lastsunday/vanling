#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::state::{Backed, Boot, DeviceManager, Page};
use embassy_executor::{SendSpawner, Spawner};
use embassy_futures::join::join;
use iot_core::config::Configurator;
use iot_core::demand::ClockSink;
use iot_core::drivers::board::{HasAudio, HasCamera, HasInput, HasMotion, HasPlayback};
use iot_core::drivers::input::PollEntry;
use iot_core::drivers::playback::Speaker;
use iot_core::drivers::surface::WiredSurface;

mod assets;
pub mod bench;
pub mod pages;
pub mod render;
pub mod runtime;
pub mod shared;
pub mod state;

use bench::replay::{BENCH, Slot};
use render::wired;
use runtime::coordinator::{
    CaptureSystem, DemandSystem, GovernorSystem, NetworkSystem, NoticeSystem, PlaybackSystem,
    Schedule, run_loop,
};
use runtime::feature::input::{INTENT_BUS, input_task};
use runtime::feature::playback::{playback_tasks, share};
use runtime::workers::{CAPTURE, Input, NETWORK, PLAYBACK, SharedDemand};
use state::boot::{boot_override, boot_page};

pub enum Feed {
    Interrupt(SendSpawner),
    Inline,
}

/// Where the I/O half runs. `Inline` spawns it on the coordinator's executor, which is what
/// a single-core chip has; `Elsewhere` parks it on `queue` for whatever executor the caller
/// started on another core. The task is the same either way — a placement changes timing,
/// never meaning.
#[derive(Clone, Copy)]
pub enum IoPlacement {
    Inline,
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
/// executor with the coordinator.
pub struct Io {
    pub sources: Vec<PollEntry>,
    pub configurator: Configurator,
    pub demand: &'static SharedDemand,
    pub speaker: Option<&'static runtime::feature::playback::Shared>,
    pub feed: Feed,
    /// How to read the second core's stack, when the part has one. The composition root
    /// injects it, so the library names no chip family.
    pub second_core_peak: fn() -> Option<(usize, usize)>,
}

/// Starts the network worker. The composition root built it from the station it took off the
/// board, so `run` starts it without ever naming the radio or the driver.
pub struct NetworkWorker(Box<dyn FnOnce(Spawner)>);

impl NetworkWorker {
    pub fn new(start: impl FnOnce(Spawner) + 'static) -> Self {
        Self(Box::new(start))
    }

    fn spawn(self, spawner: Spawner) {
        (self.0)(spawner);
    }
}

/// The radio as the composition root hands it over. One value, because there is one radio:
/// `Absent` leaves the Network page dark, `Unattended` is a wired radio this composition starts
/// no worker for — the host smoke drives the page on its own — and `Worker` is how `run` starts
/// it.
pub enum Wifi {
    Absent,
    Unattended,
    Worker(NetworkWorker),
}

impl Wifi {
    /// The page's gate is the radio, not the worker.
    pub const fn wired(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

pub struct Wiring<C> {
    pub clock: C,
    pub feed: Feed,
    /// The radio: whether there is one, and how to start the worker that uses it.
    pub wifi: Wifi,
    /// The wired output surfaces, each carrying what the board says it is for.
    pub surfaces: Vec<WiredSurface>,
    /// Where the driver half runs; the caller is the only one that knows how many cores it
    /// has.
    pub io: IoPlacement,
    /// The executor the driver half starts on. Every other worker is spawned by whoever owns
    /// its drivers, before this call.
    pub spawner: Spawner,
    /// How to read the second core's stack on this part; `None`-returning when it has no
    /// second core. Injected by the family entry point, like the clock.
    pub second_core_peak: fn() -> Option<(usize, usize)>,
}

/// The driver half: poll the sources, keep the codec fed, answer playback commands.
async fn io_loop(io: Io) {
    let Io {
        sources,
        configurator,
        demand,
        speaker,
        feed,
        second_core_peak,
    } = io;
    let input = Input::new(demand);
    match join(
        input_task(&INTENT_BUS, sources, &configurator, input, second_core_peak),
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

/// The coordinator half: one renderer per wired surface, the driver half, then the frame
/// loop. Every other worker was spawned by whoever owns its drivers, before this call.
pub async fn run<B, C>(mut board: B, wiring: Wiring<C>) -> !
where
    B: HasInput + HasMotion + HasAudio + HasPlayback + HasCamera,
    C: ClockSink,
{
    let Wiring {
        mut clock,
        feed,
        wifi,
        surfaces,
        io,
        spawner,
        second_core_peak,
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
        network: wifi.wired(),
    };
    if !matches!(&wifi, Wifi::Worker(_)) {
        // No worker means no radio to wait for: absent by wiring, or driven by the caller.
        // The boot can finish its network half here, and the first frame finishes the rest.
        iot_core::boot::note_network_settled();
    }
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
    if boot != Page::Ambient {
        log::info!("[IOT] boot page is {boot:?}");
    }

    // One shared handle: the feed runs on the preemptible executor, and the codec's I2C calls must not.
    let shared = speaker.map(|speaker| share(speaker as Box<dyn Speaker>));
    // Leaked because the driver half outlives this future: on a second core it is another
    // task, and a task cannot borrow the coordinator's stack.
    let demand: &'static SharedDemand =
        Box::leak(Box::new(SharedDemand::new(Cell::new(*manager.demand()))));
    log::info!("[IOT] demand {}", manager.demand());

    let drivers = Io {
        sources,
        configurator,
        demand,
        speaker: shared,
        feed,
        second_core_peak,
    };
    match io {
        IoPlacement::Inline => {
            spawner.spawn(run_io(drivers).expect("the driver task fits its slot"));
        }
        IoPlacement::Elsewhere(queue) => queue.send(drivers).await,
    }

    // Started here, where the coordinator is built: the closure is the composition root's, so
    // this half names neither the radio nor its driver.
    if let Wifi::Worker(worker) = wifi {
        worker.spawn(spawner);
    }

    let state = manager.state_ref();
    let mut schedule = Schedule::new();
    schedule.register(CaptureSystem::new(&CAPTURE));
    schedule.register(NetworkSystem::new(state, NETWORK));
    schedule.register(PlaybackSystem::new(state.playback, PLAYBACK));
    schedule.register(NoticeSystem);
    schedule.register(DemandSystem::new(Input::new(demand)));
    schedule.register(GovernorSystem::new(&mut clock));
    if BENCH {
        schedule.register(Slot::new());
    }

    run_loop(&INTENT_BUS, renderers, &mut manager, &mut schedule).await
}

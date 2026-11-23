#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::state::{Backed, Boot, DeviceManager, PageId};
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
pub(crate) mod render;
pub mod runtime;
pub mod services;
pub mod shared;
pub mod state;

use render::wire;
use runtime::Runtime;
use runtime::coordinator::{
    CaptureSystem, DemandSystem, GovernorSystem, NetworkSystem, NoticeSystem, PageSystem,
    PlaybackSystem, Schedule, run_loop,
};
use runtime::feature::input::{IntentBus, input_task};
use runtime::feature::playback::{playback_tasks, share};
use runtime::system::System;
use runtime::workers::{FeedWake, Input, Playback, SharedDemand};
use state::env::{boot_override, boot_page, lang};

pub enum Feed {
    Interrupt(SendSpawner),
    Inline,
}

/// Where the I/O half runs: on the coordinator's executor (`Inline`), or on another core's
/// via `queue`. The task is the same either way; placement changes timing, never meaning.
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

/// The half of the composition that touches drivers and blocks, so it must not share an
/// executor with the coordinator.
pub struct Io {
    pub sources: Vec<PollEntry>,
    pub configurator: Configurator,
    pub input: Input<'static>,
    pub speaker: Option<&'static runtime::feature::playback::Shared>,
    pub playback: &'static Playback,
    pub feed: Feed,
    /// Where this half reports upward; a second-core task cannot reach the coordinator's stack.
    pub bus: &'static IntentBus,
    /// The feeder's poke, carried for the same reason as `bus`.
    pub feed_wake: &'static FeedWake,
    /// How to read the second core's stack, when the part has one; injected by the composition
    /// root so the library names no chip family.
    pub second_core_peak: fn() -> Option<(usize, usize)>,
}

/// Starts the network worker from the station the board took off itself, so `run` names neither
/// the radio nor its driver.
pub struct NetworkWorker(Box<dyn FnOnce(Spawner)>);

impl NetworkWorker {
    pub fn new(start: impl FnOnce(Spawner) + 'static) -> Self {
        Self(Box::new(start))
    }

    fn spawn(self, spawner: Spawner) {
        (self.0)(spawner);
    }
}

/// The radio as the composition root hands it over: `Absent` leaves the Network page dark,
/// `Unattended` is a wired radio this composition starts no worker for, `Worker` starts one.
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
    /// The handles the entry built once, shared by both halves.
    pub runtime: &'static Runtime,
    pub feed: Feed,
    pub wifi: Wifi,
    /// The wired output surfaces, each carrying the role the board gave it.
    pub surfaces: Vec<WiredSurface>,
    /// Where the driver half runs; only the caller knows how many cores it has.
    pub io: IoPlacement,
    /// The executor the driver half starts on. Every other worker is spawned by whoever owns
    /// its drivers, before this call.
    pub spawner: Spawner,
    /// How to read the second core's stack; `None` when the part has none. Injected by the
    /// family entry point, like the clock.
    pub second_core_peak: fn() -> Option<(usize, usize)>,
}

async fn io_loop(io: Io) {
    let Io {
        sources,
        configurator,
        input,
        speaker,
        playback,
        feed,
        bus,
        feed_wake,
        second_core_peak,
    } = io;
    match join(
        input_task(bus, sources, &configurator, input, second_core_peak),
        playback_tasks(speaker, feed, playback, bus, feed_wake),
    )
    .await {}
}

#[embassy_executor::task]
pub async fn run_io(io: Io) {
    io_loop(io).await
}

/// The same task, picked up from the queue: on a second core it is spawned before the
/// coordinator has built the bundle.
#[embassy_executor::task]
pub async fn run_io_from(queue: &'static IoQueue) {
    let io = queue.receive().await;
    io_loop(io).await
}

/// The product entry: the coordinator half with no extra system.
pub async fn run<B, C>(board: B, wiring: Wiring<C>) -> !
where
    B: HasInput + HasMotion + HasAudio + HasPlayback + HasCamera,
    C: ClockSink,
{
    run_with(board, wiring, None).await
}

/// The coordinator half: one renderer per wired surface, the driver half, then the frame loop.
///
/// `extra` is the one system the caller may add to the schedule; only the panel replay's entry
/// uses it, so the product path names no bench.
pub(crate) async fn run_with<B, C>(
    mut board: B,
    wiring: Wiring<C>,
    extra: Option<Box<dyn System>>,
) -> !
where
    B: HasInput + HasMotion + HasAudio + HasPlayback + HasCamera,
    C: ClockSink,
{
    let Wiring {
        mut clock,
        runtime,
        feed,
        wifi,
        surfaces,
        io,
        spawner,
        second_core_peak,
    } = wiring;
    let bus = &runtime.bus;
    let capture = &runtime.capture;
    let network = &runtime.network;
    let scan = &network.scan;
    let playback = &runtime.playback;
    let feed_wake = &runtime.feed_wake;
    let readiness = &runtime.readiness;

    // Take everything off the board once; nothing below asks it anything again.
    let mut sources = board.take_input().unwrap_or_default();
    let motion = board.take_motion();
    let audio = board.take_audio(capture);
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
        // No worker means no radio to wait for: the boot can finish its network half here.
        readiness.note_network();
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

    let (renderers, surface_roles) = wire(surfaces, camera);
    let mut manager = Box::new(DeviceManager::boot(Boot {
        backed,
        demand_keys: backed.demand_keys(),
        surface_roles,
        boot_page: boot_page(),
        lang: lang(),
        overrides: boot_override(),
    }));
    let boot = manager.boot_page();
    if boot != PageId::Input {
        log::info!("[IOT] boot page is {boot:?}");
    }
    log::info!("[IOT] words in {:?}", manager.state_ref().lang);

    // One shared handle: the feed runs on the preemptible executor, and the codec's I2C calls must not.
    let shared = speaker.map(|speaker| share(speaker as Box<dyn Speaker>));
    // Leaked because the driver half outlives this future and cannot borrow the coordinator's stack.
    let demand: &'static SharedDemand =
        Box::leak(Box::new(SharedDemand::new(Cell::new(*manager.demand()))));
    log::info!("[IOT] demand {}", manager.demand());

    let drivers = Io {
        sources,
        configurator,
        input: Input::new(demand, capture),
        speaker: shared,
        playback,
        feed,
        bus,
        feed_wake,
        second_core_peak,
    };
    match io {
        IoPlacement::Inline => {
            spawner.spawn(run_io(drivers).expect("the driver task fits its slot"));
        }
        IoPlacement::Elsewhere(queue) => queue.send(drivers).await,
    }

    // The closure is the composition root's, so this half names neither the radio nor its driver.
    if let Wifi::Worker(worker) = wifi {
        worker.spawn(spawner);
    }

    let state = manager.state_ref();
    let mut schedule = Schedule::new();
    schedule.register(CaptureSystem::new(capture));
    schedule.register(NetworkSystem::new(state, network));
    schedule.register(PlaybackSystem::new(state.playback, playback));
    schedule.register(NoticeSystem);
    schedule.register(PageSystem::new());
    schedule.register(DemandSystem::new(Input::new(demand, capture)));
    schedule.register(GovernorSystem::new(&mut clock));
    if let Some(system) = extra {
        schedule.register_boxed(system);
    }

    run_loop(
        bus,
        capture,
        scan,
        readiness,
        renderers,
        &mut manager,
        &mut schedule,
    )
    .await
}

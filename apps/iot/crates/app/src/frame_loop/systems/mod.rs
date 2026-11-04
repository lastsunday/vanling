//! The systems the coordinator runs once per frame, and the contract they share.

mod bench;
mod capture;
mod demand;
mod governor;
mod network;
mod notice;
mod playback;

use iot_core::demand::ClockSink;
use iot_core::state::{DeviceManager, DeviceState};

use crate::input::IntentBus;
use crate::workers::Workers;

use self::bench::BenchSystem;
use self::capture::CaptureSystem;
use self::demand::DemandSystem;
use self::governor::GovernorSystem;
use self::network::NetworkSystem;
use self::notice::NoticeSystem;
use self::playback::PlaybackSystem;

/// One pass takes at most [`crate::input::INTENT_SLOTS`] intents and each can carry a command,
/// so the network inbox is at least that deep: on the normal path nothing is given up.
const _: () = assert!(crate::workers::COMMAND_SLOTS >= crate::input::INTENT_SLOTS);

/// Everything a system may touch in one frame. The wall clock rather than the frame counter:
/// a held executor makes a logical clock replay the steps it skipped.
pub(crate) struct Frame<'a> {
    pub(crate) bus: &'a IntentBus,
    pub(crate) manager: &'a mut DeviceManager,
    pub(crate) workers: &'a Workers<'a>,
    pub(crate) now_ms: u32,
}

/// One slice of coordination, run once per frame. A system talks to one boundary: it writes
/// state to a worker, takes what a worker published, or advances state only time moves. Systems
/// never call each other; whatever they share arrives in the [`Frame`].
pub(crate) trait System {
    fn tick(&mut self, frame: &mut Frame<'_>);
}

/// The systems in the order a frame runs them. Declaration order is execution order, so
/// reordering these fields is a behaviour change, not a tidy-up.
pub(crate) struct Schedule<'a, C: ClockSink> {
    capture: CaptureSystem,
    network: NetworkSystem,
    playback: PlaybackSystem,
    notice: NoticeSystem,
    demand: DemandSystem,
    governor: GovernorSystem<'a, C>,
    bench: BenchSystem,
}

impl<'a, C: ClockSink> Schedule<'a, C> {
    pub(crate) fn new(state: &DeviceState, clock: &'a mut C) -> Self {
        Self {
            capture: CaptureSystem,
            network: NetworkSystem::new(state),
            playback: PlaybackSystem::new(state.playback),
            notice: NoticeSystem,
            demand: DemandSystem,
            governor: GovernorSystem::new(clock),
            bench: BenchSystem::new(),
        }
    }

    /// Bus → state. The command an intent produced goes out before the next intent is taken:
    /// two edges in one frame (mute, then unmute) must not collapse into the last one. The
    /// network inbox takes it without waiting, giving up its oldest entry when it has no room,
    /// so a busy worker costs the panel a request instead of costing the coordinator a frame.
    pub(crate) fn process_intents(
        &mut self,
        bus: &IntentBus,
        workers: Workers<'_>,
        manager: &mut DeviceManager,
    ) {
        while let Ok(intent) = bus.try_receive() {
            if let Some(command) = manager.apply_intent(intent) {
                workers.network.commands.publish(command);
            }
            self.playback
                .dispatch(workers.playback.to, manager.playback_state());
        }
    }

    pub(crate) fn tick(&mut self, frame: &mut Frame<'_>) {
        self.capture.tick(frame);
        self.network.tick(frame);
        self.notice.tick(frame);
        self.demand.tick(frame);
        self.governor.tick(frame);
        self.bench.tick(frame);
    }
}

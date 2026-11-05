//! The systems the coordinator runs once per frame, and the contract they share.

mod capture;
mod demand;
mod governor;
mod network;
mod notice;
mod playback;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::model::state::Command;
use crate::runtime::system::{Frame, System};

pub(crate) use self::capture::CaptureSystem;
pub(crate) use self::demand::DemandSystem;
pub(crate) use self::governor::GovernorSystem;
pub(crate) use self::network::NetworkSystem;
pub(crate) use self::notice::NoticeSystem;
pub(crate) use self::playback::PlaybackSystem;

/// One pass takes at most [`crate::runtime::feature::input::INTENT_SLOTS`] intents and each can carry a command,
/// so the network inbox is at least that deep: on the normal path nothing is given up.
const _: () =
    assert!(crate::runtime::workers::COMMAND_SLOTS >= crate::runtime::feature::input::INTENT_SLOTS);

/// The registered systems. Registration order is execution order, so the composition root
/// decides a frame's sequence; the schedule names no worker and no feature.
pub(crate) struct Schedule<'a> {
    steps: Vec<Box<dyn System + 'a>>,
}

impl<'a> Schedule<'a> {
    pub(crate) fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub(crate) fn register(&mut self, system: impl System + 'a) {
        self.steps.push(Box::new(system));
    }

    /// Bus → state. Each intent is applied, then every system is offered what the model left
    /// behind before the next intent is taken: two edges in one frame (mute, then unmute) must
    /// not collapse into the last one. The system that owns a boundary takes its own command,
    /// so a leftover one means a boundary was registered without its system.
    pub(crate) fn process_intents(&mut self, frame: &mut Frame<'_>) {
        while let Ok(intent) = frame.bus.try_receive() {
            let mut command: Option<Command> = frame.manager.apply_intent(intent);
            for step in &mut self.steps {
                step.intent(frame, &mut command);
            }
            debug_assert!(command.is_none(), "no registered system took {command:?}");
        }
    }

    pub(crate) fn tick(&mut self, frame: &mut Frame<'_>) {
        for step in &mut self.steps {
            step.tick(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    //! The registry's contract: registration order is execution order, each intent reaches
    //! every system once, and a frame ticks each system once.

    use alloc::rc::Rc;
    use core::cell::RefCell;

    use super::*;
    use crate::model::intent::{ActionIntent, Intent, NetworkPageIntent};
    use crate::model::state::{Backed, Boot, DeviceManager, NetworkCommand};
    use crate::runtime::feature::input::IntentBus;
    use crate::runtime::workers::NETWORK;

    #[derive(Clone)]
    struct Recorder {
        name: &'static str,
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    impl System for Recorder {
        fn tick(&mut self, _frame: &mut Frame<'_>) {
            self.log.borrow_mut().push(self.name);
        }

        fn intent(&mut self, _frame: &mut Frame<'_>, _command: &mut Option<Command>) {
            self.log.borrow_mut().push(self.name);
        }
    }

    fn manager() -> DeviceManager {
        DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            ..Default::default()
        })
    }

    fn scan() -> Intent {
        Intent::Action(ActionIntent::NetworkPage(NetworkPageIntent::ScanNow))
    }

    #[test]
    fn tick_and_intent_visit_the_registry_in_registration_order() {
        let bus = IntentBus::new();
        let mut manager = manager();
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut schedule = Schedule::new();
        schedule.register(Recorder {
            name: "first",
            log: Rc::clone(&log),
        });
        schedule.register(Recorder {
            name: "second",
            log: Rc::clone(&log),
        });

        let _ = bus.try_send(Intent::Action(ActionIntent::Invalid));
        let _ = bus.try_send(Intent::Action(ActionIntent::Invalid));
        {
            let mut frame = Frame {
                bus: &bus,
                manager: &mut manager,
                now_ms: 0,
            };
            schedule.process_intents(&mut frame);
            schedule.tick(&mut frame);
        }

        assert_eq!(
            &*log.borrow(),
            &[
                "first", "second", // the first intent
                "first", "second", // the second intent
                "first", "second", // the frame's tick
            ]
        );
    }

    #[test]
    fn a_network_command_reaches_the_worker_once_per_intent() {
        while NETWORK.commands.try_receive().is_some() {}
        let bus = IntentBus::new();
        let mut manager = manager();
        let mut schedule = Schedule::new();
        schedule.register(NetworkSystem::new(manager.state_ref(), NETWORK));

        let _ = bus.try_send(scan());
        let _ = bus.try_send(scan());
        {
            let mut frame = Frame {
                bus: &bus,
                manager: &mut manager,
                now_ms: 0,
            };
            schedule.process_intents(&mut frame);
        }

        assert_eq!(
            NETWORK.commands.try_receive(),
            Some(NetworkCommand::ScanNow)
        );
        assert_eq!(
            NETWORK.commands.try_receive(),
            Some(NetworkCommand::ScanNow)
        );
        assert_eq!(
            NETWORK.commands.try_receive(),
            None,
            "two edges in one frame do not collapse into one"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "no registered system took")]
    fn a_command_nobody_takes_is_not_dropped_silently() {
        let bus = IntentBus::new();
        let mut manager = manager();
        let mut schedule = Schedule::new();

        let _ = bus.try_send(scan());
        let mut frame = Frame {
            bus: &bus,
            manager: &mut manager,
            now_ms: 0,
        };
        schedule.process_intents(&mut frame);
    }
}

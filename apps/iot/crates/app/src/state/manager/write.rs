//! Every write: an intent off the bus, a report from a worker, the clock's tick.
//!
//! Systems write, renderers read.

use super::DeviceManager;
use crate::shared::intent::{BusinessIntent, InputIntent, Intent, LinkIntent};
use crate::shared::model::{Command, Effects};
use iot_core::drivers::audio::AudioSample;
use iot_core::drivers::input::InputEvent;
use iot_core::drivers::network::ApList;

impl DeviceManager {
    /// Route one intent all the way to the state it moves, and answer with what the model
    /// cannot do itself.
    ///
    /// The two halves touch different fields: the write lands in `touch`/`motion`, which only
    /// renderers and the frame snapshot read, while the binding reads the page, the lights and
    /// the link. Their order is a reading order, not a dependency.
    pub fn apply_intent(
        &mut self,
        intent: Intent,
        layout: &iot_core::ui::Layout,
    ) -> Option<Command> {
        let business = match intent {
            Intent::Input(op) => {
                self.apply_input(op);
                self.layout = *layout;
                self.services.bind(&op, &self.state, &self.layout)?
            }
            Intent::Business(business) => {
                self.layout = *layout;
                business
            }
        };
        self.apply_business(business)
    }

    /// Route one input event to the record that owns it: motion to the part's tallies,
    /// everything the panel reports to the touch record.
    pub(crate) fn apply_input(&mut self, op: InputIntent) {
        match op.event {
            InputEvent::Button(_) => {}
            InputEvent::Motion(batch) => {
                if self.state.backed.motion.is_some() {
                    self.state.motion.apply(batch);
                }
            }
            InputEvent::Touch(event) => self.state.touch.on_frame(event),
            InputEvent::Gesture(gesture) => self.state.touch.on_gesture(gesture),
            InputEvent::ChipGesture(id) => self.state.touch.view.chip_gesture_id = id,
        }
    }

    /// One business intent, one owner: the owner settles its record and says what the device
    /// still owes. The page turn and the demand stay here — they read the stack the owner just
    /// moved, which is the manager's to hold.
    pub(crate) fn apply_business(&mut self, intent: BusinessIntent) -> Option<Command> {
        let leaving = self.state.page();
        let (command, effects) = self.services.apply(&intent, &mut self.state, &self.layout);
        self.apply_effects(effects);
        let landed = self.state.page();
        if landed != leaving {
            self.services.leave(leaving);
            self.services.enter(landed);
        }
        self.settle_demand();
        command
    }

    /// A scan round landed: the record keeps the list, and the page clamps its draft to it as
    /// any other link report.
    pub fn apply_scan(&mut self, list: ApList) {
        if !self.state.backed.network {
            return;
        }
        self.state.network.set_scan(list);
        let _ = self.apply_business(BusinessIntent::Link(LinkIntent::Scan));
    }

    pub fn apply_capture(&mut self, sample: AudioSample) {
        self.state.audio.observe(sample, self.state.backed.audio);
    }

    pub fn tick_notice(&mut self, now_ms: u32) {
        self.state.notice = self.state.notice.and_then(|notice| notice.tick(now_ms));
    }

    /// What a domain asked the rest of the device for, applied after its own state settled.
    fn apply_effects(&mut self, effects: Effects) {
        if let Some(sound) = effects.sound {
            self.state.playback.start(sound, self.state.backed.playback);
        }
        if let Some(kind) = effects.notice {
            self.state.announce(kind);
        }
    }
}

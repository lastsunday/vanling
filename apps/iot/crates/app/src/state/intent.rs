//! The write side: an intent off the bus, a report from a worker, the clock's tick.
//!
//! Systems write, renderers read.

use crate::shared::intent::{BusinessIntent, InputIntent, Intent};
use crate::shared::model::{Command, Effects, Health, Notice, NoticeKind, PlaybackPhase};
use crate::state::DeviceManager;
use iot_core::drivers::audio::AudioSample;
use iot_core::drivers::input::InputEvent;
use iot_core::drivers::network::ApList;
use iot_core::drivers::playback::Sound;

use super::translate::translate;

impl DeviceManager {
    /// Route one intent all the way to the state it moves, and answer with what the model
    /// cannot do itself.
    ///
    /// The two input halves touch different fields: the write lands in `touch`/`motion`, which
    /// only renderers and the frame snapshot read, while the translation reads the page, the lights and
    /// the link. Their order is a reading order, not a dependency.
    pub fn apply_intent(&mut self, intent: Intent) -> Option<Command> {
        let business = match intent {
            Intent::Input(op) => {
                self.apply_input(op);
                translate(&op, self.state_ref(), &self.services)?
            }
            Intent::Business(business) => business,
        };
        self.apply_business(business)
    }

    /// Route one input event to the record that owns it: motion to the part's tallies,
    /// everything the panel reports to the touch record.
    pub fn apply_input(&mut self, op: InputIntent) {
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
    pub fn apply_business(&mut self, intent: BusinessIntent) -> Option<Command> {
        let leaving = self.state.page();
        let (command, effects) = self.services.apply(&intent, &mut self.state);
        self.apply_effects(effects);
        let landed = self.state.page();
        if landed != leaving {
            self.services.leave(leaving);
            self.services.enter(landed);
        }
        self.settle_demand();
        command
    }

    /// A scan round landed: the link keeps the list everyone reads, and the page's scroll
    /// clamps to it.
    pub fn apply_scan(&mut self, list: ApList) {
        if !self.state.backed.network {
            return;
        }
        self.state.network.set_scan(list);
        self.services.on_scan(&mut self.state);
    }

    pub fn apply_capture(&mut self, sample: AudioSample) {
        if self.state.backed.audio {
            self.state.audio.health = match sample.fault {
                Some(reason) => Health::Down(reason),
                None => Health::Ready,
            };
            self.state.audio.view.live = sample;
        }
    }

    pub fn tick_notice(&mut self, now_ms: u32) {
        let Some(mut notice) = self.state.notice else {
            return;
        };
        let started = *notice.started_ms.get_or_insert(now_ms);
        match notice.kind.light_at(now_ms.wrapping_sub(started)) {
            Some(light) => {
                notice.light = light;
                self.state.notice = Some(notice);
            }
            None => self.state.notice = None,
        }
    }

    /// What a domain asked the rest of the device for, applied after its own state settled.
    fn apply_effects(&mut self, effects: Effects) {
        if let Some(sound) = effects.sound {
            self.ring(sound);
        }
        if let Some(kind) = effects.notice {
            self.announce(kind);
        }
    }

    fn ring(&mut self, sound: Sound) {
        if !self.state.backed.playback || self.state.playback.phase != PlaybackPhase::Idle {
            return;
        }
        self.state.playback.sound = sound;
        self.state.playback.phase = PlaybackPhase::Playing;
    }

    fn announce(&mut self, kind: NoticeKind) {
        if !self.state.has_lamp() || !self.state.backed.network {
            return;
        }
        self.state.notice = Some(Notice::new(kind));
    }
}

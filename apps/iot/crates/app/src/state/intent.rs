//! The write side: an intent off the bus, a report from a worker, the clock's tick.
//!
//! Systems write, renderers read.

use crate::shared::intent::{BusinessIntent, InputIntent, Intent, LinkIntent, NavigationIntent};
use crate::shared::model::{
    Command, Effects, Health, MAX_LIGHTS, NetworkCommand, Notice, NoticeKind, PlaybackPhase,
};
use crate::shared::route::Page;
use crate::state::DeviceManager;
use iot_core::drivers::audio::AudioSample;
use iot_core::drivers::input::InputEvent;
use iot_core::drivers::network::ApList;
use iot_core::drivers::playback::Sound;

use super::translate::translate;

impl DeviceManager {
    /// Route one intent all the way to the state it moves, and answer with what the model
    /// cannot do itself. The input half is written before it is translated, so the page's
    /// answer is read against the state that gesture produced.
    pub fn apply_intent(&mut self, intent: Intent) -> Option<Command> {
        let business = match intent {
            Intent::Input(op) => {
                self.apply_input(op);
                translate(&op, self.state_ref())
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

    /// Route one business intent to the domain that owns it, and answer with what that domain
    /// asked the runtime for — `None` when nothing outside the core is needed. Each domain
    /// matches its own group, so this match knows the groups and never the variants inside
    /// them.
    pub fn apply_business(&mut self, intent: BusinessIntent) -> Option<Command> {
        let command = match intent {
            BusinessIntent::Invalid => None,
            BusinessIntent::Navigation(intent) => self.apply_navigation(intent),
            BusinessIntent::Link(intent) => {
                let effects = self.state.network.apply(intent);
                if let LinkIntent::Status(status) = intent {
                    self.state.network_page.on_link(status);
                }
                self.apply_effects(effects);
                None
            }
            BusinessIntent::NetworkPage(intent) => {
                let has_pages = self.state.has_pages();
                self.state
                    .network_page
                    .apply(intent, &self.state.network, has_pages)
            }
            BusinessIntent::Audio(intent) => {
                self.state.audio.apply(intent, self.state.backed.audio);
                None
            }
            BusinessIntent::Playback(action) => {
                self.state
                    .playback
                    .apply(action, self.state.backed.playback);
                None
            }
            BusinessIntent::Light { instance, action } => {
                let instance = usize::from(instance).min(MAX_LIGHTS - 1);
                if let Some(next) = self.state.lights[instance].apply(action) {
                    self.state.lights[instance] = next;
                }
                None
            }
        };
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
        let rows = self.state.network.len();
        self.state.network_page.clamp_top(rows);
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

    /// The device's own transitions, which read the wiring, the link and the boot page
    /// together. A landing answers with the boot decision the runtime cannot observe, on the
    /// same return path as every other command.
    fn apply_navigation(&mut self, intent: NavigationIntent) -> Option<Command> {
        match intent {
            NavigationIntent::NextPage => {
                self.state.nav.advance(self.state.backed);
                None
            }
            NavigationIntent::RequestSetup => {
                if self.state.page() == Page::Splash {
                    self.state.setup_requested = true;
                }
                None
            }
            NavigationIntent::SplashOver => self.land_after_splash(),
        }
    }

    fn land_after_splash(&mut self) -> Option<Command> {
        if self.state.page() != Page::Splash {
            return None;
        }
        let setup = self.state.setup_requested;
        self.state.nav.pop();
        if setup && self.state.backed.network {
            self.state.nav.go(Page::Network, self.state.backed);
        }
        // A device that boots with nothing to join has nothing to say after the splash
        // except that: one amber burst, then the page's own light. A hold that asked for
        // setup is not this — the access point is the answer to it, and the mode has its own
        // colour.
        if !setup
            && self.state.backed.network
            && !self.state.network.link.saved
            && !self.state.network.link.mode.is_setup()
            && self.state.network.note_offline_once()
        {
            self.announce(NoticeKind::Offline);
        }
        Some(Command::Network(NetworkCommand::SplashOver { setup }))
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

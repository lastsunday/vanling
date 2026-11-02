//! The single owner of device state: it constructs the state, routes each action intent to
//! the domain that owns it, and hands the app the state and the requests it has taken.

use alloc::vec::Vec;

use crate::demand::{DemandKey, EffectiveDemand, Override};
use crate::drivers::input::InputEvent;

use crate::intent::{ActionIntent, InputIntent, PageIntent};
use crate::page::DisplayPage;

use super::device::Effects;
use super::{Boot, DeviceState, LightState, MAX_LIGHTS, NetworkCommand, NoticeKind, PlaybackState};

/// Single owner of device state.
#[cfg_attr(test, derive(Clone))]
pub struct DeviceManager {
    pub(super) state: DeviceState,
    boot_page: DisplayPage,
    pub(super) demand_keys: Vec<DemandKey>,
    pub(super) override_: Override,
    demand: EffectiveDemand,
    /// The page `demand` was derived from: the proposal reads the page and nothing else, so a
    /// state write that cannot have moved it does not need to rebuild the table.
    demand_page: DisplayPage,
    /// Whether the button asked for setup mode while the splash was up. A boot decision the
    /// splash landing reads, not something the link knows.
    setup_wanted: bool,
    /// What the splash landing has to tell the network task, once.
    splash_end: Option<bool>,
}

impl DeviceManager {
    /// A device with nothing wired: the state a bare page test starts from.
    pub fn new() -> Self {
        Self::boot(Boot::default())
    }

    /// The one constructor a board uses. Everything it lands on is in `boot`, so the boot
    /// page is clamped and the demand derived once, with no call order to get wrong.
    pub fn boot(boot: Boot) -> Self {
        let backed = boot.backed;
        let state = DeviceState {
            backed,
            page: DisplayPage::Splash,
            surface_roles: boot.surface_roles,
            ..DeviceState::default()
        };
        Self {
            demand: Self::proposed(&state, &boot.demand_keys, &boot.overrides),
            demand_page: state.page,
            boot_page: DeviceState::landed_on(boot.boot_page, backed),
            state,
            demand_keys: boot.demand_keys,
            override_: boot.overrides,
            setup_wanted: false,
            splash_end: None,
        }
    }

    pub(super) fn proposed(
        state: &DeviceState,
        keys: &[DemandKey],
        override_: &Override,
    ) -> EffectiveDemand {
        let mut demand = state.demand(keys);
        override_.clone().apply(&mut demand);
        demand
    }

    fn propose_demand(&mut self) {
        if self.demand_page == self.state.page {
            return;
        }
        self.demand = Self::proposed(&self.state, &self.demand_keys, &self.override_);
        self.demand_page = self.state.page;
    }

    /// The page this device lands on once the splash is over.
    pub const fn boot_page(&self) -> DisplayPage {
        self.boot_page
    }

    /// Apply an absolute target to the `instance`-th light surface, clamped to
    /// the installed array so one out-of-range intent can never panic.
    pub fn apply_to(&mut self, instance: usize, state: LightState) {
        self.state.lights[instance.min(MAX_LIGHTS - 1)] = state;
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

    /// Route one action intent to the domain that owns it. Each domain matches its own
    /// group, so the manager knows the groups and never a domain's actions.
    /// Route one action intent to the domain that owns it, and answer with the command the
    /// network task has to carry out. Every other group asks for nothing outside the core, so
    /// they answer `None`; the caller that owns the worker is the only one that can deliver
    /// it, and it gets exactly one pass at it.
    pub fn apply_action(&mut self, intent: ActionIntent) -> Option<NetworkCommand> {
        let command = match intent {
            ActionIntent::Invalid => None,
            ActionIntent::Page(intent) => {
                self.apply_page(intent);
                None
            }
            ActionIntent::Network(intent) => {
                let has_pages = self.state.has_pages();
                let mut effects = self.state.network.apply(intent, has_pages);
                let command = effects.command.take();
                self.apply_effects(effects);
                command
            }
            ActionIntent::Audio(intent) => {
                self.state.audio.apply(intent, self.state.backed.audio);
                None
            }
            ActionIntent::Playback(action) => {
                self.state
                    .playback
                    .apply(action, self.state.backed.playback);
                None
            }
            ActionIntent::Light(intent) => {
                self.apply_to(usize::from(intent.instance), intent.state);
                None
            }
        };
        self.propose_demand();
        command
    }

    /// What a domain asked the rest of the device for, applied after its own state settled.
    /// The one ask that leaves the core is taken out before this runs.
    fn apply_effects(&mut self, effects: Effects) {
        if let Some(sound) = effects.sound {
            self.ring(sound);
        }
        if let Some(kind) = effects.notice {
            self.announce(kind);
        }
    }

    /// The page's transitions, which read the wiring, the link and the boot page together.
    fn apply_page(&mut self, intent: PageIntent) {
        match intent {
            PageIntent::NextPage => {
                self.state.page = self.state.page.next_page(self.state.backed);
            }
            PageIntent::RequestSetup => {
                if self.state.page == DisplayPage::Splash {
                    self.setup_wanted = true;
                }
            }
            PageIntent::SplashOver => self.land_after_splash(),
        }
    }

    fn land_after_splash(&mut self) {
        if self.state.page != DisplayPage::Splash {
            return;
        }
        self.state.page = if self.setup_wanted && self.state.backed.network {
            DisplayPage::Network
        } else {
            self.boot_page
        };
        self.splash_end = Some(self.setup_wanted);
        // A device that boots with nothing to join has nothing to say after the splash
        // except that: one amber burst, then the page's own light. A hold that asked for
        // setup is not this — the access point is the answer to it, and the mode has its own
        // colour.
        if !self.setup_wanted
            && self.state.backed.network
            && !self.state.network.link.saved
            && !self.state.network.link.mode.is_setup()
            && !self.state.network.outage_announced
        {
            self.state.network.outage_announced = true;
            self.announce(NoticeKind::Offline);
        }
    }

    /// What the splash landing owes the network task, handed over once.
    pub fn take_splash_end(&mut self) -> Option<bool> {
        self.splash_end.take()
    }

    /// Whether the button asked for setup mode while the splash was up.
    #[cfg(test)]
    pub fn setup_wanted(&self) -> bool {
        self.setup_wanted
    }

    pub fn state(&self) -> DeviceState {
        self.state.clone()
    }

    /// The state itself, for a caller that only reads it: [`state`](Self::state) copies the
    /// whole record, which the coordinator did several times per pass just to compare a page
    /// or a host.
    pub fn state_ref(&self) -> &DeviceState {
        &self.state
    }

    /// What the device is committed to, which is the proposal narrowed to what the parts
    /// can actually meet.
    pub fn demand(&self) -> &EffectiveDemand {
        &self.demand
    }

    /// Commits a demand the configurator could actually meet.
    #[cfg(test)]
    pub fn commit_demand(&mut self, demand: EffectiveDemand) {
        self.demand = demand;
    }

    #[cfg(test)]
    pub fn light_state(&self) -> LightState {
        self.state.lights[0]
    }

    /// Clamped like [`Self::apply_to`]: an out-of-range read is the last surface,
    /// never a panic.
    #[cfg(test)]
    pub fn light_state_at(&self, instance: u8) -> LightState {
        self.state.lights[usize::from(instance).min(MAX_LIGHTS - 1)]
    }

    #[cfg(test)]
    pub fn touch_state(&self) -> Option<crate::drivers::input::TouchEvent> {
        self.state.touch.last
    }

    #[cfg(test)]
    pub fn motion_state(&self) -> Option<crate::drivers::motion::MotionSample> {
        self.state.motion.sample
    }

    #[cfg(test)]
    pub fn motion_counts_state(&self) -> crate::drivers::motion::MotionCounts {
        self.state.motion.counts
    }

    /// Playback state, read by the render loop to draw the Speaker page and to decide
    /// whether a play it just applied has to reach the driver.
    pub fn playback_state(&self) -> PlaybackState {
        self.state.playback
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

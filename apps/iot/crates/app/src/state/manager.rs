//! The read side: the owned state and the demand derived from it. Nothing here moves a
//! record; writes live in [`super::intent`].

use alloc::vec::Vec;

use crate::shared::model::{Backed, DeviceState, MAX_LIGHTS, PlaybackState, SurfaceRole};
use crate::shared::route::{Navigator, Page};
use iot_core::demand::{DemandKey, EffectiveDemand, Override};

/// Everything one boot decides, before the manager exists.
///
/// One record rather than a chain of setters: every field is known when it is written, so the
/// boot page is clamped and the demand derived exactly once, and no call order can change the
/// answer. `..Boot::default()` covers whatever a test does not care about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Boot {
    /// Which parts the board wired, and what the motion part declares.
    pub backed: Backed,
    /// Kept apart from the wiring on purpose: which keys a board claims is a property of the
    /// part it wired, not only of whether it wired one.
    pub demand_keys: Vec<DemandKey>,
    pub surface_roles: [SurfaceRole; MAX_LIGHTS],
    /// The page to land on after the splash; a page the board cannot back falls back to
    /// [`Page::Ambient`].
    pub boot_page: Page,
    /// What the build pins beyond the page's claim.
    pub overrides: Override,
}

/// Single owner of device state.
#[cfg_attr(test, derive(Clone))]
pub struct DeviceManager {
    pub(super) state: DeviceState,
    pub(super) demand: DemandPolicy,
}

impl DeviceManager {
    /// A device with nothing wired: the state a bare page test starts from.
    pub fn new() -> Self {
        Self::boot(Boot::default())
    }

    /// The one constructor a board uses. Everything it lands on is in `boot`, so the boot
    /// page is clamped once and pushed under the splash, with no call order to get wrong.
    pub fn boot(boot: Boot) -> Self {
        let backed = boot.backed;
        let mut nav = Navigator::new(DeviceState::landed_on(boot.boot_page, backed));
        nav.push(Page::Splash, backed);
        let state = DeviceState {
            backed,
            nav,
            surface_roles: boot.surface_roles,
            ..DeviceState::default()
        };
        Self {
            demand: DemandPolicy::boot(&state, boot.demand_keys, boot.overrides),
            state,
        }
    }

    /// The page this device lands on once the splash is over: the root of the stack.
    pub fn boot_page(&self) -> Page {
        self.state.nav.root()
    }

    /// Borrowed, not copied: the coordinator reads it several times per pass.
    pub fn state_ref(&self) -> &DeviceState {
        &self.state
    }

    /// What the device is committed to: the page's proposal with the build override applied.
    pub fn demand(&self) -> &EffectiveDemand {
        self.demand.current()
    }

    pub(super) fn settle_demand(&mut self) {
        self.demand.settle(&self.state);
    }

    /// Playback state, read by the render loop to draw the Speaker page and to decide
    /// whether a play it just applied has to reach the driver.
    pub fn playback_state(&self) -> PlaybackState {
        self.state.playback
    }

    pub fn notice_active(&self) -> bool {
        self.state.notice.is_some()
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The page's claim with the build override applied, kept while the page it was derived from
/// is still the current one. The page is the whole key: `DeviceState::demand` reads nothing
/// else, so a write that cannot have moved it leaves the held table alone.
#[cfg_attr(test, derive(Clone))]
pub(super) struct DemandPolicy {
    keys: Vec<DemandKey>,
    overrides: Override,
    current: EffectiveDemand,
    page: Page,
}

impl DemandPolicy {
    fn boot(state: &DeviceState, keys: Vec<DemandKey>, overrides: Override) -> Self {
        Self {
            current: Self::propose(state, &keys, &overrides),
            page: state.page(),
            keys,
            overrides,
        }
    }

    /// A derivation from the held keys, for tests that pin the cache to it.
    #[cfg(test)]
    pub(super) fn fresh(&self, state: &DeviceState) -> EffectiveDemand {
        Self::propose(state, &self.keys, &self.overrides)
    }

    fn propose(state: &DeviceState, keys: &[DemandKey], overrides: &Override) -> EffectiveDemand {
        let mut demand = state.demand(keys);
        overrides.clone().apply(&mut demand);
        demand
    }

    pub(super) fn settle(&mut self, state: &DeviceState) {
        if self.page == state.page() {
            return;
        }
        self.current = Self::propose(state, &self.keys, &self.overrides);
        self.page = state.page();
    }

    pub(super) fn current(&self) -> &EffectiveDemand {
        &self.current
    }
}

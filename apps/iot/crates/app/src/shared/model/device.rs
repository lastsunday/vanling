//! The whole of what the device knows: one record per domain, plus the queries that read
//! across them.

use crate::shared::i18n::Lang;
use crate::shared::intent::{BusinessIntent, InputIntent, LightAction};
use crate::shared::route::{Navigator, PageId};
use iot_core::demand::{DemandKey, EffectiveDemand};
use iot_core::drivers::input::SwipeDirection;
use iot_core::drivers::network::{LinkPhase, SetupHost};
use iot_core::ui::ThemeId;

use super::{
    AudioState, Backed, Backlight, GlowScope, LightState, MAX_LIGHTS, MotionState, NetworkState,
    Notice, NoticeKind, PlaybackState, SurfaceRole, TouchState,
};

/// All state owned by the device manager: one record per domain, each living in the module
/// that owns its behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    /// What the board wired. Every page asks this before it asks anything else.
    pub backed: Backed,
    /// Where the device is: the stack of destinations, whether or not a panel draws them.
    /// Read through [`Self::page`].
    pub nav: Navigator,
    /// What the button asked for while the splash was up: the landing reads it, and the
    /// link hears it once as `SplashOver`.
    pub setup_requested: bool,
    pub lights: [LightState; MAX_LIGHTS],
    /// The panel's backlight, independent of what the glass is showing.
    pub backlight: Backlight,
    /// Where the breathing glass may appear; the lighting page edits it.
    pub glow: GlowScope,
    /// What each slot's surface is for, fixed by the wiring rather than by the current
    /// appearance. Not part of `backed`: presence and role answer different questions.
    pub surface_roles: [SurfaceRole; MAX_LIGHTS],
    /// The one-shot news a lamp says instead of a page: a join that worked, a link that is
    /// not there, or a refusal that just landed.
    pub notice: Option<Notice>,
    pub motion: MotionState,
    pub audio: AudioState,
    pub playback: PlaybackState,
    /// What the radio reported. Written only by the link's own reports.
    pub network: NetworkState,
    pub touch: TouchState,
    /// Input intents the coordinator's ring had no room for, since boot. Folded in once per
    /// frame from the handle the input worker notes into.
    pub dropped_intents: u32,
    /// The words the panel prints; the build sets it, a settings page may change it.
    pub lang: Lang,
    /// The palette the panel draws with; the build sets it, a settings page may change it.
    pub theme: ThemeId,
}

impl DeviceState {
    /// The page on screen: the top of the navigation stack.
    pub fn page(&self) -> PageId {
        self.nav.current()
    }

    /// The light an index names: clamped to the installed array, so an out-of-range intent or
    /// snapshot lands on the last surface instead of panicking.
    pub fn light_index(instance: u8) -> u8 {
        instance.min((MAX_LIGHTS - 1) as u8)
    }

    /// What one input asks of the lamp, or `None` when that surface has nothing to move.
    pub fn light_intent(&self, op: &InputIntent, action: LightAction) -> Option<BusinessIntent> {
        let instance = Self::light_index(op.surface());
        self.lights[usize::from(instance)]
            .can(action)
            .then_some(BusinessIntent::Light { instance, action })
    }

    /// The lamp's answer to a swipe: a diagonal has no axis, so it asks for nothing.
    pub fn light_swipe(
        &self,
        op: &InputIntent,
        direction: SwipeDirection,
    ) -> Option<BusinessIntent> {
        LightAction::from_swipe(direction).and_then(|action| self.light_intent(op, action))
    }

    /// Whether any wired surface draws a frame.
    pub fn has_screen(&self) -> bool {
        self.surface_roles.contains(&SurfaceRole::Screen)
    }

    /// Whether any wired surface is a lamp the link can speak through.
    pub fn has_lamp(&self) -> bool {
        self.surface_roles.contains(&SurfaceRole::Lamp)
    }

    /// What the link has to say on a lamp: the one-shot news, the setup mode, or the attempt.
    pub fn link_notice(&self) -> Option<LightState> {
        if !self.has_lamp() || !self.backed.network {
            return None;
        }
        if let Some(notice) = self.notice {
            return Some(notice.light);
        }
        if self.network.link.mode.host() == Some(SetupHost::AccessPoint) {
            return Some(LightState::SETUP);
        }
        if self.network.link.phase == LinkPhase::Connecting && !self.network.link.saved {
            return Some(LightState::JOINING);
        }
        None
    }

    /// The page's demand with the board's wiring clamped on top.
    pub fn demand(&self, keys: &[DemandKey]) -> EffectiveDemand {
        self.page().demand(keys)
    }

    /// One-shot news on a lamp that can speak: a joined link, a missing one, a refusal.
    pub fn announce(&mut self, kind: NoticeKind) {
        if self.has_lamp() && self.backed.network {
            self.notice = Some(Notice::new(kind));
        }
    }

    /// The page that becomes the boot page. A board with no panel lands on [`PageId::Headless`]
    /// and a board with one never does; anything the build may not come up on, or the board
    /// cannot back, falls back to [`PageId::Input`].
    pub(crate) fn landed_on(page: PageId, backed: Backed, screen: bool) -> PageId {
        if page == PageId::Headless {
            return if screen {
                PageId::Input
            } else {
                PageId::Headless
            };
        }
        if !screen && matches!(page, PageId::Menu | PageId::Input) {
            return PageId::Headless;
        }
        if page.bootable() && page.is_backed(backed) {
            page
        } else if screen {
            PageId::Input
        } else {
            PageId::Headless
        }
    }
}

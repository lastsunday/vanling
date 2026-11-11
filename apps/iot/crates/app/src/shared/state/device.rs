//! The whole of what the device knows: one record per domain, plus the queries that read
//! across them.

use crate::shared::route::{Navigator, Page};
use iot_core::demand::{DemandKey, EffectiveDemand};
use iot_core::drivers::network::{LinkPhase, SetupHost};

use super::draft::NetworkDraft;
use super::{
    AudioState, Backed, LightState, MAX_LIGHTS, MotionState, NetworkState, Notice, PlaybackState,
    SurfaceRole, TouchState,
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
    pub lights: [LightState; MAX_LIGHTS],
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
    /// What the Network page is drafting. Reads the link; the link never reads or writes it.
    pub network_page: NetworkDraft,
    pub touch: TouchState,
}

impl DeviceState {
    /// The page on screen: the top of the navigation stack.
    pub fn page(&self) -> Page {
        self.nav.current()
    }

    /// Whether any wired surface draws the pages and puts the questions on them.
    pub fn has_pages(&self) -> bool {
        self.surface_roles.contains(&SurfaceRole::Pages)
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

    /// The page that becomes the boot page: a splash has no page to land on, and a page the
    /// board cannot back is not somewhere it could ever arrive.
    pub(crate) fn landed_on(page: Page, backed: Backed) -> Page {
        match page {
            Page::Splash => Page::Ambient,
            page if page.is_backed(backed) => page,
            _ => Page::Ambient,
        }
    }
}

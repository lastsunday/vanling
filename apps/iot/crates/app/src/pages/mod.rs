//! The product's pages, one vertical slice each: the page's model, controller, layout and
//! view live together, and the directory is the unit of cohesion.
//!
//! [`Page`] is what every client shares: the device asks it to apply an intent and to publish
//! its view. [`LocalPage`] is what only the screen showing the page answers: what a gesture
//! means, and when the page becomes or stops being current. Every answer is an arm here, so
//! adding a page names it in this one file and the compiler lists the rest.

pub mod ambient;
pub mod attitude;
pub mod audio;
pub mod camera;
pub mod env;
pub mod network;
pub mod speaker;
pub mod splash;

use crate::shared::diagnostics::Diagnostics;
use crate::shared::intent::{BusinessIntent, InputIntent, LightAction};
use crate::shared::kit::surface::Surface;
use crate::shared::model::{Command, DeviceState, PageSnapshot};
use crate::shared::route::PageId;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::paint::Rgb;

use self::ambient::AmbientPage;
use self::attitude::AttitudePage;
use self::audio::AudioPage;
use self::camera::CameraPage;
use self::env::EnvPage;
use self::network::NetworkPage;
use self::speaker::SpeakerPage;
use self::splash::SplashPage;

/// What every client of a page shares: the intent it applies, and the view it publishes. Both
/// default to nothing, so a page answers only what it has.
pub trait Page {
    fn apply(&mut self, _intent: &BusinessIntent, _state: &DeviceState) -> Option<Command> {
        None
    }

    fn publish(&self, _out: &mut PageSnapshot) {}
}

/// What only the screen showing the page answers. The hooks run on the device's navigation, in
/// both directions, and read or write nothing but the page's own state.
pub trait LocalPage: Page {
    /// What one gesture means on this page.
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent;

    /// The page is now the current one. Also runs at boot, for the page the splash lands on.
    fn on_show(&mut self) {}

    /// The page is no longer the current one.
    fn on_hide(&mut self) {}
}

/// The pages this build has, one instance each. Closed on purpose: the compiler checks every
/// question against the same eight fields, and no page is reached through a page-specific name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageSet {
    ambient: AmbientPage,
    attitude: AttitudePage,
    audio: AudioPage,
    speaker: SpeakerPage,
    camera: CameraPage,
    network: NetworkPage,
    splash: SplashPage,
    env: EnvPage,
}

impl PageSet {
    /// The page a route names, for the local gesture table.
    pub fn get(&self, page: PageId) -> &dyn LocalPage {
        match page {
            PageId::Ambient => &self.ambient,
            PageId::Attitude => &self.attitude,
            PageId::Audio => &self.audio,
            PageId::Speaker => &self.speaker,
            PageId::Camera => &self.camera,
            PageId::Network => &self.network,
            PageId::Splash => &self.splash,
            PageId::Env => &self.env,
        }
    }

    /// Lets the pages claim the intent one of them owns; the first claim wins, and a page that
    /// does not own the group answers `None`.
    pub fn apply(&mut self, intent: &BusinessIntent, state: &DeviceState) -> Option<Command> {
        for page in PageId::ALL {
            if let Some(command) = self.get_mut(page).apply(intent, state) {
                return Some(command);
            }
        }
        None
    }

    /// The pages' published view, for the one snapshot every renderer draws from.
    pub fn snapshot(&self) -> PageSnapshot {
        let mut snapshot = PageSnapshot::default();
        for page in PageId::ALL {
            self.get(page).publish(&mut snapshot);
        }
        snapshot
    }

    pub fn enter(&mut self, page: PageId) {
        self.get_mut(page).on_show();
    }

    pub fn leave(&mut self, page: PageId) {
        self.get_mut(page).on_hide();
    }

    fn get_mut(&mut self, page: PageId) -> &mut dyn LocalPage {
        match page {
            PageId::Ambient => &mut self.ambient,
            PageId::Attitude => &mut self.attitude,
            PageId::Audio => &mut self.audio,
            PageId::Speaker => &mut self.speaker,
            PageId::Camera => &mut self.camera,
            PageId::Network => &mut self.network,
            PageId::Splash => &mut self.splash,
            PageId::Env => &mut self.env,
        }
    }
}

/// The least wall time between diagnostics-driven repaints, before a page raises it: a repaint
/// ships the whole frame down one blocking transfer (~15 ms), and the live pages would
/// otherwise repaint every snapshot.
pub const DIAG_REPAINT_STEP_MS: u64 = 40;

/// Draws the page, and the debug badge every page but the camera carries.
pub fn stamp(page: PageId, surface: &mut Surface<'_>) {
    match page {
        PageId::Splash => splash::stamp(surface),
        PageId::Attitude => attitude::stamp(surface),
        PageId::Audio => audio::stamp(surface),
        PageId::Speaker => speaker::stamp(surface),
        PageId::Camera => camera::stamp(surface),
        PageId::Network => network::stamp(surface),
        PageId::Env => env::stamp(surface),
        PageId::Ambient => ambient::stamp(surface),
    }
    if surface.debug && page != PageId::Camera {
        surface.stamp_fps();
    }
}

/// The fill the renderer covers the panel with first, or `None` when the page draws over what
/// the light was already showing.
pub fn ground(page: PageId) -> Option<Rgb> {
    match page {
        PageId::Splash => splash::GROUND,
        PageId::Attitude => attitude::GROUND,
        PageId::Audio => audio::GROUND,
        PageId::Speaker => speaker::GROUND,
        PageId::Camera => camera::GROUND,
        PageId::Network => network::GROUND,
        PageId::Env => env::GROUND,
        PageId::Ambient => ambient::GROUND,
    }
}

/// Whether this page's frames arrive from the camera instead of being drawn.
pub fn draws_frames(page: PageId) -> bool {
    match page {
        PageId::Splash => splash::DRAWS_FRAMES,
        PageId::Attitude => attitude::DRAWS_FRAMES,
        PageId::Audio => audio::DRAWS_FRAMES,
        PageId::Speaker => speaker::DRAWS_FRAMES,
        PageId::Camera => camera::DRAWS_FRAMES,
        PageId::Network => network::DRAWS_FRAMES,
        PageId::Env => env::DRAWS_FRAMES,
        PageId::Ambient => ambient::DRAWS_FRAMES,
    }
}

/// The least wall time this page waits between diagnostics-driven repaints.
pub fn repaint_step_ms(page: PageId) -> u64 {
    match page {
        PageId::Splash => splash::REPAINT_MS,
        PageId::Attitude => attitude::REPAINT_MS,
        PageId::Audio => audio::REPAINT_MS,
        PageId::Speaker => speaker::REPAINT_MS,
        PageId::Camera => camera::REPAINT_MS,
        PageId::Network => network::REPAINT_MS,
        PageId::Env => env::REPAINT_MS,
        PageId::Ambient => ambient::REPAINT_MS,
    }
}

/// Whether this pair of diagnostics snapshots earns a repaint.
pub fn repaints_on(page: PageId, prev: &Diagnostics, next: &Diagnostics, overlay: bool) -> bool {
    match page {
        PageId::Splash => splash::repaints_on(prev, next, overlay),
        PageId::Attitude => attitude::repaints_on(prev, next, overlay),
        PageId::Audio => audio::repaints_on(prev, next, overlay),
        PageId::Speaker => speaker::repaints_on(prev, next, overlay),
        PageId::Camera => camera::repaints_on(prev, next, overlay),
        PageId::Network => network::repaints_on(prev, next, overlay),
        PageId::Env => env::repaints_on(prev, next, overlay),
        PageId::Ambient => ambient::repaints_on(prev, next, overlay),
    }
}

/// The lamp's table, for the pages whose only verb is the light.
fn lamp(op: &InputIntent, state: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            state.light_intent(op, LightAction::NextColor { forward: true })
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => state.light_intent(op, LightAction::Step),
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            state.light_intent(op, LightAction::Cycle)
        }
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            state.light_swipe(op, direction)
        }
        _ => BusinessIntent::Invalid,
    }
}

#[cfg(test)]
mod tests;

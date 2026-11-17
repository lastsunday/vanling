//! The product's pages, one vertical slice each: a page's model, controller, layout and view
//! live together, and the directory is the unit of cohesion.
//!
//! [`Page`] is what every client shares; [`LocalPage`] is what the client showing the page
//! answers. A new page names itself here and the compiler lists the rest.

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

/// What every client of a page shares: the intent it applies, the view it publishes. Both
/// default to nothing, so a page implements only what it owns.
pub trait Page {
    fn apply(&mut self, _intent: &BusinessIntent, _state: &DeviceState) -> Option<Command> {
        None
    }

    fn publish(&self, _out: &mut PageSnapshot) {}
}

/// What a page answers for the client showing it: what one gesture means, and when it becomes
/// or stops being current. The hooks run on every board, display or not.
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
pub const REPAINT_FLOOR_MS: u64 = 40;

/// When a diagnostics drift earns a repaint, as each page declares it.
#[derive(Debug, Clone, Copy)]
pub(super) enum Repaint {
    /// The page's changes arrive outside diagnostics.
    Never,
    /// Every step, snapshot or not.
    Always,
    /// Only while the debug overlay is up, which is the page's only drawing.
    Overlay,
    /// When the page's own snapshot fields moved.
    When(fn(&Diagnostics, &Diagnostics) -> bool),
}

impl Repaint {
    fn due(self, prev: &Diagnostics, next: &Diagnostics, overlay: bool) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Overlay => overlay,
            Self::When(moved) => moved(prev, next),
        }
    }
}

/// Paints the page, plus the debug badge every page but the camera carries: camera frames
/// arrive outside this dispatch, so that page paints its own badge.
pub fn paint(page: PageId, surface: &mut Surface<'_>) {
    match page {
        PageId::Splash => surface.paint_splash(),
        PageId::Attitude => surface.paint_attitude(),
        PageId::Audio => surface.paint_audio(),
        PageId::Speaker => surface.paint_speaker(),
        PageId::Camera => camera::paint(surface),
        PageId::Network => surface.paint_network(),
        PageId::Env => surface.paint_env(),
        PageId::Ambient => surface.paint_ambient(),
    }
    if surface.debug && page != PageId::Camera {
        surface.draw_fps();
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

/// Whether this page's picture is the camera's frames rather than its own drawing.
pub fn camera_frames(page: PageId) -> bool {
    match page {
        PageId::Splash => splash::CAMERA_FRAMES,
        PageId::Attitude => attitude::CAMERA_FRAMES,
        PageId::Audio => audio::CAMERA_FRAMES,
        PageId::Speaker => speaker::CAMERA_FRAMES,
        PageId::Camera => camera::CAMERA_FRAMES,
        PageId::Network => network::CAMERA_FRAMES,
        PageId::Env => env::CAMERA_FRAMES,
        PageId::Ambient => ambient::CAMERA_FRAMES,
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
    let repaint = match page {
        PageId::Splash => splash::REPAINTS,
        PageId::Attitude => attitude::REPAINTS,
        PageId::Audio => audio::REPAINTS,
        PageId::Speaker => speaker::REPAINTS,
        PageId::Camera => camera::REPAINTS,
        PageId::Network => network::REPAINTS,
        PageId::Env => env::REPAINTS,
        PageId::Ambient => ambient::REPAINTS,
    };
    repaint.due(prev, next, overlay)
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

//! The product's pages, one vertical slice each: a page and what it owns live in one file or
//! one directory, and that is the unit of cohesion.
//!
//! [`Service`] is the half every operating platform shares: a business intent comes in, the
//! page answers. [`LocalPage`] is the face the local panel and buttons see: what a gesture
//! means, the view the frame snapshot carries, the picture, and how it fills the panel. A new
//! page names itself here and the compiler lists the rest.

pub mod ambient;
pub mod attitude;
pub mod audio;
pub mod camera;
pub mod env;
pub mod network;
pub mod speaker;
pub mod splash;

use crate::shared::intent::{BusinessIntent, InputIntent, LightAction};
use crate::shared::kit::painter::Painter;
use crate::shared::model::{Command, DeviceState};
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::snapshot::PageSnapshot;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::light::Rgb;

use self::ambient::AmbientPage;
use self::attitude::AttitudePage;
use self::audio::AudioPage;
use self::camera::CameraPage;
use self::env::EnvPage;
use self::network::NetworkPage;
use self::speaker::SpeakerPage;
use self::splash::SplashPage;

/// The page's service: what a business intent means, shared by every operating platform.
pub trait Service {
    /// Claims a business intent and answers with what the model cannot do itself.
    fn apply(&mut self, _intent: &BusinessIntent, _state: &DeviceState) -> Option<Command> {
        None
    }
}

/// The page as the local panel and buttons see it.
pub trait LocalPage: Service {
    /// What one gesture means on this page.
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent;

    /// Hands the page's private UI state to the frame snapshot the local painter draws from.
    fn publish(&self, _out: &mut PageSnapshot) {}

    /// The page's picture, drawn from the frame snapshot. `where Self: Sized` keeps
    /// `&dyn LocalPage` dyn-compatible for the input table.
    fn paint(snapshot: &FrameSnapshot, painter: &mut Painter<'_>)
    where
        Self: Sized;

    /// How this page occupies the panel. `where Self: Sized` for the same reason as
    /// [`Self::paint`].
    fn descriptor() -> Descriptor
    where
        Self: Sized;

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

/// The least wall time between snapshot-driven repaints, before a page raises it: a repaint
/// ships the whole frame down one blocking transfer (~15 ms), and the live pages would
/// otherwise repaint every snapshot.
pub const REPAINT_FLOOR_MS: u64 = 40;

/// When a frame drift earns a repaint, as each page declares it.
#[derive(Debug, Clone, Copy)]
pub(super) enum Repaint {
    /// The page's changes arrive outside the frame snapshot.
    Never,
    /// Every step, snapshot or not.
    Always,
    /// Only while the debug overlay is up, which is the page's only drawing.
    Overlay,
    /// When the page's own snapshot fields moved.
    When(fn(&FrameSnapshot, &FrameSnapshot) -> bool),
}

impl Repaint {
    fn due(self, prev: &FrameSnapshot, next: &FrameSnapshot, overlay: bool) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Overlay => overlay,
            Self::When(moved) => moved(prev, next),
        }
    }
}

/// How one page occupies the panel: its draw call, the fill under it, where its frames come
/// from, and how often a snapshot drift earns a repaint.
#[derive(Debug, Clone, Copy)]
pub struct Descriptor {
    paint: fn(&FrameSnapshot, &mut Painter<'_>),
    ground: Option<Rgb>,
    camera_frames: bool,
    repaint_ms: u64,
    repaints: Repaint,
}

impl Descriptor {
    pub(super) fn new(paint: fn(&FrameSnapshot, &mut Painter<'_>)) -> Self {
        Self {
            paint,
            ground: None,
            camera_frames: false,
            repaint_ms: REPAINT_FLOOR_MS,
            repaints: Repaint::Never,
        }
    }

    pub(super) fn with_ground(mut self, ground: Option<Rgb>) -> Self {
        self.ground = ground;
        self
    }

    pub(super) fn with_camera_frames(mut self) -> Self {
        self.camera_frames = true;
        self
    }

    pub(super) fn with_repaint_ms(mut self, repaint_ms: u64) -> Self {
        self.repaint_ms = repaint_ms;
        self
    }

    pub(super) fn with_repaints(mut self, repaints: Repaint) -> Self {
        self.repaints = repaints;
        self
    }

    pub fn ground(self) -> Option<Rgb> {
        self.ground
    }

    pub fn camera_frames(self) -> bool {
        self.camera_frames
    }

    pub fn repaint_step_ms(self) -> u64 {
        self.repaint_ms
    }

    pub fn repaints_on(self, prev: &FrameSnapshot, next: &FrameSnapshot, overlay: bool) -> bool {
        self.repaints.due(prev, next, overlay)
    }
}

/// The page's descriptor. One exhaustive match: a variant with no descriptor does not compile.
pub fn descriptor(page: PageId) -> Descriptor {
    match page {
        PageId::Ambient => AmbientPage::descriptor(),
        PageId::Attitude => AttitudePage::descriptor(),
        PageId::Audio => AudioPage::descriptor(),
        PageId::Speaker => SpeakerPage::descriptor(),
        PageId::Camera => CameraPage::descriptor(),
        PageId::Network => NetworkPage::descriptor(),
        PageId::Splash => SplashPage::descriptor(),
        PageId::Env => EnvPage::descriptor(),
    }
}

/// Paints the page, plus the debug badge every page but the camera carries: camera frames
/// arrive outside this dispatch, so that page paints its own badge.
pub fn paint(page: PageId, snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
    (descriptor(page).paint)(snapshot, painter);
    if painter.debug && page != PageId::Camera {
        painter.draw_fps();
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

//! The product's services, one vertical slice each: a page or device service and what it owns.
//! `Service` is what every platform shares; `LocalPage` is what the panel and buttons see.

pub mod advanced;
pub mod attitude;
pub mod audio;
pub mod camera;
pub mod data;
pub mod env;
pub mod extended;
pub mod feedback;
pub mod gallery;
pub mod headless;
pub mod input;
pub mod light;
pub mod lighting;
pub mod list;
pub mod menu;
pub mod nav;
pub mod network;
pub mod settings;
pub mod speaker;
pub mod splash;
pub mod text;
pub mod widgets;

use crate::shared::intent::{BusinessIntent, InputIntent, MenuIntent, NavigationIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::snapshot::PageSnapshot;
use crate::shared::snapshot::ScrollShift;
use crate::shared::surface::{Custom, Panel, View, no_custom};
use iot_core::drivers::input::{ButtonEvent, InputEvent};
use iot_core::light::Rgb;
use iot_core::ui::Ui;

use self::advanced::AdvancedPage;
use self::attitude::AttitudePage;
use self::audio::AudioPage;
use self::camera::CameraPage;
use self::data::DataPage;
use self::env::EnvPage;
use self::extended::ExtendedPage;
use self::feedback::FeedbackPage;
use self::headless::HeadlessPage;
use self::input::InputPage;
use self::light::LightService;
use self::lighting::LightingPage;
use self::list::ListPage;
use self::menu::MenuPage;
use self::nav::NavService;
use self::network::NetworkPage;
use self::settings::SettingsService;
use self::speaker::SpeakerPage;
use self::splash::SplashPage;
use self::text::TextPage;
use self::widgets::WidgetsPage;

/// One intent's owner: its own record settles first, then the answer says what the rest of the
/// device owes.
pub trait Service {
    fn apply(
        &mut self,
        _intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        (None, Effects::default())
    }
}

/// The page as the local panel and buttons see it.
pub trait LocalPage: Service {
    /// What one gesture means on this page. `None` when it means nothing.
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent>;

    /// Hands the page's private UI state to the frame snapshot the local painter draws from.
    fn publish(&self, _out: &mut PageSnapshot) {}

    /// The page's picture, declared from the frame snapshot. `where Self: Sized` keeps
    /// `&dyn LocalPage` dyn-compatible for the input table.
    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>)
    where
        Self: Sized;

    /// How this page occupies the panel. `where Self: Sized` for the same reason as
    /// [`Self::view`].
    fn descriptor() -> Descriptor
    where
        Self: Sized;

    /// The page is now the current one. Also runs at boot, for the page the splash lands on.
    fn on_show(&mut self) {}

    /// The page is no longer the current one.
    fn on_hide(&mut self) {}

    /// One frame of the page's own clock, for motion the snapshot cannot carry by itself.
    fn tick(&mut self, _step_ms: u16) {}
}

/// Every service this build has: one instance each, the pages plus the device's own two.
/// Closed on purpose: the dispatch table is the one place an intent meets its owner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Services {
    nav: NavService,
    light: LightService,
    settings: SettingsService,
    menu: MenuPage,
    input: InputPage,
    attitude: AttitudePage,
    audio: AudioPage,
    speaker: SpeakerPage,
    camera: CameraPage,
    network: NetworkPage,
    splash: SplashPage,
    env: EnvPage,
    text: TextPage,
    widgets: WidgetsPage,
    advanced: AdvancedPage,
    feedback: FeedbackPage,
    extended: ExtendedPage,
    data: DataPage,
    list: ListPage,
    lighting: LightingPage,
    headless: HeadlessPage,
}

/// The dispatch key: a page the panel can stop on, or one of the device's own services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceId {
    Page(PageId),
    Navigation,
    Light,
    Settings,
}

impl Services {
    /// The page a route names, for the local gesture table.
    pub fn get(&self, page: PageId) -> &dyn LocalPage {
        match page {
            PageId::Menu => &self.menu,
            PageId::Input => &self.input,
            PageId::Attitude => &self.attitude,
            PageId::Audio => &self.audio,
            PageId::Speaker => &self.speaker,
            PageId::Camera => &self.camera,
            PageId::Network => &self.network,
            PageId::Splash => &self.splash,
            PageId::Env => &self.env,
            PageId::Text => &self.text,
            PageId::Widgets => &self.widgets,
            PageId::Advanced => &self.advanced,
            PageId::Feedback => &self.feedback,
            PageId::Extended => &self.extended,
            PageId::Data => &self.data,
            PageId::List => &self.list,
            PageId::Lighting => &self.lighting,
            PageId::Headless => &self.headless,
        }
    }

    /// The input boundary. A transient owns the panel; otherwise the button is the device's: a
    /// click turns the page and a hold goes home, and no page may claim either. A board with no
    /// panel has no page to turn, so its click stays with the page.
    pub fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        let here = state.page();
        let page = self.get(here);
        if here.is_transient() {
            return page.bind(op, state);
        }
        if matches!(
            op.event,
            InputEvent::Button(ButtonEvent::Click | ButtonEvent::LongPress)
        ) {
            return NavService::from_anywhere(op, state).or_else(|| page.bind(op, state));
        }
        page.bind(op, state)
    }

    /// Who answers which group. One arm per owner, exhaustive like [`descriptor`]: a new group
    /// does not compile until it has one. `Link` and `Network` are two directions of one
    /// domain — the radio's reports and the finger's requests, two records — so one page owns
    /// both; the shared arm is that ownership, not a missing distinction.
    pub fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        match intent {
            BusinessIntent::Navigation(_) => {
                self.service_mut(ServiceId::Navigation).apply(intent, state)
            }
            BusinessIntent::Light { .. } => self.service_mut(ServiceId::Light).apply(intent, state),
            BusinessIntent::Link(_) | BusinessIntent::Network(_) => self
                .service_mut(ServiceId::Page(PageId::Network))
                .apply(intent, state),
            BusinessIntent::Audio(_) => self
                .service_mut(ServiceId::Page(PageId::Audio))
                .apply(intent, state),
            BusinessIntent::Playback(_) => self
                .service_mut(ServiceId::Page(PageId::Speaker))
                .apply(intent, state),
            BusinessIntent::Text(_) => self
                .service_mut(ServiceId::Page(PageId::Text))
                .apply(intent, state),
            BusinessIntent::Menu(MenuIntent::Choose { page }) => {
                self.service_mut(ServiceId::Navigation).apply(
                    &BusinessIntent::Navigation(NavigationIntent::Go(*page)),
                    state,
                )
            }
            BusinessIntent::Menu(_) => self
                .service_mut(ServiceId::Page(PageId::Menu))
                .apply(intent, state),
            BusinessIntent::Widgets(_) => self
                .service_mut(ServiceId::Page(PageId::Widgets))
                .apply(intent, state),
            BusinessIntent::Gallery { wire, .. } => match PageId::from_code(*wire) {
                Some(page) => self.service_mut(ServiceId::Page(page)).apply(intent, state),
                None => (None, Effects::default()),
            },
            BusinessIntent::List(_) => self
                .service_mut(ServiceId::Page(PageId::List))
                .apply(intent, state),
            BusinessIntent::Lighting(_) => self
                .service_mut(ServiceId::Page(PageId::Lighting))
                .apply(intent, state),
            BusinessIntent::Settings(_) => {
                self.service_mut(ServiceId::Settings).apply(intent, state)
            }
        }
    }

    /// Every service has an answer here, including the two the panel never shows.
    fn service_mut(&mut self, id: ServiceId) -> &mut dyn Service {
        match id {
            ServiceId::Page(PageId::Menu) => &mut self.menu,
            ServiceId::Page(PageId::Input) => &mut self.input,
            ServiceId::Page(PageId::Attitude) => &mut self.attitude,
            ServiceId::Page(PageId::Audio) => &mut self.audio,
            ServiceId::Page(PageId::Speaker) => &mut self.speaker,
            ServiceId::Page(PageId::Camera) => &mut self.camera,
            ServiceId::Page(PageId::Network) => &mut self.network,
            ServiceId::Page(PageId::Splash) => &mut self.splash,
            ServiceId::Page(PageId::Env) => &mut self.env,
            ServiceId::Page(PageId::Text) => &mut self.text,
            ServiceId::Page(PageId::Widgets) => &mut self.widgets,
            ServiceId::Page(PageId::Advanced) => &mut self.advanced,
            ServiceId::Page(PageId::Feedback) => &mut self.feedback,
            ServiceId::Page(PageId::Extended) => &mut self.extended,
            ServiceId::Page(PageId::Data) => &mut self.data,
            ServiceId::Page(PageId::List) => &mut self.list,
            ServiceId::Page(PageId::Lighting) => &mut self.lighting,
            ServiceId::Page(PageId::Headless) => &mut self.headless,
            ServiceId::Navigation => &mut self.nav,
            ServiceId::Light => &mut self.light,
            ServiceId::Settings => &mut self.settings,
        }
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

    /// One frame of the current page's own clock. Only the page on screen spends a frame.
    pub fn tick(&mut self, page: PageId, step_ms: u16) {
        self.get_mut(page).tick(step_ms);
    }

    fn get_mut(&mut self, page: PageId) -> &mut dyn LocalPage {
        match page {
            PageId::Menu => &mut self.menu,
            PageId::Input => &mut self.input,
            PageId::Attitude => &mut self.attitude,
            PageId::Audio => &mut self.audio,
            PageId::Speaker => &mut self.speaker,
            PageId::Camera => &mut self.camera,
            PageId::Network => &mut self.network,
            PageId::Splash => &mut self.splash,
            PageId::Env => &mut self.env,
            PageId::Text => &mut self.text,
            PageId::Widgets => &mut self.widgets,
            PageId::Advanced => &mut self.advanced,
            PageId::Feedback => &mut self.feedback,
            PageId::Extended => &mut self.extended,
            PageId::Data => &mut self.data,
            PageId::List => &mut self.list,
            PageId::Lighting => &mut self.lighting,
            PageId::Headless => &mut self.headless,
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
    /// When those fields moved, or while the debug overlay is up.
    WhenOrOverlay(fn(&FrameSnapshot, &FrameSnapshot) -> bool),
}

impl Repaint {
    fn due(self, prev: &FrameSnapshot, next: &FrameSnapshot, overlay: bool) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Overlay => overlay,
            Self::When(moved) => moved(prev, next),
            Self::WhenOrOverlay(moved) => overlay || moved(prev, next),
        }
    }
}

/// How one page's frame moved since the last one, when that movement is a pure scroll.
pub(super) type ScrollFn = fn(&FrameSnapshot, &FrameSnapshot) -> Option<ScrollShift>;

fn no_scroll(_: &FrameSnapshot, _: &FrameSnapshot) -> Option<ScrollShift> {
    None
}

/// How one page occupies the panel: its draw call, the fill under it, where its frames come
/// from, how often a snapshot drift earns a repaint, and what a pure scroll of it looks like.
#[derive(Debug, Clone, Copy)]
pub struct Descriptor {
    view: View,
    custom: Custom,
    ground: Option<Rgb>,
    camera_frames: bool,
    repaint_ms: u64,
    repaints: Repaint,
    scroll: ScrollFn,
}

impl Descriptor {
    pub(super) fn new(view: View) -> Self {
        Self {
            view,
            custom: no_custom,
            ground: None,
            camera_frames: false,
            repaint_ms: REPAINT_FLOOR_MS,
            repaints: Repaint::Never,
            scroll: no_scroll,
        }
    }

    pub(super) fn with_ground(mut self, ground: Option<Rgb>) -> Self {
        self.ground = ground;
        self
    }

    /// The page's own drawing for the custom slots its declaration leaves.
    pub(super) fn with_custom(mut self, custom: Custom) -> Self {
        self.custom = custom;
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

    /// What a frame that only scrolled this page looks like. Without one, every change is a
    /// full repaint.
    pub(super) fn with_scroll(mut self, scroll: ScrollFn) -> Self {
        self.scroll = scroll;
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

    /// The shift when `next` is `prev` moved inside its scroll boxes and nothing else.
    pub fn scroll_shift(self, prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
        (self.scroll)(prev, next)
    }
}

/// The page's descriptor. One exhaustive match: a variant with no descriptor does not compile.
pub fn descriptor(page: PageId) -> Descriptor {
    match page {
        PageId::Menu => MenuPage::descriptor(),
        PageId::Input => InputPage::descriptor(),
        PageId::Attitude => AttitudePage::descriptor(),
        PageId::Audio => AudioPage::descriptor(),
        PageId::Speaker => SpeakerPage::descriptor(),
        PageId::Camera => CameraPage::descriptor(),
        PageId::Network => NetworkPage::descriptor(),
        PageId::Splash => SplashPage::descriptor(),
        PageId::Env => EnvPage::descriptor(),
        PageId::Text => TextPage::descriptor(),
        PageId::Widgets => WidgetsPage::descriptor(),
        PageId::Advanced => AdvancedPage::descriptor(),
        PageId::Feedback => FeedbackPage::descriptor(),
        PageId::Extended => ExtendedPage::descriptor(),
        PageId::Data => DataPage::descriptor(),
        PageId::List => ListPage::descriptor(),
        PageId::Lighting => LightingPage::descriptor(),
        PageId::Headless => HeadlessPage::descriptor(),
    }
}

/// Draws the page. The footer is the panel's own and is drawn for every page that comes
/// through here; a camera frame arrives outside this dispatch and draws its own.
pub fn paint(page: PageId, snapshot: &FrameSnapshot, panel: &mut Panel<'_>) {
    let descriptor = descriptor(page);
    panel.view(snapshot, descriptor.view, descriptor.custom);
}

#[cfg(test)]
mod tests;

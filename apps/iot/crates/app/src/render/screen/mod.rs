//! The screen's renderer: pages over the colour, the backlight, and the camera feed.

mod painter;
mod repaint;

pub use self::painter::AppPainter;

use self::repaint::{repaint_due, repaint_wanted};

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;

use crate::render::engine::ColorOutput;
use crate::render::{Appearance, Drive, Renderer, Topic};
use crate::services::descriptor;
use crate::shared::model::{GlassBreath, GlowPattern, LightState};
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::ui::theme::Theme;
use iot_core::demand::{DemandKey, Mode};
use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::surface::{ScreenPainter, ScreenSurface};

pub struct ScreenRenderer {
    surface: Box<dyn ScreenSurface>,
    topics: [Topic; 1],
    color: ColorOutput,
    /// The breath the glass draws, from the frame's composition.
    breath: Option<GlassBreath>,
    page: Option<PageId>,
    painter: Box<dyn ScreenPainter>,
    overlay: bool,
    /// The newest frame the state published: what a paint from here would draw.
    last_snapshot: Option<FrameSnapshot>,
    /// The frame the glass actually holds. A repaint is judged against this, not against the
    /// newest one, or a change the floor turned away is folded into the baseline and never drawn.
    shown: Option<FrameSnapshot>,
    last_repaint_ms: u32,
    /// The cadence the last snapshot's camera claim asked for, passed through to the
    /// surface rather than read there.
    camera_period_ms: u32,
}

impl ScreenRenderer {
    pub fn new(
        _instance: u8,
        surface: Box<dyn ScreenSurface>,
        painter: Box<dyn ScreenPainter>,
        overlay: bool,
    ) -> Self {
        Self {
            surface,
            topics: [Topic::Frame],
            color: ColorOutput::new(),
            breath: None,
            page: None,
            painter,
            overlay,
            last_snapshot: None,
            shown: None,
            last_repaint_ms: 0,
            camera_period_ms: 0,
        }
    }

    pub fn with_camera(mut self, camera: Box<dyn FrameSource + Send>) -> Self {
        self.surface.attach_camera(camera);
        self
    }

    /// The colour under the page: the page's own ground, else the breath's sample, else the
    /// theme's paper. The backlight is the frame's own setting and never follows the breath.
    fn present(&mut self, now_ms: u32) {
        let Some(snapshot) = self.last_snapshot else {
            return;
        };
        let step = self.surface.repaint_step();
        let color = self
            .page
            .and_then(|page| descriptor(page).ground())
            .or_else(|| snapshot.glass_breath.map(|breath| breath.at(now_ms).color))
            .unwrap_or_else(|| Theme::of(snapshot.theme).ground.rgb888());
        self.color.ground(&mut *self.surface, step, color);
        self.surface.set_backlight(snapshot.backlight.level());
    }

    /// Whether the glass still has a glow to step. A steady colour is already painted.
    fn drive(&self) -> Drive {
        match self.breath {
            Some(breath) if breath.pattern != GlowPattern::Steady => Drive::TimeDriven,
            _ => Drive::Idle,
        }
    }

    fn paint_last(&mut self, now_ms: u32) {
        if let Some(snapshot) = self.last_snapshot
            && self.surface.repaint(&mut *self.painter, &snapshot, false)
        {
            self.shown = Some(snapshot);
            self.last_repaint_ms = now_ms;
        }
    }

    /// Paints the newest frame when its turn has come. Answers whether a frame was painted.
    fn paint_due(&mut self, now_ms: u32, force: bool) -> bool {
        let Some(latest) = self.last_snapshot else {
            return false;
        };
        let since = u64::from(now_ms.wrapping_sub(self.last_repaint_ms));
        let due = force
            || self
                .shown
                .as_ref()
                .is_none_or(|prev| repaint_due(prev.page, prev, &latest, since, self.overlay));
        if !due {
            return false;
        }
        if !self.surface.repaint(&mut *self.painter, &latest, true) {
            return false;
        }
        self.shown = Some(latest);
        self.last_repaint_ms = now_ms;
        true
    }

    /// Whether a frame is wanted that the glass does not hold yet: a filled repaint floor must
    /// not park the loop, because the diff will not offer that frame again.
    fn owed(&self) -> bool {
        let (Some(latest), Some(shown)) = (self.last_snapshot, self.shown) else {
            return false;
        };
        latest != shown && repaint_wanted(shown.page, &shown, &latest, self.overlay)
    }

    fn is_camera_page(&self) -> bool {
        self.page
            .is_some_and(|page| descriptor(page).camera_frames())
    }

    fn on_frame(&mut self, frame: &FrameSnapshot, now_ms: u32) -> Drive {
        let page_changed = self.page != Some(frame.page);
        let breath_changed = self.breath != frame.glass_breath;
        let theme_changed = self
            .last_snapshot
            .is_some_and(|prev| prev.theme != frame.theme);
        let backlight_changed = self
            .last_snapshot
            .is_some_and(|prev| prev.backlight != frame.backlight);
        self.breath = frame.glass_breath;
        let camera = frame.demand.claim(DemandKey::CAMERA);
        self.camera_period_ms = camera.period_ms;
        // Handing the buffer back belongs to this side of the boundary: a chain left
        // running would overwrite every fill after it a row at a time, and the surface
        // cannot tell a demand the page withdrew from one an application never made.
        if !camera.mode.is_collecting() {
            self.surface.release_camera();
        }
        self.last_snapshot = Some(*frame);
        self.page = Some(frame.page);
        let was = self.color.demand();
        self.color
            .set_demand(frame.demand.claim(DemandKey::DISPLAY).mode);
        // A demand that starts collecting again owes the glass the frame the dark swallowed.
        let woke = was.is_collecting() != self.color.demand().is_collecting();
        match self.color.demand() {
            Mode::Off => {
                self.surface.set_backlight_ceiling(0);
                self.color.stop_breath();
                return Drive::Idle;
            }
            Mode::OnChange => self
                .surface
                .set_backlight_ceiling(LightState::ON_DEMAND_BACKLIGHT_PCT),
            Mode::Continuous => self.surface.set_backlight_ceiling(100),
        }
        if page_changed || breath_changed || backlight_changed || woke || theme_changed {
            self.color.forget_color();
            self.present(now_ms);
        }
        self.paint_due(now_ms, page_changed || theme_changed);
        if self.is_camera_page() || frame.animating() || self.owed() {
            return Drive::TimeDriven;
        }
        self.drive()
    }
}

impl Renderer for ScreenRenderer {
    fn topics(&self) -> &[Topic] {
        &self.topics
    }

    fn layout(&self) -> Option<iot_core::ui::Layout> {
        self.painter.layout()
    }

    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        self.surface.flush()
    }

    fn on_appearance(&mut self, appearance: Appearance<'_>, now_ms: u32) -> Drive {
        match appearance {
            Appearance::Frame(frame) => self.on_frame(frame, now_ms),
            Appearance::Surface { .. } => {
                debug_assert!(false, "the screen draws frames, not a lamp's colour");
                Drive::Idle
            }
            Appearance::Demand(_) => {
                debug_assert!(false, "the screen reads the demand from its frame");
                Drive::Idle
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Drive {
        if self.is_camera_page() {
            let Some(snapshot) = self.last_snapshot.as_ref() else {
                return Drive::TimeDriven;
            };
            self.surface.paint_camera(
                u64::from(now_ms),
                self.camera_period_ms,
                &mut *self.painter,
                snapshot,
            );
            return Drive::TimeDriven;
        }
        // A frame the floor turned away is owed to the glass, and no later diff will offer it.
        self.paint_due(now_ms, false);
        if self.breath.is_some() {
            self.present(now_ms);
            self.paint_last(now_ms);
        }
        if self.owed()
            || self
                .last_snapshot
                .as_ref()
                .is_some_and(FrameSnapshot::animating)
        {
            return Drive::TimeDriven;
        }
        self.drive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::snapshot::FrameSnapshot;
    use crate::state::LightState;
    use alloc::rc::Rc;
    use core::any::Any;
    use core::cell::RefCell;
    use iot_core::demand::{Claim, EffectiveDemand, Priority};
    use iot_core::drivers::camera::{
        CameraCounters, CameraSnapshot, FrameAdvance, FrameSource, WindowGeometry,
    };
    use iot_core::drivers::surface::{RgbLight, ScreenContext, ScreenTarget};
    use iot_core::light::Rgb;
    use iot_core::ui::ThemeId;

    struct NullPainter;

    impl ScreenPainter for NullPainter {
        fn paint(
            &mut self,
            _target: ScreenTarget<'_>,
            _context: ScreenContext,
            _snapshot: &dyn Any,
        ) {
        }

        fn paint_camera(
            &mut self,
            _target: ScreenTarget<'_>,
            _readout: CameraSnapshot,
            _context: ScreenContext,
            _snapshot: &dyn Any,
        ) {
        }
    }

    struct ScreenState {
        fills: u32,
        color: Rgb,
        backlight: u8,
        ceiling: u8,
        camera_paints: u32,
        camera_releases: u32,
        camera_period: u32,
        camera: bool,
        paints: u32,
        painted: Option<FrameSnapshot>,
    }

    struct FakeScreen(Rc<RefCell<ScreenState>>);

    impl FakeScreen {
        fn new() -> Self {
            Self(Rc::new(RefCell::new(ScreenState {
                fills: 0,
                color: Rgb(0, 0, 0),
                backlight: 100,
                ceiling: 100,
                camera_paints: 0,
                camera_releases: 0,
                camera_period: 0,
                camera: false,
                paints: 0,
                painted: None,
            })))
        }

        fn handle(&self) -> Rc<RefCell<ScreenState>> {
            Rc::clone(&self.0)
        }
    }

    impl RgbLight for FakeScreen {
        fn set_rgb(&mut self, color: Rgb) {
            let mut screen = self.0.borrow_mut();
            screen.fills += 1;
            screen.color = color;
        }
    }

    impl ScreenSurface for FakeScreen {
        fn set_backlight(&mut self, level_pct: u8) {
            let mut screen = self.0.borrow_mut();
            screen.backlight = level_pct.min(screen.ceiling);
        }

        fn set_backlight_ceiling(&mut self, level_pct: u8) {
            let mut screen = self.0.borrow_mut();
            screen.ceiling = level_pct;
            screen.backlight = screen.backlight.min(level_pct);
        }

        fn repaint_step(&self) -> u8 {
            0
        }

        fn repaint(
            &mut self,
            _painter: &mut dyn ScreenPainter,
            snapshot: &dyn Any,
            _force: bool,
        ) -> bool {
            let mut screen = self.0.borrow_mut();
            screen.paints += 1;
            screen.painted = snapshot.downcast_ref::<FrameSnapshot>().copied();
            true
        }
    }

    impl iot_core::drivers::camera::CameraTarget for FakeScreen {
        fn paint_camera(
            &mut self,
            _now_ms: u64,
            period_ms: u32,
            _painter: &mut dyn ScreenPainter,
            _snapshot: &dyn Any,
        ) -> bool {
            let mut screen = self.0.borrow_mut();
            screen.camera_paints += 1;
            screen.camera_period = period_ms;
            true
        }

        fn attach_camera(&mut self, _camera: Box<dyn FrameSource + Send>) {
            self.0.borrow_mut().camera = true;
        }

        fn release_camera(&mut self) {
            self.0.borrow_mut().camera_releases += 1;
        }
    }

    fn frame_on(page: PageId, display: Mode) -> FrameSnapshot {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(display, 0, Priority::Primary),
        );
        FrameSnapshot {
            demand,
            page,
            ..FrameSnapshot::default()
        }
    }

    fn frame_asked(display: Mode) -> FrameSnapshot {
        frame_on(PageId::Input, display)
    }

    fn frame_collecting(page: PageId, period_ms: u32) -> FrameSnapshot {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(Mode::Continuous, 0, Priority::Primary),
        );
        demand.set(
            DemandKey::CAMERA,
            Claim::new(Mode::Continuous, period_ms, Priority::Primary),
        );
        FrameSnapshot {
            demand,
            page,
            ..FrameSnapshot::default()
        }
    }

    fn frame_with_breath(page: PageId, breath: Option<GlassBreath>) -> FrameSnapshot {
        FrameSnapshot {
            glass_breath: breath,
            ..frame_on(page, Mode::Continuous)
        }
    }

    fn renderer() -> (ScreenRenderer, Rc<RefCell<ScreenState>>) {
        let screen = FakeScreen::new();
        let state = screen.handle();
        (
            ScreenRenderer::new(0, Box::new(screen), Box::new(NullPainter), true),
            state,
        )
    }

    #[test]
    fn a_page_with_no_breath_shows_its_theme_ground() {
        let (mut r, screen) = renderer();
        assert_eq!(
            r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 0),
            Drive::Idle,
            "the first appearance only tells the screen what it is for"
        );
        assert_eq!(
            screen.borrow().color,
            Theme::of(ThemeId::Dark).ground.rgb888()
        );
        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::Idle);
        assert_eq!(screen.borrow().fills, fills, "nothing is breathing");
    }

    #[test]
    fn a_glass_breath_reaches_the_pixels_and_steps() {
        let (mut r, screen) = renderer();
        let breath = Some(GlassBreath::CALM);
        r.on_appearance(
            Appearance::Frame(&frame_with_breath(PageId::Lighting, breath)),
            0,
        );
        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::TimeDriven);
        assert!(
            screen.borrow().fills > fills,
            "the breath the page asked for is painted"
        );
    }

    #[test]
    fn clearing_the_breath_returns_to_the_theme_ground() {
        let (mut r, screen) = renderer();
        r.on_appearance(
            Appearance::Frame(&frame_with_breath(PageId::Env, Some(GlassBreath::CALM))),
            0,
        );
        r.on_appearance(
            Appearance::Frame(&frame_with_breath(PageId::Env, None)),
            100,
        );
        assert_eq!(
            screen.borrow().color,
            Theme::of(ThemeId::Dark).ground.rgb888()
        );
        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::Idle);
        assert_eq!(screen.borrow().fills, fills);
    }

    #[test]
    fn an_unlit_screen_ships_nothing() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Off)), 0);
        assert_eq!(screen.borrow().ceiling, 0, "and its backlight is off");
        let fills_before = screen.borrow().fills;

        r.step(1_000);
        r.step(1_100);

        assert_eq!(
            screen.borrow().fills,
            fills_before,
            "the breathing loop leaves through the same gate"
        );
        assert_eq!(screen.borrow().backlight, 0);
    }

    #[test]
    fn a_screen_relit_after_being_dark_paints_the_frame_the_darkness_swallowed() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Off)), 0);
        r.step(1_000);
        assert_eq!(screen.borrow().fills, 0);

        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 2_000);
        r.step(2_000);
        assert_eq!(
            screen.borrow().fills,
            1,
            "the first frame after relighting is the one that matters"
        );
        assert_eq!(screen.borrow().ceiling, 100);
    }

    #[test]
    fn a_change_the_floor_turned_away_is_painted_when_its_turn_comes() {
        let (mut r, screen) = renderer();
        let first = frame_on(PageId::Speaker, Mode::Continuous);
        r.on_appearance(Appearance::Frame(&first), 0);
        assert_eq!(screen.borrow().paints, 1, "the first frame lands");

        let mut played = first;
        played.playback.plays = 7;
        assert_eq!(
            r.on_appearance(Appearance::Frame(&played), 20),
            Drive::TimeDriven,
            "a frame held back by the floor still keeps the loop awake"
        );
        assert_eq!(
            screen.borrow().paints,
            1,
            "the page's step has not elapsed yet"
        );

        r.step(40);
        assert_eq!(
            screen.borrow().paints,
            2,
            "the held-back frame is the one owed to the glass"
        );
        assert_eq!(screen.borrow().painted, Some(played));
    }

    #[test]
    fn a_page_that_draws_nothing_new_parks_with_the_floor_filled() {
        let screen = FakeScreen::new();
        let state = screen.handle();
        let mut r = ScreenRenderer::new(0, Box::new(screen), Box::new(NullPainter), false);
        r.on_appearance(
            Appearance::Frame(&frame_on(PageId::Env, Mode::Continuous)),
            0,
        );
        let paints = state.borrow().paints;
        let mut ticked = frame_on(PageId::Env, Mode::Continuous);
        ticked.audio.elapsed_ms = 1_234;
        assert_eq!(
            r.on_appearance(Appearance::Frame(&ticked), 20),
            Drive::Idle,
            "a drift the Env page does not draw is not an owed frame"
        );
        r.step(40);
        assert_eq!(state.borrow().paints, paints, "and it stays unpainted");
    }

    #[test]
    fn a_dimmed_screen_is_brightness_only_and_holds_still() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::OnChange)), 0);
        assert_eq!(screen.borrow().ceiling, LightState::ON_DEMAND_BACKLIGHT_PCT);
        assert_eq!(
            screen.borrow().backlight,
            LightState::ON_DEMAND_BACKLIGHT_PCT
        );

        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::Idle);
        assert_eq!(screen.borrow().fills, fills);
    }

    #[test]
    fn a_light_page_grounds_the_screen_with_its_paper() {
        let (mut r, screen) = renderer();
        let mut frame = frame_on(PageId::Env, Mode::Continuous);
        frame.theme = ThemeId::Light;
        r.on_appearance(Appearance::Frame(&frame), 0);
        assert_eq!(
            screen.borrow().color,
            Theme::of(ThemeId::Light).ground.rgb888()
        );
        assert_eq!(screen.borrow().backlight, 100);
    }

    #[test]
    fn a_page_with_its_own_ground_keeps_it_in_light() {
        for page in [PageId::Camera, PageId::Network] {
            let (mut r, screen) = renderer();
            let mut frame = frame_on(page, Mode::Continuous);
            frame.theme = ThemeId::Light;
            r.on_appearance(Appearance::Frame(&frame), 0);
            assert_eq!(screen.borrow().color, Rgb(0, 0, 0), "{page:?}");
        }
    }

    #[test]
    fn the_network_page_grounds_the_screen_black_and_still() {
        let (mut r, screen) = renderer();
        assert_eq!(
            r.on_appearance(
                Appearance::Frame(&frame_on(PageId::Network, Mode::Continuous)),
                0
            ),
            Drive::Idle,
            "a breathing light does not breathe through the page's ground"
        );
        assert_eq!(screen.borrow().color, Rgb(0, 0, 0), "the ground is black");
        assert_eq!(
            screen.borrow().backlight,
            100,
            "and the setup screen is lit to read"
        );
        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::Idle);
        r.step(1_100);
        assert_eq!(
            screen.borrow().fills,
            fills,
            "and the loop parks instead of repainting"
        );
        assert_eq!(
            screen.borrow().backlight,
            100,
            "the backlight holds instead of ramping"
        );
    }

    #[test]
    fn the_camera_page_grounds_the_screen_black() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 0);

        assert_eq!(
            r.on_appearance(
                Appearance::Frame(&frame_on(PageId::Camera, Mode::Continuous)),
                1
            ),
            Drive::TimeDriven,
            "a camera page is driven by arriving frames, not by the light"
        );
        assert_eq!(
            screen.borrow().color,
            Rgb(0, 0, 0),
            "the camera page's ground is the page's black, not the light's colour"
        );
        assert_eq!(screen.borrow().backlight, 100);
        assert_eq!(r.step(2), Drive::TimeDriven);
        assert_eq!(
            screen.borrow().camera_paints,
            1,
            "the screen pulls the next frame"
        );
    }

    #[test]
    fn the_camera_cadence_is_handed_to_the_surface_rather_than_read_there() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_collecting(PageId::Camera, 200)), 0);
        r.step(1_000);
        assert_eq!(
            screen.borrow().camera_period,
            200,
            "the claim's cadence is the policy the page asked for"
        );
        assert_eq!(
            screen.borrow().camera_releases,
            0,
            "a collecting demand keeps the buffer"
        );
    }

    #[test]
    fn the_camera_is_released_when_the_demand_stops_collecting() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_collecting(PageId::Camera, 0)), 0);
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 1);
        assert_eq!(
            screen.borrow().camera_releases,
            1,
            "the renderer hands the buffer back; the surface only does as it is told"
        );
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 2);
        assert_eq!(
            screen.borrow().camera_releases,
            2,
            "and it asks every snapshot, leaving the surface's own guard to make it a no-op"
        );
    }

    #[test]
    fn leaving_the_page_that_grounds_the_screen_puts_the_paper_back() {
        let (mut r, screen) = renderer();
        r.on_appearance(
            Appearance::Frame(&frame_on(PageId::Network, Mode::Continuous)),
            0,
        );
        assert_eq!(screen.borrow().color, Rgb(0, 0, 0));

        assert_eq!(
            r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 100),
            Drive::Idle,
            "the theme's paper returns"
        );
        assert_eq!(
            screen.borrow().color,
            Theme::of(ThemeId::Dark).ground.rgb888()
        );
    }

    #[test]
    fn the_frame_backlight_setting_reaches_the_surface() {
        let (mut r, screen) = renderer();
        let mut frame = frame_asked(Mode::Continuous);
        frame.backlight.set_pct(50);
        r.on_appearance(Appearance::Frame(&frame), 0);
        assert_eq!(
            screen.borrow().backlight,
            50,
            "the panel gets the setting itself, not the breath's"
        );
        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::Idle);
        assert_eq!(screen.borrow().fills, fills, "a still page holds it");
    }

    #[test]
    fn a_backlight_only_change_reaches_the_surface_without_a_breath() {
        let (mut r, screen) = renderer();
        let mut frame = frame_asked(Mode::Continuous);
        r.on_appearance(Appearance::Frame(&frame), 0);
        assert_eq!(screen.borrow().backlight, 100);

        frame.backlight.set_pct(0);
        r.on_appearance(Appearance::Frame(&frame), 100);
        assert_eq!(
            screen.borrow().backlight,
            0,
            "a dark setting is a dark panel"
        );
    }

    #[test]
    fn a_steady_glow_paints_once_and_holds_still() {
        let (mut r, screen) = renderer();
        let mut breath = GlassBreath::CALM;
        breath.pattern = GlowPattern::Steady;
        r.on_appearance(
            Appearance::Frame(&frame_with_breath(PageId::Lighting, Some(breath))),
            0,
        );
        let fills = screen.borrow().fills;
        assert_eq!(r.step(1_000), Drive::Idle);
        assert_eq!(
            screen.borrow().fills,
            fills,
            "a steady colour does not step"
        );
    }

    #[test]
    fn the_camera_is_attached_only_to_a_page_surface() {
        let (r, screen) = renderer();
        assert!(!screen.borrow().camera);
        let r = r.with_camera(Box::new(NullCamera));
        assert!(
            screen.borrow().camera,
            "a page surface is what a camera can write into"
        );
        assert_eq!(r.topics(), &[Topic::Frame]);
    }

    struct NullCamera;

    impl FrameSource for NullCamera {
        fn advance(&mut self, _frame: &mut [u8], _now_ms: u64) -> FrameAdvance {
            FrameAdvance::Same
        }

        fn pause(&mut self) {}

        fn resume(&mut self, _frame: &mut [u8]) {}

        fn counters(&self) -> CameraCounters {
            CameraCounters::ZERO
        }

        fn geometry(&mut self) -> Option<WindowGeometry> {
            None
        }
    }
}

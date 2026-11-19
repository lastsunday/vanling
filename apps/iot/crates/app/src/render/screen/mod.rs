//! The screen's renderer: pages over the colour, the backlight, and the camera feed.

mod painter;
mod repaint;

pub use self::painter::AppPainter;

use self::repaint::repaint_due;

use alloc::boxed::Box;
use core::any::Any;
use core::future::Future;
use core::pin::Pin;

use crate::render::engine::ColorOutput;
use crate::render::{Appearance, Drive, LightAppearance, Renderer, Topic};
use crate::services::descriptor;
use crate::shared::model::LightState;
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::demand::{DemandKey, Mode};
use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::surface::{ScreenPainter, ScreenSurface};

pub struct ScreenRenderer {
    instance: u8,
    surface: Box<dyn ScreenSurface>,
    topics: [Topic; 2],
    color: ColorOutput,
    declared: Option<LightAppearance>,
    page: Option<PageId>,
    painter: Box<dyn ScreenPainter>,
    overlay: bool,
    last_snapshot: Option<FrameSnapshot>,
    last_repaint_ms: u32,
    /// The cadence the last snapshot's camera claim asked for, passed through to the
    /// surface rather than read there.
    camera_period_ms: u32,
}

impl ScreenRenderer {
    pub fn new(
        instance: u8,
        surface: Box<dyn ScreenSurface>,
        painter: Box<dyn ScreenPainter>,
        overlay: bool,
    ) -> Self {
        Self {
            instance,
            surface,
            topics: [Topic::Surface(instance), Topic::Frame],
            color: ColorOutput::new(),
            declared: None,
            page: None,
            painter,
            overlay,
            last_snapshot: None,
            last_repaint_ms: 0,
            camera_period_ms: 0,
        }
    }

    pub fn with_camera(mut self, camera: Box<dyn FrameSource + Send>) -> Self {
        self.surface.attach_camera(camera);
        self
    }

    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.declared else {
            return;
        };
        let step = self.surface.repaint_step();
        if let Some(ground) = self.page.and_then(|page| descriptor(page).ground()) {
            self.color.ground(&mut *self.surface, step, ground);
            self.surface.set_backlight(100);
            return;
        }
        let shown = self.color.show(&mut *self.surface, step, declared, now_ms);
        self.surface
            .set_backlight(shown.backlight(now_ms, LightState::BACKLIGHT_FLOOR_PCT));
    }

    fn paint_last(&mut self) {
        if let Some(snapshot) = self.last_snapshot.as_ref() {
            self.surface.repaint(&*self.painter, snapshot, false);
        }
    }

    fn is_camera_page(&self) -> bool {
        self.page
            .is_some_and(|page| descriptor(page).camera_frames())
    }

    fn on_frame(&mut self, frame: &FrameSnapshot, now_ms: u32) -> Drive {
        let page_changed = self.page != Some(frame.page);
        let camera = frame.demand.claim(DemandKey::CAMERA);
        self.camera_period_ms = camera.period_ms;
        // Handing the buffer back belongs to this side of the boundary: a chain left
        // running would overwrite every fill after it a row at a time, and the surface
        // cannot tell a demand the page withdrew from one an application never made.
        if !camera.mode.is_collecting() {
            self.surface.release_camera();
        }
        let previous = self.last_snapshot.replace(*frame);
        let repaint = previous.as_ref().is_none_or(|prev| {
            repaint_due(
                prev.page,
                prev,
                frame,
                u64::from(now_ms.wrapping_sub(self.last_repaint_ms)),
                self.overlay,
            )
        });
        self.page = Some(frame.page);
        self.color
            .set_demand(frame.demand.claim(DemandKey::DISPLAY).mode);
        match self.color.demand() {
            Mode::Off => {
                self.surface.set_backlight_ceiling(0);
                self.color.stop_breath();
                return Drive::Idle;
            }
            Mode::OnChange => self
                .surface
                .set_backlight_ceiling(LightState::BACKLIGHT_FLOOR_PCT),
            Mode::Continuous => self.surface.set_backlight_ceiling(100),
        }
        if page_changed {
            self.color.forget_color();
            self.present(now_ms);
        }
        if (page_changed || repaint) && self.repaint(frame, true) {
            self.last_repaint_ms = now_ms;
        }
        if self.is_camera_page() {
            return Drive::TimeDriven;
        }
        self.color.drive()
    }

    fn repaint(&mut self, snapshot: &dyn Any, force: bool) -> bool {
        self.surface.repaint(&*self.painter, snapshot, force)
    }
}

impl Renderer for ScreenRenderer {
    fn topics(&self) -> &[Topic] {
        &self.topics
    }

    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        self.surface.flush()
    }

    fn on_appearance(&mut self, appearance: Appearance<'_>, now_ms: u32) -> Drive {
        match appearance {
            Appearance::Frame(frame) => self.on_frame(frame, now_ms),
            Appearance::Surface { instance, light } => {
                debug_assert_eq!(instance, self.instance);
                self.declared = Some(light);
                self.present(now_ms);
                self.paint_last();
                self.color.drive()
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
                &*self.painter,
                snapshot,
            );
            return Drive::TimeDriven;
        }
        let step = self.surface.repaint_step();
        let Some(shown) = self.color.step(&mut *self.surface, step, now_ms) else {
            return Drive::Idle;
        };
        self.surface
            .set_backlight(shown.backlight(now_ms, LightState::BACKLIGHT_FLOOR_PCT));
        self.paint_last();
        Drive::TimeDriven
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::light_appearance;
    use crate::shared::snapshot::FrameSnapshot;
    use crate::state::LightState;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use iot_core::demand::{Claim, EffectiveDemand, Priority};
    use iot_core::drivers::camera::{
        CameraCounters, CameraSnapshot, FrameAdvance, FrameSource, WindowGeometry,
    };
    use iot_core::drivers::surface::{RgbLight, ScreenContext, ScreenTarget};
    use iot_core::light::Rgb;

    struct NullPainter;

    impl ScreenPainter for NullPainter {
        fn paint(&self, _target: ScreenTarget<'_>, _context: ScreenContext, _snapshot: &dyn Any) {}

        fn paint_camera(
            &self,
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
            _painter: &dyn ScreenPainter,
            _snapshot: &dyn Any,
            _force: bool,
        ) -> bool {
            false
        }
    }

    impl iot_core::drivers::camera::CameraTarget for FakeScreen {
        fn paint_camera(
            &mut self,
            _now_ms: u64,
            period_ms: u32,
            _painter: &dyn ScreenPainter,
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
        frame_on(PageId::Ambient, display)
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

    fn ambient_breath(instance: u8) -> Appearance<'static> {
        Appearance::Surface {
            instance,
            light: light_appearance(LightState::boot()),
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
    fn a_lit_screen_keeps_breathing_on_a_page_that_never_settles() {
        let (mut r, screen) = renderer();
        assert_eq!(
            r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 0),
            Drive::Idle,
            "the first appearance only tells the screen what it is for"
        );
        assert_eq!(r.on_appearance(ambient_breath(0), 0), Drive::TimeDriven);
        r.step(1_000);
        r.step(1_100);
        assert!(screen.borrow().fills > 0, "a lit screen keeps painting");
    }

    #[test]
    fn an_unlit_screen_ships_nothing_from_any_topic() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Off)), 0);
        assert_eq!(screen.borrow().ceiling, 0, "and its backlight is off");
        let fills_before = screen.borrow().fills;

        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        r.step(1_100);
        r.on_appearance(
            Appearance::Surface {
                instance: 0,
                light: LightAppearance::Color(Rgb(255, 0, 0)),
            },
            1_200,
        );

        assert_eq!(
            screen.borrow().fills,
            fills_before,
            "the breathing loop and the colour change both leave through the same gate"
        );
        assert_eq!(screen.borrow().backlight, 0);
    }

    #[test]
    fn a_screen_relit_after_being_dark_paints_the_frame_the_darkness_swallowed() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Off)), 0);
        r.on_appearance(ambient_breath(0), 0);
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
    fn a_dimmed_screen_is_brightness_only_and_keeps_its_cadence() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::OnChange)), 0);
        assert_eq!(screen.borrow().ceiling, LightState::BACKLIGHT_FLOOR_PCT);

        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Drive::TimeDriven,
            "dimming is a brightness, not a rate"
        );
        r.step(1_000);
        assert!(screen.borrow().fills > 0);
    }

    #[test]
    fn the_network_page_grounds_the_screen_black_and_still() {
        let (mut r, screen) = renderer();
        r.on_appearance(
            Appearance::Frame(&frame_on(PageId::Network, Mode::Continuous)),
            0,
        );
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
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

        r.on_appearance(
            Appearance::Surface {
                instance: 0,
                light: LightAppearance::Off,
            },
            1_200,
        );
        assert_eq!(screen.borrow().color, Rgb(0, 0, 0));
        assert_eq!(screen.borrow().backlight, 100);
    }

    #[test]
    fn the_camera_page_grounds_the_screen_black() {
        let (mut r, screen) = renderer();
        r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 0);
        r.on_appearance(
            Appearance::Surface {
                instance: 0,
                light: LightAppearance::Color(Rgb(10, 20, 30)),
            },
            0,
        );
        assert_eq!(screen.borrow().color, Rgb(10, 20, 30));

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
    fn leaving_the_page_that_grounds_the_screen_puts_the_light_back() {
        let (mut r, screen) = renderer();
        r.on_appearance(
            Appearance::Frame(&frame_on(PageId::Network, Mode::Continuous)),
            0,
        );
        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(screen.borrow().color, Rgb(0, 0, 0));

        assert_eq!(
            r.on_appearance(Appearance::Frame(&frame_asked(Mode::Continuous)), 100),
            Drive::TimeDriven,
            "the breathing the page was hiding resumes"
        );
        let LightAppearance::Breathing(breath) = light_appearance(LightState::boot()) else {
            panic!("the boot light breathes");
        };
        assert_eq!(screen.borrow().color, breath.rgb(100));
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
        assert_eq!(r.topics(), &[Topic::Surface(0), Topic::Frame]);
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

//! The panel's renderer: pages over the colour, the backlight, and the camera feed.

mod painter;
mod policy;
mod repaint;

pub use self::painter::AppPainter;
pub use self::policy::DIAG_REPAINT_STEP_MS;
pub use self::repaint::{diag_repaint_due, windowed_percent, windowed_rate};

use alloc::boxed::Box;
use core::any::Any;
use core::future::Future;
use core::pin::Pin;

use crate::render::common::ColorDriver;
use crate::render::{Activity, Appearance, Consumer, LightAppearance, Renderer};
use crate::shared::diagnostics::Diagnostics;
use crate::state::{LightState, Page};
use iot_core::demand::{DemandKey, Mode};
use iot_core::drivers::camera::{CameraSnapshot, FrameSource};
use iot_core::drivers::surface::{PageSurface, PanelContext, PanelPainter, PanelTarget};

/// Used by tests and by a surface whose product injected no painter: the colour path still
/// runs, and no page ink is added.
struct NullPainter;

impl PanelPainter for NullPainter {
    fn paint(&self, _target: PanelTarget<'_>, _context: PanelContext, _snapshot: &dyn Any) {}

    fn paint_camera(
        &self,
        _target: PanelTarget<'_>,
        _readout: CameraSnapshot,
        _context: PanelContext,
        _snapshot: &dyn Any,
    ) {
    }
}

static NULL_PAINTER: NullPainter = NullPainter;

pub struct ScreenRenderer {
    instance: u8,
    surface: Box<dyn PageSurface>,
    consumers: [Consumer; 2],
    color: ColorDriver,
    appearance: Option<LightAppearance>,
    page: Option<Page>,
    /// The painter the product injected, and the snapshot gate it is handed.
    painter: Option<Box<dyn PanelPainter>>,
    debug: bool,
    last_diagnostics: Option<Diagnostics>,
    last_repaint_ms: u32,
    /// The cadence the last snapshot's camera claim asked for, passed through to the
    /// surface rather than read there.
    camera_period_ms: u32,
}

impl ScreenRenderer {
    pub fn new(instance: u8, surface: Box<dyn PageSurface>) -> Self {
        Self {
            instance,
            surface,
            consumers: [Consumer::Surface(instance), Consumer::Diagnostics],
            color: ColorDriver::new(),
            appearance: None,
            page: None,
            painter: None,
            debug: false,
            last_diagnostics: None,
            last_repaint_ms: 0,
            camera_period_ms: 0,
        }
    }

    pub fn with_painter(mut self, painter: Box<dyn PanelPainter>, debug: bool) -> Self {
        self.painter = Some(painter);
        self.debug = debug;
        self
    }

    pub fn with_camera(mut self, camera: Box<dyn FrameSource + Send>) -> Self {
        self.surface.attach_camera(camera);
        self.page = None;
        self
    }

    fn repaint(&mut self, snapshot: &dyn Any, force: bool) -> bool {
        let painter = self.painter.as_deref().unwrap_or(&NULL_PAINTER);
        self.surface.repaint(painter, snapshot, force)
    }
}

impl ScreenRenderer {
    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.appearance else {
            return;
        };
        let step = self.surface.repaint_step();
        if let Some(ground) = self.page.and_then(policy::ground) {
            self.color.ground(&mut *self.surface, step, ground);
            self.surface.set_backlight(100);
            return;
        }
        let lit = self.color.show(&mut *self.surface, step, declared, now_ms);
        self.surface
            .set_backlight(lit.backlight(now_ms, LightState::BACKLIGHT_FLOOR_PCT));
    }

    fn on_camera_page(&self) -> bool {
        self.page.is_some_and(policy::draws_frames)
    }
}

impl Renderer for ScreenRenderer {
    fn consumers(&self) -> &[Consumer] {
        &self.consumers
    }

    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        self.surface.flush()
    }

    fn on_appearance(&mut self, appearance: Appearance, now_ms: u32) -> Activity {
        match appearance {
            Appearance::Diagnostics(diagnostics) => {
                let page_changed = self.page != Some(diagnostics.page);
                let camera = diagnostics.demand.claim(DemandKey::CAMERA);
                self.camera_period_ms = camera.period_ms;
                // Handing the buffer back belongs to this side of the boundary: a chain
                // left running would overwrite every fill after it a row at a time, and
                // the surface cannot tell a demand the page withdrew from one an
                // application never made.
                if !camera.mode.is_collecting() {
                    self.surface.release_camera();
                }
                let previous = self.last_diagnostics.replace(diagnostics);
                let repaint = previous.as_ref().is_none_or(|prev| {
                    diag_repaint_due(
                        prev.page,
                        prev,
                        &diagnostics,
                        u64::from(now_ms.wrapping_sub(self.last_repaint_ms)),
                        self.debug,
                    )
                });
                self.page = Some(diagnostics.page);
                self.color
                    .set_display(diagnostics.demand.claim(DemandKey::DISPLAY).mode);
                match self.color.display() {
                    Mode::Off => {
                        self.surface.set_backlight_ceiling(0);
                        self.color.stop();
                        return Activity::Idle;
                    }
                    Mode::OnChange => self
                        .surface
                        .set_backlight_ceiling(LightState::BACKLIGHT_FLOOR_PCT),
                    Mode::Continuous => self.surface.set_backlight_ceiling(100),
                }
                if page_changed {
                    self.color.forget();
                    self.present(now_ms);
                }
                if (page_changed || repaint) && self.repaint(&diagnostics, true) {
                    self.last_repaint_ms = now_ms;
                }
                if self.on_camera_page() {
                    return Activity::TimeDriven;
                }
                breathing_activity(self.color.breathing())
            }
            Appearance::Surface {
                instance,
                appearance,
            } => {
                debug_assert_eq!(instance, self.instance);
                self.appearance = Some(appearance);
                self.present(now_ms);
                let Self {
                    surface,
                    painter,
                    last_diagnostics,
                    ..
                } = self;
                if let Some(snapshot) = last_diagnostics.as_ref() {
                    let painter = painter.as_deref().unwrap_or(&NULL_PAINTER);
                    surface.repaint(painter, snapshot, false);
                }
                breathing_activity(self.color.breathing())
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Activity {
        if self.on_camera_page() {
            let Some(snapshot) = self.last_diagnostics.as_ref() else {
                return Activity::TimeDriven;
            };
            let painter = self.painter.as_deref().unwrap_or(&NULL_PAINTER);
            self.surface
                .paint_camera(u64::from(now_ms), self.camera_period_ms, painter, snapshot);
            return Activity::TimeDriven;
        }
        let step = self.surface.repaint_step();
        match self.color.step(&mut *self.surface, step, now_ms) {
            Some(lit) => {
                self.surface
                    .set_backlight(lit.backlight(now_ms, LightState::BACKLIGHT_FLOOR_PCT));
                let Self {
                    surface,
                    painter,
                    last_diagnostics,
                    ..
                } = self;
                if let Some(snapshot) = last_diagnostics.as_ref() {
                    let painter = painter.as_deref().unwrap_or(&NULL_PAINTER);
                    surface.repaint(painter, snapshot, false);
                }
                Activity::TimeDriven
            }
            None => Activity::Idle,
        }
    }
}

fn breathing_activity(breathing: bool) -> Activity {
    if breathing {
        Activity::TimeDriven
    } else {
        Activity::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::light_appearance;
    use crate::shared::diagnostics::Diagnostics;
    use crate::state::LightState;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use iot_core::demand::{Claim, EffectiveDemand, Priority};
    use iot_core::drivers::camera::{CameraCounters, FrameAdvance, WindowGeometry};
    use iot_core::drivers::surface::RgbLight;
    use iot_core::paint::Rgb;

    struct PanelState {
        fills: u32,
        color: Rgb,
        backlight: u8,
        ceiling: u8,
        camera_paints: u32,
        camera_releases: u32,
        camera_period: u32,
        camera: bool,
    }

    struct FakeScreen(Rc<RefCell<PanelState>>);

    impl FakeScreen {
        fn new() -> Self {
            Self(Rc::new(RefCell::new(PanelState {
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

        fn handle(&self) -> Rc<RefCell<PanelState>> {
            Rc::clone(&self.0)
        }
    }

    impl RgbLight for FakeScreen {
        fn set_rgb(&mut self, color: Rgb) {
            let mut panel = self.0.borrow_mut();
            panel.fills += 1;
            panel.color = color;
        }
    }

    impl PageSurface for FakeScreen {
        fn set_backlight(&mut self, level_pct: u8) {
            let mut panel = self.0.borrow_mut();
            panel.backlight = level_pct.min(panel.ceiling);
        }

        fn set_backlight_ceiling(&mut self, level_pct: u8) {
            let mut panel = self.0.borrow_mut();
            panel.ceiling = level_pct;
            panel.backlight = panel.backlight.min(level_pct);
        }

        fn repaint_step(&self) -> u8 {
            0
        }
    }

    impl crate::shared::diagnostics::DiagnosticsSink for FakeScreen {
        fn consume(&mut self, _diagnostics: &Diagnostics) {}
    }

    impl iot_core::drivers::camera::CameraTarget for FakeScreen {
        fn paint_camera(
            &mut self,
            _now_ms: u64,
            period_ms: u32,
            _painter: &dyn PanelPainter,
            _snapshot: &dyn Any,
        ) -> bool {
            let mut panel = self.0.borrow_mut();
            panel.camera_paints += 1;
            panel.camera_period = period_ms;
            true
        }

        fn attach_camera(&mut self, _camera: Box<dyn FrameSource + Send>) {
            self.0.borrow_mut().camera = true;
        }

        fn release_camera(&mut self) {
            self.0.borrow_mut().camera_releases += 1;
        }
    }

    fn panel_on(page: Page, display: Mode) -> Appearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(display, 0, Priority::Primary),
        );
        Appearance::Diagnostics(Diagnostics {
            demand,
            page,
            ..Diagnostics::default()
        })
    }

    fn panel_asked(display: Mode) -> Appearance {
        panel_on(Page::Ambient, display)
    }

    fn panel_collecting(page: Page, period_ms: u32) -> Appearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(Mode::Continuous, 0, Priority::Primary),
        );
        demand.set(
            DemandKey::CAMERA,
            Claim::new(Mode::Continuous, period_ms, Priority::Primary),
        );
        Appearance::Diagnostics(Diagnostics {
            demand,
            page,
            ..Diagnostics::default()
        })
    }

    fn ambient_breath(instance: u8) -> Appearance {
        Appearance::Surface {
            instance,
            appearance: light_appearance(LightState::boot()),
        }
    }

    fn renderer() -> (ScreenRenderer, Rc<RefCell<PanelState>>) {
        let screen = FakeScreen::new();
        let panel = screen.handle();
        (ScreenRenderer::new(0, Box::new(screen)), panel)
    }

    #[test]
    fn a_lit_panel_keeps_breathing_on_a_page_that_never_settles() {
        let (mut r, panel) = renderer();
        assert_eq!(
            r.on_appearance(panel_asked(Mode::Continuous), 0),
            Activity::Idle,
            "the first appearance only tells the panel what it is for"
        );
        assert_eq!(r.on_appearance(ambient_breath(0), 0), Activity::TimeDriven);
        r.step(1_000);
        r.step(1_100);
        assert!(panel.borrow().fills > 0, "a lit panel keeps painting");
    }

    #[test]
    fn an_unlit_panel_ships_nothing_from_any_consumer() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_asked(Mode::Off), 0);
        assert_eq!(panel.borrow().ceiling, 0, "and its backlight is off");
        let fills_before = panel.borrow().fills;

        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        r.step(1_100);
        r.on_appearance(
            Appearance::Surface {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(255, 0, 0)),
            },
            1_200,
        );

        assert_eq!(
            panel.borrow().fills,
            fills_before,
            "the breathing loop and the colour change both leave through the same gate"
        );
        assert_eq!(panel.borrow().backlight, 0);
    }

    #[test]
    fn a_panel_relit_after_being_dark_paints_the_frame_the_darkness_swallowed() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_asked(Mode::Off), 0);
        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        assert_eq!(panel.borrow().fills, 0);

        r.on_appearance(panel_asked(Mode::Continuous), 2_000);
        r.step(2_000);
        assert_eq!(
            panel.borrow().fills,
            1,
            "the first frame after relighting is the one that matters"
        );
        assert_eq!(panel.borrow().ceiling, 100);
    }

    #[test]
    fn a_dimmed_panel_is_brightness_only_and_keeps_its_cadence() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_asked(Mode::OnChange), 0);
        assert_eq!(panel.borrow().ceiling, LightState::BACKLIGHT_FLOOR_PCT);

        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Activity::TimeDriven,
            "dimming is a brightness, not a rate"
        );
        r.step(1_000);
        assert!(panel.borrow().fills > 0);
    }

    #[test]
    fn the_network_page_grounds_the_panel_black_and_still() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_on(Page::Network, Mode::Continuous), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Activity::Idle,
            "a breathing light does not breathe through the page's ground"
        );
        assert_eq!(panel.borrow().color, Rgb(0, 0, 0), "the ground is black");
        assert_eq!(
            panel.borrow().backlight,
            100,
            "and the setup screen is lit to read"
        );
        let fills = panel.borrow().fills;
        assert_eq!(r.step(1_000), Activity::Idle);
        r.step(1_100);
        assert_eq!(
            panel.borrow().fills,
            fills,
            "and the loop parks instead of repainting"
        );
        assert_eq!(
            panel.borrow().backlight,
            100,
            "the backlight holds instead of ramping"
        );

        r.on_appearance(
            Appearance::Surface {
                instance: 0,
                appearance: LightAppearance::Off,
            },
            1_200,
        );
        assert_eq!(panel.borrow().color, Rgb(0, 0, 0));
        assert_eq!(panel.borrow().backlight, 100);
    }

    #[test]
    fn the_camera_page_grounds_the_panel_black() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_on(Page::Ambient, Mode::Continuous), 0);
        r.on_appearance(
            Appearance::Surface {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(10, 20, 30)),
            },
            0,
        );
        assert_eq!(panel.borrow().color, Rgb(10, 20, 30));

        assert_eq!(
            r.on_appearance(panel_on(Page::Camera, Mode::Continuous), 1),
            Activity::TimeDriven,
            "a camera page is driven by arriving frames, not by the light"
        );
        assert_eq!(
            panel.borrow().color,
            Rgb(0, 0, 0),
            "the camera page's ground is the page's black, not the light's colour"
        );
        assert_eq!(panel.borrow().backlight, 100);
        assert_eq!(r.step(2), Activity::TimeDriven);
        assert_eq!(
            panel.borrow().camera_paints,
            1,
            "the panel pulls the next frame"
        );
    }

    #[test]
    fn the_camera_cadence_is_handed_to_the_surface_rather_than_read_there() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_collecting(Page::Camera, 200), 0);
        r.step(1_000);
        assert_eq!(
            panel.borrow().camera_period,
            200,
            "the claim's cadence is the policy the page asked for"
        );
        assert_eq!(
            panel.borrow().camera_releases,
            0,
            "a collecting demand keeps the buffer"
        );
    }

    #[test]
    fn the_camera_is_released_when_the_demand_stops_collecting() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_collecting(Page::Camera, 0), 0);
        r.on_appearance(panel_on(Page::Ambient, Mode::Continuous), 1);
        assert_eq!(
            panel.borrow().camera_releases,
            1,
            "the renderer hands the buffer back; the surface only does as it is told"
        );
        r.on_appearance(panel_on(Page::Ambient, Mode::Continuous), 2);
        assert_eq!(
            panel.borrow().camera_releases,
            2,
            "and it asks every snapshot, leaving the surface's own guard to make it a no-op"
        );
    }

    #[test]
    fn leaving_the_page_that_grounds_the_panel_puts_the_light_back() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_on(Page::Network, Mode::Continuous), 0);
        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(panel.borrow().color, Rgb(0, 0, 0));

        assert_eq!(
            r.on_appearance(panel_on(Page::Ambient, Mode::Continuous), 100),
            Activity::TimeDriven,
            "the breathing the page was hiding resumes"
        );
        let LightAppearance::Breathing(breath) = light_appearance(LightState::boot()) else {
            panic!("the boot light breathes");
        };
        assert_eq!(panel.borrow().color, breath.rgb(100));
    }

    #[test]
    fn the_camera_is_attached_only_to_a_page_surface() {
        let (r, panel) = renderer();
        assert!(!panel.borrow().camera);
        let r = r.with_camera(Box::new(NullCamera));
        assert!(
            panel.borrow().camera,
            "a panel is what a camera can write into"
        );
        assert_eq!(
            r.consumers(),
            &[Consumer::Surface(0), Consumer::Diagnostics]
        );
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

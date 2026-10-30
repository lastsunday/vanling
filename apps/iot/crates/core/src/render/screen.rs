//! The panel's renderer: pages over the colour, the backlight, and the camera feed.

use alloc::boxed::Box;

use crate::demand::{DemandKey, Mode};
use crate::drivers::camera::FrameSource;
use crate::drivers::surface::PageSurface;
use crate::render::colour::ColourDriver;
use crate::render::{Activity, LightAppearance, Renderer, Slot, SlotAppearance};
use crate::state::{DeviceManager, DisplayPage};

pub struct ScreenRenderer {
    instance: u8,
    surface: Box<dyn PageSurface>,
    slots: [Slot; 2],
    colour: ColourDriver,
    appearance: Option<LightAppearance>,
    page: Option<DisplayPage>,
}

impl ScreenRenderer {
    pub fn new(instance: u8, surface: Box<dyn PageSurface>) -> Self {
        Self {
            instance,
            surface,
            slots: [Slot::Surface(instance), Slot::Diagnostics],
            colour: ColourDriver::new(),
            appearance: None,
            page: None,
        }
    }

    pub fn with_camera(mut self, camera: Box<dyn FrameSource>) -> Self {
        self.surface.attach_camera(camera);
        self.page = None;
        self
    }
}

impl ScreenRenderer {
    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.appearance else {
            return;
        };
        let step = self.surface.repaint_step();
        if let Some(ground) = self.page.and_then(|page| page.ground()) {
            self.colour.ground(&mut *self.surface, step, ground);
            self.surface.set_backlight(100);
            return;
        }
        let lit = self.colour.show(&mut *self.surface, step, declared, now_ms);
        self.surface
            .set_backlight(lit.backlight(now_ms, DeviceManager::BACKLIGHT_FLOOR_PCT));
    }

    fn on_camera_page(&self) -> bool {
        self.page.is_some_and(|page| page.draws_frames())
    }
}

impl Renderer for ScreenRenderer {
    fn slots(&self) -> &[Slot] {
        &self.slots
    }

    fn on_appearance(&mut self, appearance: SlotAppearance, now_ms: u32) -> Activity {
        match appearance {
            SlotAppearance::Diagnostics(diagnostics) => {
                let page_changed = self.page != Some(diagnostics.page);
                self.surface.consume(&diagnostics);
                self.page = Some(diagnostics.page);
                self.colour
                    .set_display(diagnostics.demand.claim(DemandKey::DISPLAY).mode);
                match self.colour.display() {
                    Mode::Off => {
                        self.surface.set_backlight_ceiling(0);
                        self.colour.stop();
                        return Activity::Idle;
                    }
                    Mode::OnChange => self
                        .surface
                        .set_backlight_ceiling(DeviceManager::BACKLIGHT_FLOOR_PCT),
                    Mode::Continuous => self.surface.set_backlight_ceiling(100),
                }
                if page_changed {
                    self.colour.forget();
                    self.present(now_ms);
                }
                if self.on_camera_page() {
                    return Activity::TimeDriven;
                }
                breathing_activity(self.colour.breathing())
            }
            SlotAppearance::Surface {
                instance,
                appearance,
            } => {
                debug_assert_eq!(instance, self.instance);
                self.appearance = Some(appearance);
                self.present(now_ms);
                breathing_activity(self.colour.breathing())
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Activity {
        if self.on_camera_page() {
            self.surface.paint_camera(u64::from(now_ms));
            return Activity::TimeDriven;
        }
        let step = self.surface.repaint_step();
        match self.colour.step(&mut *self.surface, step, now_ms) {
            Some(lit) => {
                self.surface
                    .set_backlight(lit.backlight(now_ms, DeviceManager::BACKLIGHT_FLOOR_PCT));
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
    use crate::demand::{Claim, EffectiveDemand, Sensitivity};
    use crate::diagnostics::Diagnostics;
    use crate::drivers::camera::{CameraCounters, FrameAdvance, WindowGeometry};
    use crate::drivers::surface::RgbLight;
    use crate::paint::Rgb;
    use crate::render::light_appearance;
    use crate::state::LightState;
    use alloc::rc::Rc;
    use core::cell::RefCell;

    struct PanelState {
        fills: u32,
        color: Rgb,
        backlight: u8,
        ceiling: u8,
        camera_paints: u32,
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

    impl crate::diagnostics::DiagnosticsSink for FakeScreen {
        fn consume(&mut self, _diagnostics: &Diagnostics) {}
    }

    impl crate::drivers::camera::CameraTarget for FakeScreen {
        fn paint_camera(&mut self, _now_ms: u64) -> bool {
            self.0.borrow_mut().camera_paints += 1;
            true
        }

        fn attach_camera(&mut self, _camera: Box<dyn FrameSource>) {
            self.0.borrow_mut().camera = true;
        }
    }

    fn panel_on(page: DisplayPage, display: Mode) -> SlotAppearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(display, 0, Sensitivity::Primary),
        );
        SlotAppearance::Diagnostics(Diagnostics {
            demand,
            page,
            ..Diagnostics::default()
        })
    }

    fn panel_asked(display: Mode) -> SlotAppearance {
        panel_on(DisplayPage::Ambient, display)
    }

    fn ambient_breath(instance: u8) -> SlotAppearance {
        SlotAppearance::Surface {
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
    fn an_unlit_panel_ships_nothing_from_any_slot() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_asked(Mode::Off), 0);
        assert_eq!(panel.borrow().ceiling, 0, "and its backlight is off");
        let fills_before = panel.borrow().fills;

        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        r.step(1_100);
        r.on_appearance(
            SlotAppearance::Surface {
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
        assert_eq!(panel.borrow().ceiling, DeviceManager::BACKLIGHT_FLOOR_PCT);

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
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
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
            SlotAppearance::Surface {
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
        r.on_appearance(panel_on(DisplayPage::Ambient, Mode::Continuous), 0);
        r.on_appearance(
            SlotAppearance::Surface {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(10, 20, 30)),
            },
            0,
        );
        assert_eq!(panel.borrow().color, Rgb(10, 20, 30));

        assert_eq!(
            r.on_appearance(panel_on(DisplayPage::Camera, Mode::Continuous), 1),
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
    fn leaving_the_page_that_grounds_the_panel_puts_the_light_back() {
        let (mut r, panel) = renderer();
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(panel.borrow().color, Rgb(0, 0, 0));

        assert_eq!(
            r.on_appearance(panel_on(DisplayPage::Ambient, Mode::Continuous), 100),
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
        assert_eq!(r.slots(), &[Slot::Surface(0), Slot::Diagnostics]);
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

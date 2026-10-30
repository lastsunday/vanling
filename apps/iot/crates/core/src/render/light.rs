//! One surface's renderer: what the light shows, and when it has to be stepped.

use alloc::boxed::Box;

use crate::demand::{DemandKey, Mode};
use crate::diagnostics::DiagnosticsSink;
use crate::drivers::camera::{CameraTarget, FrameSource};
use crate::drivers::light::{Fill, Rgb, RgbLight, should_repaint};
use crate::render::{Activity, LightAppearance, Renderer, Slot, SlotAppearance};
use crate::state::{Breath, DeviceManager, DisplayPage};

pub struct LightRenderer<R: RgbLight + DiagnosticsSink> {
    instance: u8,
    light: R,
    slots: [Slot; 2],
    last: Option<Rgb>,
    breath: Option<Breath>,
    appearance: Option<LightAppearance>,
    page: Option<DisplayPage>,
    display: Mode,
}

impl<R: RgbLight + DiagnosticsSink + CameraTarget> LightRenderer<R> {
    pub fn new(instance: u8, light: R) -> Self {
        Self {
            instance,
            light,
            slots: [Slot::Surface(instance), Slot::Diagnostics],
            last: None,
            breath: None,
            appearance: None,
            page: None,
            display: Mode::Continuous,
        }
    }

    pub fn with_camera(mut self, camera: Box<dyn FrameSource>) -> Self {
        self.light.attach_camera(camera);
        self.page = None;
        self
    }

    fn drive(&mut self, fill: Fill, color: Rgb) {
        if !self.display.is_collecting() {
            self.last = None;
            return;
        }
        let step = self.light.repaint_step();
        let repaint = match self.last {
            None => true,
            Some(last) => should_repaint(last, color, step),
        };
        if repaint {
            self.last = Some(color);
            self.light.set_fill(fill, color);
        }
    }

    fn draw(&mut self, now_ms: u32, breath: Breath) {
        self.drive(Fill::Uniform, breath.rgb(now_ms));
        self.light
            .set_backlight(breath.backlight(now_ms, DeviceManager::BACKLIGHT_FLOOR_PCT));
    }

    fn page_ground(&self) -> Option<Rgb> {
        self.page.and_then(|page| self.light.page_ground(page))
    }

    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.appearance else {
            return;
        };
        match self.page_ground() {
            Some(ground) => {
                self.breath = None;
                self.drive(Fill::Uniform, ground);
                self.light.set_backlight(100);
            }
            None => match declared {
                LightAppearance::Off => {
                    self.breath = None;
                    self.drive(Fill::Uniform, Rgb(0, 0, 0));
                    self.light.set_backlight(0);
                }
                LightAppearance::Color(color) => {
                    self.breath = None;
                    self.drive(Fill::Uniform, color);
                    self.light.set_backlight(100);
                }
                LightAppearance::Breathing(breath) => {
                    self.breath = Some(breath);
                    self.draw(now_ms, breath);
                }
            },
        }
    }
}

impl<R: RgbLight + DiagnosticsSink + CameraTarget> Renderer for LightRenderer<R> {
    fn slots(&self) -> &[Slot] {
        &self.slots
    }

    fn on_appearance(&mut self, appearance: SlotAppearance, now_ms: u32) -> Activity {
        match appearance {
            SlotAppearance::Diagnostics(diagnostics) => {
                let page_changed = self.page != Some(diagnostics.page);
                self.light.consume(&diagnostics);
                self.page = Some(diagnostics.page);
                let was_collecting = self.display.is_collecting();
                self.display = diagnostics.demand.claim(DemandKey::DISPLAY).mode;
                if was_collecting != self.display.is_collecting() {
                    self.last = None;
                }
                match self.display {
                    Mode::Off => {
                        self.light.set_backlight_ceiling(0);
                        self.breath = None;
                        return Activity::Idle;
                    }
                    Mode::OnChange => {
                        self.light
                            .set_backlight_ceiling(DeviceManager::BACKLIGHT_FLOOR_PCT);
                    }
                    Mode::Continuous => self.light.set_backlight_ceiling(100),
                }
                if page_changed {
                    self.last = None;
                    self.present(now_ms);
                }
                if self.on_camera_page() {
                    return Activity::TimeDriven;
                }
                if self.breath.is_some() {
                    Activity::TimeDriven
                } else {
                    Activity::Idle
                }
            }
            SlotAppearance::Surface {
                instance,
                appearance,
            } => {
                debug_assert_eq!(instance, self.instance);
                self.appearance = Some(appearance);
                self.present(now_ms);
                if self.breath.is_some() {
                    Activity::TimeDriven
                } else {
                    Activity::Idle
                }
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Activity {
        if self.on_camera_page() {
            self.light.paint_camera(u64::from(now_ms));
            return Activity::TimeDriven;
        }
        match self.breath {
            Some(breath) => {
                self.draw(now_ms, breath);
                Activity::TimeDriven
            }
            None => Activity::Idle,
        }
    }
}

impl<R: RgbLight + DiagnosticsSink + CameraTarget> LightRenderer<R> {
    fn on_camera_page(&self) -> bool {
        self.page
            .is_some_and(|page| self.light.page_draws_frames(page))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::EffectiveDemand;
    use crate::diagnostics::Diagnostics;
    use crate::render::light_appearance;
    use crate::state::LightState;

    struct FakePanel {
        fills: u32,
        color: Rgb,
        backlight: u8,
        ceiling: u8,
        pages: bool,
    }

    impl FakePanel {
        fn new() -> Self {
            Self {
                fills: 0,
                color: Rgb(0, 0, 0),
                backlight: 100,
                ceiling: 100,
                pages: false,
            }
        }

        fn panel() -> Self {
            Self {
                pages: true,
                ..Self::new()
            }
        }
    }

    impl RgbLight for FakePanel {
        fn set_fill(&mut self, _fill: Fill, color: Rgb) {
            self.fills += 1;
            self.color = color;
        }

        fn set_backlight(&mut self, level_pct: u8) {
            self.backlight = level_pct.min(self.ceiling);
        }

        fn set_backlight_ceiling(&mut self, level_pct: u8) {
            self.ceiling = level_pct;
            self.backlight = self.backlight.min(level_pct);
        }

        fn page_ground(&self, page: DisplayPage) -> Option<Rgb> {
            self.pages.then(|| page.ground()).flatten()
        }

        fn page_draws_frames(&self, page: DisplayPage) -> bool {
            self.pages && page.draws_frames()
        }
    }

    impl DiagnosticsSink for FakePanel {
        fn consume(&mut self, _diagnostics: &Diagnostics) {}
    }

    impl CameraTarget for FakePanel {
        fn takes_frames(&self) -> bool {
            self.pages
        }

        fn paint_camera(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn attach_camera(&mut self, _camera: Box<dyn FrameSource>) {}
    }

    fn panel_asked(display: Mode) -> SlotAppearance {
        panel_on(DisplayPage::Ambient, display)
    }

    fn panel_on(page: DisplayPage, display: Mode) -> SlotAppearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            crate::demand::Claim::new(display, 0, crate::demand::Sensitivity::Primary),
        );
        SlotAppearance::Diagnostics(Diagnostics {
            demand,
            page,
            ..Diagnostics::default()
        })
    }

    fn ambient_breath(instance: u8) -> SlotAppearance {
        SlotAppearance::Surface {
            instance,
            appearance: light_appearance(LightState::boot()),
        }
    }

    fn renderer(panel: FakePanel) -> LightRenderer<FakePanel> {
        LightRenderer::new(0, panel)
    }

    #[test]
    fn a_lit_panel_keeps_breathing_on_a_page_that_never_settles() {
        let mut r = renderer(FakePanel::new());
        assert_eq!(
            r.on_appearance(panel_asked(Mode::Continuous), 0),
            Activity::Idle,
            "the first appearance only tells the panel what it is for"
        );
        assert_eq!(r.on_appearance(ambient_breath(0), 0), Activity::TimeDriven);
        r.step(1_000);
        r.step(1_100);
        assert!(r.light.fills > 0, "a lit panel keeps painting");
    }

    #[test]
    fn an_unlit_panel_ships_nothing_from_any_slot() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_asked(Mode::Off), 0);
        assert_eq!(r.light.ceiling, 0, "and its backlight is off");
        let fills_before = r.light.fills;

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
            r.light.fills, fills_before,
            "the breathing loop and the colour change both leave through the same gate"
        );
        assert_eq!(r.light.backlight, 0);
    }

    #[test]
    fn a_panel_relit_after_being_dark_paints_the_frame_the_darkness_swallowed() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_asked(Mode::Off), 0);
        r.on_appearance(ambient_breath(0), 0);
        r.step(1_000);
        assert_eq!(r.light.fills, 0);

        r.on_appearance(panel_asked(Mode::Continuous), 2_000);
        r.step(2_000);
        assert_eq!(
            r.light.fills, 1,
            "the first frame after relighting is the one that matters"
        );
        assert_eq!(r.light.ceiling, 100);
    }

    #[test]
    fn a_dimmed_panel_is_brightness_only_and_keeps_its_cadence() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_asked(Mode::OnChange), 0);
        assert_eq!(r.light.ceiling, DeviceManager::BACKLIGHT_FLOOR_PCT);

        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Activity::TimeDriven,
            "dimming is a brightness, not a rate"
        );
        r.step(1_000);
        assert!(r.light.fills > 0);
    }

    #[test]
    fn the_network_page_grounds_the_panel_black_and_still() {
        let mut r = renderer(FakePanel::panel());
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        assert_eq!(
            r.on_appearance(ambient_breath(0), 0),
            Activity::Idle,
            "a breathing light does not breathe through the page's ground"
        );
        assert_eq!(r.light.color, Rgb(0, 0, 0), "the ground is black");
        assert_eq!(
            r.light.backlight, 100,
            "and the setup screen is lit to read"
        );
        let fills = r.light.fills;
        assert_eq!(r.step(1_000), Activity::Idle);
        r.step(1_100);
        assert_eq!(
            r.light.fills, fills,
            "and the loop parks instead of repainting"
        );
        assert_eq!(
            r.light.backlight, 100,
            "the backlight holds instead of ramping"
        );

        r.on_appearance(
            SlotAppearance::Surface {
                instance: 0,
                appearance: LightAppearance::Off,
            },
            1_200,
        );
        assert_eq!(r.light.color, Rgb(0, 0, 0));
        assert_eq!(r.light.backlight, 100);
    }

    #[test]
    fn the_camera_page_grounds_the_panel_black() {
        let mut r = renderer(FakePanel::panel());
        r.on_appearance(panel_on(DisplayPage::Ambient, Mode::Continuous), 0);
        r.on_appearance(
            SlotAppearance::Surface {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(10, 20, 30)),
            },
            0,
        );
        assert_eq!(r.light.color, Rgb(10, 20, 30));

        assert_eq!(
            r.on_appearance(panel_on(DisplayPage::Camera, Mode::Continuous), 1),
            Activity::TimeDriven,
            "a camera page is driven by arriving frames, not by the light"
        );
        assert_eq!(
            r.light.color,
            Rgb(0, 0, 0),
            "the camera page's ground is the page's black, not the light's colour"
        );
        assert_eq!(r.light.backlight, 100);
    }

    #[test]
    fn leaving_the_page_that_grounds_the_panel_puts_the_light_back() {
        let mut r = renderer(FakePanel::panel());
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        r.on_appearance(ambient_breath(0), 0);
        assert_eq!(r.light.color, Rgb(0, 0, 0));

        assert_eq!(
            r.on_appearance(panel_on(DisplayPage::Ambient, Mode::Continuous), 100),
            Activity::TimeDriven,
            "the breathing the page was hiding resumes"
        );
        let LightAppearance::Breathing(breath) = light_appearance(LightState::boot()) else {
            panic!("the boot light breathes");
        };
        assert_eq!(r.light.color, breath.rgb(100));
    }

    #[test]
    fn a_surface_that_draws_no_page_keeps_showing_the_light() {
        let mut r = renderer(FakePanel::new());
        r.on_appearance(panel_on(DisplayPage::Network, Mode::Continuous), 0);
        assert_eq!(r.on_appearance(ambient_breath(0), 0), Activity::TimeDriven);
        assert!(
            r.light.fills > 0,
            "the light is still what this surface shows"
        );
    }
}

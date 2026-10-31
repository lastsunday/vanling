//! The lamp's renderer: a colour, and nothing a panel would add on top of it.

use alloc::boxed::Box;

use crate::demand::{DemandKey, Mode};
use crate::drivers::surface::RgbLight;
use crate::render::color::ColorDriver;
use crate::render::{Activity, Appearance, Consumer, LightAppearance, Renderer};

pub struct LampRenderer {
    instance: u8,
    light: Box<dyn RgbLight>,
    consumers: [Consumer; 2],
    color: ColorDriver,
    appearance: Option<LightAppearance>,
}

impl LampRenderer {
    pub fn new(instance: u8, light: Box<dyn RgbLight>) -> Self {
        Self {
            instance,
            light,
            consumers: [Consumer::Surface(instance), Consumer::Diagnostics],
            color: ColorDriver::new(),
            appearance: None,
        }
    }

    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.appearance else {
            return;
        };
        // A lamp has no repaint throttle of its own: any colour difference is worth writing.
        self.color.show(&mut *self.light, 0, declared, now_ms);
    }

    fn activity(&self) -> Activity {
        if self.color.breathing() {
            Activity::TimeDriven
        } else {
            Activity::Idle
        }
    }
}

impl Renderer for LampRenderer {
    fn consumers(&self) -> &[Consumer] {
        &self.consumers
    }

    fn on_appearance(&mut self, appearance: Appearance, now_ms: u32) -> Activity {
        match appearance {
            Appearance::Diagnostics(diagnostics) => {
                self.color
                    .set_display(diagnostics.demand.claim(DemandKey::DISPLAY).mode);
                if self.color.display() == Mode::Off {
                    self.color.stop();
                    return Activity::Idle;
                }
                self.activity()
            }
            Appearance::Surface {
                instance,
                appearance,
            } => {
                debug_assert_eq!(instance, self.instance);
                self.appearance = Some(appearance);
                self.present(now_ms);
                self.activity()
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Activity {
        match self.color.step(&mut *self.light, 0, now_ms) {
            Some(_) => Activity::TimeDriven,
            None => Activity::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::{Claim, EffectiveDemand, Mode, Priority};
    use crate::diagnostics::Diagnostics;
    use crate::paint::Rgb;
    use crate::render::light_appearance;
    use crate::state::{DisplayPage, LightState};
    use alloc::rc::Rc;
    use core::cell::RefCell;

    struct LampState {
        writes: u32,
        color: Rgb,
    }

    struct FakeLamp(Rc<RefCell<LampState>>);

    impl FakeLamp {
        fn new() -> Self {
            Self(Rc::new(RefCell::new(LampState {
                writes: 0,
                color: Rgb(0, 0, 0),
            })))
        }

        fn handle(&self) -> Rc<RefCell<LampState>> {
            Rc::clone(&self.0)
        }
    }

    impl RgbLight for FakeLamp {
        fn set_rgb(&mut self, color: Rgb) {
            let mut lamp = self.0.borrow_mut();
            lamp.writes += 1;
            lamp.color = color;
        }
    }

    fn display(display: Mode) -> Appearance {
        let mut demand = EffectiveDemand::default();
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(display, 0, Priority::Primary),
        );
        Appearance::Diagnostics(Diagnostics {
            demand,
            page: DisplayPage::Network,
            ..Diagnostics::default()
        })
    }

    fn breathing() -> Appearance {
        Appearance::Surface {
            instance: 0,
            appearance: light_appearance(LightState::boot()),
        }
    }

    fn color(color: Rgb) -> Appearance {
        Appearance::Surface {
            instance: 0,
            appearance: LightAppearance::Color(color),
        }
    }

    fn renderer() -> (LampRenderer, Rc<RefCell<LampState>>) {
        let lamp = FakeLamp::new();
        let state = lamp.handle();
        (LampRenderer::new(0, Box::new(lamp)), state)
    }

    #[test]
    fn a_breathing_lamp_keeps_stepping() {
        let (mut r, lamp) = renderer();
        assert_eq!(
            r.on_appearance(display(Mode::Continuous), 0),
            Activity::Idle
        );
        assert_eq!(r.on_appearance(breathing(), 0), Activity::TimeDriven);
        assert_eq!(lamp.borrow().writes, 1);
        assert_eq!(r.step(1_000), Activity::TimeDriven);
        assert_eq!(
            lamp.borrow().writes,
            2,
            "each step writes the frame it invented"
        );
    }

    #[test]
    fn a_solid_lamp_writes_the_colour_once() {
        let (mut r, lamp) = renderer();
        assert_eq!(r.on_appearance(color(Rgb(10, 20, 30)), 0), Activity::Idle);
        assert_eq!(lamp.borrow().color, Rgb(10, 20, 30));
        assert_eq!(r.on_appearance(color(Rgb(10, 20, 30)), 100), Activity::Idle);
        assert_eq!(lamp.borrow().writes, 1, "the same colour is not rewritten");
    }

    #[test]
    fn an_off_lamp_writes_black_once() {
        let (mut r, lamp) = renderer();
        let off = Appearance::Surface {
            instance: 0,
            appearance: LightAppearance::Off,
        };
        assert_eq!(r.on_appearance(off.clone(), 0), Activity::Idle);
        assert_eq!(lamp.borrow().color, Rgb(0, 0, 0));
        r.on_appearance(off, 100);
        assert_eq!(lamp.borrow().writes, 1);
    }

    #[test]
    fn a_lamp_under_a_dark_display_writes_nothing() {
        let (mut r, lamp) = renderer();
        r.on_appearance(display(Mode::Off), 0);
        assert_eq!(r.on_appearance(color(Rgb(255, 0, 0)), 0), Activity::Idle);
        assert_eq!(
            lamp.borrow().writes,
            0,
            "the gate belongs to the display demand"
        );
    }

    #[test]
    fn a_lamp_does_not_ground_itself_for_a_page() {
        let (mut r, lamp) = renderer();
        r.on_appearance(color(Rgb(10, 20, 30)), 0);
        r.on_appearance(display(Mode::Continuous), 0);
        assert_eq!(
            lamp.borrow().color,
            Rgb(10, 20, 30),
            "the network page is black on a panel, not on an LED"
        );
    }
}

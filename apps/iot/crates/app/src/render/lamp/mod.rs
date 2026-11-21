//! The lamp's renderer: a colour, and nothing a screen would add on top of it.

use alloc::boxed::Box;

use crate::render::engine::ColorOutput;
use crate::render::{Appearance, Drive, LightAppearance, Renderer, Topic};
use iot_core::demand::Mode;
use iot_core::drivers::surface::RgbLight;

pub struct LampRenderer {
    instance: u8,
    light: Box<dyn RgbLight>,
    topics: [Topic; 3],
    color: ColorOutput,
    declared: Option<LightAppearance>,
    /// The page wants a still light: the declared colour is held where the page found it.
    steady: bool,
}

impl LampRenderer {
    pub fn new(instance: u8, light: Box<dyn RgbLight>) -> Self {
        Self {
            instance,
            light,
            topics: [Topic::Surface(instance), Topic::Demand, Topic::Frame],
            color: ColorOutput::new(),
            declared: None,
            steady: false,
        }
    }

    fn present(&mut self, now_ms: u32) {
        let Some(declared) = self.declared else {
            return;
        };
        // A lamp has no repaint throttle of its own: any colour difference is worth writing.
        let now_ms = if self.steady { 0 } else { now_ms };
        self.color.show(&mut *self.light, 0, declared, now_ms);
    }
}

impl Renderer for LampRenderer {
    fn topics(&self) -> &[Topic] {
        &self.topics
    }

    fn on_appearance(&mut self, appearance: Appearance<'_>, now_ms: u32) -> Drive {
        match appearance {
            Appearance::Demand(mode) => {
                self.color.set_demand(mode);
                if mode == Mode::Off {
                    self.color.stop_breath();
                }
                self.color.drive()
            }
            Appearance::Surface { instance, light } => {
                debug_assert_eq!(instance, self.instance);
                self.declared = Some(light);
                self.present(now_ms);
                if self.steady {
                    Drive::Idle
                } else {
                    self.color.drive()
                }
            }
            Appearance::Frame(frame) => {
                self.steady = frame.steady;
                self.present(now_ms);
                if self.steady {
                    Drive::Idle
                } else {
                    self.color.drive()
                }
            }
        }
    }

    fn step(&mut self, now_ms: u32) -> Drive {
        if self.steady {
            return Drive::Idle;
        }
        match self.color.step(&mut *self.light, 0, now_ms) {
            Some(_) => Drive::TimeDriven,
            None => Drive::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::light_appearance;
    use crate::state::LightState;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use iot_core::light::Rgb;

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

    fn gate(mode: Mode) -> Appearance<'static> {
        Appearance::Demand(mode)
    }

    fn breathing() -> Appearance<'static> {
        Appearance::Surface {
            instance: 0,
            light: light_appearance(LightState::boot()),
        }
    }

    fn color(color: Rgb) -> Appearance<'static> {
        Appearance::Surface {
            instance: 0,
            light: LightAppearance::Color(color),
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
        assert_eq!(r.on_appearance(gate(Mode::Continuous), 0), Drive::Idle);
        assert_eq!(r.on_appearance(breathing(), 0), Drive::TimeDriven);
        assert_eq!(lamp.borrow().writes, 1);
        assert_eq!(r.step(1_000), Drive::TimeDriven);
        assert_eq!(
            lamp.borrow().writes,
            2,
            "each step writes the frame it invented"
        );
    }

    #[test]
    fn a_steady_frame_stops_the_breath() {
        let (mut r, lamp) = renderer();
        r.on_appearance(gate(Mode::Continuous), 0);
        let frame = crate::shared::snapshot::FrameSnapshot {
            steady: true,
            ..Default::default()
        };
        r.on_appearance(Appearance::Frame(&frame), 0);
        assert_eq!(r.on_appearance(breathing(), 0), Drive::Idle);
        let writes = lamp.borrow().writes;
        assert_eq!(r.step(1_000), Drive::Idle);
        assert_eq!(
            lamp.borrow().writes,
            writes,
            "a held light is not rewritten"
        );
    }

    #[test]
    fn a_solid_lamp_writes_the_colour_once() {
        let (mut r, lamp) = renderer();
        assert_eq!(r.on_appearance(color(Rgb(10, 20, 30)), 0), Drive::Idle);
        assert_eq!(lamp.borrow().color, Rgb(10, 20, 30));
        assert_eq!(r.on_appearance(color(Rgb(10, 20, 30)), 100), Drive::Idle);
        assert_eq!(lamp.borrow().writes, 1, "the same colour is not rewritten");
    }

    #[test]
    fn an_off_lamp_writes_black_once() {
        let (mut r, lamp) = renderer();
        let off = Appearance::Surface {
            instance: 0,
            light: LightAppearance::Off,
        };
        assert_eq!(r.on_appearance(off, 0), Drive::Idle);
        assert_eq!(lamp.borrow().color, Rgb(0, 0, 0));
        r.on_appearance(off, 100);
        assert_eq!(lamp.borrow().writes, 1);
    }

    #[test]
    fn a_lamp_under_a_dark_display_writes_nothing() {
        let (mut r, lamp) = renderer();
        r.on_appearance(gate(Mode::Off), 0);
        assert_eq!(r.on_appearance(color(Rgb(255, 0, 0)), 0), Drive::Idle);
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
        r.on_appearance(gate(Mode::Continuous), 0);
        assert_eq!(
            lamp.borrow().color,
            Rgb(10, 20, 30),
            "the network page is black on a screen, not on an LED"
        );
    }
}

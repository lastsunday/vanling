//! The colour half of a renderer: what gets painted, and when it has to be stepped.

use crate::demand::Mode;
use crate::drivers::surface::RgbLight;
use crate::paint::Rgb;
use crate::render::LightAppearance;
use crate::state::Breath;

/// What one painted appearance is: nothing, one colour, or a breath that keeps moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lit {
    Off,
    Solid,
    Breathing(Breath),
}

impl Lit {
    /// The panel's backlight for this appearance; lamps never ask.
    pub(super) fn backlight(self, now_ms: u32, floor_pct: u8) -> u8 {
        match self {
            Self::Off => 0,
            Self::Solid => 100,
            Self::Breathing(breath) => breath.backlight(now_ms, floor_pct),
        }
    }
}

/// Writes appearances to a surface: dedupes a slow one, steps a breathing one.
pub(super) struct ColorDriver {
    last: Option<Rgb>,
    breath: Option<Breath>,
    display: Mode,
}

impl ColorDriver {
    pub(super) const fn new() -> Self {
        Self {
            last: None,
            breath: None,
            display: Mode::Continuous,
        }
    }

    /// Follows the display demand. A mode that stops collecting forgets the last colour, so
    /// the first frame after it is a full one.
    pub(super) fn set_display(&mut self, mode: Mode) {
        if self.display.is_collecting() != mode.is_collecting() {
            self.last = None;
        }
        self.display = mode;
    }

    pub(super) fn breathing(&self) -> bool {
        self.breath.is_some()
    }

    pub(super) fn display(&self) -> Mode {
        self.display
    }

    /// Stops the breath without painting: what being dark means.
    pub(super) fn stop(&mut self) {
        self.breath = None;
    }

    /// Forgets the last colour, so the next appearance repaints even if it did not move.
    pub(super) fn forget(&mut self) {
        self.last = None;
    }

    pub(super) fn show<R: RgbLight + ?Sized>(
        &mut self,
        light: &mut R,
        step: u8,
        appearance: LightAppearance,
        now_ms: u32,
    ) -> Lit {
        match appearance {
            LightAppearance::Off => {
                self.breath = None;
                self.drive(light, step, Rgb(0, 0, 0));
                Lit::Off
            }
            LightAppearance::Color(color) => {
                self.breath = None;
                self.drive(light, step, color);
                Lit::Solid
            }
            LightAppearance::Breathing(breath) => {
                self.breath = Some(breath);
                self.drive(light, step, breath.rgb(now_ms));
                Lit::Breathing(breath)
            }
        }
    }

    /// A page's own ground: solid and still, whatever the light was doing.
    pub(super) fn ground<R: RgbLight + ?Sized>(&mut self, light: &mut R, step: u8, color: Rgb) {
        self.breath = None;
        self.drive(light, step, color);
    }

    /// Paints the next breathing frame, or `None` when nothing is breathing.
    pub(super) fn step<R: RgbLight + ?Sized>(
        &mut self,
        light: &mut R,
        step: u8,
        now_ms: u32,
    ) -> Option<Lit> {
        let breath = self.breath?;
        self.drive(light, step, breath.rgb(now_ms));
        Some(Lit::Breathing(breath))
    }

    fn drive<R: RgbLight + ?Sized>(&mut self, light: &mut R, step: u8, color: Rgb) {
        if !self.display.is_collecting() {
            self.last = None;
            return;
        }
        let repaint = match self.last {
            None => true,
            Some(last) => should_repaint(last, color, step),
        };
        if repaint {
            self.last = Some(color);
            light.set_rgb(color);
        }
    }
}

/// Whether `next` differs from `last` enough to repaint a slow refresh surface.
fn should_repaint(last: Rgb, next: Rgb, step: u8) -> bool {
    if last == next {
        return false;
    }
    let delta = |a: u8, b: u8| u8::abs_diff(a, b);
    delta(last.0, next.0) >= step || delta(last.1, next.1) >= step || delta(last.2, next.2) >= step
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_repaint_fires_when_color_moves_at_least_a_step() {
        assert!(!should_repaint(Rgb(10, 20, 30), Rgb(10, 20, 30), 1));
        assert!(!should_repaint(Rgb(10, 20, 30), Rgb(19, 21, 28), 12));
        assert!(!should_repaint(Rgb(10, 20, 30), Rgb(10, 31, 30), 12));
        assert!(!should_repaint(Rgb(10, 20, 30), Rgb(10, 20, 41), 12));
        assert!(should_repaint(Rgb(10, 20, 30), Rgb(22, 20, 30), 12));
        assert!(should_repaint(Rgb(10, 20, 30), Rgb(10, 32, 30), 12));
        assert!(should_repaint(Rgb(10, 20, 30), Rgb(10, 20, 42), 12));
        assert!(should_repaint(Rgb(0, 0, 0), Rgb(255, 255, 255), 12));
        assert!(should_repaint(Rgb(10, 20, 30), Rgb(10, 21, 30), 0));
        assert!(!should_repaint(Rgb(10, 21, 30), Rgb(10, 21, 30), 0));
    }
}

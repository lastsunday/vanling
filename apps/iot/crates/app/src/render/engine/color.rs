//! The colour half of a renderer: what gets painted, and when it has to be stepped.

use crate::render::LightAppearance;
use crate::shared::model::Breath;
use iot_core::demand::Mode;
use iot_core::drivers::surface::RgbLight;
use iot_core::light::{Rgb, Sample};

use super::renderer::Drive;

/// What one painted appearance is: nothing, one colour, or one frame of a breath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    Off,
    Solid,
    Breathing(Sample),
}

/// Writes appearances to a surface: dedupes a slow one, steps a breathing one.
pub(crate) struct ColorOutput {
    last: Option<Rgb>,
    breath: Option<Breath>,
    demand: Mode,
}

impl ColorOutput {
    pub(crate) const fn new() -> Self {
        Self {
            last: None,
            breath: None,
            demand: Mode::Continuous,
        }
    }

    /// Follows the display demand. A mode that stops collecting forgets the last colour, so
    /// the first frame after it is a full one.
    pub(crate) fn set_demand(&mut self, mode: Mode) {
        if self.demand.is_collecting() != mode.is_collecting() {
            self.last = None;
        }
        self.demand = mode;
    }

    pub(crate) fn demand(&self) -> Mode {
        self.demand
    }

    /// Whether the renderer has anything to step: a breath still moving.
    pub(crate) fn drive(&self) -> Drive {
        if self.breath.is_some() {
            Drive::TimeDriven
        } else {
            Drive::Idle
        }
    }

    /// Stops the breath without painting: what being dark means.
    pub(crate) fn stop_breath(&mut self) {
        self.breath = None;
    }

    /// Forgets the last colour, so the next appearance repaints even if it did not move.
    pub(crate) fn forget_color(&mut self) {
        self.last = None;
    }

    pub(crate) fn show<R: RgbLight + ?Sized>(
        &mut self,
        light: &mut R,
        repaint_step: u8,
        appearance: LightAppearance,
        now_ms: u32,
    ) -> Shown {
        match appearance {
            LightAppearance::Off => {
                self.breath = None;
                self.write(light, repaint_step, Rgb(0, 0, 0));
                Shown::Off
            }
            LightAppearance::Color(color) => {
                self.breath = None;
                self.write(light, repaint_step, color);
                Shown::Solid
            }
            LightAppearance::Breathing(breath) => {
                self.breath = Some(breath);
                let sample = breath.sample(now_ms);
                self.write(light, repaint_step, sample.color);
                Shown::Breathing(sample)
            }
        }
    }

    /// A page's own ground: solid and still, whatever the light was doing.
    pub(crate) fn ground<R: RgbLight + ?Sized>(
        &mut self,
        light: &mut R,
        repaint_step: u8,
        color: Rgb,
    ) {
        self.breath = None;
        self.write(light, repaint_step, color);
    }

    /// Paints the next breathing frame, or `None` when nothing is breathing.
    pub(crate) fn step<R: RgbLight + ?Sized>(
        &mut self,
        light: &mut R,
        repaint_step: u8,
        now_ms: u32,
    ) -> Option<Shown> {
        let sample = self.breath?.sample(now_ms);
        self.write(light, repaint_step, sample.color);
        Some(Shown::Breathing(sample))
    }

    fn write<R: RgbLight + ?Sized>(&mut self, light: &mut R, repaint_step: u8, color: Rgb) {
        if !self.demand.is_collecting() {
            self.last = None;
            return;
        }
        let repaint = match self.last {
            None => true,
            Some(last) => should_repaint(last, color, repaint_step),
        };
        if repaint {
            self.last = Some(color);
            light.set_rgb(color);
        }
    }
}

/// Whether `next` differs from `last` enough to repaint a slow refresh surface.
fn should_repaint(last: Rgb, next: Rgb, repaint_step: u8) -> bool {
    if last == next {
        return false;
    }
    let delta = |a: u8, b: u8| u8::abs_diff(a, b);
    delta(last.0, next.0) >= repaint_step
        || delta(last.1, next.1) >= repaint_step
        || delta(last.2, next.2) >= repaint_step
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

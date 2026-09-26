use crate::diagnostics::DiagnosticsSink;
use crate::drivers::input::PollEntry;
use crate::drivers::light::RgbLight;
use crate::drivers::motion::MotionCapabilities;
use alloc::vec::Vec;

/// A hardware board instance. Implemented per board in the bsp crate.
pub trait Board: Sized {}

/// Board exposing light channels (RGB light surfaces).
///
/// A channel may drive several pixels wired in the same chain (e.g. a WS2812
/// strip); a board with several distinct surfaces returns them all in wiring
/// order, each with its own instance the app binds to its own renderer. The
/// sink bound keeps panel surfaces able to overlay diagnostics while plain
/// strips ignore the snapshot.
pub trait HasLight: Board {
    /// Owned light surfaces; `'static` so they can live behind a boxed renderer
    /// in the render task for the board's lifetime.
    type Light: RgbLight + DiagnosticsSink + 'static;

    /// Take the board's light surfaces. Returns `None` when none are wired or
    /// they were already taken.
    fn take_lights(&mut self) -> Option<Vec<Self::Light>>;
}

/// Board providing the input sources for the input pipeline.
///
/// The board owns the wiring and the driver choice, so a touchscreen board
/// simply assembles different sources behind the same trait; the app consumes
/// the ready-made entries and never names a specific device.
pub trait HasInput: Board {
    /// Take the board's input sources. Returns `None` when no input is wired
    /// or it was already taken.
    fn take_input(&mut self) -> Option<Vec<PollEntry>>;
}

pub trait HasMotion: Board {
    fn take_motion(&mut self) -> Option<PollEntry>;

    /// What this board's motion stack can report, driver engines plus core
    /// classifier. Declared rather than switched: the data plane ships
    /// unconditionally, so a capability the stack lacks is simply never raised.
    /// A board with no accelerometer returns `MotionCapabilities::EMPTY`.
    fn motion_capabilities(&self) -> MotionCapabilities;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{BUTTON_SCAN_MS, Button, ButtonScanner, DoubleClickAggregator};
    use crate::drivers::light::{Fill, Rgb};
    use alloc::{boxed::Box, vec};

    struct FakeLight;

    impl RgbLight for FakeLight {
        fn set_fill(&mut self, _fill: Fill, _color: Rgb) {}
    }

    impl DiagnosticsSink for FakeLight {}

    struct FakeButton;

    impl Button for FakeButton {
        fn is_pressed(&self) -> bool {
            false
        }
    }

    struct FakeBoard {
        lights: Option<Vec<FakeLight>>,
        input: Option<Vec<PollEntry>>,
    }

    impl Board for FakeBoard {}

    impl HasLight for FakeBoard {
        type Light = FakeLight;

        fn take_lights(&mut self) -> Option<Vec<Self::Light>> {
            self.lights.take()
        }
    }

    impl HasInput for FakeBoard {
        fn take_input(&mut self) -> Option<Vec<PollEntry>> {
            self.input.take()
        }
    }

    #[test]
    fn has_light_delivers_every_surface_then_none() {
        let mut board = FakeBoard {
            lights: Some(vec![FakeLight, FakeLight]),
            input: None,
        };
        let lights = board.take_lights().expect("lights present");
        assert_eq!(lights.len(), 2, "both surfaces in wiring order");
        let mut only = lights.into_iter();
        only.next().unwrap().set_rgb(Rgb(1, 2, 3));
        only.next().unwrap().set_rgb(Rgb(3, 2, 1));
        assert!(board.take_lights().is_none(), "taken exactly once");
    }

    #[test]
    fn has_light_delivers_a_single_surface_once() {
        let mut board = FakeBoard {
            lights: Some(vec![FakeLight]),
            input: None,
        };
        let mut lights = board.take_lights().expect("lights present");
        assert_eq!(lights.len(), 1);
        lights.get_mut(0).unwrap().set_rgb(Rgb(1, 2, 3));
        assert!(board.take_lights().is_none(), "taken exactly once");
    }

    #[test]
    fn has_input_delivers_sources_then_none() {
        let mut board = FakeBoard {
            lights: None,
            input: Some(vec![PollEntry::new(
                0,
                Box::new(ButtonScanner::new(FakeButton)),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            )]),
        };
        let sources = board.take_input().expect("input present");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].cadence_ms(), BUTTON_SCAN_MS);
        assert!(board.take_input().is_none(), "taken exactly once");
    }
}

use crate::config::MotionConfig;
use crate::diagnostics::DiagnosticsSink;
use crate::drivers::audio::CaptureSink;
use crate::drivers::input::PollEntry;
use crate::drivers::light::RgbLight;
use crate::drivers::motion::MotionCapabilities;
use crate::drivers::network::Station;
use crate::drivers::playback::Speaker;
use crate::state::SurfaceRole;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// A hardware board instance.
pub trait Board: Sized {}

/// One surface as the board hands it over: the device and what the wiring says it is for.
///
/// The board is the only place that knows which channel the panel and the lamp sit on, so the
/// role travels with the surface instead of being asked back out of it. `Absent` has no handle,
/// which is why a wired slot can never be mistaken for an empty one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WiredSurface<S> {
    Pages(S),
    Lamp(S),
}

impl<S> WiredSurface<S> {
    pub const fn role(&self) -> SurfaceRole {
        match self {
            WiredSurface::Pages(_) => SurfaceRole::Pages,
            WiredSurface::Lamp(_) => SurfaceRole::Lamp,
        }
    }

    pub fn into_surface(self) -> S {
        match self {
            WiredSurface::Pages(surface) | WiredSurface::Lamp(surface) => surface,
        }
    }
}

/// Board exposing the wired output surfaces: colour, pages and camera frames.
pub trait HasSurface: Board {
    type Surface: RgbLight + DiagnosticsSink + crate::drivers::camera::CameraTarget + 'static;

    fn take_surfaces(&mut self) -> Option<Vec<WiredSurface<Self::Surface>>>;
}

/// Board providing the input sources for the input pipeline.
pub trait HasInput: Board {
    fn take_input(&mut self) -> Option<Vec<PollEntry>>;
}

pub trait HasMotion: Board {
    fn take_motion(&mut self) -> Option<PollEntry>;

    fn motion_capabilities(&self) -> MotionCapabilities;

    fn motion_configurations(&self) -> &'static [MotionConfig] {
        &[]
    }
}

/// Board providing an audio capture source.
pub trait HasAudio: Board {
    fn take_audio(&mut self, sink: &'static dyn CaptureSink) -> Option<PollEntry>;
}

/// Board providing a camera.
pub trait HasCamera: Board {
    fn take_camera(&mut self) -> Option<Box<dyn crate::drivers::camera::FrameSource>>;
}

/// Board providing a speaker.
pub trait HasPlayback: Board {
    type Speaker: Speaker + 'static;

    fn take_playback(&mut self) -> Option<Box<Self::Speaker>>;
}

/// Board providing a Wi-Fi station.
pub trait HasWifi: Board {
    type Station: Station + 'static;

    fn take_wifi(&mut self) -> Option<Box<Self::Station>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::DemandKey;
    use crate::drivers::input::{BUTTON_SCAN_MS, Button, ButtonScanner, DoubleClickAggregator};
    use crate::drivers::light::{Fill, Rgb};
    use crate::drivers::playback::{Recovery, SpeakerFault, Tone};
    use alloc::{boxed::Box, vec};

    struct FakeLight;

    impl RgbLight for FakeLight {
        fn set_fill(&mut self, _fill: Fill, _color: Rgb) {}
    }

    impl DiagnosticsSink for FakeLight {}

    impl crate::drivers::camera::CameraTarget for FakeLight {
        fn paint_camera(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn attach_camera(&mut self, _camera: Box<dyn crate::drivers::camera::FrameSource>) {}
    }

    struct FakeButton;

    impl Button for FakeButton {
        fn is_pressed(&self) -> bool {
            false
        }
    }

    struct FakeSpeaker;

    impl Speaker for FakeSpeaker {
        fn play(
            &mut self,
            _source: Box<dyn crate::drivers::playback::SampleSource>,
        ) -> Result<(), SpeakerFault> {
            Ok(())
        }

        fn set_muted(&mut self, _muted: bool) -> Result<(), SpeakerFault> {
            Ok(())
        }

        fn feed(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn recover(&mut self) -> Option<Recovery> {
            None
        }
    }

    struct FakeBoard {
        lights: Option<Vec<WiredSurface<FakeLight>>>,
        input: Option<Vec<PollEntry>>,
        playback: Option<FakeSpeaker>,
    }

    impl Board for FakeBoard {}

    impl HasSurface for FakeBoard {
        type Surface = FakeLight;

        fn take_surfaces(&mut self) -> Option<Vec<WiredSurface<Self::Surface>>> {
            self.lights.take()
        }
    }

    impl HasInput for FakeBoard {
        fn take_input(&mut self) -> Option<Vec<PollEntry>> {
            self.input.take()
        }
    }

    impl HasPlayback for FakeBoard {
        type Speaker = FakeSpeaker;

        fn take_playback(&mut self) -> Option<Box<Self::Speaker>> {
            self.playback.take().map(Box::new)
        }
    }

    #[test]
    fn has_surface_delivers_every_surface_with_its_role_then_none() {
        let mut board = FakeBoard {
            lights: Some(vec![
                WiredSurface::Pages(FakeLight),
                WiredSurface::Lamp(FakeLight),
            ]),
            input: None,
            playback: None,
        };
        let wired = board.take_surfaces().expect("surfaces present");
        assert_eq!(wired.len(), 2, "both surfaces in wiring order");
        assert_eq!(
            wired.iter().map(WiredSurface::role).collect::<Vec<_>>(),
            vec![SurfaceRole::Pages, SurfaceRole::Lamp],
            "each surface carries the role the board gave it"
        );
        let mut only = wired.into_iter();
        only.next().unwrap().into_surface().set_rgb(Rgb(1, 2, 3));
        only.next().unwrap().into_surface().set_rgb(Rgb(3, 2, 1));
        assert!(board.take_surfaces().is_none(), "taken exactly once");
    }

    #[test]
    fn has_surface_delivers_a_single_surface_once() {
        let mut board = FakeBoard {
            lights: Some(vec![WiredSurface::Lamp(FakeLight)]),
            input: None,
            playback: None,
        };
        let mut wired = board.take_surfaces().expect("surfaces present");
        assert_eq!(wired.len(), 1);
        assert_eq!(wired[0].role(), SurfaceRole::Lamp);
        wired.pop().unwrap().into_surface().set_rgb(Rgb(1, 2, 3));
        assert!(board.take_surfaces().is_none(), "taken exactly once");
    }

    #[test]
    fn has_input_delivers_sources_then_none() {
        let mut board = FakeBoard {
            lights: None,
            playback: None,
            input: Some(vec![PollEntry::new(
                0,
                DemandKey::INTERACTION,
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

    #[test]
    fn has_playback_delivers_a_speaker_once_or_not_at_all() {
        let mut unwired = FakeBoard {
            lights: None,
            input: None,
            playback: None,
        };
        assert!(unwired.take_playback().is_none());

        let mut wired = FakeBoard {
            lights: None,
            input: None,
            playback: Some(FakeSpeaker),
        };
        let mut speaker = wired.take_playback().expect("speaker present");
        speaker
            .play(Box::new(Tone::playing(vec![])))
            .expect("the fake codec answers");
        assert!(
            wired.take_playback().is_none(),
            "taken exactly once, so the app cannot start a second ring"
        );
    }
}

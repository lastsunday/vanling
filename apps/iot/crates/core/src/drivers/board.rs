use crate::config::MotionConfig;
use crate::drivers::audio::CaptureSink;
use crate::drivers::input::PollEntry;
use crate::drivers::motion::MotionCapabilities;
use crate::drivers::network::Station;
use crate::drivers::playback::Speaker;
use crate::drivers::surface::WiredSurface;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// A hardware board instance.
pub trait Board: Sized {}

/// Board exposing the wired output surfaces, each carrying what it is for.
pub trait HasSurface: Board {
    fn take_surfaces(&mut self) -> Option<Vec<WiredSurface>>;
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
    use crate::drivers::input::{BUTTON_SCAN_MS, Button};
    use crate::drivers::playback::{Recovery, SpeakerFault, Tone};
    use crate::drivers::surface::{PageSurface, RgbLight};
    use crate::paint::Rgb;
    use crate::state::SurfaceRole;
    use alloc::{boxed::Box, vec};

    struct FakeLight;

    impl RgbLight for FakeLight {
        fn set_rgb(&mut self, _color: Rgb) {}
    }

    struct FakePage;

    impl RgbLight for FakePage {
        fn set_rgb(&mut self, _color: Rgb) {}
    }

    impl crate::diagnostics::DiagnosticsSink for FakePage {}

    impl crate::drivers::camera::CameraTarget for FakePage {
        fn paint_camera(&mut self, _now_ms: u64) -> bool {
            false
        }

        fn attach_camera(&mut self, _camera: Box<dyn crate::drivers::camera::FrameSource>) {}
    }

    impl PageSurface for FakePage {}

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
        lights: Option<Vec<WiredSurface>>,
        input: Option<Vec<PollEntry>>,
        playback: Option<FakeSpeaker>,
    }

    impl Board for FakeBoard {}

    impl HasSurface for FakeBoard {
        fn take_surfaces(&mut self) -> Option<Vec<WiredSurface>> {
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
                WiredSurface::Pages(Box::new(FakePage)),
                WiredSurface::Lamp(Box::new(FakeLight)),
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
        match only.next().unwrap() {
            WiredSurface::Pages(mut page) => page.set_rgb(Rgb(1, 2, 3)),
            WiredSurface::Lamp(_) => panic!("the panel is wired first"),
        }
        match only.next().unwrap() {
            WiredSurface::Lamp(mut lamp) => lamp.set_rgb(Rgb(3, 2, 1)),
            WiredSurface::Pages(_) => panic!("the lamp is wired second"),
        }
        assert!(board.take_surfaces().is_none(), "taken exactly once");
    }

    #[test]
    fn has_surface_delivers_a_single_surface_once() {
        let mut board = FakeBoard {
            lights: Some(vec![WiredSurface::Lamp(Box::new(FakeLight))]),
            input: None,
            playback: None,
        };
        let mut wired = board.take_surfaces().expect("surfaces present");
        assert_eq!(wired.len(), 1);
        assert_eq!(wired[0].role(), SurfaceRole::Lamp);
        match wired.pop().unwrap() {
            WiredSurface::Lamp(mut lamp) => lamp.set_rgb(Rgb(1, 2, 3)),
            WiredSurface::Pages(_) => panic!("the only surface is a lamp"),
        }
        assert!(board.take_surfaces().is_none(), "taken exactly once");
    }

    #[test]
    fn has_input_delivers_sources_then_none() {
        let mut board = FakeBoard {
            lights: None,
            playback: None,
            input: Some(vec![PollEntry::button(0, FakeButton)]),
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

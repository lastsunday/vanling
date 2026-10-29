use crate::config::MotionConfig;
use crate::diagnostics::DiagnosticsSink;
use crate::drivers::audio::CaptureSink;
use crate::drivers::input::PollEntry;
use crate::drivers::light::RgbLight;
use crate::drivers::motion::MotionCapabilities;
use crate::drivers::network::Station;
use crate::drivers::playback::Speaker;
use crate::state::Panel;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// A hardware board instance.
pub trait Board: Sized {}

/// Board exposing light channels (RGB light surfaces).
pub trait HasLight: Board {
    type Light: RgbLight + DiagnosticsSink + crate::drivers::camera::CameraTarget + 'static;

    fn take_lights(&mut self) -> Option<Vec<Self::Light>>;
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

pub trait HasPanel: Board {
    fn panel(&self) -> Panel;
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
        lights: Option<Vec<FakeLight>>,
        input: Option<Vec<PollEntry>>,
        playback: Option<FakeSpeaker>,
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

    impl HasPlayback for FakeBoard {
        type Speaker = FakeSpeaker;

        fn take_playback(&mut self) -> Option<Box<Self::Speaker>> {
            self.playback.take().map(Box::new)
        }
    }

    #[test]
    fn has_light_delivers_every_surface_then_none() {
        let mut board = FakeBoard {
            lights: Some(vec![FakeLight, FakeLight]),
            input: None,
            playback: None,
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
            playback: None,
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

use crate::diagnostics::DiagnosticsSink;
use crate::drivers::input::PollEntry;
use crate::drivers::light::RgbLight;
use crate::drivers::motion::MotionCapabilities;
use crate::drivers::playback::Speaker;
use alloc::boxed::Box;
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

/// Board providing an audio capture source.
///
/// Shaped like [`HasMotion`] because it is the same kind of wiring: the driver
/// lives in the bsp crate behind the shared input interface, so the app only
/// ever sees a ready-made [`PollEntry`] and never names a codec. A board with no
/// microphone returns `None` and the Audio page simply never appears.
pub trait HasAudio: Board {
    fn take_audio(&mut self) -> Option<PollEntry>;
}

/// Board providing a speaker.
///
/// Independent of [`HasAudio`]: a board can capture, play, both or neither, so
/// this is its own trait. Shaped like the audio one for the same reason — the
/// codec and the DMA ring live in the bsp crate, and the app only ever sees a
/// [`Speaker`] and never names a device. A board with no speaker returns `None`
/// and the Speaker page simply never appears.
pub trait HasPlayback: Board {
    /// Owned speaker; `'static` so it can sit in the app's playback task for the
    /// board's lifetime, and `Send` because that task is not necessarily on the
    /// same executor as the rest of the app — a board whose feed has a hard
    /// cadence runs it on its own higher-priority interrupt executor, and a
    /// driver that cannot cross executors could not be fed on one. Every transport
    /// a board wires a speaker to has to be `Send` for that, which the I2C-backed
    /// codecs are: their bus sits behind a `CriticalSectionRawMutex`, whose
    /// `RefCell` is only ever reached with interrupts masked.
    type Speaker: Speaker + 'static;

    /// Hands the speaker over boxed. The app awaits the input and render futures
    /// in one cooperative join and hands the playback future to whichever executor
    /// the board's feed cadence demands, so every arm of that join is part of the
    /// single task's stack frame — and that task's stack is a fixed few tens of
    /// kilobytes it cannot grow. A driver this size held by value would be
    /// carried down the whole frame on top of the render path's own call chain, so
    /// it lives on the heap instead: the app only ever holds a pointer to it.
    fn take_playback(&mut self) -> Option<Box<Self::Speaker>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{BUTTON_SCAN_MS, Button, ButtonScanner, DoubleClickAggregator};
    use crate::drivers::light::{Fill, Rgb};
    use crate::drivers::playback::{Recovery, Sound, SpeakerFault};
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

    /// A speaker that answers every call, and remembers the last one so a case
    /// can assert what the driver was actually asked to do.
    struct FakeSpeaker {
        last: Option<(Sound, bool)>,
    }

    impl FakeSpeaker {
        fn new() -> Self {
            Self { last: None }
        }
    }

    impl Speaker for FakeSpeaker {
        fn play(&mut self, sound: Sound) -> Result<(), SpeakerFault> {
            self.last = Some((sound, false));
            Ok(())
        }

        fn set_muted(&mut self, muted: bool) -> Result<(), SpeakerFault> {
            self.last = Some((Sound::Chime, muted));
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
        // Two different no-speaker shapes, and both have to read the same to
        // the app: a board that wired none, and one that already handed its
        // speaker to the playback task.
        let mut unwired = FakeBoard {
            lights: None,
            input: None,
            playback: None,
        };
        assert!(unwired.take_playback().is_none());

        let mut wired = FakeBoard {
            lights: None,
            input: None,
            playback: Some(FakeSpeaker::new()),
        };
        let mut speaker = wired.take_playback().expect("speaker present");
        speaker.play(Sound::Asset).expect("the fake codec answers");
        assert!(
            wired.take_playback().is_none(),
            "taken exactly once, so the app cannot start a second ring"
        );
    }
}

//! The fake board the smoke runs on: two surfaces, two buttons, and a recorder for
//! everything the real pipeline paints.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

use embassy_time::{Duration, Timer};
use iot_app::display::diagnostics::Diagnostics;
use iot_app::model::state::{DisplayPage, PlaybackPhase};
use iot_core::config::MotionConfig;
use iot_core::demand::{ClockSink, DemandKey, DemandKind, Speed, SpeedTable};
use iot_core::drivers::audio::{
    AudioEnvelope, AudioInput, AudioSample, AudioSource, CAPTURE_MS, COLUMNS_PER_POLL, CaptureSink,
    SAMPLES_PER_COLUMN, SampleStream, dbfs,
};
use iot_core::drivers::board::{
    Board as BoardTrait, HasAudio, HasCamera, HasInput, HasMotion, HasPlayback, HasSurface,
};
use iot_core::drivers::camera::{
    CameraCounters, CameraTarget, FrameAdvance, FrameOwner, FrameSource, WindowGeometry,
};
use iot_core::drivers::input::{Button, PassThrough, PollEntry};
use iot_core::drivers::motion::{
    MOTION_SCAN_MS, MotionCapabilities, MotionError, MotionEvents, MotionInput, MotionReading,
    MotionSample, MotionScale, MotionSource,
};
use iot_core::drivers::playback::{Recovery, SampleSource, Speaker, SpeakerFault};
use iot_core::drivers::surface::{PageSurface, RgbLight, WiredSurface};
use iot_core::paint::Rgb;

pub(super) static PAINTED: Mutex<Vec<(u8, Rgb)>> = Mutex::new(Vec::new());

pub(super) static CAMERA_PAINTED: AtomicUsize = AtomicUsize::new(0);
pub(super) static RELEASED: AtomicUsize = AtomicUsize::new(0);

pub(super) static PRESSED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
pub(super) static PAGES: Mutex<Vec<DisplayPage>> = Mutex::new(Vec::new());

pub(super) static AUDIO_POLLS: AtomicUsize = AtomicUsize::new(0);

pub(super) static AUDIO_COLUMNS: AtomicUsize = AtomicUsize::new(0);

pub(super) static NETWORK_APS: AtomicUsize = AtomicUsize::new(0);

pub(super) static NETWORK_TYPED: AtomicUsize = AtomicUsize::new(0);

pub(super) static NETWORK_SAVED: AtomicUsize = AtomicUsize::new(0);

pub(super) static AUDIO_RESTARTS: AtomicUsize = AtomicUsize::new(0);

pub(super) static AUDIO_PEAK_DB: AtomicIsize = AtomicIsize::new(isize::MIN);

pub(super) static AUDIO_DBA_LSB: AtomicUsize = AtomicUsize::new(0);

pub(super) static PLAYED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

pub(super) static MUTED: Mutex<Vec<bool>> = Mutex::new(Vec::new());

pub(super) static PLAYBACK_PLAYS: AtomicUsize = AtomicUsize::new(0);

pub(super) static PLAYBACK_DROPPED: AtomicUsize = AtomicUsize::new(0);

pub(super) static PLAYBACK_EVER_MUTED: AtomicBool = AtomicBool::new(false);

pub(super) static PLAYBACK_PHASES: Mutex<Vec<PlaybackPhase>> = Mutex::new(Vec::new());

pub(super) const HOST_LOUD: i16 = 8_000;

const HOST_POLL_SAMPLES: usize = SAMPLES_PER_COLUMN as usize * COLUMNS_PER_POLL as usize;

pub(super) const HOST_RESTARTS: u16 = 3;

// Half a period of the fake's 1 kHz tone (a 48-sample square at 48 kHz): its impulsive
// harmonics pass the A-weighting, so the meter climbs. A DC constant would be filtered to
// silence and prove nothing.
const HOST_TONE_HALF_PERIOD: usize = 24;

struct HostAudio {
    envelope: AudioEnvelope,
    stream: SampleStream,
    phase: usize,
}

impl AudioSource for HostAudio {
    fn sample(&mut self, now_ms: u64) -> AudioSample {
        let mut bytes = [0u8; HOST_POLL_SAMPLES * 2];
        for (i, pair) in bytes.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let sample = if (self.phase + i) % (2 * HOST_TONE_HALF_PERIOD) < HOST_TONE_HALF_PERIOD {
                HOST_LOUD
            } else {
                -HOST_LOUD
            };
            pair.copy_from_slice(&sample.to_le_bytes());
        }
        self.phase = (self.phase + HOST_POLL_SAMPLES) % (2 * HOST_TONE_HALF_PERIOD);
        self.envelope.push(&[HOST_LOUD; HOST_POLL_SAMPLES]);
        self.stream.push_bytes(&bytes);
        AUDIO_POLLS.fetch_add(1, Ordering::SeqCst);
        AudioSample {
            envelope: self.envelope,
            elapsed_ms: now_ms.min(u64::from(u32::MAX)) as u32,
            restarts: HOST_RESTARTS,
            dba_lsb: self.stream.dba_lsb(),
            fault: None,
        }
    }
}

struct HostMotion;

const HOST_SCALE: MotionScale = MotionScale::from_ranges(8_192, 64);

impl MotionSource for HostMotion {
    fn configure_for(
        &mut self,
        wanted: Option<MotionConfig>,
    ) -> Result<Option<MotionConfig>, MotionConfig> {
        Ok(wanted)
    }

    fn sample(&mut self, _now_ms: u64) -> Result<Option<MotionReading>, MotionError> {
        Ok(Some(MotionReading {
            sample: MotionSample::from_raw([1, 2, 3], [4, 5, 6], HOST_SCALE, 3, 0),
            hardware: MotionEvents::new(),
        }))
    }
}

const HOST_FEEDS: usize = 220;

pub(super) struct HostSpeaker {
    feeds_left: usize,
}

impl Speaker for HostSpeaker {
    fn play(&mut self, mut source: Box<dyn SampleSource>) -> Result<(), SpeakerFault> {
        let mut buf = [0_i16; 256];
        let mut frames = 0;
        while !source.done() {
            let produced = source.fill(&mut buf);
            if produced == 0 {
                break;
            }
            frames += produced;
        }
        PLAYED.lock().unwrap().push(frames);
        self.feeds_left = HOST_FEEDS;
        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), SpeakerFault> {
        MUTED.lock().unwrap().push(muted);
        Ok(())
    }

    fn feed(&mut self, _now_ms: u64) -> bool {
        if self.feeds_left == 0 {
            return false;
        }
        self.feeds_left -= 1;
        true
    }

    fn recover(&mut self) -> Option<Recovery> {
        None
    }
}

struct HostButton(u8);

impl Button for HostButton {
    fn is_pressed(&self) -> bool {
        PRESSED[self.0 as usize].load(Ordering::SeqCst)
    }
}

struct HostLight {
    instance: u8,
    camera: Option<Box<dyn FrameSource + Send>>,
    // The panel's ownership rule, so a page change that would deadlock there is shown to survive here.
    owner: FrameOwner,
    page: Option<DisplayPage>,
}

impl HostLight {
    fn record(&mut self, color: Rgb) {
        if !self.owner.accepts_color(color) {
            return;
        }
        self.owner.painted(color);
        PAINTED.lock().unwrap().push((self.instance, color));
    }
}

impl RgbLight for HostLight {
    fn set_rgb(&mut self, color: Rgb) {
        self.record(color);
    }
}

impl PageSurface for HostLight {
    fn repaint(
        &mut self,
        _painter: &dyn iot_core::drivers::surface::PanelPainter,
        snapshot: &dyn core::any::Any,
        _force: bool,
    ) -> bool {
        let Some(diagnostics) = snapshot.downcast_ref::<Diagnostics>() else {
            return false;
        };
        self.consume(diagnostics);
        false
    }
}

struct HostCamera {
    frames: u32,
}

impl FrameSource for HostCamera {
    fn advance(&mut self, frame: &mut [u8], _now_ms: u64) -> FrameAdvance {
        self.frames = self.frames.saturating_add(1);
        frame.fill(0x5a);
        FrameAdvance::Fresh
    }

    fn pause(&mut self) {}

    fn resume(&mut self, _frame: &mut [u8]) {}

    fn counters(&self) -> CameraCounters {
        CameraCounters {
            frames: self.frames,
            finished: 40,
            ..CameraCounters::ZERO
        }
    }

    fn geometry(&mut self) -> Option<WindowGeometry> {
        Some(WindowGeometry {
            out_width: 320,
            out_height: 240,
            win_width: 1616,
            win_height: 1208,
            subsample: 0x55,
            scalar: 0,
            ..WindowGeometry::default()
        })
    }
}

impl CameraTarget for HostLight {
    fn paint_camera(
        &mut self,
        now_ms: u64,
        _period_ms: u32,
        _painter: &dyn iot_core::drivers::surface::PanelPainter,
        _snapshot: &dyn core::any::Any,
    ) -> bool {
        if !self.owner.is_camera() {
            let Some(camera) = self.camera.as_mut() else {
                return false;
            };
            camera.resume(&mut []);
            self.owner.taken();
        }
        let Some(camera) = self.camera.as_mut() else {
            return false;
        };
        let painted = camera.advance(&mut [0u8; 4], now_ms) == FrameAdvance::Fresh;
        if painted {
            CAMERA_PAINTED.fetch_add(1, Ordering::SeqCst);
        }
        painted
    }

    fn attach_camera(&mut self, camera: Box<dyn FrameSource + Send>) {
        self.camera = Some(camera);
    }

    fn release_camera(&mut self) {
        if self.owner.is_camera() {
            if let Some(camera) = self.camera.as_mut() {
                camera.pause();
            }
            self.owner.released();
            RELEASED.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl HostLight {
    fn consume(&mut self, diagnostics: &Diagnostics) {
        self.page = Some(diagnostics.page);
        PAGES.lock().unwrap().push(diagnostics.page);
        AUDIO_COLUMNS.fetch_max(
            usize::from(diagnostics.audio.envelope.committed()),
            Ordering::SeqCst,
        );
        AUDIO_RESTARTS.fetch_max(usize::from(diagnostics.audio.restarts), Ordering::SeqCst);
        AUDIO_PEAK_DB.fetch_max(
            isize::from(dbfs(diagnostics.audio.envelope.loudest())),
            Ordering::SeqCst,
        );
        AUDIO_DBA_LSB.fetch_max(usize::from(diagnostics.audio.dba_lsb), Ordering::SeqCst);
        NETWORK_APS.fetch_max(
            diagnostics.aps.map_or(0, |list| list.len()),
            Ordering::SeqCst,
        );
        NETWORK_TYPED.fetch_max(
            diagnostics.draft.entry.map_or(0, |entry| entry.len()),
            Ordering::SeqCst,
        );
        NETWORK_SAVED.fetch_max(
            diagnostics.aps.map_or(0, |list| {
                (0..list.len())
                    .filter(|&index| list.is_saved(index))
                    .count()
            }),
            Ordering::SeqCst,
        );
        PLAYBACK_PLAYS.fetch_max(usize::from(diagnostics.playback.plays), Ordering::SeqCst);
        PLAYBACK_DROPPED.fetch_max(usize::from(diagnostics.playback.dropped), Ordering::SeqCst);
        if diagnostics.playback.muted {
            PLAYBACK_EVER_MUTED.store(true, Ordering::SeqCst);
        }
        PLAYBACK_PHASES
            .lock()
            .unwrap()
            .push(diagnostics.playback.phase);
    }
}

pub(super) struct HostBoard {
    lights: Option<Vec<HostLight>>,
    input: Option<Vec<PollEntry>>,
    motion: Option<PollEntry>,
    audio: Option<HostAudio>,
    playback: Option<HostSpeaker>,
    camera: Option<Box<dyn FrameSource + Send>>,
}

impl HostBoard {
    pub(super) fn new() -> Self {
        Self {
            lights: Some(vec![
                HostLight {
                    instance: 0,
                    camera: Some(Box::new(HostCamera { frames: 0 })),
                    owner: FrameOwner::NeedsPaint,
                    page: None,
                },
                HostLight {
                    instance: 1,
                    camera: None,
                    owner: FrameOwner::NeedsPaint,
                    page: None,
                },
            ]),
            input: Some(vec![
                PollEntry::button(0, HostButton(0)),
                PollEntry::button(1, HostButton(1)),
            ]),
            motion: Some(PollEntry::new(
                2,
                DemandKey::MOTION,
                Box::new(MotionInput::new(HostMotion)),
                Box::new(PassThrough),
                MOTION_SCAN_MS,
            )),
            audio: Some(HostAudio {
                envelope: AudioEnvelope::ZERO,
                stream: SampleStream::new(),
                phase: 0,
            }),
            playback: Some(HostSpeaker { feeds_left: 0 }),
            camera: Some(Box::new(HostCamera { frames: 0 })),
        }
    }
}

impl BoardTrait for HostBoard {}

impl HasSurface for HostBoard {
    fn take_surfaces(&mut self) -> Option<Vec<WiredSurface>> {
        self.lights.take().map(|lights| {
            lights
                .into_iter()
                .map(|light| WiredSurface::Pages(Box::new(light)))
                .collect()
        })
    }
}

impl HasInput for HostBoard {
    fn take_input(&mut self) -> Option<Vec<PollEntry>> {
        self.input.take()
    }
}

impl HasMotion for HostBoard {
    fn take_motion(&mut self) -> Option<PollEntry> {
        self.motion.take()
    }

    fn motion_capabilities(&self) -> MotionCapabilities {
        MotionCapabilities::TELEMETRY
    }
}

impl HasAudio for HostBoard {
    fn take_audio(&mut self, sink: &'static dyn CaptureSink) -> Option<PollEntry> {
        self.audio.take().map(|source| {
            PollEntry::new(
                3,
                DemandKey::AUDIO,
                Box::new(AudioInput::new(source, sink)),
                Box::new(PassThrough),
                CAPTURE_MS,
            )
        })
    }
}

impl HasPlayback for HostBoard {
    type Speaker = HostSpeaker;

    fn take_playback(&mut self) -> Option<Box<Self::Speaker>> {
        self.playback.take().map(Box::new)
    }
}

impl HasCamera for HostBoard {
    fn take_camera(&mut self) -> Option<Box<dyn FrameSource + Send>> {
        self.camera.take()
    }
}

pub(super) async fn triple_click() {
    for _ in 0..3 {
        PRESSED[0].store(true, Ordering::SeqCst);
        Timer::after(Duration::from_millis(80)).await;
        PRESSED[0].store(false, Ordering::SeqCst);
        Timer::after(Duration::from_millis(50)).await;
    }
}

pub(super) async fn click(button: usize) {
    PRESSED[button].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(120)).await;
    PRESSED[button].store(false, Ordering::SeqCst);
}

pub(super) fn painted(instance: u8, color: Rgb) -> bool {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .any(|&(i, c)| i == instance && c == color)
}

pub(super) fn colors_on(instance: u8) -> Vec<Rgb> {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .filter(|&&(i, _)| i == instance)
        .map(|&(_, c)| c)
        .collect()
}

pub(super) fn paints_on(instance: u8) -> usize {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .filter(|&&(i, _)| i == instance)
        .count()
}

pub(super) fn page_seen(page: DisplayPage) -> bool {
    PAGES.lock().unwrap().contains(&page)
}

pub(super) fn phase_playing() -> bool {
    PLAYBACK_PHASES
        .lock()
        .unwrap()
        .contains(&PlaybackPhase::Playing)
}

pub(super) fn phase_idle_after_playing() -> bool {
    let phases = PLAYBACK_PHASES.lock().unwrap();
    let mut playing = false;
    for &phase in phases.iter() {
        match phase {
            PlaybackPhase::Playing => playing = true,
            PlaybackPhase::Idle if playing => return true,
            PlaybackPhase::Idle => {}
        }
    }
    false
}

pub(super) struct Unmeasured;

impl SpeedTable for Unmeasured {
    fn speed_for(_field: DemandKind) -> Speed {
        Speed::Fast
    }
}

pub(super) struct HostClock;

impl ClockSink for HostClock {
    type Table = Unmeasured;

    fn set_speed(&mut self, speed: Speed) -> Speed {
        speed
    }
}

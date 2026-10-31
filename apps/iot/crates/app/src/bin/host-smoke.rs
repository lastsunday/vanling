//! Host smoke for the app composition: boots [`iot_app::run`] on a fake board under the std
//! backend and asserts the real intent → render pipeline paints a click target on the
//! surface its source drives, with two surfaces and two buttons so multi-instance routing
//! is exercised.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use iot_app::workers::NET;
use iot_app::{Feed, Wiring, run};
use iot_core::config::MotionConfig;
use iot_core::demand::{ClockSink, DemandKey, DemandKind, Speed, SpeedTable};
use iot_core::diagnostics::{Diagnostics, DiagnosticsSink};
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
use iot_core::drivers::network::{
    AccessPoint, ApList, Credentials, JoinRequest, SavedNetworks, Ssid,
};
use iot_core::drivers::playback::{Recovery, SampleSource, Speaker, SpeakerFault};
use iot_core::drivers::surface::{PageSurface, RgbLight, WiredSurface};
use iot_core::intent::{BusinessIntent, Intent, NetworkIntent, PALETTE};
use iot_core::keyboard::Key;
use iot_core::paint::Rgb;
use iot_core::state::{Confirm, DisplayPage, NetworkCommand, PlaybackPhase};

static PAINTED: Mutex<Vec<(u8, Rgb)>> = Mutex::new(Vec::new());

static CAMERA_PAINTED: AtomicUsize = AtomicUsize::new(0);
static RELEASED: AtomicUsize = AtomicUsize::new(0);

static PRESSED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static PAGES: Mutex<Vec<DisplayPage>> = Mutex::new(Vec::new());

static AUDIO_POLLS: AtomicUsize = AtomicUsize::new(0);

static AUDIO_COLUMNS: AtomicUsize = AtomicUsize::new(0);

static NETWORK_APS: AtomicUsize = AtomicUsize::new(0);

static NETWORK_TYPED: AtomicUsize = AtomicUsize::new(0);

static NETWORK_SAVED: AtomicUsize = AtomicUsize::new(0);

static AUDIO_RESTARTS: AtomicUsize = AtomicUsize::new(0);

static AUDIO_PEAK_DB: AtomicIsize = AtomicIsize::new(isize::MIN);

static AUDIO_DBA_LSB: AtomicUsize = AtomicUsize::new(0);

static PLAYED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

static MUTED: Mutex<Vec<bool>> = Mutex::new(Vec::new());

static PLAYBACK_PLAYS: AtomicUsize = AtomicUsize::new(0);

static PLAYBACK_DROPPED: AtomicUsize = AtomicUsize::new(0);

static PLAYBACK_EVER_MUTED: AtomicBool = AtomicBool::new(false);

static PLAYBACK_PHASES: Mutex<Vec<PlaybackPhase>> = Mutex::new(Vec::new());

const HOST_LOUD: i16 = 8_000;

const HOST_POLL_SAMPLES: usize = SAMPLES_PER_COLUMN as usize * COLUMNS_PER_POLL as usize;

const HOST_RESTARTS: u16 = 3;

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

struct HostSpeaker {
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
    camera: Option<Box<dyn FrameSource>>,
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

impl PageSurface for HostLight {}

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
    fn paint_camera(&mut self, now_ms: u64) -> bool {
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

    fn attach_camera(&mut self, camera: Box<dyn FrameSource>) {
        self.camera = Some(camera);
    }
}

impl DiagnosticsSink for HostLight {
    fn consume(&mut self, diagnostics: &Diagnostics) {
        let page_changed = self.page != Some(diagnostics.page);
        if page_changed && self.owner.is_camera() {
            if let Some(camera) = self.camera.as_mut() {
                camera.pause();
            }
            self.owner.released();
            RELEASED.fetch_add(1, Ordering::SeqCst);
        }
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
            diagnostics.setup.entry.map_or(0, |entry| entry.len()),
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

struct HostBoard {
    lights: Option<Vec<HostLight>>,
    input: Option<Vec<PollEntry>>,
    motion: Option<PollEntry>,
    audio: Option<HostAudio>,
    playback: Option<HostSpeaker>,
    camera: Option<Box<dyn FrameSource>>,
}

impl HostBoard {
    fn new() -> Self {
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
    fn take_camera(&mut self) -> Option<Box<dyn FrameSource>> {
        self.camera.take()
    }
}

async fn triple_click() {
    for _ in 0..3 {
        PRESSED[0].store(true, Ordering::SeqCst);
        Timer::after(Duration::from_millis(80)).await;
        PRESSED[0].store(false, Ordering::SeqCst);
        Timer::after(Duration::from_millis(50)).await;
    }
}

async fn click(button: usize) {
    PRESSED[button].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(120)).await;
    PRESSED[button].store(false, Ordering::SeqCst);
}

fn painted(instance: u8, color: Rgb) -> bool {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .any(|&(i, c)| i == instance && c == color)
}

fn colors_on(instance: u8) -> Vec<Rgb> {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .filter(|&&(i, _)| i == instance)
        .map(|&(_, c)| c)
        .collect()
}

fn paints_on(instance: u8) -> usize {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .filter(|&&(i, _)| i == instance)
        .count()
}

fn page_seen(page: DisplayPage) -> bool {
    PAGES.lock().unwrap().contains(&page)
}

fn phase_playing() -> bool {
    PLAYBACK_PHASES
        .lock()
        .unwrap()
        .contains(&PlaybackPhase::Playing)
}

fn phase_idle_after_playing() -> bool {
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

struct Unmeasured;

impl SpeedTable for Unmeasured {
    fn speed_for(_field: DemandKind) -> Speed {
        Speed::Fast
    }
}

struct HostClock;

impl ClockSink for HostClock {
    type Table = Unmeasured;

    fn set_speed(&mut self, speed: Speed) -> Speed {
        speed
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut board = HostBoard::new();
    let surfaces = board
        .take_surfaces()
        .expect("the host board wires two surfaces");
    match select(
        run(
            board,
            Wiring {
                clock: HostClock,
                feed: Feed::Inline,
                network: true,
                surfaces,
            },
        ),
        scenario(),
    )
    .await
    {
        Either::First(never) => match never {},
        Either::Second(()) => {}
    }
}

/// Takes everything the coordinator handed the network worker. Nothing consumes these
/// commands on the host, so this is where a command is read rather than acted on.
fn take_commands() -> Vec<NetworkCommand> {
    let mut taken = Vec::new();
    while let Ok(command) = NET.commands.try_receive() {
        taken.push(command);
    }
    taken
}

async fn scenario() {
    Timer::after(Duration::from_millis(200)).await;
    assert!(
        !PAINTED.lock().unwrap().is_empty(),
        "boot breath did not paint any frame"
    );

    Timer::after(Duration::from_millis(3_200)).await;

    PRESSED[0].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(600)).await;
    PRESSED[0].store(false, Ordering::SeqCst);

    let mut attempts = 0;
    while !painted(0, PALETTE[0]) && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        painted(0, PALETTE[0]),
        "long press did not paint palette[0]"
    );

    let rest0 = colors_on(0);
    let count1 = paints_on(1);
    PRESSED[1].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(120)).await;
    PRESSED[1].store(false, Ordering::SeqCst);

    let mut attempts = 0;
    while paints_on(1) == count1 && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        paints_on(1) > count1,
        "button 1's click did not paint its own surface"
    );
    assert_eq!(
        colors_on(0),
        rest0,
        "button 1 must not touch surface 0's solid color"
    );

    triple_click().await;

    let mut attempts = 0;
    while !page_seen(DisplayPage::Attitude) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(DisplayPage::Attitude),
        "triple click did not reach the motion-enabled page"
    );

    triple_click().await;

    attempts = 0;
    while !page_seen(DisplayPage::Audio) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(DisplayPage::Audio),
        "the capture-enabled page was never reached"
    );
    assert!(
        AUDIO_POLLS.load(Ordering::SeqCst) > 0,
        "the capture source was never polled"
    );
    // The page can be on screen before the state layer commits a column: poll and commit are
    // separate passes, so wait for the column this assertion is about.
    attempts = 0;
    while AUDIO_COLUMNS.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        AUDIO_COLUMNS.load(Ordering::SeqCst) > 0,
        "no capture poll ever reached the state layer"
    );
    assert_eq!(
        AUDIO_RESTARTS.load(Ordering::SeqCst),
        usize::from(HOST_RESTARTS),
        "the capture's repair count did not reach the panel's diagnostics"
    );
    assert_eq!(
        AUDIO_PEAK_DB.load(Ordering::SeqCst),
        isize::from(dbfs(HOST_LOUD.unsigned_abs())),
        "the capture's level did not reach the panel as a decibel reading"
    );
    assert!(
        AUDIO_DBA_LSB.load(Ordering::SeqCst) > 0,
        "the capture's A-weighted level did not reach the panel's diagnostics"
    );

    triple_click().await;

    attempts = 0;
    while !page_seen(DisplayPage::Speaker) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(DisplayPage::Speaker),
        "the speaker-enabled page was never reached"
    );

    click(1).await;

    attempts = 0;
    while !phase_playing() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        phase_playing(),
        "a tap on the speaker page never started a sound"
    );
    attempts = 0;
    while PLAYED.lock().unwrap().is_empty() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    let played = PLAYED.lock().unwrap().clone();
    assert_eq!(
        played.len(),
        1,
        "a tap did not reach the driver exactly once"
    );
    assert!(
        played[0] > 0,
        "the driver was handed a source with no sound in it"
    );
    assert_eq!(
        PLAYBACK_PLAYS.load(Ordering::SeqCst),
        1,
        "the panel's play count did not follow the tap"
    );

    click(1).await;

    attempts = 0;
    while PLAYBACK_DROPPED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PLAYBACK_DROPPED.load(Ordering::SeqCst) > 0,
        "a tap over a sounding one was neither dropped nor played"
    );
    assert_eq!(
        PLAYED.lock().unwrap().len(),
        1,
        "a dropped tap still reached the driver"
    );

    attempts = 0;
    while !phase_idle_after_playing() && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        phase_idle_after_playing(),
        "the page never left Playing after the driver reported the sound done"
    );

    PRESSED[0].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(700)).await;
    PRESSED[0].store(false, Ordering::SeqCst);

    attempts = 0;
    while !PLAYBACK_EVER_MUTED.load(Ordering::SeqCst) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PLAYBACK_EVER_MUTED.load(Ordering::SeqCst),
        "a long press did not latch the speaker mute"
    );
    attempts = 0;
    while MUTED.lock().unwrap().is_empty() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        MUTED.lock().unwrap().as_slice(),
        &[true],
        "the mute did not reach the driver as a single latch"
    );

    triple_click().await;

    attempts = 0;
    while !page_seen(DisplayPage::Camera) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(DisplayPage::Camera),
        "the camera-enabled page was never reached"
    );

    attempts = 0;
    while CAMERA_PAINTED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        CAMERA_PAINTED.load(Ordering::SeqCst) > 0,
        "the camera page was reached but no frame ever reached the surface"
    );

    let painted_before = PAINTED.lock().unwrap().len();
    let released_before = RELEASED.load(Ordering::SeqCst);
    let pages_before = PAGES.lock().unwrap().len();
    triple_click().await;
    attempts = 0;
    while !PAGES.lock().unwrap()[pages_before..]
        .iter()
        .any(|page| *page != DisplayPage::Camera)
        && attempts < 100
    {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PAGES.lock().unwrap()[pages_before..]
            .iter()
            .any(|page| *page != DisplayPage::Camera),
        "the page cycle did not come back round from the camera page"
    );

    assert!(
        RELEASED.load(Ordering::SeqCst) > released_before,
        "the camera page was left without the buffer being handed back"
    );

    // Both halves of the deadlock: the release above makes a fill reachable again, and a refused
    // fill here is what the panel showed — the last camera frame left on screen for good.
    attempts = 0;
    while PAINTED.lock().unwrap().len() == painted_before && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PAINTED.lock().unwrap().len() > painted_before,
        "no fill reached the surface after leaving the camera page"
    );

    assert!(
        page_seen(DisplayPage::Network),
        "the link's page was never reached by the cycle"
    );

    let list = ApList::from_ranked(&[AccessPoint {
        ssid: Ssid::from_octets(b"neighbour"),
        bssid: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
        channel: 11,
        rssi: -55,
        secured: true,
    }]);
    NET.scan.publish(&list);

    attempts = 0;
    while NETWORK_APS.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_APS.load(Ordering::SeqCst),
        1,
        "a scan published through the link never reached the panel's snapshot"
    );

    let send = |intent: BusinessIntent| {
        let _ = iot_app::input::INTENT_BUS.try_send(Intent::Business(intent));
    };

    send(BusinessIntent::Network(NetworkIntent::Choose(
        list.get(0).expect("a row to pick"),
    )));
    Timer::after(Duration::from_millis(100)).await;

    let mut at_ms = 100;
    for key in [
        Key::Char(b'4'),
        Key::Char(b'4'),
        Key::Char(b'2'),
        Key::Shift,
        Key::Char(b'8'),
    ] {
        send(BusinessIntent::Network(NetworkIntent::Key { key, at_ms }));
        at_ms += 100;
    }
    attempts = 0;
    while NETWORK_TYPED.load(Ordering::SeqCst) < 3 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_TYPED.load(Ordering::SeqCst),
        3,
        "the keyboard's draft never reached the panel's snapshot"
    );

    send(BusinessIntent::Network(NetworkIntent::Key {
        key: Key::Connect,
        at_ms,
    }));
    attempts = 0;
    let mut handed = None;
    while handed.is_none() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        for command in take_commands() {
            if let NetworkCommand::Join(asked) = command {
                handed = Some(asked);
            }
        }
        attempts += 1;
    }
    let handed = handed.expect("connect never reached the network worker");
    let JoinRequest::Typed(handed) = handed else {
        panic!("connect handed over a row's name rather than the passphrase it was given");
    };
    assert_eq!(
        handed.ssid.as_bytes(),
        b"neighbour",
        "the credential did not carry the row that was picked"
    );
    assert_eq!(
        handed.password.as_bytes(),
        b"haT",
        "the credential did not carry what was typed"
    );

    send(BusinessIntent::Network(NetworkIntent::Key {
        key: Key::Cancel,
        at_ms,
    }));
    Timer::after(Duration::from_millis(100)).await;

    let mut store = SavedNetworks::empty();
    store.remember(
        &Credentials::from_text("neighbour", "haT").expect("a credential the radio takes"),
    );
    let mut marked = list;
    let marks = store.mask(&marked);
    marked.mark_saved(marks);
    NET.scan.publish(&marked);
    attempts = 0;
    while NETWORK_SAVED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_SAVED.load(Ordering::SeqCst),
        1,
        "a marked list published through the link never reached the panel's snapshot"
    );

    send(BusinessIntent::Network(NetworkIntent::ScanNow));
    attempts = 0;
    let mut scanned = false;
    while !scanned && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        for command in take_commands() {
            if matches!(command, NetworkCommand::ScanNow) {
                scanned = true;
            }
        }
        attempts += 1;
    }
    assert!(
        scanned,
        "the strip's request to look around never reached the network worker"
    );

    send(BusinessIntent::Network(NetworkIntent::AskForget(
        list.get(0).expect("a row to ask about"),
    )));
    Timer::after(Duration::from_millis(100)).await;
    send(BusinessIntent::Network(NetworkIntent::Answer(
        Confirm::Forget(list.get(0).expect("the row the question named")),
    )));
    attempts = 0;
    let mut forgotten = None;
    while forgotten.is_none() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        for command in take_commands() {
            if let NetworkCommand::Forget(ssid) = command {
                forgotten = Some(ssid);
            }
        }
        attempts += 1;
    }
    match forgotten {
        Some(ssid) => assert_eq!(
            ssid.as_bytes(),
            b"neighbour",
            "the forget named a network other than the row that was held"
        ),
        None => panic!("the hold's answer never reached the network worker"),
    }

    std::process::exit(0);
}

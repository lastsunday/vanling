//! Host smoke for the app composition: boots [`iot_app::run`] on a fake board under
//! the std backend and asserts the real intent → render pipeline paints a click
//! target on the surface its source drives, with two surfaces and two buttons so
//! multi-instance routing is exercised. The CI gate that needs no ESP hardware — see
//! docs/content/development/iot/emulation.md; run it with
//! `cargo run -p iot-app --bin host-smoke --no-default-features --features host`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use iot_app::render::{FORGET, JOIN, RESCAN, SCAN};
use iot_app::{Link, SpeakerRunner, run};
use iot_core::config::MotionConfig;
use iot_core::demand::{ClockSink, DemandField, PlaneKey, Speed, SpeedTable};
use iot_core::diagnostics::Diagnostics;
use iot_core::drivers::audio::{
    AudioEnvelope, AudioInput, AudioSample, AudioSource, CAPTURE_MS, COLUMNS_PER_POLL, CaptureSink,
    SAMPLES_PER_COLUMN, SampleStream, dbfs,
};
use iot_core::drivers::board::{
    Board as BoardTrait, HasAudio, HasCamera, HasInput, HasLight, HasMotion, HasPlayback,
};
use iot_core::drivers::camera::{
    CameraCounters, CameraTarget, FrameAdvance, FrameOwner, FrameSource, WindowGeometry,
};
use iot_core::drivers::input::{
    BUTTON_SCAN_MS, Button, ButtonScanner, DoubleClickAggregator, PassThrough, PollEntry,
};
use iot_core::drivers::light::{Fill, Rgb, RgbLight};
use iot_core::drivers::motion::{
    MOTION_SCAN_MS, MotionCapabilities, MotionError, MotionEvents, MotionInput, MotionReading,
    MotionSample, MotionScale, MotionSource,
};
use iot_core::drivers::network::{
    AccessPoint, ApList, Credentials, JoinRequest, SavedNetworks, Ssid,
};
use iot_core::drivers::playback::{Recovery, Sound, Speaker, SpeakerFault};
use iot_core::intent::{BusinessIntent, Intent, PALETTE};
use iot_core::keyboard::Key;
use iot_core::state::{DisplayPage, PlaybackPhase};

/// Recorded paint operations per surface, so the smoke can assert what `run`
/// drew and on which instance.
static PAINTED: Mutex<Vec<(u8, Fill, Rgb)>> = Mutex::new(Vec::new());

/// Camera frames the fake surface was asked to ship, so the smoke can prove the Camera page
/// reaches the surface rather than only appearing in the page cycle.
static CAMERA_PAINTED: AtomicUsize = AtomicUsize::new(0);
/// Page changes that handed the frame buffer back from the camera.
static RELEASED: AtomicUsize = AtomicUsize::new(0);

/// Firmware-visible press states, flipped by the smoke scenario: one per
/// button, matching the wiring-order sources the board assembles.
static PRESSED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static PAGES: Mutex<Vec<DisplayPage>> = Mutex::new(Vec::new());

/// Capture polls the fake source has served, so the smoke can prove the audio
/// entry is on the shared input tick.
static AUDIO_POLLS: AtomicUsize = AtomicUsize::new(0);

/// The most columns any real diagnostics snapshot reported, so a poll is proven
/// to have travelled the whole input → intent → state → sink path and not just
/// to have been served by the source.
static AUDIO_COLUMNS: AtomicUsize = AtomicUsize::new(0);

/// The most access points any diagnostics snapshot carried, so a list published on the link's
/// own plane is proven to have travelled the whole plane → state → sink path. Zero is what a
/// snapshot says before anything has scanned, so the maximum is the reading that matters.
static NETWORK_APS: AtomicUsize = AtomicUsize::new(0);

/// The most glyphs any snapshot's passphrase draft carried, so a key typed on the panel's
/// keyboard is proven to reach a snapshot — which is to say that it is drawn — rather than
/// stopping in the state layer.
static NETWORK_TYPED: AtomicUsize = AtomicUsize::new(0);

/// How many names the store's own report carried into the panel's snapshot.
static NETWORK_SAVED: AtomicUsize = AtomicUsize::new(0);

/// The highest repair count any sink saw, so the smoke can prove the count
/// reaches the panel rather than stopping at the state layer.
static AUDIO_RESTARTS: AtomicUsize = AtomicUsize::new(0);

/// Loudest level any poll delivered, in dBFS, so the smoke can prove the panel's
/// decibel readout is fed by a real measurement. Seeded at the smallest signed
/// value rather than zero: every reading a level meter reports is at or below
/// 0 dBFS, so a zero seed would leave the maximum at "no level at all" and the
/// smoke would pass on a capture that measured nothing.
static AUDIO_PEAK_DB: AtomicIsize = AtomicIsize::new(isize::MIN);

/// The loudest A-weighted level any sink saw, so the smoke can prove the corner
/// dB(A) readout is fed by the capture rather than hard-wired. Zero is exactly
/// what a broken wiring looks like after at least one tone poll, so a plain
/// "above zero" assert is a mean leash.
static AUDIO_DBA_LSB: AtomicUsize = AtomicUsize::new(0);

/// Sounds the fake speaker was asked to start, in order, so the smoke can
/// assert which sound a tap reached the driver with — and that a tap the state
/// dropped never arrived here.
static PLAYED: Mutex<Vec<Sound>> = Mutex::new(Vec::new());

/// Mute writes the fake speaker was asked to make, in order.
static MUTED: Mutex<Vec<bool>> = Mutex::new(Vec::new());

/// The highest play count the panel's diagnostics reported, so the smoke can
/// prove the tap reached the state layer as well as the driver.
static PLAYBACK_PLAYS: AtomicUsize = AtomicUsize::new(0);

/// The highest drop count any sink saw, so the smoke can prove a tap over a
/// sounding one is absorbed by the state rather than restarting the sound.
static PLAYBACK_DROPPED: AtomicUsize = AtomicUsize::new(0);

/// Whether any sink ever saw the panel muted, so the smoke can prove the long
/// press reached the state layer as well as the driver. Sticky, because the
/// assert is about a latch having happened rather than about its current value.
static PLAYBACK_EVER_MUTED: AtomicBool = AtomicBool::new(false);

/// Playback phases the sinks saw, in order, so the smoke can assert the whole
/// round trip — idle → playing → idle — and not merely a phase that once was
/// playing.
static PLAYBACK_PHASES: Mutex<Vec<PlaybackPhase>> = Mutex::new(Vec::new());

/// A loud constant capture. What matters to the smoke is that columns commit and
/// travel, not what they hold — only that the level it holds is a level the panel
/// can read back, so the metering path is proved to have survived the hop.
const HOST_LOUD: i16 = 8_000;

/// A whole poll of audio, which is what the device's capture hands over every
/// time. Sizing the fake by the column instead would have it delivering half of
/// what the real driver does and the smoke would not have noticed.
const HOST_POLL_SAMPLES: usize = SAMPLES_PER_COLUMN as usize * COLUMNS_PER_POLL as usize;

/// Repairs the fake capture reports, nonzero so the smoke can prove the panel's
/// `RST` readout is wired to a real field rather than hard-wired to zero.
const HOST_RESTARTS: u16 = 3;

/// Half a period of the fake's 1 kHz tone: a 48-sample square wave at the 48 kHz
/// capture rate, whose impulsive harmonics the A-weighting passes substantially,
/// so the meter climbs and the smoke can see the level travel. A DC constant
/// would be filtered to silence and prove nothing.
const HOST_TONE_HALF_PERIOD: usize = 24;

struct HostAudio {
    envelope: AudioEnvelope,
    stream: SampleStream,
    phase: usize,
}

impl AudioSource for HostAudio {
    fn sample(&mut self, now_ms: u64) -> AudioSample {
        let mut bytes = [0u8; HOST_POLL_SAMPLES * 2];
        for (i, pair) in bytes.chunks_exact_mut(2).enumerate() {
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

/// How many feeds a fake sound lasts. A real one is fed for as long as it
/// sounds; this is a fixed count, sized past a second so the smoke's second tap
/// reliably lands inside the sound it is meant to be dropped by, and no longer
/// than the smoke waits for the finish.
const HOST_FEEDS: usize = 220;

struct HostSpeaker {
    feeds_left: usize,
}

impl Speaker for HostSpeaker {
    fn play(&mut self, sound: Sound) -> Result<(), SpeakerFault> {
        PLAYED.lock().unwrap().push(sound);
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
    /// The same ownership rule the panel uses, so that a page change the panel would
    /// deadlock on is one this surface can be shown to survive.
    owner: FrameOwner,
    page: Option<DisplayPage>,
}

impl HostLight {
    /// Records a fill reaching the panel, unless the camera owns the buffer or this is the
    /// colour already there.
    fn record(&mut self, fill: Fill, color: Rgb) {
        if !self.owner.accepts_fill(fill, color) {
            return;
        }
        self.owner.painted(fill, color);
        PAINTED.lock().unwrap().push((self.instance, fill, color));
    }
}

impl RgbLight for HostLight {
    fn set_fill(&mut self, fill: Fill, color: Rgb) {
        self.record(fill, color);
    }

    fn set_backlight(&mut self, _level_pct: u8) {}
}

/// A camera with no sensor behind it: it reports frames without filling anything, which is
/// all the host needs to walk the page and see that the surface is asked to draw.
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

impl iot_core::diagnostics::DiagnosticsSink for HostLight {
    fn consume(&mut self, diagnostics: &Diagnostics) {
        // The panel hands the buffer back on a page change, before it decides whether to
        // repaint — otherwise a camera page, which is the one page with no colour to
        // repaint, is also the one page that can never release the camera.
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
                PollEntry::new(
                    0,
                    PlaneKey::INTERACTION,
                    Box::new(ButtonScanner::new(HostButton(0))),
                    Box::new(DoubleClickAggregator::new()),
                    BUTTON_SCAN_MS,
                ),
                PollEntry::new(
                    1,
                    PlaneKey::INTERACTION,
                    Box::new(ButtonScanner::new(HostButton(1))),
                    Box::new(DoubleClickAggregator::new()),
                    BUTTON_SCAN_MS,
                ),
            ]),
            motion: Some(PollEntry::new(
                2,
                PlaneKey::MOTION,
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

impl HasLight for HostBoard {
    type Light = HostLight;

    fn take_lights(&mut self) -> Option<Vec<Self::Light>> {
        self.lights.take()
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
                PlaneKey::AUDIO,
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

/// Three press/release pairs inside the multi-click window: the gesture that
/// turns the page.
async fn triple_click() {
    for _ in 0..3 {
        PRESSED[0].store(true, Ordering::SeqCst);
        Timer::after(Duration::from_millis(80)).await;
        PRESSED[0].store(false, Ordering::SeqCst);
        Timer::after(Duration::from_millis(50)).await;
    }
}

/// One press/release pair, long enough for the scanner to debounce.
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
        .any(|&(i, _, c)| i == instance && c == color)
}

/// The colors recorded on `instance`, in recorded order; the snapshots the
/// isolation assertion compares were recorded while that surface was at rest,
/// so equality means no cross-surface move leaked in.
fn colors_on(instance: u8) -> Vec<Rgb> {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .filter(|&&(i, _, _)| i == instance)
        .map(|&(_, _, c)| c)
        .collect()
}

fn paints_on(instance: u8) -> usize {
    PAINTED
        .lock()
        .unwrap()
        .iter()
        .filter(|&&(i, _, _)| i == instance)
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

/// Whether a sink saw the page return to idle *after* it was playing, which is
/// the only way to tell a sound that ended from a page that was never started.
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

/// No plane has been measured on the host, and an unmeasured worst case is not a
/// small one — the same rule the C6's table follows, for the same reason.
struct Unmeasured;

impl SpeedTable for Unmeasured {
    fn speed_for(_field: DemandField) -> Speed {
        Speed::Fast
    }
}

/// Stands in for the chip's clock on the host, which has no clock.
///
/// Every speed is the same speed here, so it reaches whatever it is asked. That is
/// true rather than convenient: the alternative would be a host build quietly
/// asserting a deadline against timings that were never measured, which is the one
/// thing the table exists to prevent.
struct HostClock;

impl ClockSink for HostClock {
    type Table = Unmeasured;

    fn set_speed(&mut self, speed: Speed) -> Speed {
        speed
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Boot the real composition: input task + render loop on a fake board.
    // `run` and the std executor never return, so the scenario verdicts by
    // terminating the process explicitly.
    let board = HostBoard::new();
    // Host has no interrupt executor to hand the feed's cadence to, and the
    // scenarios assert against a loop they can observe, so the feed is joined
    // onto this executor exactly as it was before the split.
    match select(
        run(board, HostClock, SpeakerRunner::Inline, Link::Wired),
        scenario(),
    )
    .await
    {
        Either::First(never) => match never {},
        Either::Second(()) => {}
    }
}

async fn scenario() {
    // 1. Boot: the breathing light paints some frames immediately.
    Timer::after(Duration::from_millis(200)).await;
    assert!(
        !PAINTED.lock().unwrap().is_empty(),
        "boot breath did not paint any frame"
    );

    // 2. Long-press button 0: breathing → solid `PALETTE[0]` on surface 0.
    PRESSED[0].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(600)).await;
    PRESSED[0].store(false, Ordering::SeqCst);

    // Long press resolves on release, then the mode applies.
    let mut attempts = 0;
    while !painted(0, PALETTE[0]) && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        painted(0, PALETTE[0]),
        "long press did not paint palette[0]"
    );

    // 3. Click button 1: surface 1 own move — and surface 0 stays solid.
    // Surface 0 is at rest now (solid), so any leaked move would repaint it.
    let rest0 = colors_on(0);
    let count1 = paints_on(1);
    PRESSED[1].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(120)).await;
    PRESSED[1].store(false, Ordering::SeqCst);

    // The click resolves after debounce + double-click window expiration.
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

    // 5. One more triple click reaches the audio-enabled page — which exists at
    //    all only because the composition picked the board's capture source up
    //    and told the state layer so. And the polls behind it must have landed
    //    in a snapshot, not just been served.
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

    // 6. One more triple click reaches the speaker page, which exists only
    //    because the composition picked the board's speaker up and told the
    //    state layer so.
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

    // 7. A click on the speaker page plays: the driver is asked for the sound
    //    after the boot tap, and the panel reports the play.
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
    // The play is written onto the bus by the render pass and reaches the driver
    // on the control pass, one cadence later — so `Playing` is observable before
    // the driver has been asked. Wait for the driver's own record, which is what
    // this assertion is actually about; the bound is the same generous one the
    // phase waits above use, and it is far longer than a cadence.
    attempts = 0;
    while PLAYED.lock().unwrap().is_empty() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        PLAYED.lock().unwrap().as_slice(),
        &[Sound::Notice],
        "a tap did not reach the driver with the next sound"
    );
    assert_eq!(
        PLAYBACK_PLAYS.load(Ordering::SeqCst),
        1,
        "the panel's play count did not follow the tap"
    );

    // 8. A second tap while the sound is still sounding is dropped: the state
    //    counts it and the driver is left alone, so the sound is not restarted
    //    under the listener.
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
        PLAYED.lock().unwrap().as_slice(),
        &[Sound::Notice],
        "a dropped tap still reached the driver"
    );

    // 9. The sound's own end comes back from the driver, and the page leaves
    //    `Playing` on that word alone.
    attempts = 0;
    while !phase_idle_after_playing() && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        phase_idle_after_playing(),
        "the page never left Playing after the driver reported the sound done"
    );

    // 10. A long press latches the output quiet, on the driver and the panel.
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
    // Same one-cadence gap as above, in the other direction: the render pass
    // records the latched mute, the control pass is what pushes it to the driver.
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

    // 11. The camera page, which the cycle reaches after the speaker one and
    //     which exists because the composition picked the board's sensor up. It
    //     has to reach the surface and not only the page cycle: the page is what
    //     the user sees, and a frame that never left the surface would be a page
    //     of nothing.
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

    // And the camera page must not have taken the fill away from the rest. A click there
    // walks the colour without leaving — that is the page's contract — so the page is
    // left the same way it was entered, and what has to survive is the surface's.
    //
    // `page_seen` would answer this from the whole run: ambient is the boot page, so it
    // is already in the log and the old check here passed without the page ever moving.
    // What has to be watched is the entries appended from here on.
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

    // Leaving the camera page has to hand the buffer back. This is the regression the
    // panel shipped with: the camera held the buffer, and the only call that released it
    // was gated on a colour being held, which taking the buffer had already cleared — so
    // the camera kept writing over every fill afterwards and the panel never came back.
    assert!(
        RELEASED.load(Ordering::SeqCst) > released_before,
        "the camera page was left without the buffer being handed back"
    );

    // ... and the surface has to accept a fill again. Both halves of the deadlock: the
    // release above is what makes this reachable, and a refused fill here is what it looked
    // like on the panel — the last camera frame left on screen for good.
    attempts = 0;
    while PAINTED.lock().unwrap().len() == painted_before && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PAINTED.lock().unwrap().len() > painted_before,
        "no fill reached the surface after leaving the camera page"
    );

    // 12. The Network page, which comes after the camera one in the cycle and exists because
    //     the entry point said a station came up. Nothing here wires a radio — the host has
    //     none — so what is asserted is the composition's half: the capability reaches the
    //     state machine, the page is drawn, and a scan published on the link's own plane
    //     arrives in the snapshot a sink reads.
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
    SCAN.publish(list);

    attempts = 0;
    while NETWORK_APS.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_APS.load(Ordering::SeqCst),
        1,
        "a scan published on the link's plane never reached the panel's snapshot"
    );

    // 13. The keyboard, driven the way the panel would drive it and asserted the way the
    //     network task reads it. The host wires no touch panel, so the taps are sent as the
    //     business intents `translate` produces from them — which is the half of the path the
    //     composition owns; the hit test and the multi-tap are core's, and pinned there.
    let send = |intent: BusinessIntent| {
        let _ = iot_app::input::INTENT_BUS.try_send(Intent::Business(intent));
    };

    // Picking a band opens the keyboard on the network that band showed, and the phrase it starts
    // with is empty: `NETWORK_TYPED` counts a draft of no glyphs as nothing typed.
    send(BusinessIntent::Row(0));
    Timer::after(Duration::from_millis(100)).await;

    // Typing reaches the snapshot, which is what makes a key visible on the panel. `hat` is the
    // two shapes a finger makes that no single tap shows: `h` is the `ghi` key tapped twice, which
    // is the key's own character cycling rather than a second character, and the capital is a
    // one-shot shift spent on the `t` after it.
    let mut at_ms = 100;
    for key in [
        Key::Char(b'4'),
        Key::Char(b'4'),
        Key::Char(b'2'),
        Key::Shift,
        Key::Char(b'8'),
    ] {
        send(BusinessIntent::Key { key, at_ms });
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

    // ... and connect hands it to the plane the network task reads from. The passphrase is
    // compared here and written nowhere: the smoke's output carries it no further.
    send(BusinessIntent::Key {
        key: Key::Connect,
        at_ms,
    });
    attempts = 0;
    let mut handed = JOIN.take();
    while handed.is_none() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        handed = JOIN.take();
        attempts += 1;
    }
    let handed = handed.expect("connect never reached the join plane");
    // The page hands over one of two shapes, and this is the one a keyboard's own key produces: a
    // passphrase, printed nowhere.
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

    // 14. The page's own controls, on the planes the network task reads, and the marks the store
    //     puts on the list coming back the other way. The host wires neither a radio nor a
    //     partition, so what is asserted is the composition's half: the loop publishes what the
    //     state was asked for, and a list marked the way the network task marks one lands in the
    //     snapshot the panel draws its marks from.
    send(BusinessIntent::Key {
        key: Key::Cancel,
        at_ms,
    });
    Timer::after(Duration::from_millis(100)).await;

    // The list the network task publishes, with the marks the store would put on its rows: the
    // snapshot the panel draws its marks from carries them on the list itself, and a mark drawn
    // from anything else would be a guess.
    let mut store = SavedNetworks::empty();
    store.remember(
        &Credentials::from_text("neighbour", "haT").expect("a credential the radio takes"),
    );
    let mut marked = list;
    let marks = store.mask(&marked);
    marked.mark_saved(marks);
    SCAN.publish(marked);
    attempts = 0;
    while NETWORK_SAVED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_SAVED.load(Ordering::SeqCst),
        1,
        "a marked list published on the link's plane never reached the panel's snapshot"
    );

    // The strip with no link on it: one request, and it lands on the survey's own plane. A tap that
    // reached the state and stopped there would be a panel that asks and a radio that never hears.
    send(BusinessIntent::Toolbar);
    assert!(
        select(RESCAN.ready(), Timer::after(Duration::from_secs(1)))
            .await
            .is_first(),
        "the strip's request to look around never reached the survey's plane"
    );

    // And the row a finger holds: the question, its answer, and the name on the plane the network
    // task erases from. The network is the one the store reported, which is what makes the hold a
    // question at all.
    send(BusinessIntent::RowHold(0));
    Timer::after(Duration::from_millis(100)).await;
    send(BusinessIntent::Confirm);
    match select(FORGET.ready(), Timer::after(Duration::from_secs(1))).await {
        Either::First(ssid) => assert_eq!(
            ssid.as_bytes(),
            b"neighbour",
            "the forget named a network other than the row that was held"
        ),
        Either::Second(()) => panic!("the hold's answer never reached the store's plane"),
    }

    std::process::exit(0);
}

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
use iot_app::run;
use iot_core::diagnostics::Diagnostics;
use iot_core::drivers::audio::{
    AudioEnvelope, AudioInput, AudioSample, AudioSource, CAPTURE_MS, COLUMNS_PER_POLL,
    SAMPLES_PER_COLUMN, dbfs,
};
use iot_core::drivers::board::{Board as BoardTrait, HasAudio, HasInput, HasLight, HasMotion};
use iot_core::drivers::input::{
    BUTTON_SCAN_MS, Button, ButtonScanner, DoubleClickAggregator, PassThrough, PollEntry,
};
use iot_core::drivers::light::{Fill, Rgb, RgbLight};
use iot_core::drivers::motion::{
    MOTION_SCAN_MS, MotionCapabilities, MotionError, MotionEvents, MotionInput, MotionReading,
    MotionSample, MotionScale, MotionSource,
};
use iot_core::intent::PALETTE;
use iot_core::state::DisplayPage;

/// Recorded paint operations per surface, so the smoke can assert what `run`
/// drew and on which instance.
static PAINTED: Mutex<Vec<(u8, Fill, Rgb)>> = Mutex::new(Vec::new());

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

/// The highest repair count any sink saw, so the smoke can prove the count
/// reaches the panel rather than stopping at the state layer.
static AUDIO_RESTARTS: AtomicUsize = AtomicUsize::new(0);

/// Loudest level any poll delivered, in dBFS, so the smoke can prove the panel's
/// decibel readout is fed by a real measurement. Seeded at the smallest signed
/// value rather than zero: every reading a level meter reports is at or below
/// 0 dBFS, so a zero seed would leave the maximum at "no level at all" and the
/// smoke would pass on a capture that measured nothing.
static AUDIO_PEAK_DB: AtomicIsize = AtomicIsize::new(isize::MIN);

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

struct HostAudio {
    envelope: AudioEnvelope,
}

impl AudioSource for HostAudio {
    fn sample(&mut self, now_ms: u64) -> AudioSample {
        self.envelope.push(&[HOST_LOUD; HOST_POLL_SAMPLES]);
        AUDIO_POLLS.fetch_add(1, Ordering::SeqCst);
        AudioSample {
            envelope: self.envelope,
            elapsed_ms: now_ms.min(u64::from(u32::MAX)) as u32,
            restarts: HOST_RESTARTS,
        }
    }
}

struct HostMotion;

const HOST_SCALE: MotionScale = MotionScale::from_ranges(8_192, 64);

impl MotionSource for HostMotion {
    fn sample(&mut self, _now_ms: u64) -> Result<Option<MotionReading>, MotionError> {
        Ok(Some(MotionReading {
            sample: MotionSample::from_raw([1, 2, 3], [4, 5, 6], HOST_SCALE, 3, 0),
            hardware: MotionEvents::new(),
        }))
    }
}

struct HostButton(u8);

impl Button for HostButton {
    fn is_pressed(&self) -> bool {
        PRESSED[self.0 as usize].load(Ordering::SeqCst)
    }
}

struct HostLight(u8);

impl RgbLight for HostLight {
    fn set_fill(&mut self, fill: Fill, color: Rgb) {
        PAINTED.lock().unwrap().push((self.0, fill, color));
    }

    fn set_backlight(&mut self, _level_pct: u8) {}
}

impl iot_core::diagnostics::DiagnosticsSink for HostLight {
    fn consume(&mut self, diagnostics: &Diagnostics) {
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
    }
}

struct HostBoard {
    lights: Option<Vec<HostLight>>,
    input: Option<Vec<PollEntry>>,
    motion: Option<PollEntry>,
    audio: Option<PollEntry>,
}

impl HostBoard {
    fn new() -> Self {
        Self {
            lights: Some(vec![HostLight(0), HostLight(1)]),
            input: Some(vec![
                PollEntry::new(
                    0,
                    Box::new(ButtonScanner::new(HostButton(0))),
                    Box::new(DoubleClickAggregator::new()),
                    BUTTON_SCAN_MS,
                ),
                PollEntry::new(
                    1,
                    Box::new(ButtonScanner::new(HostButton(1))),
                    Box::new(DoubleClickAggregator::new()),
                    BUTTON_SCAN_MS,
                ),
            ]),
            motion: Some(PollEntry::new(
                2,
                Box::new(MotionInput::new(HostMotion)),
                Box::new(PassThrough),
                MOTION_SCAN_MS,
            )),
            audio: Some(PollEntry::new(
                3,
                Box::new(AudioInput::new(HostAudio {
                    envelope: AudioEnvelope::ZERO,
                })),
                Box::new(PassThrough),
                CAPTURE_MS,
            )),
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
    fn take_audio(&mut self) -> Option<PollEntry> {
        self.audio.take()
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

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Boot the real composition: input task + render loop on a fake board.
    // `run` and the std executor never return, so the scenario verdicts by
    // terminating the process explicitly.
    let board = HostBoard::new();
    match select(run(board), scenario()).await {
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

    std::process::exit(0);
}

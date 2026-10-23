use crate::demand::{EffectiveDemand, Mode, PlaneKey};
use crate::diagnostics::{
    AudioDiagnostics, BreathSnapshot, Diagnostics, LightSnapshot, PlaybackDiagnostics,
    TouchDiagnostics,
};
use crate::drivers::light::{MAX_LIGHTS, Rgb, rgb_hue, scale_brightness};
use crate::state::{AudioPhase, Breath, DeviceState, DisplayPage, LightState};
use alloc::boxed::Box;

/// Light-mode codes carried by [`LightSnapshot::mode`]: `0` off, `1` breathing,
/// `2` solid. The panel overlay renders from this snapshot, so the codes are a
/// shared contract between the core diff and the bsp surface — never literals
/// mirrored in two crates.
pub const MODE_OFF: u8 = 0;
pub const MODE_BREATH: u8 = 1;
pub const MODE_SOLID: u8 = 2;

/// A device slot the render layer knows about. Light slots are per instance:
/// a board wiring several light surfaces subscribes one renderer per surface;
/// diagnostics is device-level, delivered to every diagnostics subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Light(u8),
    Diagnostics,
}

/// The declared appearance of the light, derived from state; breathing is the
/// schedule, not a color frame — time-driven rendering is the renderer's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightAppearance {
    Off,
    Color(Rgb),
    Breathing(Breath),
}

/// The intended appearance of every known slot, derived from [`DeviceState`].
///
/// `Diagnostics` rides by value for the same reason the audio envelope does in
/// `InputEvent`: a fixed snapshot that a renderer forwards whole, with no
/// allocation and no lifetime to thread through the diff.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotAppearance {
    /// The `instance`-th light surface's declared appearance.
    Light {
        instance: u8,
        appearance: LightAppearance,
    },
    /// Device-level diagnostics (the `Diagnostics` payload in `crate::diagnostics`): a
    /// renderer forwards it to its panel, which overlays the digit rows. Kept a
    /// separate slot so state diffs drive it independently of any light mode.
    Diagnostics(Diagnostics),
}

impl SlotAppearance {
    pub fn slot(&self) -> Slot {
        match self {
            SlotAppearance::Light { instance, .. } => Slot::Light(*instance),
            SlotAppearance::Diagnostics(_) => Slot::Diagnostics,
        }
    }
}

/// What each surface actually shows: the page's own light, with a light-only board's status
/// lamp standing in for the first surface. [`DeviceState::status_light`] answers `None` on a
/// board with a screen, so the substitution costs that board nothing.
fn status_lights(state: &DeviceState) -> [LightState; MAX_LIGHTS] {
    let mut lights = state.lights;
    if let Some(status) = state.status_light() {
        lights[0] = status;
    }
    lights
}

/// Appearance of the light under `state`, without time information.
pub fn light_appearance(state: LightState) -> LightAppearance {
    match state {
        LightState::Off => LightAppearance::Off,
        LightState::Solid { color, brightness } => {
            // Brightness is presentation, folded into the color the renderer
            // drives; the state object keeps color and brightness orthogonal.
            LightAppearance::Color(scale_brightness(color, brightness))
        }
        LightState::Breath(breath) => LightAppearance::Breathing(breath),
    }
}

fn light_snapshot(state: LightState) -> LightSnapshot {
    match state {
        LightState::Off => LightSnapshot {
            mode: MODE_OFF,
            brightness: 0,
            hue: 0,
            breath: BreathSnapshot::default(),
        },
        LightState::Solid { color, brightness } => LightSnapshot {
            mode: MODE_SOLID,
            brightness,
            hue: rgb_hue(color),
            breath: BreathSnapshot::default(),
        },
        LightState::Breath(Breath {
            period_ms,
            hue_period_ms,
            hue_span,
            min_brightness,
            max_brightness,
            saturation,
            hue,
            group,
            group_len,
        }) => LightSnapshot {
            mode: MODE_BREATH,
            brightness: max_brightness,
            hue: if group_len > 0 { group[0] } else { hue },
            breath: BreathSnapshot {
                period_ms: period_ms.try_into().unwrap_or(u16::MAX),
                hue_period_ms: hue_period_ms.try_into().unwrap_or(u16::MAX),
                hue_span,
                group_len,
                min_brightness,
                max_brightness,
                saturation,
            },
        },
    }
}

/// The diagnostics a `DeviceState` folds into for the renderer's digit rows:
/// the touch counters/persistence carry into [`TouchDiagnostics`] and every
/// light surface's driving values ride along in wiring order.
fn light_diagnostics(state: &DeviceState, demand: &EffectiveDemand) -> Diagnostics {
    Diagnostics {
        demand: demand.clone(),
        touch: TouchDiagnostics {
            taps: state.tap_count,
            presses: state.press_count,
            long_presses: state.long_press_count,
            ghost: state.ghost_count,
            swipes: state.swipe_count,
            last_swipe_dir: state.last_swipe.map_or(0, |(dir, _)| dir.code()),
            last_swipe_dist: state.last_swipe.map_or(0, |(_, dist)| dist),
            last_gesture_origin: state.last_gesture_origin,
            last_gesture_end: state.last_gesture_end,
            chip_gesture_id: state.chip_gesture_id,
            points: state.touch_points.map(|p| p.map(|l| (l.x, l.y))),
            live_dir: state.live_dir,
            two_finger_runs: state.two_finger_runs,
            frames: state.touch_frames,
            finger: state.finger,
            held_ms: state.touch_held_ms,
        },
        lights: state.lights.map(light_snapshot),
        page: state.page,
        link: state.link,
        mac: state.mac,
        aps: state.aps,
        setup: state.setup,
        motion_enabled: state.motion_enabled,
        motion: state.motion,
        motion_counts: state.motion_counts,
        motion_caps: state.motion_caps,
        audio: audio_diagnostics(state),
        playback: playback_diagnostics(state),
    }
}

/// Resolves which envelope the Audio page draws, done here so a sink reads one
/// envelope and never re-implements the phase rule.
fn audio_diagnostics(state: &DeviceState) -> AudioDiagnostics {
    let shown = match state.audio.phase {
        AudioPhase::Stopped => &state.audio.captured,
        AudioPhase::Recording | AudioPhase::Idle => &state.audio.live,
    };
    AudioDiagnostics {
        phase: state.audio.phase,
        envelope: shown.envelope,
        elapsed_ms: shown.elapsed_ms,
        restarts: shown.restarts,
        dba_lsb: shown.dba_lsb,
    }
}

/// Minimum wall time between full-frame panel repaints driven by a diagnostics
/// drift. A repaint ships the whole 320×240 frame down one blocking SPI
/// transfer (on the order of 15 ms), so without a bound the live pages — the
/// Audio sweep follows the capture, the attitude dial the motion sample —
/// repaint every 20 ms snapshot and hold the shared cooperative executor in a
/// ~15 ms block almost continuously, starving the playback feed and the
/// capture poll past their ring runway.
pub const DIAG_REPAINT_STEP_MS: u64 = 40;

/// Whether a diagnostics drift warrants a full-frame repaint now. The rate gate
/// bounds the blocking-SPI duty cycle, and each page predicate must cover exactly
/// the diagnostics that page draws: the Audio page redraws on everything visible
/// except the wall-time counter, the attitude dial on every snapshot, the Speaker
/// page on the playback row it stamps, Ambient on the corner overlay when enabled.
///
/// A predicate must never widen to `true` on a drift it does not draw. The snapshot
/// carries the live envelope, which keeps folding whether or not a capture runs, so a
/// page that repaints on any drift ships a full 150 KiB frame every
/// [`DIAG_REPAINT_STEP_MS`] forever — and that SPI duty cycle is what starves the
/// playback feed and capture poll into restarting their DMAs.
pub fn diag_repaint_due(
    page: DisplayPage,
    prev: &Diagnostics,
    next: &Diagnostics,
    since_ms: u64,
    overlay: bool,
) -> bool {
    // The panel's own mode is not a page rule: a change-driven panel repaints when something
    // changes and an unlit one never repaints at all. `Continuous` falls through to the page's.
    match next.demand.plane(PlaneKey::DISPLAY).mode {
        Mode::Off => return false,
        Mode::OnChange => return since_ms >= DIAG_REPAINT_STEP_MS * 4 && prev != next,
        Mode::Continuous => {}
    }
    if since_ms < DIAG_REPAINT_STEP_MS {
        return false;
    }
    match page {
        DisplayPage::Audio => {
            prev.audio.phase != next.audio.phase
                || prev.audio.envelope != next.audio.envelope
                || prev.audio.restarts != next.audio.restarts
                || prev.audio.dba_lsb != next.audio.dba_lsb
        }
        DisplayPage::Attitude => true,
        DisplayPage::Speaker => prev.playback != next.playback,
        DisplayPage::Ambient => overlay,
        // The camera page redraws on a frame arriving, which is a data-plane event and not
        // anything this diff can see — so a snapshot drift must not force a repaint here.
        // Forcing one would ship a full frame that is a copy of the last one, ~15 ms of
        // blocking SPI for nothing, and the picture is only new every few hundred ms anyway.
        DisplayPage::Camera => false,
        // The Network page draws the link, the scan list, the entry being taken on it, and the marks
        // the store puts on the list's rows — so a drift in any of them is exactly a repaint, and a
        // drift in none of them (the touch counters the same snapshot carries) is not. The entry is
        // here because it is what a finger is typing: a key that did not repaint would be a key the
        // user cannot see arrive. The marks ride the list itself, so `aps` covers them: a save or a
        // forget is a new list from the network task, and the survey's own list deliberately does
        // not republish a round it read the same.
        //
        // The board's address is here for the same reason the rest is: it arrives once, after the
        // radio's first report, and a header that drew before it landed would keep a blank cell
        // for as long as the list stayed the same — which the survey makes a point of doing.
        DisplayPage::Network => {
            prev.link != next.link
                || prev.aps != next.aps
                || prev.setup != next.setup
                || prev.mac != next.mac
        }
        // The splash draws the device's own name and what the button is for, and neither changes
        // while it is up: entering and leaving it are the page change itself, which repaints
        // without asking here.
        DisplayPage::Splash => false,
    }
}

/// Folds the Speaker page's playback state into a snapshot. Straight copy rather
/// than a resolution like the audio one: the phase is the state layer's rule, and
/// the page only has to print what the state already decided.
fn playback_diagnostics(state: &DeviceState) -> PlaybackDiagnostics {
    PlaybackDiagnostics {
        phase: state.playback.phase,
        sound: state.playback.sound,
        muted: state.playback.muted,
        plays: state.playback.plays,
        dropped: state.playback.dropped,
    }
}

/// How a renderer wants to be driven after an appearance sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Appearance settled; no callbacks until the next state change.
    Idle,
    /// The render layer must step this renderer every tick to animate.
    TimeDriven,
}

/// Pluggable binding between one device and the render layer: the controller
/// decides *what* to show (central state diff), the renderer answers *how*,
/// including whether it needs time-driven frames. A renderer subscribes to the
/// slots it owns (a panel: its light instance plus device diagnostics). Not
/// `Send`: a renderer is built beside the surface it drives and moves into the
/// render task as part of the registry.
pub trait Renderer {
    /// The device slots this renderer subscribes to, in diff order.
    fn slots(&self) -> &[Slot];

    /// A slot's appearance changed; apply it to the device.
    fn on_appearance(&mut self, appearance: SlotAppearance, now_ms: u32) -> Activity;

    /// One animation frame, called only while the renderer reported
    /// [`Activity::TimeDriven`].
    fn step(&mut self, now_ms: u32) -> Activity;
}

/// Central judge of the device surface: diffs successive [`DeviceState`]
/// snapshots and reports only the appearances that changed, via a callback the
/// render layer fans out to matching renderers. Allocation-free.
pub struct RenderController {
    /// Boxed because `Render` is handed to the app's render task by value, so an
    /// inline snapshot would ride down that task's fixed stack frame. The box is
    /// allocated once and then overwritten in place.
    last: Option<Box<Snapshot>>,
}

/// What the controller diffs successive [`DeviceState`]s on: the diagnostics the
/// renderers receive, plus each light's own state, which is what decides whether
/// its appearance moved. A `DeviceState` change that reaches neither — the raw
/// touch snapshot, which no diagnostics row carries — cannot change what is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    diagnostics: Diagnostics,
    lights: [LightState; MAX_LIGHTS],
}

impl RenderController {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Diff `state` against the last snapshot, invoking `notify` per changed
    /// slot; the first call reports every known slot. Returns whether any slot
    /// changed, so bindings can react once.
    /// Folds the state into a snapshot and hands each changed slot to `notify`.
    ///
    /// `demand` is passed rather than derived from `state` because a build's override lives
    /// on the manager: a cost report naming the page's demand would be evidence about a
    /// configuration nobody is running.
    pub fn reconcile(
        &mut self,
        state: &DeviceState,
        demand: &EffectiveDemand,
        mut notify: impl FnMut(SlotAppearance),
    ) -> bool {
        let next = Snapshot {
            diagnostics: light_diagnostics(state, demand),
            lights: status_lights(state),
        };
        let changed = self.last.as_deref() != Some(&next);
        if changed {
            // Diagnostics first so panel renderers repaint the digits onto the
            // current surface, then each moved light appearance repaints over
            // it in one write instead of an old-color frame followed by a new
            // one.
            notify(SlotAppearance::Diagnostics(next.diagnostics.clone()));
            for (instance, light) in next.lights.iter().enumerate() {
                let moved = match self.last.as_deref() {
                    None => true,
                    Some(last) => last.lights[instance] != *light,
                };
                if moved {
                    notify(SlotAppearance::Light {
                        instance: instance as u8,
                        appearance: light_appearance(*light),
                    });
                }
            }
            match &mut self.last {
                Some(last) => **last = next,
                None => self.last = Some(Box::new(next)),
            }
        }
        changed
    }

    /// Last synced appearance for `slot` (or `None` before the first sync), so a
    /// freshly registered renderer can catch up before the next diff.
    pub fn current(&self, slot: Slot) -> Option<SlotAppearance> {
        let last = self.last.as_deref()?;
        Some(match slot {
            Slot::Light(instance) => SlotAppearance::Light {
                instance,
                appearance: light_appearance(
                    last.lights[usize::from(instance).min(MAX_LIGHTS - 1)],
                ),
            },
            Slot::Diagnostics => SlotAppearance::Diagnostics(last.diagnostics.clone()),
        })
    }
}

impl Default for RenderController {
    fn default() -> Self {
        Self::new()
    }
}

/// Repaint rate (frames per second) over one window, `0` for an empty or
/// degenerate window. Saturates at `u8::MAX` so a burst never wraps.
pub fn windowed_rate(frames: u32, elapsed_ms: u64) -> u8 {
    if elapsed_ms == 0 {
        return 0;
    }
    u64::from(frames)
        .saturating_mul(1_000)
        .checked_div(elapsed_ms)
        .and_then(|rate| u8::try_from(rate).ok())
        .unwrap_or(u8::MAX)
}

/// A share of a window as a whole percent, or `0` for a window nothing was measured in.
///
/// Capped at 100: the sum can exceed its own window when the clock the two ends read
/// differs, and that is a clock artefact rather than a finding.
pub fn windowed_percent(part_us: u64, window_us: u64) -> u32 {
    part_us
        .saturating_mul(100)
        .checked_div(window_us)
        .and_then(|pct| u32::try_from(pct).ok())
        .unwrap_or(100)
        .min(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::audio::{AudioEnvelope, AudioSample};

    #[test]
    fn a_share_is_reported_whole_and_never_over_a_hundred() {
        assert_eq!(windowed_percent(500_000, 2_000_000), 25);
        assert_eq!(windowed_percent(2_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(1, 1_000_000), 0);
        // A hold longer than its own window is two clocks disagreeing, not a real cost.
        assert_eq!(windowed_percent(4_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(7_000, 0), 100);
    }
    use crate::drivers::network::{AccessPoint, ApList, Front, Ipv4, LinkPhase, LinkStatus, Ssid};
    use crate::drivers::playback::Sound;
    use crate::intent::{BusinessIntent, CaptureAction};
    use crate::state::{DeviceManager, PlaybackPhase};

    const BOOT_BREATH: crate::state::Breath = crate::state::Breath {
        period_ms: 3_000,
        hue_period_ms: 60_000,
        hue_span: 255,
        min_brightness: 24,
        max_brightness: 80,
        saturation: 200,
        hue: 0,
        group: [16, 32, 3, 0, 0, 0, 0],
        group_len: 3,
    };

    fn state(light: LightState) -> DeviceState {
        DeviceState {
            lights: [light; MAX_LIGHTS],
            ..DeviceState::default()
        }
    }

    fn collect(ctl: &mut RenderController, s: &DeviceState) -> Vec<SlotAppearance> {
        let mut seen = Vec::new();
        ctl.reconcile(s, &EffectiveDemand::default(), |a| seen.push(a));
        seen
    }

    /// A device that has left its splash: what the diagnostics describe, so the comparison is the
    /// page a running board is on.
    fn landed() -> DeviceManager {
        let mut manager = DeviceManager::new();
        manager.apply_business(BusinessIntent::SplashOver);
        manager
    }

    fn diagnostics_for(light: LightState) -> Diagnostics {
        light_diagnostics(&state(light), &EffectiveDemand::default())
    }

    #[test]
    fn the_board_address_reaches_the_panel_with_the_rest_of_the_header() {
        // The panel draws it from the snapshot like every other header reading, so it has to survive
        // the fold rather than be asked of the state through a second path.
        let mac = [0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef];
        let mut state = state(LightState::boot());
        state.mac = Some(mac);
        assert_eq!(
            light_diagnostics(&state, &EffectiveDemand::default()).mac,
            Some(mac)
        );
    }

    fn live_envelope() -> AudioEnvelope {
        let mut envelope = AudioEnvelope::default();
        let mut samples = [0_i16; crate::drivers::audio::SAMPLES_PER_COLUMN as usize];
        samples[0] = 0x7F;
        envelope.push(&samples);
        envelope
    }

    #[test]
    fn diag_repaint_waits_out_the_step() {
        let base = Diagnostics::default();
        let mut moved = base.clone();
        moved.audio.phase = AudioPhase::Recording;
        assert!(!diag_repaint_due(
            DisplayPage::Audio,
            &base,
            &moved,
            DIAG_REPAINT_STEP_MS - 1,
            false
        ));
        assert!(diag_repaint_due(
            DisplayPage::Audio,
            &base,
            &moved,
            DIAG_REPAINT_STEP_MS,
            false
        ));
    }

    #[test]
    fn diag_repaint_ignores_the_wall_time_counter() {
        let base = Diagnostics::default();
        let mut ticked = base.clone();
        ticked.audio.elapsed_ms = 1_234;
        assert!(!diag_repaint_due(
            DisplayPage::Audio,
            &base,
            &ticked,
            DIAG_REPAINT_STEP_MS,
            false
        ));
    }

    #[test]
    fn diag_repaint_audio_tracks_everything_else_it_draws() {
        let base = Diagnostics::default();
        let mut phase = base.clone();
        phase.audio.phase = AudioPhase::Recording;
        let mut envelope = base.clone();
        envelope.audio.envelope = live_envelope();
        let mut restarts = base.clone();
        restarts.audio.restarts = 3;
        let mut level = base.clone();
        level.audio.dba_lsb = 0x8000;
        for next in [phase, envelope, restarts, level] {
            assert!(
                diag_repaint_due(
                    DisplayPage::Audio,
                    &base,
                    &next,
                    DIAG_REPAINT_STEP_MS,
                    false
                ),
                "an Audio drift the page draws must repaint"
            );
        }
    }

    #[test]
    fn diag_repaint_attitude_follows_every_snapshot() {
        let base = Diagnostics::default();
        let mut ticked = base.clone();
        ticked.audio.elapsed_ms = 10;
        assert!(!diag_repaint_due(
            DisplayPage::Attitude,
            &base,
            &ticked,
            DIAG_REPAINT_STEP_MS - 1,
            false
        ));
        assert!(diag_repaint_due(
            DisplayPage::Attitude,
            &base,
            &ticked,
            DIAG_REPAINT_STEP_MS,
            false
        ));
    }

    #[test]
    fn diag_repaint_ambient_only_for_the_overlay() {
        let base = Diagnostics::default();
        let mut ticked = base.clone();
        ticked.audio.phase = AudioPhase::Recording;
        assert!(!diag_repaint_due(
            DisplayPage::Ambient,
            &base,
            &ticked,
            DIAG_REPAINT_STEP_MS,
            false
        ));
        assert!(diag_repaint_due(
            DisplayPage::Ambient,
            &base,
            &ticked,
            DIAG_REPAINT_STEP_MS,
            true
        ));
    }

    #[test]
    fn diag_repaint_speaker_tracks_the_row_it_stamps() {
        let base = Diagnostics::default();
        let mut playing = base.clone();
        playing.playback.phase = PlaybackPhase::Playing;
        let mut source = base.clone();
        source.playback.sound = Sound::Rise;
        let mut muted = base.clone();
        muted.playback.muted = true;
        let mut played = base.clone();
        played.playback.plays = 7;
        let mut dropped = base.clone();
        dropped.playback.dropped = 2;
        for next in [playing, source, muted, played, dropped] {
            assert!(
                diag_repaint_due(
                    DisplayPage::Speaker,
                    &base,
                    &next,
                    DIAG_REPAINT_STEP_MS,
                    false
                ),
                "a playback drift the Speaker page stamps must repaint even with the overlay off"
            );
        }
    }

    #[test]
    fn diag_repaint_speaker_ignores_a_drift_it_does_not_draw() {
        let base = Diagnostics::default();
        // The envelope is live on every poll, so a Speaker-page predicate that
        // accepted it would ship a full frame every step for the whole session.
        let mut drifting = base.clone();
        drifting.audio.envelope = live_envelope();
        for overlay in [false, true] {
            assert!(
                !diag_repaint_due(
                    DisplayPage::Speaker,
                    &base,
                    &drifting,
                    DIAG_REPAINT_STEP_MS,
                    overlay
                ),
                "the Speaker page stamps no envelope, so the overlay flag must not re-open it"
            );
        }
    }

    #[test]
    fn diag_repaint_network_tracks_the_link_and_the_list() {
        let base = Diagnostics::default();
        let mut associated = base.clone();
        associated.link.phase = LinkPhase::Connecting;
        associated.link.ap = Some(AccessPoint {
            ssid: Ssid::from_octets(b"neighbour"),
            bssid: [1, 2, 3, 4, 5, 6],
            channel: 11,
            rssi: -55,
            secured: true,
        });
        let mut addressed = base.clone();
        addressed.link.address = Some(Ipv4::from_octets([192, 168, 1, 136]));
        let mut scanned = base.clone();
        scanned.aps = Some(ApList::from_ranked(&[AccessPoint::default()]));
        for next in [associated, addressed, scanned] {
            assert!(
                diag_repaint_due(
                    DisplayPage::Network,
                    &base,
                    &next,
                    DIAG_REPAINT_STEP_MS,
                    false
                ),
                "a link or list drift the Network page draws must repaint even with the \
                 overlay off"
            );
        }
    }

    #[test]
    fn diag_repaint_network_takes_the_board_address_the_first_time_it_arrives() {
        // The address is sent once, by the network task's first report, and by then the page has
        // usually drawn itself already — the list being the same round after round is exactly what
        // the survey tries to do. Without this in the diff the header keeps its blank cell for as
        // long as nothing else moves.
        let base = Diagnostics::default();
        let mut addressed = base.clone();
        addressed.mac = Some([0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef]);
        assert!(diag_repaint_due(
            DisplayPage::Network,
            &base,
            &addressed,
            DIAG_REPAINT_STEP_MS,
            false
        ));
    }

    #[test]
    fn diag_repaint_network_ignores_a_drift_it_does_not_draw() {
        let base = Diagnostics::default();
        // The envelope folds on every poll and the touch counters move on every gesture, and
        // the Network page draws neither: a predicate that accepted them would ship a full
        // frame every step for as long as the device is on.
        let mut drifting = base.clone();
        drifting.audio.envelope = live_envelope();
        drifting.touch.taps = 12;
        for overlay in [false, true] {
            assert!(
                !diag_repaint_due(
                    DisplayPage::Network,
                    &base,
                    &drifting,
                    DIAG_REPAINT_STEP_MS,
                    overlay
                ),
                "the Network page stamps no envelope and no counter"
            );
        }
    }

    #[test]
    fn the_audio_page_stamps_the_capture_the_phase_points_at() {
        let mut manager = DeviceManager::new().with_audio(true);
        let mut envelope = AudioEnvelope::default();
        let mut samples = [0_i16; crate::drivers::audio::SAMPLES_PER_COLUMN as usize];
        samples[0] = i16::MIN;
        envelope.push(&samples);
        let poll = AudioSample {
            envelope,
            elapsed_ms: 1_000,
            restarts: 2,
            dba_lsb: 0,
        };
        manager.apply_capture(poll);
        assert_eq!(
            light_diagnostics(&manager.state(), manager.demand())
                .audio
                .phase,
            AudioPhase::Idle,
            "polling alone never starts a capture"
        );
        manager.apply_business(BusinessIntent::Capture(CaptureAction::Start));
        manager.apply_business(BusinessIntent::NextPage);
        let recording = light_diagnostics(&manager.state(), manager.demand()).audio;
        assert_eq!(recording.phase, AudioPhase::Recording);
        assert_eq!(
            recording.envelope, poll.envelope,
            "a live capture draws live"
        );
        assert_eq!(
            recording.restarts, 2,
            "the repair count has to reach the panel, or a stalled capture that was \
             recovered from still reads as a capture that simply went quiet"
        );

        manager.apply_business(BusinessIntent::Capture(CaptureAction::Stop));
        let stopped = light_diagnostics(&manager.state(), manager.demand()).audio;
        assert_eq!(stopped.phase, AudioPhase::Stopped);
        assert_eq!(
            stopped.envelope, poll.envelope,
            "the latch stands in once the driver moves on"
        );
    }

    #[test]
    fn first_sync_reports_boot_then_silence() {
        let mut ctl = RenderController::new();
        let boot = state(LightState::boot());
        let mut seen = Vec::new();
        assert!(
            ctl.reconcile(&boot, &EffectiveDemand::default(), |a| seen.push(a)),
            "first sync is a change"
        );
        assert_eq!(
            seen,
            vec![
                SlotAppearance::Diagnostics(diagnostics_for(LightState::boot())),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Breathing(BOOT_BREATH)
                },
                SlotAppearance::Light {
                    instance: 1,
                    appearance: LightAppearance::Breathing(BOOT_BREATH)
                },
            ]
        );
        assert!(
            !ctl.reconcile(&boot, &EffectiveDemand::default(), |_| {}),
            "identical state is silent"
        );
        let off = state(LightState::Off);
        assert!(
            ctl.reconcile(&off, &EffectiveDemand::default(), |_| {}),
            "changed state is a change"
        );
        assert!(
            !ctl.reconcile(&off, &EffectiveDemand::default(), |_| {}),
            "settled state is silent"
        );
    }

    #[test]
    fn off_emits_off_once() {
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::boot()));
        let off = state(LightState::Off);
        assert_eq!(
            collect(&mut ctl, &off),
            vec![
                SlotAppearance::Diagnostics(diagnostics_for(LightState::Off)),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Off
                },
                SlotAppearance::Light {
                    instance: 1,
                    appearance: LightAppearance::Off
                },
            ]
        );
        assert_eq!(collect(&mut ctl, &off), vec![]);
    }

    #[test]
    fn a_state_change_no_renderer_can_see_stays_silent() {
        let mut ctl = RenderController::new();
        let boot = state(LightState::boot());
        collect(&mut ctl, &boot);
        // The raw touch snapshot rides no diagnostics row, so a state differing
        // only there cannot change what a renderer is handed.
        let mut moved = boot.clone();
        moved.touch = Some(crate::drivers::input::TouchEvent {
            points: [crate::drivers::input::TouchPoint {
                id: 0,
                x: 7,
                y: 8,
                status: crate::drivers::input::TouchStatus::Down,
            }; crate::drivers::input::MAX_TOUCH_POINTS],
            len: 1,
            contacts: 1,
        });
        assert!(collect(&mut ctl, &moved).is_empty());
    }

    #[test]
    fn light_changes_emit_new_appearances() {
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::boot()));
        // Boot → solid: diagnostics first, then every surface.
        let target = state(LightState::Solid {
            color: Rgb(1, 2, 3),
            brightness: 255,
        });
        assert_eq!(
            collect(&mut ctl, &target),
            vec![
                SlotAppearance::Diagnostics(diagnostics_for(target.lights[0])),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(1, 2, 3))
                },
                SlotAppearance::Light {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(1, 2, 3))
                },
            ]
        );
        // Solid → solid with a new color re-notifies too.
        let recolored = state(LightState::Solid {
            color: Rgb(9, 9, 9),
            brightness: 255,
        });
        assert_eq!(
            collect(&mut ctl, &recolored),
            vec![
                SlotAppearance::Diagnostics(diagnostics_for(recolored.lights[0])),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(9, 9, 9))
                },
                SlotAppearance::Light {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(9, 9, 9))
                },
            ]
        );
        // Breathing → breathing with new dimensions re-notifies as well.
        let second = state(LightState::Breath(crate::state::Breath {
            period_ms: 1_000,
            hue_period_ms: 3_000,
            hue_span: 80,
            min_brightness: 4,
            max_brightness: 40,
            saturation: 120,
            hue: 200,
            group: [200, 250, 30, 0, 0, 0, 0],
            group_len: 3,
        }));
        let seen = collect(&mut ctl, &second);
        assert_eq!(seen.len(), 3, "diagnostics ride along with the light diff");
        assert!(matches!(
            seen[0].clone(),
            SlotAppearance::Diagnostics(t) if t == diagnostics_for(second.lights[0])
        ));
        assert!(matches!(
            seen[1],
            SlotAppearance::Light {
                instance: 0,
                appearance: LightAppearance::Breathing(_),
                ..
            }
        ));
        assert!(matches!(
            seen[2],
            SlotAppearance::Light {
                instance: 1,
                appearance: LightAppearance::Breathing(_),
                ..
            }
        ));
    }

    #[test]
    fn current_reports_last_synced_appearance_per_slot() {
        let mut ctl = RenderController::new();
        assert_eq!(ctl.current(Slot::Light(0)), None, "nothing synced yet");
        let plain = state(LightState::Solid {
            color: Rgb(7, 6, 5),
            brightness: 255,
        });
        collect(&mut ctl, &plain);
        assert_eq!(
            ctl.current(Slot::Light(0)),
            Some(SlotAppearance::Light {
                instance: 0,
                appearance: LightAppearance::Color(Rgb(7, 6, 5))
            })
        );
        let two = DeviceState {
            lights: [
                LightState::Off,
                LightState::Solid {
                    color: Rgb(4, 4, 4),
                    brightness: 200,
                },
            ],
            ..DeviceState::default()
        };
        collect(&mut ctl, &two);
        assert_eq!(
            ctl.current(Slot::Light(0)),
            Some(SlotAppearance::Light {
                instance: 0,
                appearance: LightAppearance::Off
            })
        );
        assert_eq!(
            ctl.current(Slot::Light(1)),
            Some(SlotAppearance::Light {
                instance: 1,
                appearance: LightAppearance::Color(Rgb(3, 3, 3))
            })
        );
        assert!(matches!(
            ctl.current(Slot::Diagnostics),
            Some(SlotAppearance::Diagnostics(_))
        ));
        // An instance beyond the installed array clamps to the last surface.
        assert_eq!(
            ctl.current(Slot::Light(9)),
            Some(SlotAppearance::Light {
                instance: 9,
                appearance: LightAppearance::Color(Rgb(3, 3, 3))
            })
        );
    }

    #[test]
    fn diff_carries_diagnostics_before_surfaces_in_wiring_order() {
        let mut ctl = RenderController::new();
        let two = DeviceState {
            lights: [
                LightState::Off,
                LightState::Solid {
                    color: Rgb(5, 5, 5),
                    brightness: 255,
                },
            ],
            ..DeviceState::default()
        };
        let seen = collect(&mut ctl, &two);
        assert_eq!(
            seen,
            vec![
                // Device-level diagnostics first, then surface 0, then
                // surface 1, in wiring order.
                SlotAppearance::Diagnostics(light_diagnostics(&two, &EffectiveDemand::default())),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Off,
                },
                SlotAppearance::Light {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(5, 5, 5)),
                },
            ]
        );
        // The second instance's snapshot is carried beside the first.
        let SlotAppearance::Diagnostics(d) = seen[0].clone() else {
            panic!("first diff must carry the diagnostics");
        };
        assert_eq!(d.lights.len(), MAX_LIGHTS);
        assert_eq!(d.lights[0].mode, 0);
        assert_eq!(d.lights[1].mode, 2);
        // Surface 0 advances; surface 1 stays on its boot breathing and the
        // counters stay put—so the diff carries only the moved surface.
        let mut ctl = RenderController::new();
        collect(&mut ctl, &DeviceState::default());
        let changed = DeviceState {
            lights: [
                LightState::Solid {
                    color: Rgb(1, 2, 3),
                    brightness: 255,
                },
                LightState::boot(),
            ],
            ..DeviceState::default()
        };
        assert_eq!(
            collect(&mut ctl, &changed),
            vec![
                SlotAppearance::Diagnostics(light_diagnostics(
                    &changed,
                    &EffectiveDemand::default()
                )),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(1, 2, 3)),
                },
            ],
            "only the moved surface and the diagnostics it carries are emitted"
        );
    }

    #[test]
    fn diagnostics_change_emits_both_fields_before_light() {
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::boot()));
        let mut changed = state(LightState::Off);
        changed.tap_count = 7;
        changed.press_count = 9;
        changed.long_press_count = 1;
        changed.touch_held_ms = 590;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.taps = 7;
        expected.touch.presses = 9;
        expected.touch.long_presses = 1;
        expected.touch.held_ms = 590;
        let seen = collect(&mut ctl, &changed);
        assert_eq!(
            seen,
            vec![
                SlotAppearance::Diagnostics(expected),
                SlotAppearance::Light {
                    instance: 0,
                    appearance: LightAppearance::Off
                },
                SlotAppearance::Light {
                    instance: 1,
                    appearance: LightAppearance::Off
                },
            ]
        );
    }

    #[test]
    fn diagnostics_diff_only_sets_the_shifted_field() {
        // Phantom contacts observed with no classified result: the signal that
        // a ghost (rather than an un-reported press) ate the tap.
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut ghost = state(LightState::Off);
        ghost.ghost_count = 3;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.ghost = 3;
        assert_eq!(
            collect(&mut ctl, &ghost),
            vec![SlotAppearance::Diagnostics(expected)]
        );
        // A raw press with no classified tap: the gap the diagnostic is after.
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut presses = state(LightState::Off);
        presses.press_count = 1;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.presses = 1;
        assert_eq!(
            collect(&mut ctl, &presses),
            vec![SlotAppearance::Diagnostics(expected)]
        );
    }

    #[test]
    fn press_and_frame_diagnostics_carry_coordinates() {
        // A raw press leaves its coordinates in the live point list.
        let mut ctl = RenderController::new();
        let mut manager = landed();
        collect(&mut ctl, &manager.state());
        manager.apply_operation(crate::intent::recognize(
            crate::drivers::input::InputEvent::Gesture(
                crate::drivers::input::GestureEvent::Press {
                    id: 0,
                    x: 42,
                    y: 97,
                },
            ),
            0,
        ));
        let state = manager.state();
        let mut expected = diagnostics_for(state.lights[0]);
        expected.touch.presses = 1;
        expected.touch.points = [Some((42, 97)), None];
        assert_eq!(
            collect(&mut ctl, &state),
            vec![SlotAppearance::Diagnostics(expected)]
        );
        // The raw-frame heartbeat rides the diagnostics with its live point.
        let mut ctl = RenderController::new();
        let mut manager = landed();
        collect(&mut ctl, &manager.state());
        manager.apply_operation(crate::intent::recognize(
            crate::drivers::input::InputEvent::Touch(crate::drivers::input::TouchEvent {
                points: [crate::drivers::input::TouchPoint {
                    id: 0,
                    x: 7,
                    y: 8,
                    status: crate::drivers::input::TouchStatus::Down,
                }; crate::drivers::input::MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            }),
            0,
        ));
        manager.apply_operation(crate::intent::recognize(
            crate::drivers::input::InputEvent::Touch(crate::drivers::input::TouchEvent {
                points: [crate::drivers::input::TouchPoint {
                    id: 0,
                    x: 9,
                    y: 10,
                    status: crate::drivers::input::TouchStatus::Contact,
                }; crate::drivers::input::MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            }),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let SlotAppearance::Diagnostics(diagnostics) = seen[0].clone() else {
            panic!("first diff must carry the diagnostics");
        };
        assert_eq!(
            (diagnostics.touch.frames, diagnostics.touch.points[0]),
            (2, Some((9, 10)))
        );
    }

    #[test]
    fn swipe_chip_and_tap_reach_the_diagnostics() {
        let mut ctl = RenderController::new();
        let mut manager = crate::state::DeviceManager::new();
        collect(&mut ctl, &manager.state());
        manager.apply_operation(crate::intent::recognize(
            crate::drivers::input::InputEvent::Gesture(
                crate::drivers::input::GestureEvent::Swipe {
                    id: 0,
                    x: 20,
                    y: 90,
                    end_x: 20,
                    end_y: 15,
                    direction: crate::drivers::input::SwipeDirection::Up,
                    held_ms: 80,
                    distance_px: 99,
                },
            ),
            0,
        ));
        manager.apply_operation(crate::intent::recognize(
            crate::drivers::input::InputEvent::ChipGesture(0x10),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let SlotAppearance::Diagnostics(diagnostics) = seen[0].clone() else {
            panic!("first diff must carry the diagnostics");
        };
        assert_eq!(diagnostics.touch.swipes, 1);
        assert_eq!(
            diagnostics.touch.last_swipe_dir,
            crate::drivers::input::SwipeDirection::Up.code()
        );
        assert_eq!(diagnostics.touch.last_swipe_dist, 99);
        assert_eq!(diagnostics.touch.last_gesture_origin, Some((20, 90)));
        assert_eq!(diagnostics.touch.last_gesture_end, Some((20, 15)));
        assert_eq!(diagnostics.touch.chip_gesture_id, 0x10);
        // A tap's hold time and touch point land on the same diagnostics.
        let mut ctl = RenderController::new();
        let mut manager = crate::state::DeviceManager::new();
        collect(&mut ctl, &manager.state());
        manager.apply_operation(crate::intent::recognize(
            crate::drivers::input::InputEvent::Gesture(crate::drivers::input::GestureEvent::Tap {
                id: 0,
                x: 5,
                y: 7,
                held_ms: 590,
                at_ms: 590,
            }),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let SlotAppearance::Diagnostics(diagnostics) = seen[0].clone() else {
            panic!("first diff must carry the diagnostics");
        };
        assert_eq!(diagnostics.touch.taps, 1);
        assert_eq!(diagnostics.touch.held_ms, 590);
        assert_eq!(
            diagnostics.touch.last_gesture_origin,
            Some((5, 7)),
            "a tap reports its touch point"
        );
    }

    #[test]
    fn light_snapshot_maps_every_mode() {
        assert_eq!(
            light_snapshot(LightState::Off),
            LightSnapshot {
                mode: MODE_OFF,
                brightness: 0,
                hue: 0,
                breath: BreathSnapshot::default(),
            }
        );
        assert_eq!(
            light_snapshot(LightState::Solid {
                color: Rgb(255, 0, 0),
                brightness: 140,
            }),
            LightSnapshot {
                mode: MODE_SOLID,
                brightness: 140,
                hue: 0,
                breath: BreathSnapshot::default(),
            },
            "solid reports its brightness and hue, no breathing dimensions"
        );
        assert_eq!(
            light_snapshot(LightState::Breath(crate::state::Breath {
                hue: 17,
                group: [0, 0, 0, 0, 0, 0, 0],
                group_len: 0,
                ..BOOT_BREATH
            }))
            .hue,
            17,
            "an empty group falls back to the standalone hue"
        );
        let breath = light_snapshot(LightState::Breath(BOOT_BREATH));
        assert_eq!(
            (breath.mode, breath.brightness, breath.hue),
            (MODE_BREATH, 80, 16),
            "a breathing light reports its ceiling and group head"
        );
        assert_eq!(
            breath.breath,
            BreathSnapshot {
                period_ms: crate::state::DeviceManager::DEFAULT_PERIOD_MS as u16,
                hue_period_ms: crate::state::DeviceManager::DEFAULT_HUE_PERIOD_MS as u16,
                hue_span: crate::state::DeviceManager::DEFAULT_HUE_SPAN,
                group_len: crate::state::DeviceManager::DEFAULT_GROUP_LEN,
                min_brightness: crate::state::DeviceManager::DEFAULT_MIN_BRIGHTNESS,
                max_brightness: crate::state::DeviceManager::DEFAULT_MAX_BRIGHTNESS,
                saturation: crate::state::DeviceManager::DEFAULT_SATURATION,
            },
            "breathing carries every dimension the overlay prints"
        );
    }

    #[test]
    fn light_appearance_maps_every_state() {
        assert_eq!(light_appearance(LightState::Off), LightAppearance::Off);
        assert_eq!(
            light_appearance(LightState::Solid {
                color: Rgb(1, 2, 3),
                brightness: 255,
            }),
            LightAppearance::Color(Rgb(1, 2, 3))
        );
        assert_eq!(
            light_appearance(LightState::Solid {
                color: Rgb(200, 100, 50),
                brightness: 128,
            }),
            LightAppearance::Color(Rgb(100, 50, 25)),
            "solid brightness scales the color down toward black"
        );
        assert!(matches!(
            light_appearance(LightState::Breath(crate::state::Breath {
                period_ms: 1,
                hue_period_ms: 2,
                hue_span: 3,
                min_brightness: 1,
                max_brightness: 3,
                saturation: 4,
                hue: 5,
                group: [6, 7, 8, 0, 0, 0, 0],
                group_len: 3,
            })),
            LightAppearance::Breathing(_)
        ));
    }

    #[test]
    fn rate_scales_frames_over_the_window() {
        assert_eq!(windowed_rate(13, 250), 52);
        assert_eq!(windowed_rate(0, 250), 0);
        assert_eq!(windowed_rate(5, 0), 0);
    }

    #[test]
    fn overheated_rate_saturates_at_max() {
        assert_eq!(windowed_rate(1_000, 1_000), u8::MAX);
        assert_eq!(windowed_rate(u32::MAX, 1), u8::MAX);
    }

    #[test]
    fn an_unlit_panel_never_repaints_and_a_dim_one_only_on_a_change() {
        let base = Diagnostics::default();
        assert_eq!(base.demand.plane(PlaneKey::DISPLAY).mode, Mode::Continuous);

        let mut unlit = base.clone();
        unlit.demand.set(
            PlaneKey::DISPLAY,
            crate::demand::PlaneDemand::new(Mode::Off, 0, crate::demand::Sensitivity::Primary),
        );
        let mut wobble = unlit.clone();
        wobble.audio.phase = AudioPhase::Recording;

        assert!(
            !diag_repaint_due(
                DisplayPage::Audio,
                &unlit,
                &wobble,
                10 * DIAG_REPAINT_STEP_MS,
                true
            ),
            "a panel nobody is looking at does not get frames no matter what drifts"
        );

        let mut dim = base.clone();
        dim.demand.set(
            PlaneKey::DISPLAY,
            crate::demand::PlaneDemand::new(Mode::OnChange, 0, crate::demand::Sensitivity::Primary),
        );
        assert!(
            !diag_repaint_due(
                DisplayPage::Audio,
                &dim,
                &dim,
                10 * DIAG_REPAINT_STEP_MS,
                false
            ),
            "nothing changed, so there is nothing to redraw"
        );

        let mut dim_moved = dim.clone();
        dim_moved.audio.phase = AudioPhase::Recording;
        assert!(
            !diag_repaint_due(
                DisplayPage::Audio,
                &dim,
                &dim_moved,
                DIAG_REPAINT_STEP_MS,
                false
            ),
            "a dim panel waits out the base cadence before it spends a frame"
        );
        assert!(diag_repaint_due(
            DisplayPage::Audio,
            &dim,
            &dim_moved,
            DIAG_REPAINT_STEP_MS * 4,
            false
        ));
    }

    #[test]
    fn a_lamp_only_board_gets_the_status_on_its_first_surface_only() {
        // The setup lamp stands in for the page's first surface on a board that has no page to
        // draw it. Every other surface keeps whatever it declared.
        let mut ctl = RenderController::new();
        let status = DeviceState {
            lights: [LightState::Off, LightState::Off],
            network_enabled: true,
            panel: crate::state::Panel::Absent,
            link: LinkStatus {
                setup: true,
                front: Front::AccessPoint,
                ..LinkStatus::default()
            },
            ..DeviceState::default()
        };
        collect(&mut ctl, &status);
        assert_eq!(
            ctl.current(Slot::Light(0)),
            Some(SlotAppearance::Light {
                instance: 0,
                appearance: light_appearance(LightState::SETUP),
            }),
            "the lamp says the access point is up even though the page's light is off"
        );
        assert_eq!(
            ctl.current(Slot::Light(1)),
            Some(SlotAppearance::Light {
                instance: 1,
                appearance: LightAppearance::Off,
            }),
            "the substitution does not leak onto a second surface"
        );
    }

    #[test]
    fn the_snapshot_carries_the_demand_in_force_not_the_pages_own() {
        // Passing the demand in rather than deriving it: a build that turns a plane down
        // reaches the snapshot with the level it is actually running.
        let mut manager =
            DeviceManager::with_motion(true, crate::drivers::motion::MotionCapabilities::EMPTY)
                .with_planes(alloc::vec![
                    PlaneKey::MOTION,
                    PlaneKey::AUDIO,
                    PlaneKey::CAMERA,
                    PlaneKey::DISPLAY
                ])
                .with_audio(true)
                .with_camera(true)
                .with_boot_page(DisplayPage::Audio);
        // The demand follows the page that is up, and a device starts on its splash: ask what the
        // landed device's own page claims.
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(manager.state().page, DisplayPage::Audio);
        let quiet = manager.demand().clone();
        assert_eq!(
            quiet.plane(PlaneKey::AUDIO).mode,
            crate::demand::Mode::Continuous
        );

        manager = manager.with_override(crate::demand::parse_override("audio=off"));
        assert_eq!(
            manager.demand().plane(PlaneKey::AUDIO).mode,
            crate::demand::Mode::Off
        );

        let snapshot = light_diagnostics(&manager.state(), manager.demand());
        assert_eq!(
            snapshot.demand.plane(PlaneKey::AUDIO).mode,
            crate::demand::Mode::Off
        );
        assert_eq!(
            light_diagnostics(&manager.state(), &quiet)
                .demand
                .plane(PlaneKey::AUDIO)
                .mode,
            crate::demand::Mode::Continuous,
            "the page's own demand is still there to be passed when it is the one in force"
        );
    }
}

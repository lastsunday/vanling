//! The render layer: what the device's surfaces should show, and the registry that drives
//! the renderers which show it.

mod color;
mod lamp;
mod registry;
mod screen;

pub use lamp::LampRenderer;
pub use registry::Registry;
pub use screen::ScreenRenderer;

use crate::demand::{DemandKey, EffectiveDemand, Mode};
use crate::diagnostics::{BreathSnapshot, Diagnostics, LightSnapshot, TouchDiagnostics};
use crate::paint::{Rgb, rgb_hue, scale_brightness};
use crate::state::{Breath, DeviceState, DisplayPage, LightState, MAX_LIGHTS, SurfaceRole};
use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;

/// Light-mode codes carried by [`LightSnapshot::mode`]: `0` off, `1` breathing, `2` solid.
pub const MODE_OFF: u8 = 0;
pub const MODE_BREATH: u8 = 1;
pub const MODE_SOLID: u8 = 2;

/// Who an appearance is for: one wired surface, or the diagnostics overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    Surface(u8),
    Diagnostics,
}

/// The declared appearance of the light, derived from state; breathing is the schedule, not
/// a color frame — time-driven rendering is the renderer's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightAppearance {
    Off,
    Color(Rgb),
    Breathing(Breath),
}

/// The intended appearance of every known consumer, derived from [`DeviceState`].
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Surface {
        instance: u8,
        appearance: LightAppearance,
    },
    Diagnostics(Diagnostics),
}

impl Appearance {
    pub fn consumer(&self) -> Consumer {
        match self {
            Appearance::Surface { instance, .. } => Consumer::Surface(*instance),
            Appearance::Diagnostics(_) => Consumer::Diagnostics,
        }
    }
}

fn status_lights(state: &DeviceState) -> [LightState; MAX_LIGHTS] {
    let mut lights = state.lights;
    if let Some(status) = state.link_notice() {
        for (light, role) in lights.iter_mut().zip(state.surface_roles) {
            if role == SurfaceRole::Lamp {
                *light = status;
            }
        }
    }
    lights
}

/// Appearance of the light under `state`, without time information.
pub fn light_appearance(state: LightState) -> LightAppearance {
    match state {
        LightState::Off => LightAppearance::Off,
        LightState::Solid { color, brightness } => {
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

fn light_diagnostics(state: &DeviceState, demand: &EffectiveDemand) -> Diagnostics {
    Diagnostics {
        demand: *demand,
        touch: TouchDiagnostics {
            taps: state.touch.view.tap_count,
            presses: state.touch.view.press_count,
            long_presses: state.touch.view.long_press_count,
            ghost: state.touch.view.ghost_count,
            swipes: state.touch.view.swipe_count,
            last_swipe_dir: state.touch.view.last_swipe.map_or(0, |(dir, _)| dir.code()),
            last_swipe_dist: state.touch.view.last_swipe.map_or(0, |(_, dist)| dist),
            last_gesture_origin: state.touch.view.last_gesture_origin,
            last_gesture_end: state.touch.view.last_gesture_end,
            chip_gesture_id: state.touch.view.chip_gesture_id,
            points: state.touch.points.map(|p| p.map(|l| (l.x, l.y))),
            live_dir: state.touch.live_dir,
            two_finger_runs: state.touch.two_finger_runs,
            frames: state.touch.frames,
            finger: state.touch.view.finger,
            held_ms: state.touch.view.held_ms,
        },
        lights: state.lights.map(light_snapshot),
        page: state.page(),
        link: state.network.link,
        mac: state.network.mac,
        aps: state.network.aps,
        draft: state.network_page,
        motion: state.motion.sample,
        motion_counts: state.motion.counts,
        motion_caps: state.backed.motion_caps(),
        audio: state.audio_diagnostics(),
        playback: state.playback_diagnostics(),
    }
}

/// Minimum wall time between full-frame panel repaints driven by a diagnostics
/// drift. A repaint ships the whole 320×240 frame down one blocking SPI
/// transfer (on the order of 15 ms), so without a bound the live pages — the
/// Audio sweep follows the capture, the attitude dial the motion sample —
/// repaint every 20 ms snapshot and hold the shared cooperative executor in a
/// ~15 ms block almost continuously, starving the playback feed and the
/// capture poll past their ring runway. A page may raise it for itself in its
/// page manifest row; see [`repaint_step_ms`].
pub use crate::page::DIAG_REPAINT_STEP_MS;

pub fn diag_repaint_due(
    page: DisplayPage,
    prev: &Diagnostics,
    next: &Diagnostics,
    since_ms: u64,
    overlay: bool,
) -> bool {
    let step = repaint_step_ms(page);
    match next.demand.claim(DemandKey::DISPLAY).mode {
        Mode::Off => return false,
        Mode::OnChange => return since_ms >= step * 4 && prev != next,
        Mode::Continuous => {}
    }
    if since_ms < step {
        return false;
    }
    repaint_due(page, prev, next, overlay)
}

/// The least wall time this page waits between diagnostics-driven repaints. The Audio page
/// raises it in its manifest row: a capture poll closes two envelope columns every 20 ms, so
/// the default step already redraws a 200-column sweep every ~45 ms and this page alone held
/// the executor 40% of the wall time on the S3. At twice the step a frame advances the sweep
/// eight columns instead of four — still a sweep, at half the cost.
pub(crate) fn repaint_step_ms(page: DisplayPage) -> u64 {
    page.repaint_step_ms()
}

fn repaint_due(page: DisplayPage, prev: &Diagnostics, next: &Diagnostics, overlay: bool) -> bool {
    match page {
        // A frame arriving is a data event and not anything a snapshot diff can see.
        // Forcing a repaint here would ship a full frame that is a copy of the last one —
        // ~15 ms of blocking SPI.
        DisplayPage::Camera | DisplayPage::Splash => false,
        DisplayPage::Ambient => overlay,
        DisplayPage::Attitude => true,
        DisplayPage::Audio => {
            prev.audio.phase != next.audio.phase
                || prev.audio.envelope != next.audio.envelope
                || prev.audio.restarts != next.audio.restarts
                || prev.audio.dba_lsb != next.audio.dba_lsb
        }
        DisplayPage::Speaker => prev.playback != next.playback,
        DisplayPage::Network => {
            prev.link != next.link
                || prev.aps != next.aps
                || prev.draft != next.draft
                || prev.mac != next.mac
        }
    }
}

/// How a renderer wants to be driven after an appearance sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    TimeDriven,
}

/// Pluggable binding between one device and the render layer: the controller decides *what*
/// to show (central state diff), the renderer answers *how*, including whether it needs
/// time-driven frames.
pub trait Renderer {
    fn consumers(&self) -> &[Consumer];

    fn on_appearance(&mut self, appearance: Appearance, now_ms: u32) -> Activity;

    fn step(&mut self, now_ms: u32) -> Activity;

    /// Ships whatever the last appearance or step left buffered, if anything. Renderers that
    /// own no bus, or have nothing pending, answer `None`; the panel's transfer is awaited
    /// here so the executor keeps running between its DMA chunks.
    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        None
    }
}

/// Central judge of the device surface: diffs successive [`DeviceState`] snapshots and
/// reports only the appearances that changed, via a callback the render layer fans out to
/// matching renderers.
pub struct RenderController {
    last: Option<Box<Snapshot>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    diagnostics: Diagnostics,
    lights: [LightState; MAX_LIGHTS],
}

impl RenderController {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Diff `state` against the last snapshot, invoking `notify` per changed consumer; the
    /// first call reports every known consumer.
    ///
    /// What each consumer shows is selected from the state first — the diagnostics the screen
    /// reads, the appearance one surface reads — and only the consumers whose selection moved
    /// are notified. A renderer therefore never receives an appearance it would have to throw
    /// away, and the state is written once at the end, after the selections are compared.
    pub fn reconcile(
        &mut self,
        state: &DeviceState,
        demand: &EffectiveDemand,
        mut notify: impl FnMut(Appearance),
    ) -> bool {
        let next = Snapshot {
            diagnostics: light_diagnostics(state, demand),
            lights: status_lights(state),
        };
        let diagnostics_moved = self.last.as_deref() != Some(&next);
        let surfaces: [bool; MAX_LIGHTS] = core::array::from_fn(|instance| {
            state.surface_roles[instance] != SurfaceRole::Absent
                && self
                    .last
                    .as_deref()
                    .is_none_or(|last| last.lights[instance] != next.lights[instance])
        });
        // The screen draws the lights as well as the page, so a surface moving is also a
        // diagnostics move; the reverse is not true, and that is what this selection keeps.
        if diagnostics_moved || surfaces.iter().any(|moved| *moved) {
            notify(Appearance::Diagnostics(next.diagnostics));
        }
        for (instance, moved) in surfaces.iter().enumerate() {
            if *moved {
                notify(Appearance::Surface {
                    instance: instance as u8,
                    appearance: light_appearance(next.lights[instance]),
                });
            }
        }
        let changed = diagnostics_moved || surfaces.iter().any(|moved| *moved);
        if changed {
            match &mut self.last {
                Some(last) => **last = next,
                None => self.last = Some(Box::new(next)),
            }
        }
        changed
    }

    /// Last synced appearance for `consumer` (or `None` before the first sync), so a freshly
    /// registered renderer can catch up before the next diff.
    pub fn current(&self, consumer: Consumer) -> Option<Appearance> {
        let last = self.last.as_deref()?;
        Some(match consumer {
            Consumer::Surface(instance) => Appearance::Surface {
                instance,
                appearance: light_appearance(
                    last.lights[usize::from(instance).min(MAX_LIGHTS - 1)],
                ),
            },
            Consumer::Diagnostics => Appearance::Diagnostics(last.diagnostics),
        })
    }
}

impl Default for RenderController {
    fn default() -> Self {
        Self::new()
    }
}

/// Repaint rate (frames per second) over one window, `0` for an empty or degenerate window.
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
    use crate::state::{AudioPhase, Backed};

    #[test]
    fn a_share_is_reported_whole_and_never_over_a_hundred() {
        assert_eq!(windowed_percent(500_000, 2_000_000), 25);
        assert_eq!(windowed_percent(2_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(1, 1_000_000), 0);
        assert_eq!(windowed_percent(4_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(7_000, 0), 100);
    }
    use crate::drivers::network::{
        AccessPoint, ApList, Ipv4, LinkPhase, LinkStatus, SetupHost, Ssid,
    };
    use crate::drivers::playback::Sound;
    use crate::intent::{ActionIntent, AudioIntent, PageIntent};
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

    fn collect(ctl: &mut RenderController, s: &DeviceState) -> Vec<Appearance> {
        let mut seen = Vec::new();
        ctl.reconcile(s, &EffectiveDemand::default(), |a| seen.push(a));
        seen
    }

    fn landed() -> DeviceManager {
        let mut manager = DeviceManager::new();
        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
        manager
    }

    fn diagnostics_for(light: LightState) -> Diagnostics {
        light_diagnostics(&state(light), &EffectiveDemand::default())
    }

    #[test]
    fn the_board_address_reaches_the_panel_with_the_rest_of_the_header() {
        let mac = [0x02, 0x1f, 0x88, 0xab, 0xcd, 0xef];
        let mut state = state(LightState::boot());
        state.network.mac = Some(mac);
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

    /// The Audio page sets its own step, so the tests follow the table rather than the
    /// default — a page that changed its cadence without changing this would fail here.
    fn audio_step() -> u64 {
        repaint_step_ms(DisplayPage::Audio)
    }

    #[test]
    fn diag_repaint_waits_out_the_step() {
        let base = Diagnostics::default();
        let mut moved = base;
        moved.audio.phase = AudioPhase::Recording;
        assert!(!diag_repaint_due(
            DisplayPage::Audio,
            &base,
            &moved,
            audio_step() - 1,
            false
        ));
        assert!(diag_repaint_due(
            DisplayPage::Audio,
            &base,
            &moved,
            audio_step(),
            false
        ));
    }

    #[test]
    fn diag_repaint_ignores_the_wall_time_counter() {
        let base = Diagnostics::default();
        let mut ticked = base;
        ticked.audio.elapsed_ms = 1_234;
        assert!(!diag_repaint_due(
            DisplayPage::Audio,
            &base,
            &ticked,
            audio_step(),
            false
        ));
    }

    #[test]
    fn diag_repaint_audio_tracks_everything_else_it_draws() {
        let base = Diagnostics::default();
        let mut phase = base;
        phase.audio.phase = AudioPhase::Recording;
        let mut envelope = base;
        envelope.audio.envelope = live_envelope();
        let mut restarts = base;
        restarts.audio.restarts = 3;
        let mut level = base;
        level.audio.dba_lsb = 0x8000;
        for next in [phase, envelope, restarts, level] {
            assert!(
                diag_repaint_due(DisplayPage::Audio, &base, &next, audio_step(), false),
                "an Audio drift the page draws must repaint"
            );
        }
    }

    #[test]
    fn diag_repaint_attitude_follows_every_snapshot() {
        let base = Diagnostics::default();
        let mut ticked = base;
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
        let mut ticked = base;
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
        let mut playing = base;
        playing.playback.phase = PlaybackPhase::Playing;
        let mut source = base;
        source.playback.sound = Sound::Rise;
        let mut muted = base;
        muted.playback.muted = true;
        let mut played = base;
        played.playback.plays = 7;
        let mut dropped = base;
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
        let mut drifting = base;
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
        let mut associated = base;
        associated.link.phase = LinkPhase::Connecting;
        associated.link.ap = Some(AccessPoint {
            ssid: Ssid::from_octets(b"neighbour"),
            bssid: [1, 2, 3, 4, 5, 6],
            channel: 11,
            rssi: -55,
            secured: true,
        });
        let mut addressed = base;
        addressed.link.address = Some(Ipv4::from_octets([192, 168, 1, 136]));
        let mut scanned = base;
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
        let base = Diagnostics::default();
        let mut addressed = base;
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
        let mut drifting = base;
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
        let mut manager = DeviceManager::boot(crate::state::Boot {
            backed: Backed {
                audio: true,
                ..Backed::NONE
            },
            ..Default::default()
        });
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
        manager.apply_action(ActionIntent::Audio(AudioIntent::Start));
        manager.apply_action(ActionIntent::Page(PageIntent::NextPage));
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

        manager.apply_action(ActionIntent::Audio(AudioIntent::Stop));
        let stopped = light_diagnostics(&manager.state(), manager.demand()).audio;
        assert_eq!(stopped.phase, AudioPhase::Stopped);
        assert_eq!(
            stopped.envelope, poll.envelope,
            "the latch stands in once the driver moves on"
        );
    }

    #[test]
    fn the_diagnostics_projection_owns_every_state_domain() {
        // Compile-time guard rather than a behaviour test: `light_diagnostics` projects
        // `DeviceState` field by field, and a new domain must be looked at there instead of
        // silently reading as its `Default` on the panel.
        fn project_every_field(state: DeviceState) {
            let DeviceState {
                backed: _,
                nav: _,
                lights: _,
                surface_roles: _,
                notice: _,
                motion: _,
                audio: _,
                playback: _,
                network: _,
                network_page: _,
                touch: _,
            } = state;
        }
        project_every_field(DeviceState::default());
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
                Appearance::Diagnostics(diagnostics_for(LightState::boot())),
                Appearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Breathing(BOOT_BREATH)
                },
                Appearance::Surface {
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
                Appearance::Diagnostics(diagnostics_for(LightState::Off)),
                Appearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Off
                },
                Appearance::Surface {
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
        let mut moved = boot.clone();
        moved.touch.last = Some(crate::drivers::input::TouchEvent {
            points: [crate::drivers::input::TouchPoint {
                id: 0,
                x: 7,
                y: 8,
                status: crate::drivers::input::TouchStatus::Down,
            }; crate::drivers::input::MAX_TOUCH_POINTS],
            len: 1,
        });
        assert!(collect(&mut ctl, &moved).is_empty());
    }

    #[test]
    fn light_changes_emit_new_appearances() {
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::boot()));
        let target = state(LightState::Solid {
            color: Rgb(1, 2, 3),
            brightness: 255,
        });
        assert_eq!(
            collect(&mut ctl, &target),
            vec![
                Appearance::Diagnostics(diagnostics_for(target.lights[0])),
                Appearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(1, 2, 3))
                },
                Appearance::Surface {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(1, 2, 3))
                },
            ]
        );
        let recolored = state(LightState::Solid {
            color: Rgb(9, 9, 9),
            brightness: 255,
        });
        assert_eq!(
            collect(&mut ctl, &recolored),
            vec![
                Appearance::Diagnostics(diagnostics_for(recolored.lights[0])),
                Appearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(9, 9, 9))
                },
                Appearance::Surface {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(9, 9, 9))
                },
            ]
        );
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
            seen[0],
            Appearance::Diagnostics(t) if t == diagnostics_for(second.lights[0])
        ));
        assert!(matches!(
            seen[1],
            Appearance::Surface {
                instance: 0,
                appearance: LightAppearance::Breathing(_),
                ..
            }
        ));
        assert!(matches!(
            seen[2],
            Appearance::Surface {
                instance: 1,
                appearance: LightAppearance::Breathing(_),
                ..
            }
        ));
    }

    #[test]
    fn current_reports_last_synced_appearance_per_consumer() {
        let mut ctl = RenderController::new();
        assert_eq!(
            ctl.current(Consumer::Surface(0)),
            None,
            "nothing synced yet"
        );
        let plain = state(LightState::Solid {
            color: Rgb(7, 6, 5),
            brightness: 255,
        });
        collect(&mut ctl, &plain);
        assert_eq!(
            ctl.current(Consumer::Surface(0)),
            Some(Appearance::Surface {
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
            ctl.current(Consumer::Surface(0)),
            Some(Appearance::Surface {
                instance: 0,
                appearance: LightAppearance::Off
            })
        );
        assert_eq!(
            ctl.current(Consumer::Surface(1)),
            Some(Appearance::Surface {
                instance: 1,
                appearance: LightAppearance::Color(Rgb(3, 3, 3))
            })
        );
        assert!(matches!(
            ctl.current(Consumer::Diagnostics),
            Some(Appearance::Diagnostics(_))
        ));
        assert_eq!(
            ctl.current(Consumer::Surface(9)),
            Some(Appearance::Surface {
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
                Appearance::Diagnostics(light_diagnostics(&two, &EffectiveDemand::default())),
                Appearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Off,
                },
                Appearance::Surface {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(5, 5, 5)),
                },
            ]
        );
        let Appearance::Diagnostics(d) = seen[0] else {
            panic!("first diff must carry the diagnostics");
        };
        assert_eq!(d.lights.len(), MAX_LIGHTS);
        assert_eq!(d.lights[0].mode, 0);
        assert_eq!(d.lights[1].mode, 2);
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
                Appearance::Diagnostics(light_diagnostics(&changed, &EffectiveDemand::default())),
                Appearance::Surface {
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
        changed.touch.view.tap_count = 7;
        changed.touch.view.press_count = 9;
        changed.touch.view.long_press_count = 1;
        changed.touch.view.held_ms = 590;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.taps = 7;
        expected.touch.presses = 9;
        expected.touch.long_presses = 1;
        expected.touch.held_ms = 590;
        let seen = collect(&mut ctl, &changed);
        assert_eq!(
            seen,
            vec![
                Appearance::Diagnostics(expected),
                Appearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Off
                },
                Appearance::Surface {
                    instance: 1,
                    appearance: LightAppearance::Off
                },
            ]
        );
    }

    #[test]
    fn diagnostics_diff_only_sets_the_shifted_field() {
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut ghost = state(LightState::Off);
        ghost.touch.view.ghost_count = 3;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.ghost = 3;
        assert_eq!(
            collect(&mut ctl, &ghost),
            vec![Appearance::Diagnostics(expected)]
        );
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut presses = state(LightState::Off);
        presses.touch.view.press_count = 1;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.presses = 1;
        assert_eq!(
            collect(&mut ctl, &presses),
            vec![Appearance::Diagnostics(expected)]
        );
    }

    #[test]
    fn press_and_frame_diagnostics_carry_coordinates() {
        let mut ctl = RenderController::new();
        let mut manager = landed();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::intent::recognize(
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
            vec![Appearance::Diagnostics(expected)]
        );
        let mut ctl = RenderController::new();
        let mut manager = landed();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::intent::recognize(
            crate::drivers::input::InputEvent::Touch(crate::drivers::input::TouchEvent {
                points: [crate::drivers::input::TouchPoint {
                    id: 0,
                    x: 7,
                    y: 8,
                    status: crate::drivers::input::TouchStatus::Down,
                }; crate::drivers::input::MAX_TOUCH_POINTS],
                len: 1,
            }),
            0,
        ));
        manager.apply_input(crate::intent::recognize(
            crate::drivers::input::InputEvent::Touch(crate::drivers::input::TouchEvent {
                points: [crate::drivers::input::TouchPoint {
                    id: 0,
                    x: 9,
                    y: 10,
                    status: crate::drivers::input::TouchStatus::Contact,
                }; crate::drivers::input::MAX_TOUCH_POINTS],
                len: 1,
            }),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let Appearance::Diagnostics(diagnostics) = seen[0] else {
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
        manager.apply_input(crate::intent::recognize(
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
        manager.apply_input(crate::intent::recognize(
            crate::drivers::input::InputEvent::ChipGesture(0x10),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let Appearance::Diagnostics(diagnostics) = seen[0] else {
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
        let mut ctl = RenderController::new();
        let mut manager = crate::state::DeviceManager::new();
        collect(&mut ctl, &manager.state());
        manager.apply_input(crate::intent::recognize(
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
        let Appearance::Diagnostics(diagnostics) = seen[0] else {
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
                period_ms: crate::state::LightState::DEFAULT_PERIOD_MS as u16,
                hue_period_ms: crate::state::LightState::DEFAULT_HUE_PERIOD_MS as u16,
                hue_span: crate::state::LightState::DEFAULT_HUE_SPAN,
                group_len: crate::state::LightState::DEFAULT_GROUP_LEN,
                min_brightness: crate::state::LightState::DEFAULT_MIN_BRIGHTNESS,
                max_brightness: crate::state::LightState::DEFAULT_MAX_BRIGHTNESS,
                saturation: crate::state::LightState::DEFAULT_SATURATION,
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
        assert_eq!(base.demand.claim(DemandKey::DISPLAY).mode, Mode::Continuous);

        let mut unlit = base;
        unlit.demand.set(
            DemandKey::DISPLAY,
            crate::demand::Claim::new(Mode::Off, 0, crate::demand::Priority::Primary),
        );
        let mut wobble = unlit;
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

        let mut dim = base;
        dim.demand.set(
            DemandKey::DISPLAY,
            crate::demand::Claim::new(Mode::OnChange, 0, crate::demand::Priority::Primary),
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

        let mut dim_moved = dim;
        dim_moved.audio.phase = AudioPhase::Recording;
        assert!(
            !diag_repaint_due(DisplayPage::Audio, &dim, &dim_moved, audio_step(), false),
            "a dim panel waits out the base cadence before it spends a frame"
        );
        assert!(diag_repaint_due(
            DisplayPage::Audio,
            &dim,
            &dim_moved,
            audio_step() * 4,
            false
        ));
    }

    fn light_appearances(seen: Vec<Appearance>) -> Vec<(u8, LightAppearance)> {
        seen.into_iter()
            .filter_map(|appearance| match appearance {
                Appearance::Surface {
                    instance,
                    appearance,
                } => Some((instance, appearance)),
                Appearance::Diagnostics(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_link_notice_lands_on_every_lamp_and_no_display() {
        for (kinds, expected) in [
            (
                [SurfaceRole::Pages, SurfaceRole::Lamp],
                alloc::vec![
                    (0, LightAppearance::Off),
                    (1, light_appearance(LightState::SETUP))
                ],
            ),
            (
                [SurfaceRole::Lamp, SurfaceRole::Lamp],
                alloc::vec![
                    (0, light_appearance(LightState::SETUP)),
                    (1, light_appearance(LightState::SETUP)),
                ],
            ),
            (
                [SurfaceRole::Pages, SurfaceRole::Absent],
                alloc::vec![(0, LightAppearance::Off)],
            ),
            ([SurfaceRole::Absent, SurfaceRole::Absent], alloc::vec![]),
        ] {
            let mut ctl = RenderController::new();
            let status = DeviceState {
                lights: [LightState::Off, LightState::Off],
                surface_roles: kinds,
                backed: crate::state::Backed {
                    network: true,
                    ..crate::state::Backed::NONE
                },
                network: crate::state::NetworkState {
                    link: LinkStatus {
                        mode: crate::drivers::network::NetworkMode::Setup {
                            host: SetupHost::AccessPoint,
                            cancellable: false,
                        },
                        ..LinkStatus::default()
                    },
                    ..Default::default()
                },
                ..DeviceState::default()
            };
            assert_eq!(
                light_appearances(collect(&mut ctl, &status)),
                expected,
                "{kinds:?}"
            );
        }
    }

    #[test]
    fn the_snapshot_carries_the_demand_in_force_not_the_pages_own() {
        let setup = crate::state::Boot {
            backed: Backed {
                motion: Some(crate::drivers::motion::MotionCapabilities::EMPTY),
                audio: true,
                camera: true,
                ..Backed::NONE
            },
            demand_keys: alloc::vec![
                DemandKey::MOTION,
                DemandKey::AUDIO,
                DemandKey::CAMERA,
                DemandKey::DISPLAY
            ],
            boot_page: DisplayPage::Audio,
            ..Default::default()
        };
        let mut quiet_manager = DeviceManager::boot(setup.clone());
        quiet_manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
        assert_eq!(quiet_manager.state().page(), DisplayPage::Audio);
        let quiet = *quiet_manager.demand();
        assert_eq!(
            quiet.claim(DemandKey::AUDIO).mode,
            crate::demand::Mode::Continuous
        );

        let mut manager = DeviceManager::boot(crate::state::Boot {
            overrides: crate::demand::parse_override("audio=off"),
            ..setup
        });
        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
        assert_eq!(
            manager.demand().claim(DemandKey::AUDIO).mode,
            crate::demand::Mode::Off
        );

        let snapshot = light_diagnostics(&manager.state(), manager.demand());
        assert_eq!(
            snapshot.demand.claim(DemandKey::AUDIO).mode,
            crate::demand::Mode::Off
        );
        assert_eq!(
            light_diagnostics(&manager.state(), &quiet)
                .demand
                .claim(DemandKey::AUDIO)
                .mode,
            crate::demand::Mode::Continuous,
            "the page's own demand is still there to be passed when it is the one in force"
        );
    }
}

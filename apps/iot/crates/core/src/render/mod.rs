//! The render layer: what the device's surfaces should show, and the registry that drives
//! the renderers which show it.

mod colour;
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

/// Light-mode codes carried by [`LightSnapshot::mode`]: `0` off, `1` breathing, `2` solid.
pub const MODE_OFF: u8 = 0;
pub const MODE_BREATH: u8 = 1;
pub const MODE_SOLID: u8 = 2;

/// A device slot the render layer knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
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

/// The intended appearance of every known slot, derived from [`DeviceState`].
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotAppearance {
    Surface {
        instance: u8,
        appearance: LightAppearance,
    },
    Diagnostics(Diagnostics),
}

impl SlotAppearance {
    pub fn slot(&self) -> Slot {
        match self {
            SlotAppearance::Surface { instance, .. } => Slot::Surface(*instance),
            SlotAppearance::Diagnostics(_) => Slot::Diagnostics,
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
        demand: demand.clone(),
        touch: TouchDiagnostics {
            taps: state.touch.tap_count,
            presses: state.touch.press_count,
            long_presses: state.touch.long_press_count,
            ghost: state.touch.ghost_count,
            swipes: state.touch.swipe_count,
            last_swipe_dir: state.touch.last_swipe.map_or(0, |(dir, _)| dir.code()),
            last_swipe_dist: state.touch.last_swipe.map_or(0, |(_, dist)| dist),
            last_gesture_origin: state.touch.last_gesture_origin,
            last_gesture_end: state.touch.last_gesture_end,
            chip_gesture_id: state.touch.chip_gesture_id,
            points: state.touch.points.map(|p| p.map(|l| (l.x, l.y))),
            live_dir: state.touch.live_dir,
            two_finger_runs: state.touch.two_finger_runs,
            frames: state.touch.frames,
            finger: state.touch.finger,
            held_ms: state.touch.held_ms,
        },
        lights: state.lights.map(light_snapshot),
        page: state.page,
        link: state.network.link,
        mac: state.network.mac,
        aps: state.network.aps,
        setup: state.network.setup,
        motion_enabled: state.backed.motion.is_some(),
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
/// capture poll past their ring runway.
pub const DIAG_REPAINT_STEP_MS: u64 = 40;

pub fn diag_repaint_due(
    page: DisplayPage,
    prev: &Diagnostics,
    next: &Diagnostics,
    since_ms: u64,
    overlay: bool,
) -> bool {
    match next.demand.claim(DemandKey::DISPLAY).mode {
        Mode::Off => return false,
        Mode::OnChange => return since_ms >= DIAG_REPAINT_STEP_MS * 4 && prev != next,
        Mode::Continuous => {}
    }
    if since_ms < DIAG_REPAINT_STEP_MS {
        return false;
    }
    crate::page::repaint_due(page, prev, next, overlay)
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
    fn slots(&self) -> &[Slot];

    fn on_appearance(&mut self, appearance: SlotAppearance, now_ms: u32) -> Activity;

    fn step(&mut self, now_ms: u32) -> Activity;
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

    /// Diff `state` against the last snapshot, invoking `notify` per changed slot; the
    /// first call reports every known slot.
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
            notify(SlotAppearance::Diagnostics(next.diagnostics.clone()));
            for (instance, light) in next.lights.iter().enumerate() {
                if state.surface_roles[instance] == SurfaceRole::Absent {
                    continue;
                }
                let moved = match self.last.as_deref() {
                    None => true,
                    Some(last) => last.lights[instance] != *light,
                };
                if moved {
                    notify(SlotAppearance::Surface {
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

    /// Last synced appearance for `slot` (or `None` before the first sync), so a freshly
    /// registered renderer can catch up before the next diff.
    pub fn current(&self, slot: Slot) -> Option<SlotAppearance> {
        let last = self.last.as_deref()?;
        Some(match slot {
            Slot::Surface(instance) => SlotAppearance::Surface {
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
    use crate::drivers::network::{AccessPoint, ApList, Front, Ipv4, LinkPhase, LinkStatus, Ssid};
    use crate::drivers::playback::Sound;
    use crate::intent::{AudioIntent, BusinessIntent, PageIntent};
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

    fn landed() -> DeviceManager {
        let mut manager = DeviceManager::new();
        manager.apply_business(BusinessIntent::Page(PageIntent::SplashOver));
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
        let mut manager = DeviceManager::boot(crate::state::Setup {
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
        manager.apply_business(BusinessIntent::Audio(AudioIntent::Start));
        manager.apply_business(BusinessIntent::Page(PageIntent::NextPage));
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

        manager.apply_business(BusinessIntent::Audio(AudioIntent::Stop));
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
                SlotAppearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Breathing(BOOT_BREATH)
                },
                SlotAppearance::Surface {
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
                SlotAppearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Off
                },
                SlotAppearance::Surface {
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
            contacts: 1,
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
                SlotAppearance::Diagnostics(diagnostics_for(target.lights[0])),
                SlotAppearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(1, 2, 3))
                },
                SlotAppearance::Surface {
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
                SlotAppearance::Diagnostics(diagnostics_for(recolored.lights[0])),
                SlotAppearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Color(Rgb(9, 9, 9))
                },
                SlotAppearance::Surface {
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
            seen[0].clone(),
            SlotAppearance::Diagnostics(t) if t == diagnostics_for(second.lights[0])
        ));
        assert!(matches!(
            seen[1],
            SlotAppearance::Surface {
                instance: 0,
                appearance: LightAppearance::Breathing(_),
                ..
            }
        ));
        assert!(matches!(
            seen[2],
            SlotAppearance::Surface {
                instance: 1,
                appearance: LightAppearance::Breathing(_),
                ..
            }
        ));
    }

    #[test]
    fn current_reports_last_synced_appearance_per_slot() {
        let mut ctl = RenderController::new();
        assert_eq!(ctl.current(Slot::Surface(0)), None, "nothing synced yet");
        let plain = state(LightState::Solid {
            color: Rgb(7, 6, 5),
            brightness: 255,
        });
        collect(&mut ctl, &plain);
        assert_eq!(
            ctl.current(Slot::Surface(0)),
            Some(SlotAppearance::Surface {
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
            ctl.current(Slot::Surface(0)),
            Some(SlotAppearance::Surface {
                instance: 0,
                appearance: LightAppearance::Off
            })
        );
        assert_eq!(
            ctl.current(Slot::Surface(1)),
            Some(SlotAppearance::Surface {
                instance: 1,
                appearance: LightAppearance::Color(Rgb(3, 3, 3))
            })
        );
        assert!(matches!(
            ctl.current(Slot::Diagnostics),
            Some(SlotAppearance::Diagnostics(_))
        ));
        assert_eq!(
            ctl.current(Slot::Surface(9)),
            Some(SlotAppearance::Surface {
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
                SlotAppearance::Diagnostics(light_diagnostics(&two, &EffectiveDemand::default())),
                SlotAppearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Off,
                },
                SlotAppearance::Surface {
                    instance: 1,
                    appearance: LightAppearance::Color(Rgb(5, 5, 5)),
                },
            ]
        );
        let SlotAppearance::Diagnostics(d) = seen[0].clone() else {
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
                SlotAppearance::Diagnostics(light_diagnostics(
                    &changed,
                    &EffectiveDemand::default()
                )),
                SlotAppearance::Surface {
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
        changed.touch.tap_count = 7;
        changed.touch.press_count = 9;
        changed.touch.long_press_count = 1;
        changed.touch.held_ms = 590;
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
                SlotAppearance::Surface {
                    instance: 0,
                    appearance: LightAppearance::Off
                },
                SlotAppearance::Surface {
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
        ghost.touch.ghost_count = 3;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.ghost = 3;
        assert_eq!(
            collect(&mut ctl, &ghost),
            vec![SlotAppearance::Diagnostics(expected)]
        );
        let mut ctl = RenderController::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut presses = state(LightState::Off);
        presses.touch.press_count = 1;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.presses = 1;
        assert_eq!(
            collect(&mut ctl, &presses),
            vec![SlotAppearance::Diagnostics(expected)]
        );
    }

    #[test]
    fn press_and_frame_diagnostics_carry_coordinates() {
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

        let mut unlit = base.clone();
        unlit.demand.set(
            DemandKey::DISPLAY,
            crate::demand::Claim::new(Mode::Off, 0, crate::demand::Sensitivity::Primary),
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
            DemandKey::DISPLAY,
            crate::demand::Claim::new(Mode::OnChange, 0, crate::demand::Sensitivity::Primary),
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

    fn light_appearances(seen: Vec<SlotAppearance>) -> Vec<(u8, LightAppearance)> {
        seen.into_iter()
            .filter_map(|appearance| match appearance {
                SlotAppearance::Surface {
                    instance,
                    appearance,
                } => Some((instance, appearance)),
                SlotAppearance::Diagnostics(_) => None,
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
                        setup: true,
                        front: Front::AccessPoint,
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
        let setup = crate::state::Setup {
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
        quiet_manager.apply_business(BusinessIntent::Page(PageIntent::SplashOver));
        assert_eq!(quiet_manager.state().page, DisplayPage::Audio);
        let quiet = quiet_manager.demand().clone();
        assert_eq!(
            quiet.claim(DemandKey::AUDIO).mode,
            crate::demand::Mode::Continuous
        );

        let mut manager = DeviceManager::boot(crate::state::Setup {
            overrides: crate::demand::parse_override("audio=off"),
            ..setup
        });
        manager.apply_business(BusinessIntent::Page(PageIntent::SplashOver));
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

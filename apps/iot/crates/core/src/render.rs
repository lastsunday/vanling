use crate::diagnostics::{BreathSnapshot, Diagnostics, LightSnapshot, TouchDiagnostics};
use crate::drivers::light::{MAX_LIGHTS, Rgb, rgb_hue, scale_brightness};
use crate::state::{Breath, DeviceState, LightState};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Which slot this appearance belongs to.
    pub fn slot(&self) -> Slot {
        match self {
            SlotAppearance::Light { instance, .. } => Slot::Light(*instance),
            SlotAppearance::Diagnostics(_) => Slot::Diagnostics,
        }
    }
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
fn light_diagnostics(state: &DeviceState) -> Diagnostics {
    Diagnostics {
        touch: TouchDiagnostics {
            taps: state.tap_count,
            presses: state.press_count,
            double_taps: state.double_tap_count,
            triple_taps: state.triple_tap_count,
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
        motion_enabled: state.motion_enabled,
        motion: state.motion,
        motion_counts: state.motion_counts,
        motion_caps: state.motion_caps,
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
/// `Send`: renderers stay in the render task; cross-task registration wraps
/// them in `Box<dyn Renderer + Send>` at the app seam.
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
    last: Option<DeviceState>,
}

impl RenderController {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Diff `state` against the last snapshot, invoking `notify` per changed
    /// slot; the first call reports every known slot. Returns whether any slot
    /// changed, so bindings can react once.
    pub fn reconcile(
        &mut self,
        state: &DeviceState,
        mut notify: impl FnMut(SlotAppearance),
    ) -> bool {
        let last = self.last.as_ref();
        let changed = match last {
            None => true,
            Some(last) => last != state,
        };
        if changed {
            // Diagnostics first so panel renderers repaint the digits onto the
            // current surface, then each moved light appearance repaints over
            // it in one write instead of an old-color frame followed by a new
            // one.
            notify(SlotAppearance::Diagnostics(light_diagnostics(state)));
            for (instance, light) in state.lights.iter().enumerate() {
                let moved = match last {
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
            self.last = Some(state.clone());
        }
        changed
    }

    /// Last synced appearance for `slot` (or `None` before the first sync), so a
    /// freshly registered renderer can catch up before the next diff.
    pub fn current(&self, slot: Slot) -> Option<SlotAppearance> {
        let state = self.last.as_ref()?;
        Some(match slot {
            Slot::Light(instance) => SlotAppearance::Light {
                instance,
                appearance: light_appearance(
                    state.lights[usize::from(instance).min(MAX_LIGHTS - 1)],
                ),
            },
            Slot::Diagnostics => SlotAppearance::Diagnostics(light_diagnostics(state)),
        })
    }
}

impl Default for RenderController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ctl.reconcile(s, |a| seen.push(a));
        seen
    }

    /// The diagnostics the controller derives for a given light over a zero
    /// base: the light snapshot is the only field determined by the light
    /// itself.
    fn diagnostics_for(light: LightState) -> Diagnostics {
        light_diagnostics(&state(light))
    }

    #[test]
    fn first_sync_reports_boot_then_silence() {
        let mut ctl = RenderController::new();
        let boot = state(LightState::boot());
        let mut seen = Vec::new();
        assert!(
            ctl.reconcile(&boot, |a| seen.push(a)),
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
        assert!(!ctl.reconcile(&boot, |_| {}), "identical state is silent");
        let off = state(LightState::Off);
        assert!(ctl.reconcile(&off, |_| {}), "changed state is a change");
        assert!(!ctl.reconcile(&off, |_| {}), "settled state is silent");
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
            seen[0],
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
                SlotAppearance::Diagnostics(light_diagnostics(&two)),
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
        let SlotAppearance::Diagnostics(d) = seen[0] else {
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
                SlotAppearance::Diagnostics(light_diagnostics(&changed)),
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
        changed.double_tap_count = 2;
        changed.long_press_count = 1;
        changed.touch_held_ms = 590;
        let mut expected = diagnostics_for(LightState::Off);
        expected.touch.taps = 7;
        expected.touch.presses = 9;
        expected.touch.double_taps = 2;
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
        let mut manager = crate::state::DeviceManager::new();
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
        let mut manager = crate::state::DeviceManager::new();
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
        let SlotAppearance::Diagnostics(diagnostics) = seen[0] else {
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
        let SlotAppearance::Diagnostics(diagnostics) = seen[0] else {
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
            }),
            0,
        ));
        let state = manager.state();
        let seen = collect(&mut ctl, &state);
        let SlotAppearance::Diagnostics(diagnostics) = seen[0] else {
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
}

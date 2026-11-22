//! The central judge: diffs successive state snapshots and reports only what moved.

use crate::shared::model::{DeviceState, LightState, MAX_LIGHTS, SurfaceRole};
use crate::shared::snapshot::{FrameSnapshot, PageSnapshot};
use alloc::boxed::Box;
use iot_core::demand::{DemandKey, EffectiveDemand};

use super::appearance::{Appearance, Topic};
use super::projection::{frame_snapshot, light_appearance, status_lights};

/// Central judge of the device surface: diffs successive [`DeviceState`] snapshots and
/// reports only the appearances that changed, via a callback the render layer fans out to
/// matching renderers.
pub struct Differ {
    last: Option<Box<Baseline>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Baseline {
    frame: FrameSnapshot,
    lights: [LightState; MAX_LIGHTS],
}

impl Differ {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Diff `state` against the last snapshot, invoking `notify` per changed topic; the first
    /// call reports every known topic.
    ///
    /// Each topic's selection is taken from the state first, and only topics whose selection
    /// moved are notified, so a renderer never receives an appearance it would have to throw
    /// away. The state is written once at the end, after the selections are compared.
    pub fn diff(
        &mut self,
        state: &DeviceState,
        pages: PageSnapshot,
        demand: &EffectiveDemand,
        mut notify: impl FnMut(Appearance<'_>),
    ) -> bool {
        let next = Baseline {
            frame: frame_snapshot(state, pages, demand),
            lights: status_lights(state),
        };
        let display = next.frame.demand.claim(DemandKey::DISPLAY).mode;
        let display_moved = self
            .last
            .as_deref()
            .is_none_or(|last| last.frame.demand.claim(DemandKey::DISPLAY).mode != display);
        let surfaces: [bool; MAX_LIGHTS] = core::array::from_fn(|instance| {
            state.surface_roles[instance] != SurfaceRole::Absent
                && self
                    .last
                    .as_deref()
                    .is_none_or(|last| last.lights[instance] != next.lights[instance])
        });
        // The screen draws the lights as well as the page, so a surface moving is also a
        // frame move; the reverse is not true, and that is what this selection keeps.
        let moved = self.last.as_deref() != Some(&next) || surfaces.iter().any(|moved| *moved);
        if display_moved {
            notify(Appearance::Demand(display));
        }
        if moved {
            notify(Appearance::Frame(&next.frame));
        }
        for (instance, moved) in surfaces.iter().enumerate() {
            if *moved {
                notify(Appearance::Surface {
                    instance: instance as u8,
                    light: light_appearance(next.lights[instance]),
                });
            }
        }
        if moved {
            match &mut self.last {
                Some(last) => **last = next,
                None => self.last = Some(Box::new(next)),
            }
        }
        moved
    }

    /// Last diffed appearance for `topic` (or `None` before the first diff), so a freshly
    /// registered renderer can catch up before the next diff.
    pub fn latest(&self, topic: Topic) -> Option<Appearance<'_>> {
        let last = self.last.as_deref()?;
        Some(match topic {
            Topic::Surface(instance) => Appearance::Surface {
                instance,
                light: light_appearance(
                    last.lights[usize::from(DeviceState::light_index(instance))],
                ),
            },
            Topic::Frame => Appearance::Frame(&last.frame),
            Topic::Demand => Appearance::Demand(last.frame.demand.claim(DemandKey::DISPLAY).mode),
        })
    }
}

impl Default for Differ {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::render::LightAppearance;
    use iot_core::demand::Mode;
    use iot_core::light::Rgb;

    #[test]
    fn first_diff_reports_boot_then_silence() {
        let mut ctl = Differ::new();
        let boot = state(LightState::boot());
        let mut seen = Vec::new();
        assert!(
            ctl.diff(
                &boot,
                PageSnapshot::default(),
                &EffectiveDemand::default(),
                |a| seen.push(Seen::of(a))
            ),
            "first sync is a change"
        );
        assert_eq!(
            seen,
            vec![
                Seen::Demand(Mode::Continuous),
                Seen::Frame(frame_for(LightState::boot())),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Breathing(BOOT_BREATH)
                },
                Seen::Surface {
                    instance: 1,
                    light: LightAppearance::Breathing(BOOT_BREATH)
                },
            ]
        );
        assert!(
            !ctl.diff(
                &boot,
                PageSnapshot::default(),
                &EffectiveDemand::default(),
                |_| {}
            ),
            "identical state is silent"
        );
        let off = state(LightState::Off);
        assert!(
            ctl.diff(
                &off,
                PageSnapshot::default(),
                &EffectiveDemand::default(),
                |_| {}
            ),
            "changed state is a change"
        );
        assert!(
            !ctl.diff(
                &off,
                PageSnapshot::default(),
                &EffectiveDemand::default(),
                |_| {}
            ),
            "settled state is silent"
        );
    }

    #[test]
    fn off_emits_off_once() {
        let mut ctl = Differ::new();
        collect(&mut ctl, &state(LightState::boot()));
        let off = state(LightState::Off);
        assert_eq!(
            collect(&mut ctl, &off),
            vec![
                Seen::Frame(frame_for(LightState::Off)),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Off
                },
                Seen::Surface {
                    instance: 1,
                    light: LightAppearance::Off
                },
            ]
        );
        assert_eq!(collect(&mut ctl, &off), vec![]);
    }

    #[test]
    fn a_state_change_no_renderer_can_see_stays_silent() {
        let mut ctl = Differ::new();
        let boot = state(LightState::boot());
        collect(&mut ctl, &boot);
        let mut moved = boot.clone();
        moved.touch.last = Some(iot_core::drivers::input::TouchEvent {
            points: [iot_core::drivers::input::TouchPoint {
                id: 0,
                x: 7,
                y: 8,
                status: iot_core::drivers::input::TouchStatus::Down,
            }; iot_core::drivers::input::MAX_TOUCH_POINTS],
            len: 1,
        });
        assert!(collect(&mut ctl, &moved).is_empty());
    }

    #[test]
    fn light_changes_emit_new_appearances() {
        let mut ctl = Differ::new();
        collect(&mut ctl, &state(LightState::boot()));
        let target = state(LightState::Solid {
            color: Rgb(1, 2, 3),
            brightness: 255,
        });
        assert_eq!(
            collect(&mut ctl, &target),
            vec![
                Seen::Frame(frame_for(target.lights[0])),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Color(Rgb(1, 2, 3))
                },
                Seen::Surface {
                    instance: 1,
                    light: LightAppearance::Color(Rgb(1, 2, 3))
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
                Seen::Frame(frame_for(recolored.lights[0])),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Color(Rgb(9, 9, 9))
                },
                Seen::Surface {
                    instance: 1,
                    light: LightAppearance::Color(Rgb(9, 9, 9))
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
        assert_eq!(seen.len(), 3, "the frame rides along with the light diff");
        assert!(matches!(
            seen[0],
            Seen::Frame(ref t) if *t == frame_for(second.lights[0])
        ));
        assert!(matches!(
            seen[1],
            Seen::Surface {
                instance: 0,
                light: LightAppearance::Breathing(_),
            }
        ));
        assert!(matches!(
            seen[2],
            Seen::Surface {
                instance: 1,
                light: LightAppearance::Breathing(_),
            }
        ));
    }

    #[test]
    fn current_reports_last_synced_appearance_per_topic() {
        let mut ctl = Differ::new();
        assert_eq!(ctl.latest(Topic::Surface(0)), None, "nothing synced yet");
        let plain = state(LightState::Solid {
            color: Rgb(7, 6, 5),
            brightness: 255,
        });
        collect(&mut ctl, &plain);
        assert_eq!(
            ctl.latest(Topic::Surface(0)),
            Some(Appearance::Surface {
                instance: 0,
                light: LightAppearance::Color(Rgb(7, 6, 5))
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
            ctl.latest(Topic::Surface(0)),
            Some(Appearance::Surface {
                instance: 0,
                light: LightAppearance::Off
            })
        );
        assert_eq!(
            ctl.latest(Topic::Surface(1)),
            Some(Appearance::Surface {
                instance: 1,
                light: LightAppearance::Color(Rgb(3, 3, 3))
            })
        );
        assert!(matches!(
            ctl.latest(Topic::Frame),
            Some(Appearance::Frame(_))
        ));
        assert_eq!(
            ctl.latest(Topic::Demand),
            Some(Appearance::Demand(Mode::Continuous))
        );
        assert_eq!(
            ctl.latest(Topic::Surface(9)),
            Some(Appearance::Surface {
                instance: 9,
                light: LightAppearance::Color(Rgb(3, 3, 3))
            })
        );
    }

    #[test]
    fn diff_carries_the_frame_before_surfaces_in_wiring_order() {
        let mut ctl = Differ::new();
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
                Seen::Demand(Mode::Continuous),
                Seen::Frame(frame_snapshot(
                    &two,
                    PageSnapshot::default(),
                    &EffectiveDemand::default()
                )),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Off,
                },
                Seen::Surface {
                    instance: 1,
                    light: LightAppearance::Color(Rgb(5, 5, 5)),
                },
            ]
        );
        let Seen::Frame(d) = &seen[1] else {
            panic!("first diff must carry the frame");
        };
        assert_eq!(d.lights.len(), MAX_LIGHTS);
        assert_eq!(d.lights[0].mode, 0);
        assert_eq!(d.lights[1].mode, 2);
        let mut ctl = Differ::new();
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
                Seen::Frame(frame_snapshot(
                    &changed,
                    PageSnapshot::default(),
                    &EffectiveDemand::default()
                )),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Color(Rgb(1, 2, 3)),
                },
            ],
            "only the moved surface and the frame it carries are emitted"
        );
    }

    #[test]
    fn frame_change_emits_both_fields_before_light() {
        let mut ctl = Differ::new();
        collect(&mut ctl, &state(LightState::boot()));
        let mut changed = state(LightState::Off);
        changed.touch.view.tap_count = 7;
        changed.touch.view.press_count = 9;
        changed.touch.view.long_press_count = 1;
        changed.touch.view.held_ms = 590;
        let mut expected = frame_for(LightState::Off);
        expected.touch.taps = 7;
        expected.touch.presses = 9;
        expected.touch.long_presses = 1;
        expected.touch.held_ms = 590;
        let seen = collect(&mut ctl, &changed);
        assert_eq!(
            seen,
            vec![
                Seen::Frame(expected),
                Seen::Surface {
                    instance: 0,
                    light: LightAppearance::Off
                },
                Seen::Surface {
                    instance: 1,
                    light: LightAppearance::Off
                },
            ]
        );
    }

    #[test]
    fn frame_diff_only_sets_the_shifted_field() {
        let mut ctl = Differ::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut ghost = state(LightState::Off);
        ghost.touch.view.ghost_count = 3;
        let mut expected = frame_for(LightState::Off);
        expected.touch.ghost = 3;
        assert_eq!(collect(&mut ctl, &ghost), vec![Seen::Frame(expected)]);
        let mut ctl = Differ::new();
        collect(&mut ctl, &state(LightState::Off));
        let mut presses = state(LightState::Off);
        presses.touch.view.press_count = 1;
        let mut expected = frame_for(LightState::Off);
        expected.touch.presses = 1;
        assert_eq!(collect(&mut ctl, &presses), vec![Seen::Frame(expected)]);
    }
}

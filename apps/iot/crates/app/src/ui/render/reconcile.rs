//! The central judge: diffs successive state snapshots and reports only what moved.

use crate::model::state::{DeviceState, LightState, MAX_LIGHTS, SurfaceRole};
use crate::ui::diagnostics::Diagnostics;
use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use iot_core::demand::EffectiveDemand;

use super::appearance::{Appearance, Consumer, light_appearance, light_diagnostics, status_lights};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::render::LightAppearance;
    use crate::ui::render::test_support::*;
    use iot_core::paint::Rgb;

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
        let second = state(LightState::Breath(crate::model::state::Breath {
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
}

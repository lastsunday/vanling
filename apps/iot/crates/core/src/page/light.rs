use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::intent::{ActionIntent, InputIntent, LightIntent};
use crate::paint::{GROUP_CAPACITY, Rgb};
use crate::state::{Breath, DeviceState, LightState};

pub const PALETTE: [Rgb; 7] = [
    Rgb(255, 64, 48),
    Rgb(255, 160, 48),
    Rgb(255, 224, 64),
    Rgb(80, 200, 96),
    Rgb(32, 200, 220),
    Rgb(72, 128, 255),
    Rgb(190, 96, 255),
];

pub const BREATH_PERIODS_MS: [u32; 10] =
    [125, 250, 375, 500, 750, 1_000, 2_000, 3_000, 5_000, 8_000];

pub const SOLID_BRIGHTNESS_STEPS: [u8; 6] = [32, 64, 100, 140, 200, 255];

pub const BRIGHTNESS_ENVELOPE_STEP: u8 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorGroup {
    pub hue_span: u8,
    pub group: [u8; GROUP_CAPACITY],
    pub group_len: u8,
}

impl ColorGroup {
    pub fn into_breath(self, base: Breath) -> Breath {
        Breath {
            hue_span: self.hue_span,
            group: self.group,
            group_len: self.group_len,
            ..base
        }
    }
}

pub const COLOR_GROUPS: [ColorGroup; 4] = [
    ColorGroup {
        hue_span: 255,
        group: [16, 32, 3, 0, 0, 0, 0],
        group_len: 3,
    },
    ColorGroup {
        hue_span: 255,
        group: [0, 21, 43, 85, 128, 170, 213],
        group_len: 7,
    },
    ColorGroup {
        hue_span: 255,
        group: [28, 0, 0, 0, 0, 0, 0],
        group_len: 1,
    },
    ColorGroup {
        hue_span: 255,
        group: [0, 0, 0, 0, 0, 0, 0],
        group_len: 0,
    },
];

pub(super) fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) => click(current, op.source),
        InputEvent::Button(ButtonEvent::DoubleClick) => step(current, op.source),
        InputEvent::Button(ButtonEvent::LongPress) => cycle(current, op.source),
        InputEvent::Gesture(GestureEvent::Tap { .. }) => tap(current, op.source),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            swipe(current, op.source, direction)
        }
        InputEvent::Gesture(GestureEvent::LongPress { .. }) => cycle_panel(current, op.source),
        _ => ActionIntent::Invalid,
    }
}

pub(super) fn click(current: &DeviceState, source: u8) -> ActionIntent {
    let slot = slot(current, source);
    target_for(advance_color(current.lights[slot]), slot)
}

pub(super) fn tap(current: &DeviceState, source: u8) -> ActionIntent {
    target_for(advance_color(current.lights[slot(current, source)]), 0)
}

pub(super) fn step(current: &DeviceState, source: u8) -> ActionIntent {
    let slot = slot(current, source);
    target_for(advance_step(current.lights[slot]), slot)
}

pub(super) fn cycle(current: &DeviceState, source: u8) -> ActionIntent {
    let slot = slot(current, source);
    ActionIntent::Light(LightIntent {
        instance: slot as u8,
        state: cycle_mode(current.lights[slot]),
    })
}

pub(super) fn cycle_panel(current: &DeviceState, source: u8) -> ActionIntent {
    ActionIntent::Light(LightIntent {
        instance: 0,
        state: cycle_mode(current.lights[slot(current, source)]),
    })
}

pub(super) fn swipe(current: &DeviceState, source: u8, direction: SwipeDirection) -> ActionIntent {
    target_for(
        swipe_light(current.lights[slot(current, source)], direction),
        0,
    )
}

// An out-of-range source clamps to the last installed surface rather than panicking.
fn slot(current: &DeviceState, source: u8) -> usize {
    usize::from(source).min(current.lights.len() - 1)
}

fn target_for(step: Option<LightState>, instance: usize) -> ActionIntent {
    step.map_or(ActionIntent::Invalid, |state| {
        ActionIntent::Light(LightIntent {
            instance: instance as u8,
            state,
        })
    })
}

fn cycle_mode(current: LightState) -> LightState {
    match current {
        LightState::Off => LightState::default(),
        LightState::Breath { .. } => LightState::Solid {
            color: PALETTE[0],
            brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
        },
        LightState::Solid { .. } => LightState::Off,
    }
}

fn advance_color(current: LightState) -> Option<LightState> {
    swipe_color(current, true)
}

fn advance_step(current: LightState) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => {
            let next_period = advance(&BREATH_PERIODS_MS, breath.period_ms);
            Some(LightState::Breath(Breath {
                period_ms: next_period,
                ..breath
            }))
        }
        LightState::Solid { color, brightness } => {
            let next = advance_u8(&SOLID_BRIGHTNESS_STEPS, brightness);
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

fn swipe_light(current: LightState, direction: SwipeDirection) -> Option<LightState> {
    match direction {
        SwipeDirection::Left => swipe_color(current, false),
        SwipeDirection::Right => swipe_color(current, true),
        SwipeDirection::Down => swipe_brightness(current, false),
        SwipeDirection::Up => swipe_brightness(current, true),
        SwipeDirection::UpLeft
        | SwipeDirection::UpRight
        | SwipeDirection::DownLeft
        | SwipeDirection::DownRight => Some(current),
    }
}

fn swipe_color(current: LightState, forward: bool) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => {
            let next_group = if forward {
                advance_color_group(&breath)
            } else {
                retreat_color_group(&breath)
            };
            Some(LightState::Breath(next_group.into_breath(breath)))
        }
        LightState::Solid { color, brightness } => {
            let next_color = if forward {
                advance_palette(color)
            } else {
                retreat_palette(color)
            };
            Some(LightState::Solid {
                color: next_color,
                brightness,
            })
        }
    }
}

fn swipe_brightness(current: LightState, up: bool) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => Some(LightState::Breath(shift_envelope(breath, up))),
        LightState::Solid { color, brightness } => {
            let next = if up {
                advance_u8(&SOLID_BRIGHTNESS_STEPS, brightness)
            } else {
                retreat_u8(&SOLID_BRIGHTNESS_STEPS, brightness)
            };
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

fn shift_envelope(breath: Breath, up: bool) -> Breath {
    let step = if up {
        i16::from(BRIGHTNESS_ENVELOPE_STEP)
    } else {
        -i16::from(BRIGHTNESS_ENVELOPE_STEP)
    };
    let guard = i16::from(BRIGHTNESS_ENVELOPE_STEP);
    let min = (i16::from(breath.min_brightness) + step).clamp(0, 255 - guard);
    let max = (i16::from(breath.max_brightness) + step).clamp(guard, 255);
    Breath {
        min_brightness: min as u8,
        max_brightness: max as u8,
        ..breath
    }
}

fn table_step<T: Copy + PartialEq>(table: &[T], current: T, dir: isize) -> T {
    let idx = table.iter().position(|&v| v == current).unwrap_or(0);
    let len = table.len() as isize;
    table[((idx as isize + dir).rem_euclid(len)) as usize]
}

fn advance(table: &[u32], current: u32) -> u32 {
    table_step(table, current, 1)
}

fn advance_u8(table: &[u8], current: u8) -> u8 {
    table_step(table, current, 1)
}

fn retreat_u8(table: &[u8], current: u8) -> u8 {
    table_step(table, current, -1)
}

fn advance_palette(current: Rgb) -> Rgb {
    table_step(&PALETTE, current, 1)
}

fn retreat_palette(current: Rgb) -> Rgb {
    table_step(&PALETTE, current, -1)
}

fn color_group_idx(breath: &Breath) -> usize {
    COLOR_GROUPS
        .iter()
        .position(|c| c.group == breath.group && c.group_len == breath.group_len)
        .unwrap_or(0)
}

fn advance_color_group(breath: &Breath) -> ColorGroup {
    let idx = color_group_idx(breath);
    COLOR_GROUPS[(idx + 1) % COLOR_GROUPS.len()]
}

fn retreat_color_group(breath: &Breath) -> ColorGroup {
    let idx = color_group_idx(breath);
    COLOR_GROUPS[(idx + COLOR_GROUPS.len() - 1) % COLOR_GROUPS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{ActionIntent, LightIntent, recognize, translate};
    use crate::page::test_support::{
        BREATH_BASE, DEFAULT_BREATH, long_press, op, state, swipe_event, tap_gesture, target,
    };
    use crate::state::{DeviceState, LightState};

    mod buttons {
        use super::*;

        #[test]
        fn click_advances_color_in_every_mode() {
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(LightState::Off)
                ),
                ActionIntent::Invalid
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                })
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(LightState::Solid {
                        color: PALETTE[2],
                        brightness: 140,
                    })
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[3],
                        brightness: 140,
                    },
                })
            );
        }

        #[test]
        fn long_press_cycles_the_mode_ring() {
            assert_eq!(
                translate(&op(long_press()), &state(LightState::Off)),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: DEFAULT_BREATH,
                })
            );
            let slow = Breath {
                period_ms: 5_000,
                ..BREATH_BASE
            };
            assert_eq!(
                translate(&op(long_press()), &state(LightState::Breath(slow))),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
                    },
                })
            );
            assert_eq!(
                translate(
                    &op(long_press()),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    })
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Off,
                })
            );
            let mut light = LightState::Off;
            light = target(long_press(), light).unwrap();
            assert!(matches!(light, LightState::Breath(_)));
            light = target(long_press(), light).unwrap();
            assert!(matches!(light, LightState::Solid { .. }));
            light = target(long_press(), light).unwrap();
            assert_eq!(light, LightState::Off);
        }

        #[test]
        fn double_click_steps_period_and_brightness() {
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Off)
                ),
                ActionIntent::Invalid
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(DEFAULT_BREATH)
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Breath(Breath {
                        period_ms: 5_000,
                        ..BREATH_BASE
                    }),
                })
            );
            let mut breath = Breath {
                period_ms: BREATH_PERIODS_MS[0],
                ..BREATH_BASE
            };
            for i in 0..BREATH_PERIODS_MS.len() {
                let business = translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Breath(breath)),
                );
                let ActionIntent::Light(LightIntent { state, .. }) = business else {
                    panic!("double click must stay in breathing");
                };
                assert!(
                    matches!(state, LightState::Breath(_)),
                    "double click must stay in breathing"
                );
                let LightState::Breath(next) = state else {
                    unreachable!()
                };
                assert_eq!(
                    next.period_ms,
                    BREATH_PERIODS_MS[(i + 1) % BREATH_PERIODS_MS.len()],
                    "step {i}: wrong period reached"
                );
                breath = next;
            }
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    })
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 200,
                    },
                })
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    })
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                })
            );
        }
    }
    mod gestures {
        use super::*;

        #[test]
        fn tap_mirrors_the_button_click() {
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                ActionIntent::Invalid
            );
            assert_eq!(
                target(tap_gesture(0), DEFAULT_BREATH),
                Some(LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)))
            );
            assert_eq!(
                target(
                    tap_gesture(0),
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    }
                ),
                Some(LightState::Solid {
                    color: PALETTE[1],
                    brightness: 255,
                })
            );
        }

        #[test]
        fn long_press_mirrors_the_button_mode_ring() {
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::LongPress {
                        id: 0,
                        x: 0,
                        y: 0,
                        held_ms: 0
                    })),
                    &state(LightState::Off)
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: DEFAULT_BREATH,
                })
            );
            assert_eq!(
                target(
                    InputEvent::Gesture(GestureEvent::LongPress {
                        id: 0,
                        x: 1,
                        y: 2,
                        held_ms: 0
                    }),
                    DEFAULT_BREATH
                ),
                Some(LightState::Solid {
                    color: PALETTE[0],
                    brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
                })
            );
        }
    }
    mod swipes {
        use super::*;

        #[test]
        fn step_color_and_brightness_per_axis() {
            let last = *PALETTE.last().unwrap();
            let breathe = LightState::Breath(BREATH_BASE);
            let cases: [(SwipeDirection, LightState, LightState, &str); 8] = [
                (
                    SwipeDirection::Right,
                    breathe,
                    LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                    "step right walks into the rainbow-seven group",
                ),
                (
                    SwipeDirection::Left,
                    breathe,
                    LightState::Breath(COLOR_GROUPS[3].into_breath(BREATH_BASE)),
                    "step left from head wraps to the full-hue sweep",
                ),
                (
                    SwipeDirection::Right,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    },
                    LightState::Solid {
                        color: PALETTE[1],
                        brightness: 140,
                    },
                    "step right advances the palette",
                ),
                (
                    SwipeDirection::Left,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 60,
                    },
                    LightState::Solid {
                        color: last,
                        brightness: 60,
                    },
                    "step left from head wraps to the last palette entry",
                ),
                (
                    SwipeDirection::Up,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 60,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 64,
                    },
                    "an unknown 60 falls back to head 32 and advances to 64",
                ),
                (
                    SwipeDirection::Up,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                    "the top step wraps to the floor",
                ),
                (
                    SwipeDirection::Down,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 100,
                    },
                    "step down the brightness ladder",
                ),
                (
                    SwipeDirection::Down,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    },
                    "the floor wraps to the top step",
                ),
            ];
            for (direction, current, expected, why) in cases {
                assert_eq!(
                    target(swipe_event(direction), current),
                    Some(expected),
                    "{why} ({direction:?})"
                );
            }
            for direction in [
                SwipeDirection::Right,
                SwipeDirection::Left,
                SwipeDirection::Up,
                SwipeDirection::Down,
            ] {
                assert_eq!(
                    translate(&op(swipe_event(direction)), &state(LightState::Off)),
                    ActionIntent::Invalid,
                    "{direction:?} on Off has no table to step"
                );
            }
        }

        #[test]
        fn shift_the_brightness_envelope_together() {
            assert_eq!(
                target(swipe_event(SwipeDirection::Up), DEFAULT_BREATH),
                Some(LightState::Breath(Breath {
                    min_brightness: 24 + BRIGHTNESS_ENVELOPE_STEP,
                    max_brightness: 80 + BRIGHTNESS_ENVELOPE_STEP,
                    ..BREATH_BASE
                }))
            );
            assert_eq!(
                target(swipe_event(SwipeDirection::Down), DEFAULT_BREATH),
                Some(LightState::Breath(Breath {
                    min_brightness: 0,
                    max_brightness: 48,
                    ..BREATH_BASE
                }))
            );
            let at_bottom = Breath {
                min_brightness: 8,
                max_brightness: 40,
                ..BREATH_BASE
            };
            let sunk = shift_envelope(at_bottom, false);
            assert_eq!(sunk.min_brightness, 0, "floor clamps at 0");
            assert_eq!(
                sunk.max_brightness, BRIGHTNESS_ENVELOPE_STEP,
                "ceiling rides the wall guard so the envelope keeps its span"
            );
            let at_top = Breath {
                min_brightness: 240,
                max_brightness: 255,
                ..BREATH_BASE
            };
            let lifted = shift_envelope(at_top, true);
            assert_eq!(
                lifted.min_brightness,
                255 - BRIGHTNESS_ENVELOPE_STEP,
                "floor parks at 255-guard so the envelope keeps its span"
            );
            assert_eq!(lifted.max_brightness, 255, "ceiling clamps at 255");
            let mut breath = Breath {
                min_brightness: 24,
                max_brightness: 80,
                ..BREATH_BASE
            };
            for _ in 0..32 {
                breath = shift_envelope(breath, true);
                assert!(
                    breath.max_brightness as u16 - breath.min_brightness as u16
                        >= u16::from(BRIGHTNESS_ENVELOPE_STEP),
                    "upward shifts keep at least a full step of span"
                );
            }
            for _ in 0..32 {
                breath = shift_envelope(breath, false);
                assert!(
                    breath.max_brightness as u16 - breath.min_brightness as u16
                        >= u16::from(BRIGHTNESS_ENVELOPE_STEP),
                    "downward shifts keep at least a full step of span"
                );
            }
        }

        #[test]
        fn diagonal_axis_targets_identity() {
            for direction in [
                SwipeDirection::UpLeft,
                SwipeDirection::UpRight,
                SwipeDirection::DownLeft,
                SwipeDirection::DownRight,
            ] {
                assert_eq!(
                    target(swipe_event(direction), DEFAULT_BREATH),
                    Some(DEFAULT_BREATH),
                    "{direction:?} must not move the light"
                );
            }
        }
    }
    mod routing {
        use super::*;

        fn two_lights() -> DeviceState {
            DeviceState {
                lights: [
                    LightState::Off,
                    LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    },
                ],
                ..state(LightState::Off)
            }
        }

        #[test]
        fn button_source_routes_to_its_light_surface() {
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::Click), 1),
                    &two_lights()
                ),
                ActionIntent::Light(LightIntent {
                    instance: 1,
                    state: LightState::Solid {
                        color: PALETTE[1],
                        brightness: 140,
                    },
                }),
                "each button drives the surface its wiring order names"
            );
            assert_eq!(
                translate(&recognize(long_press(), 1), &two_lights()),
                ActionIntent::Light(LightIntent {
                    instance: 1,
                    state: LightState::Off,
                }),
                "the mode cycle walks surface 1 from its own solid head"
            );
        }

        #[test]
        fn out_of_range_source_clamps_to_last_surface() {
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::DoubleClick), 7),
                    &two_lights()
                ),
                ActionIntent::Light(LightIntent {
                    instance: 1,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 200,
                    },
                }),
                "out-of-range sources clamp to the last surface"
            );
        }

        #[test]
        fn panel_touches_only_the_first_surface() {
            assert_eq!(
                translate(&op(tap_gesture(0)), &two_lights()),
                ActionIntent::Invalid,
                "a tap on the panel touches only the first surface"
            );
        }
    }
    mod tables {
        use super::*;

        #[test]
        fn advance_falls_back_to_head_when_current_not_in_table() {
            assert_eq!(advance(&BREATH_PERIODS_MS, 9999), BREATH_PERIODS_MS[1]);
            assert_eq!(advance_palette(Rgb(0, 0, 0)), PALETTE[1]);
            assert_eq!(
                advance_u8(&SOLID_BRIGHTNESS_STEPS, 7),
                SOLID_BRIGHTNESS_STEPS[1]
            );
            let unknown = Breath {
                group: [200, 250, 30, 0, 0, 0, 0],
                group_len: 3,
                ..BREATH_BASE
            };
            assert_eq!(advance_color_group(&unknown), COLOR_GROUPS[1]);
        }

        #[test]
        fn retreat_and_table_step_wrap_backwards() {
            let last = *PALETTE.last().unwrap();
            assert_eq!(retreat_palette(PALETTE[0]), last);
            assert_eq!(advance_palette(last), PALETTE[0]);
            assert_eq!(retreat_u8(&SOLID_BRIGHTNESS_STEPS, 32), 255);
            assert_eq!(retreat_u8(&SOLID_BRIGHTNESS_STEPS, 140), 100);
            assert_eq!(table_step(&SOLID_BRIGHTNESS_STEPS, 7, -1), 255);
        }

        #[test]
        fn seven_color_preset_uses_every_hue_slot() {
            let expected: [u8; GROUP_CAPACITY] = [0, 21, 43, 85, 128, 170, 213];
            let rainbow = COLOR_GROUPS[1];
            assert_eq!(rainbow.group_len, expected.len() as u8);
            assert_eq!(rainbow.group, expected);
        }
    }
}

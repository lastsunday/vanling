use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::light::{GROUP_CAPACITY, Rgb};
use crate::state::{Breath, DeviceManager, DeviceState, LightState};

/// Standard-color palette for solid mode, cycled by a single click or a
/// horizontal swipe: seven hues spanning the wheel
/// (red→orange→yellow→green→cyan→blue→purple), so the readout digit and the
/// eye agree.
pub const PALETTE: [Rgb; 7] = [
    Rgb(255, 64, 48),
    Rgb(255, 160, 48),
    Rgb(255, 224, 64),
    Rgb(80, 200, 96),
    Rgb(32, 200, 220),
    Rgb(72, 128, 255),
    Rgb(190, 96, 255),
];

/// Breathing periods (ms) cycled by the button's double-click advance on
/// breathing mode, from a sub-second shimmer to a slow ambient drift. The boot
/// default (`DeviceManager::DEFAULT_PERIOD_MS`, 3 s) sits in the table so a
/// wrap lands on it.
pub const BREATH_PERIODS_MS: [u32; 10] =
    [125, 250, 375, 500, 750, 1_000, 2_000, 3_000, 5_000, 8_000];

/// Solid-mode brightness steps (8-bit), dark to full, cycled by the button's
/// double-click advance and the vertical swipes on the solid color; the color
/// rides on top unchanged.
pub const SOLID_BRIGHTNESS_STEPS: [u8; 6] = [32, 64, 100, 140, 200, 255];

/// One vertical swipe moves the breathing envelope (`min`/`max` brightness
/// together) this many levels. The same width doubles as the wall guard kept
/// next to either 8-bit edge, so a swipe can never drive `max` down to `min`.
pub const BRIGHTNESS_ENVELOPE_STEP: u8 = 32;

/// A color-group preset for breathing mode, switched by the single click (or a
/// horizontal swipe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorGroup {
    pub hue_span: u8,
    pub group: [u8; GROUP_CAPACITY],
    pub group_len: u8,
}

impl ColorGroup {
    /// Overlay the group's hue path onto a base breath: a preset only varies
    /// the hue path, so the envelope/saturation ride along unchanged. The one
    /// place the triplet is spread back into a [`Breath`].
    pub fn into_breath(self, base: Breath) -> Breath {
        Breath {
            hue_span: self.hue_span,
            group: self.group,
            group_len: self.group_len,
            ..base
        }
    }
}

/// Color-group presets cycled by the single click's advance on breathing
/// mode; the last entry is the full-hue sweep, the "七彩" finale of the color
/// walk.
pub const COLOR_GROUPS: [ColorGroup; 4] = [
    // Warm sunset: amber, golden, deep rose.
    ColorGroup {
        hue_span: 255,
        group: [16, 32, 3, 0, 0, 0, 0],
        group_len: 3,
    },
    // Rainbow seven: red → orange → yellow → green → cyan → blue → purple,
    // walking the hue wheel the short way around.
    ColorGroup {
        hue_span: 255,
        group: [0, 21, 43, 85, 128, 170, 213],
        group_len: 7,
    },
    // Standalone warm white.
    ColorGroup {
        hue_span: 255,
        group: [28, 0, 0, 0, 0, 0, 0],
        group_len: 1,
    },
    // Full-hue sweep across the whole wheel.
    ColorGroup {
        hue_span: 255,
        group: [0, 0, 0, 0, 0, 0, 0],
        group_len: 0,
    },
];

/// A pure device-level operation: what an input source reported, carrying the
/// wiring-order source that raised it. Recognition attaches no business
/// meaning — no target, no mode, no tables — so the operation plane never reads
/// or encodes device state. [`recognize`] is the only producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationIntent {
    /// Wiring-order source that raised the event.
    pub source: u8,
    /// The raw/folded input signal, verbatim from the driver.
    pub event: InputEvent,
}

/// A business intent: an absolute target the device should move to. Producers
/// compute it from the pipeline context (the manager holds the current state);
/// the two light-bearing planes never share a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessIntent {
    /// A no-op: the operation recorded its diagnostics but moves nothing
    /// (e.g. a click/tap on `Off`, whose tables have no step to take, or a
    /// pulse/read-back that is diagnostic-only by nature).
    Invalid,
    /// Set the `instance`-th light surface to an absolute target state.
    SetLight {
        instance: u8,
        state: LightState,
    },
    TogglePage,
}

/// The management-pipe message: one pipe carries both planes. The operation
/// plane's signals travel as `Operation` and are interpreted into a `Business`
/// intent by the consumer that owns the device state; producers that already
/// speak business (e.g. a network drive) send `Business` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Operation(OperationIntent),
    Business(BusinessIntent),
}

/// Stateless recognition: fold a raw input event plus its wiring-order source
/// into an operation intent. Recognition never reads device state or the
/// business tables — the input task only packages what the panel reported.
pub fn recognize(event: InputEvent, source: u8) -> OperationIntent {
    OperationIntent { source, event }
}

/// Interpret an operation against the current device state into an absolute
/// business target. The target computation stays here: buttons advance their
/// own surface (the slot the wiring order names), touch gestures always move
/// the first surface (one device-level panel drives surface 0). Pulses and raw
/// snapshots only ever record diagnostics, so they land on [`BusinessIntent::Invalid`].
pub fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    let slot = light_slot(current, op.source);
    let light = current.lights[slot];
    match op.event {
        // A click advances the color; `Off` has none, so it reads as no-op.
        InputEvent::Button(ButtonEvent::Click) => target_for(advance_color(light), slot),
        InputEvent::Button(ButtonEvent::DoubleClick) => target_for(advance_step(light), slot),
        InputEvent::Button(ButtonEvent::TripleClick) => BusinessIntent::TogglePage,
        InputEvent::Gesture(GestureEvent::TripleTap { .. }) => BusinessIntent::TogglePage,
        // Cycles the mode; always resolves, so any state is the way back on.
        InputEvent::Button(ButtonEvent::LongPress) => BusinessIntent::SetLight {
            instance: slot as u8,
            state: cycle_mode(light),
        },
        // A press-down pulse folds into the live points; no light move.
        InputEvent::Gesture(GestureEvent::Press { .. }) => BusinessIntent::Invalid,
        // An anomaly pulse tallies; no light move.
        InputEvent::Gesture(GestureEvent::Ghost) => BusinessIntent::Invalid,
        // A raw chip gesture is a diagnostic read-back; never moves the light.
        InputEvent::ChipGesture(_) | InputEvent::Motion(_) => BusinessIntent::Invalid,
        // Classified touch gestures mirror the button moves in their own
        // tallying variants; the device-level panel always drives surface 0.
        InputEvent::Gesture(GestureEvent::Tap { .. }) => target_for(advance_color(light), 0),
        InputEvent::Gesture(GestureEvent::DoubleTap { .. }) => target_for(advance_step(light), 0),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            target_for(swipe(light, direction), 0)
        }
        InputEvent::Gesture(GestureEvent::LongPress { .. }) => BusinessIntent::SetLight {
            instance: 0,
            state: cycle_mode(light),
        },
        // A raw snapshot is diagnostic-only; the render layer keeps the
        // controller's native multi-point ability, never a light transition.
        InputEvent::Touch(_) => BusinessIntent::Invalid,
    }
}

/// Fold an optional table step into a business intent: `None` means the mode
/// has no state to advance (an `Off` surface), which is an `Invalid` no-op.
fn target_for(step: Option<LightState>, instance: usize) -> BusinessIntent {
    step.map_or(BusinessIntent::Invalid, |state| BusinessIntent::SetLight {
        instance: instance as u8,
        state,
    })
}

/// The light surface a wiring-order source drives: its own button instance,
/// or the first surface for the device-level panel's gestures (`source == 0`).
/// Out-of-range sources clamp to the last installed surface rather than panic
/// the interpreter.
fn light_slot(current: &DeviceState, source: u8) -> usize {
    usize::from(source).min(current.lights.len() - 1)
}

/// Cycle the light mode one step: `Off` wakes to boot breathing, breathing
/// hands the wheel to the first solid color at full brightness, solid powers
/// off. The long-press's action — the most deliberate input — owns the
/// heaviest transition and never needs a no-op arm.
fn cycle_mode(current: LightState) -> LightState {
    match current {
        LightState::Off => LightState::default(),
        LightState::Breath { .. } => LightState::Solid {
            color: PALETTE[0],
            brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
        },
        LightState::Solid { .. } => LightState::Off,
    }
}

/// Advance the color one slot (wrapping): the color group in breath, the
/// palette in solid. The single click's action — exactly a right-swipe, so the
/// two land on the same next color.
fn advance_color(current: LightState) -> Option<LightState> {
    swipe_color(current, true)
}

/// Step the current mode's table by one slot (wrapping): breathing period for
/// breath, brightness for solid. The double-click's action.
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

/// A resolved swipe's light move: horizontal steps the color, vertical the
/// brightness (the breathing envelope as a whole, the solid step table
/// otherwise); the diagonals resolve to an identity target so they tally
/// without moving a light. `None` on `Off`.
fn swipe(current: LightState, direction: SwipeDirection) -> Option<LightState> {
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

/// Slide a breathing envelope's floor and ceiling together by
/// [`BRIGHTNESS_ENVELOPE_STEP`], guarding either 8-bit wall — both bounds move
/// together, so `max` always stays a full step above `min` and an equal
/// envelope (which would make `LO HI` meaningless) is impossible.
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

/// Look up `current` in `table` and move `dir` slots (wrapping), where `dir`
/// is `+1` forward and `-1` backward. Falls back to the first entry if
/// `current` is not found, then still moves, so an unknown value walks off
/// position zero rather than staying stuck there.
fn table_step<T: Copy + PartialEq>(table: &[T], current: T, dir: isize) -> T {
    let idx = table.iter().position(|&v| v == current).unwrap_or(0);
    let len = table.len() as isize;
    table[((idx as isize + dir).rem_euclid(len)) as usize]
}

/// Look up `current` in `table` and return the next value (wrapping).
/// Falls back to the first entry if `current` is not found.
fn advance(table: &[u32], current: u32) -> u32 {
    table_step(table, current, 1)
}

fn advance_u8(table: &[u8], current: u8) -> u8 {
    table_step(table, current, 1)
}

fn retreat_u8(table: &[u8], current: u8) -> u8 {
    table_step(table, current, -1)
}

/// Look up `current` in `palette` and return the next color (wrapping).
/// Falls back to the first entry if `current` is not found.
fn advance_palette(current: Rgb) -> Rgb {
    table_step(&PALETTE, current, 1)
}

fn retreat_palette(current: Rgb) -> Rgb {
    table_step(&PALETTE, current, -1)
}

/// Look up `breath`'s color group in `COLOR_GROUPS` and return the next
/// preset (wrapping); falls back to the first entry if not found. Matching
/// keys on `group`/`group_len` — the hue span rides along with every preset,
/// so an unknown span must not hide a known trajectory.
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
    use crate::drivers::input::{
        FingerLast, MAX_TOUCH_POINTS, MAX_TRACKED_POINTS, TouchEvent, TouchPoint, TouchStatus,
    };
    use crate::drivers::light::MAX_LIGHTS;
    use crate::drivers::motion::{MotionCapabilities, MotionCounts};

    const DEFAULT_BREATH: LightState = LightState::Breath(BREATH_BASE);

    const BREATH_BASE: Breath = Breath {
        period_ms: DeviceManager::DEFAULT_PERIOD_MS,
        hue_period_ms: DeviceManager::DEFAULT_HUE_PERIOD_MS,
        hue_span: DeviceManager::DEFAULT_HUE_SPAN,
        min_brightness: DeviceManager::DEFAULT_MIN_BRIGHTNESS,
        max_brightness: DeviceManager::DEFAULT_MAX_BRIGHTNESS,
        saturation: DeviceManager::DEFAULT_SATURATION,
        hue: DeviceManager::DEFAULT_HUE,
        group: DeviceManager::DEFAULT_GROUP,
        group_len: DeviceManager::DEFAULT_GROUP_LEN,
    };

    fn state(light: LightState) -> DeviceState {
        DeviceState {
            lights: [light; MAX_LIGHTS],
            page: crate::state::DisplayPage::Ambient,
            motion_enabled: true,
            motion: None,
            motion_counts: MotionCounts::default(),
            motion_caps: MotionCapabilities::EMPTY,
            touch: None,
            touch_points: [None; MAX_TRACKED_POINTS],
            live_dir: [0; MAX_TRACKED_POINTS],
            two_finger_runs: 0,
            touch_frames: 0,
            finger: [FingerLast::default(); MAX_TRACKED_POINTS],
            touch_held_ms: 0,
            tap_count: 0,
            press_count: 0,
            double_tap_count: 0,
            triple_tap_count: 0,
            long_press_count: 0,
            ghost_count: 0,
            swipe_count: 0,
            last_swipe: None,
            last_gesture_origin: None,
            last_gesture_end: None,
            chip_gesture_id: 0,
        }
    }

    fn op(event: InputEvent) -> OperationIntent {
        recognize(event, 0)
    }

    /// The `SetLight` target `translate` yields for surface 0, or `None` when
    /// the gesture is an `Invalid` no-op.
    fn target(event: InputEvent, current: LightState) -> Option<LightState> {
        match translate(&op(event), &state(current)) {
            BusinessIntent::SetLight { state, .. } => Some(state),
            BusinessIntent::Invalid | BusinessIntent::TogglePage => None,
        }
    }

    fn tap_gesture(held_ms: u16) -> InputEvent {
        InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 0,
            y: 0,
            held_ms,
        })
    }

    fn double_tap() -> InputEvent {
        InputEvent::Gesture(GestureEvent::DoubleTap {
            id: 0,
            x: 0,
            y: 0,
            end_x: 1,
            end_y: 1,
            held_ms: 0,
        })
    }

    fn swipe_event(direction: SwipeDirection) -> InputEvent {
        InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction,
            x: 0,
            y: 0,
            end_x: 0,
            end_y: 0,
            held_ms: 0,
            distance_px: 80,
        })
    }

    mod recognize {
        use super::*;

        #[test]
        fn wraps_event_and_source() {
            let event = InputEvent::Button(ButtonEvent::Click);
            let intent = recognize(event, 3);
            assert_eq!(intent.source, 3);
            assert_eq!(intent.event, event);
        }

        #[test]
        fn carries_swipe_fields_verbatim() {
            let event = InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Up,
                x: 10,
                y: 20,
                end_x: 10,
                end_y: 5,
                held_ms: 75,
                distance_px: 99,
            });
            let intent = recognize(event, 0);
            assert_eq!(intent.source, 0);
            assert_eq!(
                intent.event, event,
                "recognition keeps the gesture's tracking fields byte-for-byte"
            );
        }
    }

    mod buttons {
        use super::*;

        #[test]
        fn click_advances_color_in_every_mode() {
            // Off has no color a click can advance; only a long press turns it on.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(LightState::Off)
                ),
                BusinessIntent::Invalid
            );
            // Breath walks the color groups; solid walks the palette.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(LightState::Solid {
                        color: PALETTE[2],
                        brightness: 140,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[3],
                        brightness: 140,
                    },
                }
            );
        }

        #[test]
        fn long_press_cycles_the_mode_ring() {
            // Off wakes to the default breath.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::LongPress)),
                    &state(LightState::Off)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: DEFAULT_BREATH,
                }
            );
            // The outgoing state's period and brightness never leak into solid.
            let slow = Breath {
                period_ms: 5_000,
                ..BREATH_BASE
            };
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::LongPress)),
                    &state(LightState::Breath(slow))
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
                    },
                }
            );
            // Solid powers back off from any brightness.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::LongPress)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Off,
                }
            );
            // A full ring returns home: off → breathing → solid → off.
            let mut light = LightState::Off;
            light = target(InputEvent::Button(ButtonEvent::LongPress), light).unwrap();
            assert!(matches!(light, LightState::Breath(_)));
            light = target(InputEvent::Button(ButtonEvent::LongPress), light).unwrap();
            assert!(matches!(light, LightState::Solid { .. }));
            light = target(InputEvent::Button(ButtonEvent::LongPress), light).unwrap();
            assert_eq!(light, LightState::Off);
        }

        #[test]
        fn double_click_steps_period_and_brightness() {
            // Off is a no-op.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Off)
                ),
                BusinessIntent::Invalid
            );
            // Breathing period steps 3000 → 5000...
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(Breath {
                        period_ms: 5_000,
                        ..BREATH_BASE
                    }),
                }
            );
            // ...then one full lap from the head (125): every slot in order,
            // wrapping back to the head.
            let mut breath = Breath {
                period_ms: BREATH_PERIODS_MS[0],
                ..BREATH_BASE
            };
            for i in 0..BREATH_PERIODS_MS.len() {
                let business = translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Breath(breath)),
                );
                let BusinessIntent::SetLight { state, .. } = business else {
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
            // Solid brightness steps 140 → 200 and wraps 255 → 32, color intact.
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 140,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 200,
                    },
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::DoubleClick)),
                    &state(LightState::Solid {
                        color: PALETTE[0],
                        brightness: 255,
                    })
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 32,
                    },
                }
            );
        }
    }

    mod gestures {
        use super::*;

        #[test]
        fn pulses_tally_without_moving_the_light() {
            // A raw touch snapshot is diagnostic-only, whatever the light mode.
            let touch = InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 0,
                    x: 10,
                    y: 20,
                    status: TouchStatus::Down,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 2,
            });
            assert_eq!(
                translate(&op(touch), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            let release = InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 3,
                    x: 1,
                    y: 2,
                    status: TouchStatus::Release,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 0,
            });
            assert_eq!(
                translate(&op(release), &state(DEFAULT_BREATH)),
                BusinessIntent::Invalid
            );
            // Press-down, ghost anomalies and chip read-backs tally on the
            // operation plane without ever moving a light.
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::Press {
                        id: 0,
                        x: 3,
                        y: 4
                    })),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Gesture(GestureEvent::Ghost)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(&op(InputEvent::ChipGesture(0x10)), &state(DEFAULT_BREATH)),
                BusinessIntent::Invalid
            );
        }

        #[test]
        fn tap_and_double_tap_mirror_the_button() {
            // No color, period or brightness to step on Off.
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            assert_eq!(
                translate(&op(double_tap()), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            // A tap walks the same color the button's click does: the color
            // group in breathing, the palette in solid.
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
            // And the double-tap steps the same period table the button's
            // double-click does: 3000 → 5000.
            assert_eq!(
                target(double_tap(), DEFAULT_BREATH),
                Some(LightState::Breath(Breath {
                    period_ms: 5_000,
                    ..BREATH_BASE
                }))
            );
        }

        #[test]
        fn long_press_mirrors_the_button_mode_ring() {
            // A long press on Off is never a no-op: it is the way back on.
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
                BusinessIntent::SetLight {
                    instance: 0,
                    state: DEFAULT_BREATH,
                }
            );
            // From breathing it hands off to the same solid the button's long
            // press does.
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
                    brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
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
            // Every axis on Off is a no-op: no color or brightness to step.
            for direction in [
                SwipeDirection::Right,
                SwipeDirection::Left,
                SwipeDirection::Up,
                SwipeDirection::Down,
            ] {
                assert_eq!(
                    translate(&op(swipe_event(direction)), &state(LightState::Off)),
                    BusinessIntent::Invalid,
                    "{direction:?} on Off has no table to step"
                );
            }
        }

        #[test]
        fn shift_the_brightness_envelope_together() {
            // Lifting and sinking move both bounds together from the defaults.
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
            // At the walls the bounds clamp while the guards keep the span.
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
            // Hammer both walls from a mid envelope: a full step of span must
            // survive every press, so `LO HI` never reads equal.
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
            // The diagonals are recognized and tallied by the input stack and
            // resolve to an identity target — the current light unchanged — so
            // the tally sees them while a straight axis still owns each
            // adjustment.
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
            // A click from wiring-order source 1 advances only surface 1.
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::Click), 1),
                    &two_lights()
                ),
                BusinessIntent::SetLight {
                    instance: 1,
                    state: LightState::Solid {
                        color: PALETTE[1],
                        brightness: 140,
                    },
                },
                "each button drives the surface its wiring order names"
            );
            // A long press from source 1 cycles surface 1's own mode: solid off.
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::LongPress), 1),
                    &two_lights()
                ),
                BusinessIntent::SetLight {
                    instance: 1,
                    state: LightState::Off,
                },
                "the mode cycle walks surface 1 from its own solid head"
            );
        }

        #[test]
        fn out_of_range_source_clamps_to_last_surface() {
            // A source beyond the installed boards (e.g. a driver token
            // mismatch) must never panic the interpreter: it drives the last
            // surface.
            assert_eq!(
                translate(
                    &recognize(InputEvent::Button(ButtonEvent::DoubleClick), 7),
                    &two_lights()
                ),
                BusinessIntent::SetLight {
                    instance: 1,
                    state: LightState::Solid {
                        color: PALETTE[0],
                        brightness: 200,
                    },
                },
                "out-of-range sources clamp to the last surface"
            );
        }

        #[test]
        fn panel_touches_only_the_first_surface() {
            // The device-level panel drives surface 0: a tap on an `Off` first
            // surface is a no-op there, never leaking onto surface 1.
            assert_eq!(
                translate(&op(tap_gesture(0)), &two_lights()),
                BusinessIntent::Invalid,
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

use crate::drivers::input::{
    FINGER_DOUBLE_TAP, FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP, FingerLast, GestureEvent,
    InputEvent, MAX_TRACKED_POINTS, MOVE_DEADBAND_PX, SwipeDirection, TouchEvent, TouchStatus,
    direction_between,
};
use crate::drivers::light::{GROUP_CAPACITY, MAX_LIGHTS, Rgb};
use crate::intent::{BusinessIntent, OperationIntent};

/// A live contact slot: the tracker `id` that owns it plus where it last
/// landed, in framebuffer space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivePoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

/// All state owned by the device manager. The light surfaces come as one
/// array slot per instance (`MAX_LIGHTS` cap); unwired slots sit in the boot
/// state. Touch stays device-level: one panel per device, its digits and
/// readout beside the per-surface lights in the same snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    /// Independent state of every light surface a device carries, in board
    /// wiring order.
    pub lights: [LightState; MAX_LIGHTS],
    /// Most recent touch snapshot, mirroring the lights' absolute-target
    /// contract: the render loop always reads the latest whole event.
    pub touch: Option<TouchEvent>,
    /// Live contact positions, slot-aligned with the classifier's tracker ids
    /// and freed as soon as a finger lifts or resolves a gesture.
    pub touch_points: [Option<LivePoint>; MAX_TRACKED_POINTS],
    /// `SwipeDirection::code` of the last frame-to-frame movement each live
    /// slot made while its finger is down; `0` while still or free.
    pub live_dir: [u8; MAX_TRACKED_POINTS],
    /// Rises when two slots are live in the same snapshot; saturated (a run
    /// counter asks "ever concurrent", not "how many frames").
    pub two_finger_runs: u8,
    /// Applied raw-frame heartbeat: incremented on every stored touch
    /// snapshot. Frozen while the controller stalls, still while a held finger
    /// rests — the on-panel digit tells the two apart.
    pub touch_frames: u16,
    /// Last resolved gesture of each tracked slot, aligned with `touch_points`.
    pub finger: [FingerLast; MAX_TRACKED_POINTS],
    /// Classifier-measured hold duration (ms) of the most recent resolved
    /// gesture lift, for the on-panel diagnostic digit.
    pub touch_held_ms: u16,
    /// Tap tally: the operation plane counts every resolved tap it saw, even
    /// one whose business side no-ops (a tap on `Off`), so the corner digit
    /// answers "was there a tap", not "did the light move".
    pub tap_count: u8,
    /// Classifier press pulses, one per down edge. Press without a tap = the
    /// classifier swallowed it; no press at all = the controller never
    /// reported it — the drop-layer split.
    pub press_count: u8,
    /// Double-tap tally, bumped per resolved double-tap in the operation
    /// plane regardless of whether the business side moved a light.
    pub double_tap_count: u8,
    /// Long-press tally, bumped per resolved long-press in the operation
    /// plane regardless of whether the business side moved a light.
    pub long_press_count: u8,
    /// Input-path anomaly runs (rejected extra contacts, I2C failure runs);
    /// nonzero means the input path saw trouble.
    pub ghost_count: u8,
    /// Resolved swipes, bumped per classified slide in the operation plane;
    /// a diagonal tallies without a light move.
    pub swipe_count: u8,
    /// Direction + travel of the most recent resolved swipe.
    pub last_swipe: Option<(SwipeDirection, u16)>,
    /// Where the most recent resolved gesture began, framebuffer space.
    pub last_gesture_origin: Option<(u16, u16)>,
    /// Trailing point of the most recent multi-point gesture; `None` for the
    /// single-point gestures.
    pub last_gesture_end: Option<(u16, u16)>,
    /// Last raw touch-controller `GESTURE_ID`, stored verbatim so the overlay
    /// can compare the chip's engine with the software classifier. Never
    /// drives the light.
    pub chip_gesture_id: u8,
}

/// A breathing schedule: brightness envelope plus the hue path it travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breath {
    pub period_ms: u32,
    pub hue_period_ms: u32,
    /// Hue sweep width; `0` means a static standalone color. Ignored when
    /// `group_len > 0`.
    pub hue_span: u8,
    /// Trough value of the brightness envelope. A nonzero floor keeps the
    /// breathing glow lit instead of fading fully to black.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub saturation: u8,
    /// Base hue the sweep starts from (or holds when `hue_span == 0`). Ignored
    /// when `group_len > 0`.
    pub hue: u8,
    /// Color waypoints on the 256-step hue wheel. `group_len` picks the
    /// trajectory: `0` = the `hue`/`hue_span` sweep above; `1` = a static
    /// standalone color, `group[0]`; `>=2` = rotate through the waypoints,
    /// walking each `group[i] -> group[i+1]` segment over
    /// `hue_period_ms / group_len` and wrapping `group[last] -> group[0]`.
    pub group: [u8; Self::MAX_GROUP],
    pub group_len: u8,
}

impl Breath {
    pub const MAX_GROUP: usize = GROUP_CAPACITY;
}

/// Light behavior, read by the render loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    Off,
    Solid { color: Rgb, brightness: u8 },
    Breath(Breath),
}

impl LightState {
    pub const fn boot() -> Self {
        LightState::Breath(Breath {
            period_ms: DeviceManager::DEFAULT_PERIOD_MS,
            hue_period_ms: DeviceManager::DEFAULT_HUE_PERIOD_MS,
            hue_span: DeviceManager::DEFAULT_HUE_SPAN,
            min_brightness: DeviceManager::DEFAULT_MIN_BRIGHTNESS,
            max_brightness: DeviceManager::DEFAULT_MAX_BRIGHTNESS,
            saturation: DeviceManager::DEFAULT_SATURATION,
            hue: DeviceManager::DEFAULT_HUE,
            group: DeviceManager::DEFAULT_GROUP,
            group_len: DeviceManager::DEFAULT_GROUP_LEN,
        })
    }
}

impl Default for LightState {
    fn default() -> Self {
        LightState::boot()
    }
}

/// The per-lift data a resolved touch gesture carries into its tally: the
/// slot's last [`FingerLast`], the hold and endpoints; the counter bump and
/// target light are passed alongside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GestureRecord {
    id: u8,
    last: FingerLast,
    held_ms: u16,
    origin: (u16, u16),
    end: Option<(u16, u16)>,
}

/// Single owner of device state. Producers apply absolute target states
/// wholesale, so the device never needs to remember "last breath".
pub struct DeviceManager {
    state: DeviceState,
}

impl DeviceManager {
    pub const DEFAULT_PERIOD_MS: u32 = 3_000;
    pub const DEFAULT_HUE_PERIOD_MS: u32 = 60_000;
    pub const DEFAULT_HUE_SPAN: u8 = 255;
    pub const DEFAULT_MIN_BRIGHTNESS: u8 = 24;
    pub const DEFAULT_MAX_BRIGHTNESS: u8 = 80;
    pub const DEFAULT_SATURATION: u8 = 200;
    pub const DEFAULT_HUE: u8 = 0;
    /// Warm sunset: amber, golden, deep rose — low-blue palette favored by
    /// circadian research for a calm ambient breathing glow.
    pub const DEFAULT_GROUP: [u8; Breath::MAX_GROUP] = [16, 32, 3, 0, 0, 0, 0];
    pub const DEFAULT_GROUP_LEN: u8 = 3;

    /// Full-brightness entry point for solid mode; dimming is a long-press
    /// advance from here.
    pub const SOLID_DEFAULT_BRIGHTNESS: u8 = 255;

    /// Backlight floor for the breathing envelope: the panel never sinks below
    /// this percent while pixels are lit, so the trough stays a dim glow
    /// instead of a hard on/off blip.
    pub const BACKLIGHT_FLOOR_PCT: u8 = 12;

    pub const fn new() -> Self {
        Self {
            state: DeviceState {
                lights: [LightState::boot(); MAX_LIGHTS],
                touch: None,
                touch_points: [None; MAX_TRACKED_POINTS],
                live_dir: [0; MAX_TRACKED_POINTS],
                two_finger_runs: 0,
                touch_frames: 0,
                finger: [FingerLast::empty(); MAX_TRACKED_POINTS],
                touch_held_ms: 0,
                tap_count: 0,
                press_count: 0,
                double_tap_count: 0,
                long_press_count: 0,
                ghost_count: 0,
                swipe_count: 0,
                last_swipe: None,
                last_gesture_origin: None,
                last_gesture_end: None,
                chip_gesture_id: 0,
            },
        }
    }

    /// Apply an absolute target to the `instance`-th light surface, clamped to
    /// the installed array so one out-of-range intent can never panic.
    pub fn apply_to(&mut self, instance: usize, state: LightState) {
        self.state.lights[instance.min(MAX_LIGHTS - 1)] = state;
    }

    /// Apply an operation-plane intent: record whatever diagnostics the raw
    /// signal carries (press/ghost pulses, touch snapshots, resolved gesture
    /// tallies). Never moves a light — the target the operation resolves to is
    /// a business intent applied by [`Self::apply_business`].
    pub fn apply_operation(&mut self, op: OperationIntent) {
        match op.event {
            // Buttons carry no touch fingerprint; their operation is a pure
            // mode/color move resolved by the interpreter, so nothing to tally.
            InputEvent::Button(_) => {}
            // A press-down pulse folds into the live points and tallies
            // independently of the lift's fate, so a tap the classifier
            // swallowed still surfaces its physical press.
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let before = self.live_point_count();
                if let Some(slot) = self.upsert_point(id, x, y) {
                    self.state.live_dir[slot] = 0;
                }
                self.bump_two_finger_runs(before);
                self.state.press_count = self.state.press_count.wrapping_add(1);
            }
            // An anomaly pulse saturates: each failure/extra-contact run is
            // already collapsed upstream.
            InputEvent::Gesture(GestureEvent::Ghost) => {
                self.state.ghost_count = self.state.ghost_count.saturating_add(1);
            }
            // A tap tallies and records its hold/origin; the color walk is the
            // business side of the same signal.
            InputEvent::Gesture(GestureEvent::Tap { id, x, y, held_ms }) => {
                self.state.tap_count = self.tally_gesture(
                    self.state.tap_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_TAP,
                            dir: 0,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: None,
                    },
                );
            }
            // A double-tap tallies separately from a tap; both overlay rows
            // show its two tap positions.
            InputEvent::Gesture(GestureEvent::DoubleTap {
                id,
                x,
                y,
                end_x,
                end_y,
                held_ms,
            }) => {
                self.state.double_tap_count = self.tally_gesture(
                    self.state.double_tap_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_DOUBLE_TAP,
                            dir: 0,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: Some((end_x, end_y)),
                    },
                );
            }
            // A long-press tallies separately from the button's mode cycle;
            // the wake/cycle business move is resolved by the interpreter,
            // never here.
            InputEvent::Gesture(GestureEvent::LongPress { id, x, y, held_ms }) => {
                self.state.long_press_count = self.tally_gesture(
                    self.state.long_press_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_LONG_PRESS,
                            dir: 0,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: None,
                    },
                );
            }
            // A swipe tallies, records its travel, and frees the live slot;
            // the diagonal identity target resolves in translate.
            InputEvent::Gesture(GestureEvent::Swipe {
                id,
                x,
                y,
                end_x,
                end_y,
                direction,
                held_ms,
                distance_px,
            }) => {
                self.state.last_swipe = Some((direction, distance_px));
                self.state.swipe_count = self.tally_gesture(
                    self.state.swipe_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_SWIPE,
                            dir: direction.code(),
                            value: distance_px,
                        },
                        held_ms,
                        origin: (x, y),
                        end: Some((end_x, end_y)),
                    },
                );
            }
            // A raw chip gesture read-back is diagnostic-only: store it, never
            // move the light.
            InputEvent::ChipGesture(id) => {
                self.state.chip_gesture_id = id;
            }
            // A raw snapshot folds its points into the live slots (upsert on
            // down/contact, free on release) before storing the frame.
            InputEvent::Touch(event) => {
                let before = self.live_point_count();
                for point in event.points.iter().take(usize::from(event.len)) {
                    match point.status {
                        TouchStatus::Down | TouchStatus::Contact => {
                            if let Some(slot) = self.point_slot(point.id)
                                && let Some(live) = self.state.touch_points[slot]
                                && let Some(dir) = direction_between(
                                    live.x,
                                    live.y,
                                    point.x,
                                    point.y,
                                    MOVE_DEADBAND_PX,
                                )
                            {
                                self.state.live_dir[slot] = dir.code();
                            }
                            self.upsert_point(point.id, point.x, point.y);
                        }
                        TouchStatus::Release => {
                            if let Some(slot) = self.point_slot(point.id) {
                                self.state.live_dir[slot] = 0;
                            }
                            self.clear_point(point.id);
                        }
                    }
                }
                self.bump_two_finger_runs(before);
                self.state.touch_frames = self.state.touch_frames.wrapping_add(1);
                self.state.touch = Some(event);
            }
        }
    }

    /// Apply a business-plane intent: an absolute target the device moves to.
    /// A new business intent variant only grows this match, never the
    /// consuming task.
    pub fn apply_business(&mut self, intent: BusinessIntent) {
        match intent {
            BusinessIntent::Invalid => {}
            BusinessIntent::SetLight { instance, state } => {
                self.apply_to(usize::from(instance), state)
            }
        }
    }

    fn live_point_count(&self) -> usize {
        self.state
            .touch_points
            .iter()
            .filter(|p| p.is_some())
            .count()
    }

    /// The slot owning `id`, if that tracker is live.
    fn point_slot(&self, id: u8) -> Option<usize> {
        self.state
            .touch_points
            .iter()
            .position(|p| p.is_some_and(|l| l.id == id))
    }

    /// Upsert a live point into its tracker's slot, or the first free slot for
    /// a new tracker; a point with no free slot to spare (the panel's cap
    /// already live to other trackers) is dropped rather than evicted. Returns
    /// the slot that now owns the tracker, or `None` when the point was dropped.
    fn upsert_point(&mut self, id: u8, x: u16, y: u16) -> Option<usize> {
        let slot = self
            .point_slot(id)
            .or_else(|| self.state.touch_points.iter().position(Option::is_none));
        if let Some(slot) = slot {
            self.state.touch_points[slot] = Some(LivePoint { id, x, y });
        }
        slot
    }

    fn clear_point(&mut self, id: u8) {
        if let Some(slot) = self.point_slot(id) {
            self.state.touch_points[slot] = None;
        }
    }

    /// Count a resolved touch gesture: record hold/origin/end and the slot's
    /// last gesture, free the finger's live slot, and return the counter
    /// bumped. The operation plane only tallies; the light move travels as a
    /// separate business intent.
    fn tally_gesture(&mut self, counter: u8, record: GestureRecord) -> u8 {
        self.state.touch_held_ms = record.held_ms;
        self.state.last_gesture_origin = Some(record.origin);
        self.state.last_gesture_end = record.end;
        self.record_gesture(record.id, record.last);
        counter.wrapping_add(1)
    }

    /// Stamp a slot's last resolved gesture, then free the slot: resolving a
    /// gesture means the finger lifted, so its live point and movement arrow
    /// both go back to `-` until the next press.
    fn record_gesture(&mut self, id: u8, last: FingerLast) {
        if let Some(slot) = self.point_slot(id) {
            self.state.finger[slot] = last;
            self.state.touch_points[slot] = None;
            self.state.live_dir[slot] = 0;
        }
    }

    /// A saturated rise counter: bump once per transition into ≥2 live slots.
    fn bump_two_finger_runs(&mut self, before: usize) {
        if before < 2 && self.live_point_count() >= 2 {
            self.state.two_finger_runs = self.state.two_finger_runs.saturating_add(1);
        }
    }

    pub fn state(&self) -> DeviceState {
        self.state.clone()
    }

    pub fn light_state(&self) -> LightState {
        self.state.lights[0]
    }

    /// State of the `instance`-th light surface, clamped like
    /// [`Self::apply_to`] so an out-of-range read is the last surface, never a
    /// panic.
    pub fn light_state_at(&self, instance: u8) -> LightState {
        self.state.lights[usize::from(instance).min(MAX_LIGHTS - 1)]
    }

    /// Latest touch snapshot, or `None` before the first contact. Consumed by
    /// the render loop alongside the light state.
    pub fn touch_state(&self) -> Option<TouchEvent> {
        self.state.touch
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{MAX_TOUCH_POINTS, TouchEvent, TouchPoint, TouchStatus};
    use crate::intent::translate;

    const BREATH: LightState = LightState::Breath(Breath {
        period_ms: 3_000,
        hue_period_ms: 60_000,
        hue_span: 255,
        min_brightness: 24,
        max_brightness: 80,
        saturation: 200,
        hue: 0,
        group: [16, 32, 3, 0, 0, 0, 0],
        group_len: 3,
    });

    fn op(event: InputEvent) -> OperationIntent {
        OperationIntent { source: 0, event }
    }

    fn tap() -> OperationIntent {
        op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 1,
            y: 2,
            held_ms: 0,
        }))
    }

    fn double_tap() -> OperationIntent {
        op(InputEvent::Gesture(GestureEvent::DoubleTap {
            id: 0,
            x: 1,
            y: 2,
            end_x: 1,
            end_y: 2,
            held_ms: 0,
        }))
    }

    fn long_press() -> OperationIntent {
        op(InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x: 3,
            y: 4,
            held_ms: 0,
        }))
    }

    fn swipe() -> OperationIntent {
        op(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            x: 80,
            y: 30,
            end_x: 80,
            end_y: 60,
            direction: SwipeDirection::Down,
            held_ms: 42,
            distance_px: 73,
        }))
    }

    fn touch_event(status: TouchStatus) -> OperationIntent {
        op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 1,
                y: 2,
                status,
            }; MAX_TOUCH_POINTS],
            len: 1,
            contacts: 1,
        }))
    }

    fn set_light(instance: u8, state: LightState) -> BusinessIntent {
        BusinessIntent::SetLight { instance, state }
    }

    fn press(id: u8, x: u16, y: u16) -> OperationIntent {
        op(InputEvent::Gesture(GestureEvent::Press { id, x, y }))
    }

    mod boot {
        use super::*;

        #[test]
        fn state_is_clean() {
            let manager = DeviceManager::new();
            assert_eq!(manager.light_state(), BREATH, "boot wakes to breathing");
            assert_eq!(DeviceState::default().lights[0], BREATH);
            assert_eq!(DeviceManager::default().light_state(), BREATH);
            let state = manager.state();
            assert_eq!(manager.touch_state(), None);
            assert_eq!(state.tap_count, 0);
            assert_eq!(state.press_count, 0);
            assert_eq!(state.double_tap_count, 0);
            assert_eq!(state.long_press_count, 0);
            assert_eq!(state.ghost_count, 0);
            assert_eq!(state.swipe_count, 0);
            assert_eq!(state.two_finger_runs, 0);
            assert_eq!(state.touch_frames, 0);
            assert_eq!(state.chip_gesture_id, 0);
            assert_eq!(state.last_swipe, None);
            assert_eq!(state.last_gesture_origin, None);
            assert_eq!(state.last_gesture_end, None);
        }
    }

    mod business {
        use super::*;

        #[test]
        fn set_light_replaces_state_and_snapshot() {
            let mut manager = DeviceManager::new();
            manager.apply_business(set_light(0, LightState::Off));
            assert_eq!(manager.light_state(), LightState::Off);
            let solid = LightState::Solid {
                color: Rgb(3, 4, 5),
                brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
            };
            manager.apply_business(set_light(0, solid));
            assert_eq!(manager.light_state(), solid);
            assert_eq!(
                manager.state().lights[0],
                solid,
                "the snapshot mirrors the applied target"
            );
            // Out-of-range instances clamp to the last installed surface.
            manager.apply_business(set_light(7, LightState::Off));
            assert_eq!(manager.light_state_at(1), LightState::Off);
            // An invalid intent is a no-op.
            manager.apply_business(BusinessIntent::Invalid);
            assert_eq!(manager.light_state(), solid, "an invalid intent is a no-op");
        }
    }

    mod touch {
        use super::*;

        #[test]
        fn snapshots_store_and_replace_the_last_frame() {
            let mut manager = DeviceManager::new();
            let first = TouchEvent {
                points: [TouchPoint {
                    id: 0,
                    x: 1,
                    y: 2,
                    status: TouchStatus::Down,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            };
            manager.apply_operation(op(InputEvent::Touch(first)));
            assert_eq!(manager.touch_state(), Some(first));
            assert_eq!(
                manager.state().press_count,
                0,
                "a raw frame is a snapshot, not the classifier press pulse"
            );
            assert_eq!(
                manager.light_state(),
                LightState::default(),
                "an operation snapshot never moves a light"
            );
            let second = TouchEvent {
                points: [TouchPoint {
                    id: 1,
                    x: 3,
                    y: 4,
                    status: TouchStatus::Contact,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            };
            manager.apply_operation(op(InputEvent::Touch(second)));
            assert_eq!(
                manager.touch_state(),
                Some(second),
                "the latest frame replaces the previous snapshot"
            );
        }

        #[test]
        fn heartbeat_counts_only_applied_frames() {
            let mut manager = DeviceManager::new();
            assert_eq!(manager.state().touch_frames, 0);
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Press {
                id: 0,
                x: 10,
                y: 20,
            })));
            assert_eq!(
                manager.state().touch_frames,
                0,
                "a classifier press pulse is not a raw frame"
            );
            manager.apply_operation(touch_event(TouchStatus::Down));
            manager.apply_operation(touch_event(TouchStatus::Contact));
            assert_eq!(
                manager.state().touch_frames,
                2,
                "every stored snapshot ticks the heartbeat once"
            );
            manager.apply_operation(tap());
            assert_eq!(
                manager.state().touch_frames,
                2,
                "resolved gestures never tick the raw-frame heartbeat"
            );
        }
    }

    mod tallies {
        use super::*;

        #[test]
        fn tap_tallies_stamps_origin_and_accumulates() {
            let mut manager = DeviceManager::new();
            for _ in 0..3 {
                manager.apply_operation(tap());
            }
            let state = manager.state();
            assert_eq!(state.tap_count, 3);
            assert_eq!(
                state.last_gesture_origin,
                Some((1, 2)),
                "the tap records its touch point as the gesture origin"
            );
            assert_eq!(
                manager.light_state(),
                LightState::default(),
                "the tally never moves the light; only the translated business intent does"
            );
        }

        #[test]
        fn double_tap_tallies_on_its_own_row() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(double_tap());
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::DoubleTap {
                id: 0,
                x: 3,
                y: 4,
                end_x: 5,
                end_y: 6,
                held_ms: 0,
            })));
            assert_eq!(manager.state().double_tap_count, 2);
            assert_eq!(
                manager.state().tap_count,
                0,
                "a double-tap is its own tally, never counted as a tap"
            );
        }

        #[test]
        fn long_press_tallies_on_its_own_row() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(long_press());
            assert_eq!(manager.state().long_press_count, 1);
            assert_eq!(
                manager.state().press_count,
                0,
                "the hold's down-edge press and the long-press are separate tallies"
            );
        }

        #[test]
        fn tap_count_wraps_at_255() {
            let mut manager = DeviceManager::new();
            for _ in 0..(u8::MAX as u32).saturating_add(1) {
                manager.apply_operation(tap());
            }
            assert_eq!(
                manager.state().tap_count,
                0,
                "255 taps wrap to 0, keeping the diagnostic digit bounded"
            );
        }

        #[test]
        fn press_pulse_tallies_upserts_and_wraps() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(press(0, 12, 34));
            manager.apply_operation(press(0, 12, 34));
            assert_eq!(manager.state().press_count, 2);
            assert_eq!(
                manager.state().touch_points,
                [
                    Some(LivePoint {
                        id: 0,
                        x: 12,
                        y: 34
                    }),
                    None
                ]
            );
            let mut wraps = DeviceManager::new();
            for _ in 0..(u8::MAX as u32).saturating_add(1) {
                wraps.apply_operation(press(0, 12, 34));
            }
            assert_eq!(
                wraps.state().press_count,
                0,
                "255 presses wrap to 0, keeping the diagnostic digit bounded"
            );
        }

        #[test]
        fn ghost_tallies_and_saturates() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
            assert_eq!(manager.state().ghost_count, 2);
            for _ in 0..(u8::MAX as u32).saturating_add(1) {
                manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
            }
            assert_eq!(
                manager.state().ghost_count,
                u8::MAX,
                "the ghost tally saturates instead of wrapping, preserving the record"
            );
        }

        #[test]
        fn swipe_tallies_records_travel_and_wraps() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(swipe());
            let state = manager.state();
            assert_eq!(state.swipe_count, 1);
            assert_eq!(state.last_swipe, Some((SwipeDirection::Down, 73)));
            assert_eq!(state.last_gesture_origin, Some((80, 30)));
            assert_eq!(state.last_gesture_end, Some((80, 60)));
            assert_eq!(state.touch_held_ms, 42);
            assert_eq!(
                state.lights[0],
                LightState::default(),
                "the swipe op tallies; the light move is the translated business intent"
            );
            assert_eq!(
                state.touch_points,
                [None, None],
                "resolution frees the live slot"
            );
            assert_eq!(state.live_dir[0], 0, "resolution clears the movement arrow");
            let mut wraps = DeviceManager::new();
            for _ in 0..(u8::MAX as u32).saturating_add(1) {
                wraps.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    x: 3,
                    y: 9,
                    end_x: 9,
                    end_y: 3,
                    direction: SwipeDirection::Up,
                    held_ms: 0,
                    distance_px: 60,
                })));
            }
            assert_eq!(
                wraps.state().swipe_count,
                0,
                "255 swipes wrap to 0, keeping the diagnostic digit bounded"
            );
        }

        #[test]
        fn chip_gesture_stores_verbatim_without_touching_light() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(op(InputEvent::ChipGesture(0x10)));
            assert_eq!(manager.state().chip_gesture_id, 0x10);
            assert_eq!(manager.light_state(), LightState::default());
        }
    }

    mod separation {
        use super::*;

        #[test]
        fn light_moves_only_via_a_business_intent() {
            let mut manager = DeviceManager::new();
            // Operation-plane diagnostics never move the light.
            manager.apply_operation(tap());
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Ghost)));
            manager.apply_operation(press(0, 12, 34));
            manager.apply_operation(touch_event(TouchStatus::Contact));
            manager.apply_operation(touch_event(TouchStatus::Release));
            assert_eq!(manager.light_state(), LightState::default());
            assert_eq!(manager.state().tap_count, 1);
            assert_eq!(manager.state().ghost_count, 1);
            assert_eq!(
                manager.state().press_count,
                1,
                "only a classifier press pulse earns a press tally, never a raw frame"
            );
            // A business intent is the only path to a new color.
            let target = LightState::Solid {
                color: Rgb(1, 2, 3),
                brightness: DeviceManager::SOLID_DEFAULT_BRIGHTNESS,
            };
            manager.apply_business(set_light(0, target));
            assert_eq!(manager.light_state(), target);
            manager.apply_business(BusinessIntent::Invalid);
            assert_eq!(
                manager.light_state(),
                target,
                "an invalid intent is a no-op"
            );
            assert_eq!(
                manager.state().tap_count,
                1,
                "the business move never inflates the operation tally"
            );
        }

        #[test]
        fn off_tap_full_path_records_diagnostics_and_keeps_off() {
            // The dispatcher's sequence for an operation: record its operation
            // diagnostics, translate against the freshest state, apply the
            // business side. A tap on `Off` has no color step, so it resolves
            // `Invalid` — the light must not move while the physical input
            // still records.
            let mut manager = DeviceManager::new();
            manager.apply_business(set_light(0, LightState::Off));
            let op = tap();
            manager.apply_operation(op);
            let business = translate(&op, &manager.state());
            manager.apply_business(business);
            assert_eq!(business, BusinessIntent::Invalid);
            assert_eq!(manager.light_state(), LightState::Off);
            assert_eq!(manager.state().tap_count, 1);
            assert_eq!(manager.state().last_gesture_origin, Some((1, 2)));
        }

        #[test]
        fn off_swipe_full_path_records_diagnostics_and_keeps_off() {
            let mut manager = DeviceManager::new();
            manager.apply_business(set_light(0, LightState::Off));
            let op = swipe();
            manager.apply_operation(op);
            let business = translate(&op, &manager.state());
            manager.apply_business(business);
            assert_eq!(business, BusinessIntent::Invalid);
            assert_eq!(manager.light_state(), LightState::Off);
            assert_eq!(manager.state().swipe_count, 1);
            assert_eq!(manager.state().last_swipe, Some((SwipeDirection::Down, 73)));
        }
    }

    mod trackers {
        use super::*;

        #[test]
        fn live_points_follow_down_contact_release() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 0,
                    x: 10,
                    y: 20,
                    status: TouchStatus::Down,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            })));
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 1,
                    x: 150,
                    y: 40,
                    status: TouchStatus::Contact,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            })));
            assert_eq!(
                manager.state().touch_points,
                [
                    Some(LivePoint {
                        id: 0,
                        x: 10,
                        y: 20
                    }),
                    Some(LivePoint {
                        id: 1,
                        x: 150,
                        y: 40
                    })
                ],
                "each tracker upserts into its own slot"
            );
            assert_eq!(
                manager.state().two_finger_runs,
                1,
                "two live slots in one transition counts one run"
            );
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 0,
                    x: 0,
                    y: 0,
                    status: TouchStatus::Release,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            })));
            assert_eq!(
                manager.state().touch_points,
                [
                    None,
                    Some(LivePoint {
                        id: 1,
                        x: 150,
                        y: 40
                    })
                ],
                "a release frees only the tracker's own slot"
            );
            manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                points: [TouchPoint {
                    id: 1,
                    x: 0,
                    y: 0,
                    status: TouchStatus::Release,
                }; MAX_TOUCH_POINTS],
                len: 1,
                contacts: 1,
            })));
            assert_eq!(manager.state().touch_points, [None, None]);
            assert_eq!(
                manager.state().two_finger_runs,
                1,
                "releasing a finger never grows the run counter"
            );
        }

        #[test]
        fn live_dir_tracks_moves_and_keeps_axis_under_deadband() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Press {
                id: 0,
                x: 100,
                y: 200,
            })));
            assert_eq!(
                manager.state().live_dir,
                [0, 0],
                "a fresh landing reads as still until the finger moves"
            );
            manager.apply_operation(touch_at(0, 100, 150, TouchStatus::Contact));
            assert_eq!(
                manager.state().live_dir[0],
                SwipeDirection::Up.code(),
                "climbing the panel is an up-move in framebuffer space"
            );
            manager.apply_operation(touch_at(0, 60, 150, TouchStatus::Contact));
            assert_eq!(manager.state().live_dir[0], SwipeDirection::Left.code());
            manager.apply_operation(touch_at(0, 61, 151, TouchStatus::Contact));
            assert_eq!(
                manager.state().live_dir[0],
                SwipeDirection::Left.code(),
                "sub-deadband jitter keeps the last axis instead of flickering to 0"
            );
        }

        #[test]
        fn live_dir_reads_each_finger_independently() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(touch_at(0, 100, 200, TouchStatus::Down));
            manager.apply_operation(touch_at(1, 200, 200, TouchStatus::Down));
            manager.apply_operation(touch_at(1, 150, 200, TouchStatus::Contact));
            assert_eq!(
                manager.state().live_dir,
                [0, SwipeDirection::Left.code()],
                "each slot carries only its own finger's axis"
            );
            manager.apply_operation(touch_at(1, 0, 0, TouchStatus::Release));
            assert_eq!(
                manager.state().live_dir,
                [0, 0],
                "releasing a finger frees its slot back to still"
            );
        }

        #[test]
        fn simultaneous_presses_upsert_slots_and_count_runs() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(press(0, 10, 20));
            assert_eq!(
                manager.state().touch_points,
                [
                    Some(LivePoint {
                        id: 0,
                        x: 10,
                        y: 20
                    }),
                    None
                ],
                "the first press lands in the lowest free slot"
            );
            manager.apply_operation(press(1, 50, 60));
            assert_eq!(
                manager.state().two_finger_runs,
                1,
                "two live slots count one two-finger run"
            );
            manager.apply_operation(press(0, 11, 21));
            assert_eq!(
                manager.state().two_finger_runs,
                1,
                "an upsert over a live slot is not a new run"
            );
        }

        #[test]
        fn two_finger_runs_saturates_at_255() {
            let mut manager = DeviceManager::new();
            let down = [
                TouchPoint {
                    id: 1,
                    x: 10,
                    y: 10,
                    status: TouchStatus::Down,
                },
                TouchPoint {
                    id: 2,
                    x: 150,
                    y: 90,
                    status: TouchStatus::Down,
                },
            ];
            let up = [
                TouchPoint {
                    id: 1,
                    x: 0,
                    y: 0,
                    status: TouchStatus::Release,
                },
                TouchPoint {
                    id: 2,
                    x: 0,
                    y: 0,
                    status: TouchStatus::Release,
                },
            ];
            let mut down_snapshot = [down[0]; MAX_TOUCH_POINTS];
            down_snapshot[1] = down[1];
            let mut up_snapshot = [up[0]; MAX_TOUCH_POINTS];
            up_snapshot[1] = up[1];
            for _ in 0..(u8::MAX as u32).saturating_add(1) {
                manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                    points: down_snapshot,
                    len: 2,
                    contacts: 2,
                })));
                manager.apply_operation(op(InputEvent::Touch(TouchEvent {
                    points: up_snapshot,
                    len: 2,
                    contacts: 0,
                })));
            }
            assert_eq!(
                manager.state().two_finger_runs,
                u8::MAX,
                "the run counter saturates instead of wrapping, preserving the record"
            );
        }

        #[test]
        fn gestures_stamp_their_own_slot_and_free_the_point() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(press(0, 40, 50));
            manager.apply_operation(press(1, 120, 30));
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                x: 10,
                y: 200,
                end_x: 10,
                end_y: 50,
                direction: SwipeDirection::Up,
                held_ms: 30,
                distance_px: 77,
            })));
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
                id: 1,
                x: 120,
                y: 40,
                held_ms: 90,
            })));
            assert_eq!(
                manager.state().touch_points,
                [None, None],
                "each resolved gesture frees only its own slot"
            );
            assert_eq!(
                manager.state().finger,
                [
                    FingerLast {
                        kind: FINGER_SWIPE,
                        dir: SwipeDirection::Up.code(),
                        value: 77,
                    },
                    FingerLast {
                        kind: FINGER_TAP,
                        dir: 0,
                        value: 90,
                    }
                ],
                "concurrent fingers record their own last gestures, slot-aligned"
            );
        }

        #[test]
        fn diagonal_swipe_frees_the_live_slot_and_arrow() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(touch_at(0, 60, 60, TouchStatus::Down));
            manager.apply_operation(touch_at(0, 80, 80, TouchStatus::Contact));
            let light = manager.state().lights[0];
            assert_ne!(
                manager.state().live_dir[0],
                0,
                "the slide mints a diagonal arrow"
            );
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                x: 60,
                y: 60,
                end_x: 80,
                end_y: 80,
                direction: SwipeDirection::DownRight,
                held_ms: 40,
                distance_px: 28,
            })));
            let state = manager.state();
            assert_eq!(
                state.lights[0], light,
                "an operation never moves the light; the identity resolves in translate"
            );
            assert_eq!(state.swipe_count, 1, "the diagonal still tallies");
            assert_eq!(state.last_swipe, Some((SwipeDirection::DownRight, 28)));
            assert_eq!(
                state.touch_points[0], None,
                "resolution frees the live slot"
            );
            assert_eq!(state.live_dir[0], 0, "resolution clears the movement arrow");
        }

        #[test]
        fn last_gesture_points_follow_resolved_gestures() {
            let mut manager = DeviceManager::new();
            manager.apply_operation(swipe());
            assert_eq!(
                manager.state().last_gesture_origin,
                Some((80, 30)),
                "a swipe records its origin"
            );
            assert_eq!(manager.state().last_gesture_end, Some((80, 60)));

            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x: 11,
                y: 12,
                held_ms: 0,
            })));
            assert_eq!(
                manager.state().last_gesture_origin,
                Some((11, 12)),
                "a tap stamps its own point as the origin"
            );
            assert_eq!(
                manager.state().last_gesture_end,
                None,
                "a single-point tap clears the previous trailing point"
            );

            manager.apply_operation(swipe());
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x: 13,
                y: 14,
                held_ms: 0,
            })));
            assert_eq!(manager.state().last_gesture_origin, Some((13, 14)));
            assert_eq!(
                manager.state().last_gesture_end,
                None,
                "a single-point long-press clears the previous trailing point"
            );

            manager.apply_operation(swipe());
            manager.apply_operation(op(InputEvent::Gesture(GestureEvent::DoubleTap {
                id: 0,
                x: 21,
                y: 22,
                end_x: 23,
                end_y: 24,
                held_ms: 0,
            })));
            assert_eq!(
                manager.state().last_gesture_origin,
                Some((21, 22)),
                "a double-tap shows its first tap as the origin"
            );
            assert_eq!(
                manager.state().last_gesture_end,
                Some((23, 24)),
                "a double-tap shows its second tap as the trailing point"
            );
        }
    }

    fn touch_at(id: u8, x: u16, y: u16, status: TouchStatus) -> OperationIntent {
        op(InputEvent::Touch(TouchEvent {
            points: [TouchPoint { id, x, y, status }; MAX_TOUCH_POINTS],
            len: 1,
            contacts: 1,
        }))
    }
}

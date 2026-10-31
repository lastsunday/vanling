//! Touch: the live contacts, the classifier's tallies, and the per-finger record the overlay
//! draws. Everything the input path learns about a finger lands here and nowhere else.

use crate::drivers::input::{
    FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP, FingerLast, GestureEvent, MAX_TRACKED_POINTS,
    MOVE_DEADBAND_PX, SwipeDirection, TouchEvent, TouchStatus, direction_between,
};

/// A live contact slot: the tracker `id` that owns it plus where it last landed, in
/// framebuffer space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivePoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

/// What the input path has seen since boot: the frame it last stored, the contacts still
/// down, and one tally per semantic so a threshold can be calibrated by watching its bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TouchState {
    /// Most recent touch snapshot, mirroring the lights' absolute targets: the
    /// render loop always reads the latest whole event.
    pub last: Option<TouchEvent>,
    /// Live contact positions, slot-aligned with the classifier's tracker ids and freed as
    /// soon as a finger lifts or resolves a gesture.
    pub points: [Option<LivePoint>; MAX_TRACKED_POINTS],
    /// `SwipeDirection::code` of the last frame-to-frame movement each live slot made while
    /// its finger is down; `0` while still or free.
    pub live_dir: [u8; MAX_TRACKED_POINTS],
    /// Rises when two slots are live in the same snapshot; saturated (a run counter asks
    /// "ever concurrent", not "how many frames").
    pub two_finger_runs: u8,
    /// Applied raw-frame heartbeat: incremented on every stored touch snapshot.
    pub frames: u16,
    /// Last resolved gesture of each tracked slot, aligned with `points`.
    pub finger: [FingerLast; MAX_TRACKED_POINTS],
    /// Classifier-measured hold duration (ms) of the most recent resolved gesture lift, for
    /// the on-panel diagnostic digit.
    pub held_ms: u16,
    pub tap_count: u8,
    /// Classifier press pulses, one per down edge.
    pub press_count: u8,
    pub long_press_count: u8,
    /// Input-path anomaly runs (rejected extra contacts, I2C failure runs); nonzero means
    /// the input path saw trouble.
    pub ghost_count: u8,
    pub swipe_count: u8,
    /// Direction + travel of the most recent resolved swipe.
    pub last_swipe: Option<(SwipeDirection, u16)>,
    /// Where the most recent resolved gesture began, framebuffer space.
    pub last_gesture_origin: Option<(u16, u16)>,
    /// Trailing point of the most recent multi-point gesture; `None` for the single-point
    /// gestures.
    pub last_gesture_end: Option<(u16, u16)>,
    /// Last raw touch-controller `GESTURE_ID`, stored verbatim so the overlay can compare
    /// the chip's engine with the software classifier.
    pub chip_gesture_id: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GestureRecord {
    id: u8,
    last: FingerLast,
    held_ms: u16,
    origin: (u16, u16),
    end: Option<(u16, u16)>,
}

impl TouchState {
    /// A gesture the classifier resolved: it tallies, stamps the finger it belongs to, and
    /// frees that slot.
    pub fn on_gesture(&mut self, gesture: GestureEvent) {
        match gesture {
            GestureEvent::Press { id, x, y } => {
                let before = self.live_point_count();
                if let Some(slot) = self.upsert_point(id, x, y) {
                    self.live_dir[slot] = 0;
                }
                self.bump_two_finger_runs(before);
                self.press_count = self.press_count.wrapping_add(1);
            }
            GestureEvent::Ghost => {
                self.ghost_count = self.ghost_count.saturating_add(1);
            }
            GestureEvent::Tap {
                id, x, y, held_ms, ..
            } => {
                self.tap_count = self.tally(
                    self.tap_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_TAP,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: None,
                    },
                );
            }
            GestureEvent::LongPress { id, x, y, held_ms } => {
                self.long_press_count = self.tally(
                    self.long_press_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_LONG_PRESS,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: None,
                    },
                );
            }
            GestureEvent::Swipe {
                id,
                x,
                y,
                end_x,
                end_y,
                direction,
                held_ms,
                distance_px,
            } => {
                self.last_swipe = Some((direction, distance_px));
                self.swipe_count = self.tally(
                    self.swipe_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_SWIPE,
                            value: distance_px,
                        },
                        held_ms,
                        origin: (x, y),
                        end: Some((end_x, end_y)),
                    },
                );
            }
        }
    }

    /// One whole touch-controller frame: move the live contacts, free the ones that lifted,
    /// and keep the frame itself for the render loop.
    pub fn on_frame(&mut self, event: TouchEvent) {
        let before = self.live_point_count();
        for point in event.points.iter().take(usize::from(event.len)) {
            match point.status {
                TouchStatus::Down | TouchStatus::Contact => {
                    if let Some(slot) = self.point_slot(point.id)
                        && let Some(live) = self.points[slot]
                        && let Some(dir) =
                            direction_between(live.x, live.y, point.x, point.y, MOVE_DEADBAND_PX)
                    {
                        self.live_dir[slot] = dir.code();
                    }
                    self.upsert_point(point.id, point.x, point.y);
                }
                TouchStatus::Release => {
                    if let Some(slot) = self.point_slot(point.id) {
                        self.live_dir[slot] = 0;
                    }
                    self.clear_point(point.id);
                }
            }
        }
        self.bump_two_finger_runs(before);
        self.frames = self.frames.wrapping_add(1);
        self.last = Some(event);
    }

    fn live_point_count(&self) -> usize {
        self.points.iter().filter(|p| p.is_some()).count()
    }

    fn point_slot(&self, id: u8) -> Option<usize> {
        self.points
            .iter()
            .position(|p| p.is_some_and(|l| l.id == id))
    }

    fn upsert_point(&mut self, id: u8, x: u16, y: u16) -> Option<usize> {
        let slot = self
            .point_slot(id)
            .or_else(|| self.points.iter().position(Option::is_none));
        if let Some(slot) = slot {
            self.points[slot] = Some(LivePoint { id, x, y });
        }
        slot
    }

    fn clear_point(&mut self, id: u8) {
        if let Some(slot) = self.point_slot(id) {
            self.points[slot] = None;
        }
    }

    fn tally(&mut self, counter: u8, record: GestureRecord) -> u8 {
        self.held_ms = record.held_ms;
        self.last_gesture_origin = Some(record.origin);
        self.last_gesture_end = record.end;
        self.record_gesture(record.id, record.last);
        counter.wrapping_add(1)
    }

    fn record_gesture(&mut self, id: u8, last: FingerLast) {
        if let Some(slot) = self.point_slot(id) {
            self.finger[slot] = last;
            self.points[slot] = None;
            self.live_dir[slot] = 0;
        }
    }

    fn bump_two_finger_runs(&mut self, before: usize) {
        if before < 2 && self.live_point_count() >= 2 {
            self.two_finger_runs = self.two_finger_runs.saturating_add(1);
        }
    }
}

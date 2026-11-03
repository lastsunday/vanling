use alloc::boxed::Box;

use super::motion::MotionBatch;
use crate::config::MotionConfig;
use crate::demand::DemandKey;

/// A physical button, reported as `true` when pressed.
pub trait Button {
    fn is_pressed(&self) -> bool;
}

/// A button gesture after debounce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Click,
    DoubleClick,
    TripleClick,
    LongPress,
}

/// A discrete touch gesture folded from the raw contact stream by [`TouchGestures`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureEvent {
    Press {
        id: u8,
        x: u16,
        y: u16,
    },
    Ghost,
    Tap {
        id: u8,
        x: u16,
        y: u16,
        held_ms: u16,
        at_ms: u32,
    },
    Swipe {
        id: u8,
        direction: SwipeDirection,
        x: u16,
        y: u16,
        end_x: u16,
        end_y: u16,
        held_ms: u16,
        distance_px: u16,
    },
    LongPress {
        id: u8,
        x: u16,
        y: u16,
        held_ms: u16,
    },
}

/// Dominant travel direction of a resolved [`GestureEvent::Swipe`]: the slide vector's
/// octant, with a ±22.5° diagonal band (`|dx|*12 >= |dy|*5`) read as diagonal and
/// everything steeper folding to the nearest straight axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl SwipeDirection {
    /// Numpad digit for the on-panel readout (8/2/4/6 axes, 7/9/1/3 the diagonals; `5`
    /// reserved).
    pub const fn code(self) -> u8 {
        match self {
            SwipeDirection::Up => 8,
            SwipeDirection::Down => 2,
            SwipeDirection::Left => 4,
            SwipeDirection::Right => 6,
            SwipeDirection::UpLeft => 7,
            SwipeDirection::UpRight => 9,
            SwipeDirection::DownLeft => 1,
            SwipeDirection::DownRight => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Button(ButtonEvent),
    Touch(TouchEvent),
    Gesture(GestureEvent),
    ChipGesture(u8),
    Motion(MotionBatch),
}

pub const BUTTON_SCAN_MS: u64 = 10;

/// Shared base tick of the app's input loop.
pub const INPUT_BASE_MS: u64 = 5;

/// Minimum hold before a lift may read as [`GestureEvent::LongPress`]: the press must have
/// held the panel *alone* and never travelled to [`SWIPE_MIN_DISTANCE_PX`].
pub const LONG_PRESS_MS: u64 = 600;

/// How long a click sequence stays pending while the driver waits for the next click.
pub const DOUBLE_CLICK_WINDOW_MS: u64 = 400;

/// Minimum press travel ever observed (px, Euclidean) that reads as a
/// [`GestureEvent::Swipe`] — a quarter of the 240-px panel, so a tap's drift never reaches
/// it.
pub const SWIPE_MIN_DISTANCE_PX: u16 = 60;

/// Smallest dominant-direction travel (px) that re-reads as live movement while a finger is
/// down; below it the per-finger direction row keeps its last axis instead of flickering
/// with a stationary contact's micro-drift.
pub const MOVE_DEADBAND_PX: u16 = 2;

/// Farthest a second contact may land from an already-down finger and still read as a
/// self-cap phantom instead of a genuine second finger: a duplicate point the controller
/// echoes next to the real contact on press-in.
pub const PHANTOM_RADIUS_PX: u16 = 60;

/// How recently a finger's down edge may be for a nearby second contact to count as its
/// phantom echo — one touch-scan cadence: a genuine second finger lands later or farther, a
/// self-cap echo duplicates the same frame's contact.
pub const PHANTOM_WINDOW_MS: u64 = TOUCH_SCAN_MS;

/// How many fingers the touch path tracks side-by-side: the FT6336 self-capacitance panel
/// exposes exactly two, and the live-point/last-gesture/ run-counter tuple is dimensioned
/// to that ceiling.
pub const MAX_TRACKED_POINTS: usize = 2;

/// Gesture kind a finger most recently resolved, stamped into [`FingerLast::kind`]; `0` =
/// none yet.
pub const FINGER_NONE: u8 = 0;
pub const FINGER_TAP: u8 = 1;
pub const FINGER_LONG_PRESS: u8 = 2;
pub const FINGER_SWIPE: u8 = 3;

/// The gesture one tracked finger last resolved, so the overlay can answer per finger what
/// it did: `kind` the `FINGER_*` code, `value` the measured hold (ms) or swipe travel (px).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FingerLast {
    pub kind: u8,
    pub value: u16,
}

impl FingerLast {
    pub const fn empty() -> Self {
        Self {
            kind: FINGER_NONE,
            value: 0,
        }
    }
}

const DEBOUNCE_SAMPLES: u32 = 3;

/// Maximum simultaneous contacts a touch event can carry, aligned with Android's
/// `MotionEvent.MAX_POINTERS` ceiling: 10-point controllers can occasionally report an
/// extra contact, and the unused slots cost nothing because a `TouchEvent` is a transient
/// `Copy` value.
pub const MAX_TOUCH_POINTS: usize = 16;

/// Touch poll cadence, a whole multiple of [`INPUT_BASE_MS`] so the touch
/// source lands on the same shared grid. 10 ms keeps ~2 catch windows within a
/// 20 ms threshold dropout for marginal contacts.
pub const TOUCH_SCAN_MS: u64 = 10;

/// Consecutive absent samples that confirm a contact release, so a single-frame dropout of
/// a held contact does not read as a lift.
pub const RELEASE_CONFIRM_SAMPLES: u8 = 3;

/// Per-contact lifecycle: appeared / tracked / left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchStatus {
    Down,
    Contact,
    Release,
}

/// A tracked contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
    pub status: TouchStatus,
}

/// Snapshot of every contact in one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchEvent {
    pub points: [TouchPoint; MAX_TOUCH_POINTS],
    pub len: u8,
}

/// Maps raw touch-panel coordinates into framebuffer space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchMap {
    pub swap_xy: bool,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl TouchMap {
    /// Raw coordinates are already in framebuffer space.
    #[cfg(test)]
    pub const IDENTITY: Self = Self {
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
    };

    pub fn map(&self, (x, y): (u16, u16), width: u16, height: u16) -> (u16, u16) {
        let (x, y) = if self.swap_xy { (y, x) } else { (x, y) };
        let x = if self.mirror_x { width - 1 - x } else { x };
        let y = if self.mirror_y { height - 1 - y } else { y };
        (x, y)
    }
}

/// `XH[7:6]` event bits of an FT5x06-family (FT6336) touch point: `0b00` the down edge and
/// `0b10` a held contact both carry live coordinates; `0b01` is the lift frame.
pub fn ft6x06_point_is_live(xh: u8) -> bool {
    matches!((xh >> 6) & 0x03, 0b00 | 0b10)
}

fn dominant_axis(dx: i32, dy: i32) -> SwipeDirection {
    let ax = dx.unsigned_abs();
    let ay = dy.unsigned_abs();
    let (max, min) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    if min * 12 >= max * 5 {
        match (dy > 0, dx > 0) {
            (true, true) => SwipeDirection::DownRight,
            (true, false) => SwipeDirection::DownLeft,
            (false, true) => SwipeDirection::UpRight,
            (false, false) => SwipeDirection::UpLeft,
        }
    } else if ax >= ay {
        if dx > 0 {
            SwipeDirection::Right
        } else {
            SwipeDirection::Left
        }
    } else if dy > 0 {
        SwipeDirection::Down
    } else {
        SwipeDirection::Up
    }
}

/// Direction of the latest frame-to-frame move while a finger is down, for the live
/// per-finger rows: `None` within `deadband_px` (jitter reads as holding still), otherwise
/// the same answer as a resolved [`GestureEvent::Swipe`] so the live arrow and the slide
/// digit never disagree.
pub fn direction_between(
    from_x: u16,
    from_y: u16,
    to_x: u16,
    to_y: u16,
    deadband_px: u16,
) -> Option<SwipeDirection> {
    let dx = i32::from(to_x) - i32::from(from_x);
    let dy = i32::from(to_y) - i32::from(from_y);
    let drift = dx.unsigned_abs().max(dy.unsigned_abs());
    (drift >= u32::from(deadband_px)).then(|| dominant_axis(dx, dy))
}

fn travel_px(from_x: u16, from_y: u16, to_x: u16, to_y: u16) -> u16 {
    swipe_distance(sq_dist(from_x, from_y, to_x, to_y))
}

/// Classifies a displacement into a swipe: `None` under [`SWIPE_MIN_DISTANCE_PX`], else the
/// dominant destination octant and travel in pixels.
pub fn swipe_classify(
    from_x: u16,
    from_y: u16,
    to_x: u16,
    to_y: u16,
) -> Option<(SwipeDirection, u16)> {
    let dx = i32::from(to_x) - i32::from(from_x);
    let dy = i32::from(to_y) - i32::from(from_y);
    let distance_px = travel_px(from_x, from_y, to_x, to_y);
    if distance_px < SWIPE_MIN_DISTANCE_PX {
        return None;
    }
    Some((dominant_axis(dx, dy), distance_px))
}

// Euclidean length of a displacement for the swipe distance digit; nothing
// panel-sized can overflow `u16`, so saturate rather than wrap.
fn swipe_distance(sq: u32) -> u16 {
    u16::try_from(isqrt_u32(sq)).unwrap_or(u16::MAX)
}

fn sq_dist(x0: u16, y0: u16, x1: u16, y1: u16) -> u32 {
    let dx = (x0 as i32 - x1 as i32).unsigned_abs();
    let dy = (y0 as i32 - y1 as i32).unsigned_abs();
    dx * dx + dy * dy
}

fn isqrt_u32(n: u32) -> u32 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x / 2 + 1;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Poll-based input source.
pub trait InputSource {
    /// The name a diagnostic line calls this source by. The board decides what it wired;
    /// the driver knows which kind of part it is.
    fn name(&self) -> &'static str;

    fn poll(&mut self, now_ms: u64) -> Option<InputEvent>;

    fn held(&self) -> bool {
        false
    }

    fn set_demand_collecting(&mut self, _now_ms: u64, _collecting: bool) {}

    fn accepts_slower_poll(&self) -> bool {
        true
    }

    fn configure_for(
        &mut self,
        wanted: Option<MotionConfig>,
    ) -> Result<Option<MotionConfig>, MotionConfig> {
        let _ = wanted;
        Ok(None)
    }
}

/// Combines/coalesces raw input events into higher-level gestures.
pub trait EventAggregator {
    fn feed(&mut self, event: InputEvent, now_ms: u64) -> Option<InputEvent>;

    fn tick(&mut self, _now_ms: u64) -> Option<InputEvent> {
        None
    }
}

/// Identity aggregator — passes every event through unchanged.
pub struct PassThrough;

impl EventAggregator for PassThrough {
    fn feed(&mut self, event: InputEvent, _now_ms: u64) -> Option<InputEvent> {
        Some(event)
    }
}

struct EventQueue([Option<InputEvent>; 4]);

impl EventQueue {
    const fn new() -> Self {
        Self([None, None, None, None])
    }

    fn push(&mut self, event: InputEvent) {
        for slot in self.0.iter_mut() {
            if slot.is_none() {
                *slot = Some(event);
                return;
            }
        }
        self.0.copy_within(1.., 0);
        self.0[3] = Some(event);
    }

    fn pop(&mut self) -> Option<InputEvent> {
        let first = self.0[0].take();
        self.0.copy_within(1.., 0);
        self.0[3] = None;
        first
    }
}

pub struct TouchGestures {
    fingers: [FingerState; MAX_TOUCH_POINTS],
    ghosted: bool,
    queued: EventQueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FingerState {
    id: Option<u8>,
    pressed: bool,
    down_at_ms: Option<u64>,
    down_x: u16,
    down_y: u16,
    farthest: Option<(u16, u16, u16)>,
}

impl FingerState {
    const fn free() -> Self {
        Self {
            id: None,
            pressed: false,
            down_at_ms: None,
            down_x: 0,
            down_y: 0,
            farthest: None,
        }
    }
}

impl TouchGestures {
    pub fn new() -> Self {
        Self {
            fingers: [FingerState::free(); MAX_TOUCH_POINTS],
            ghosted: false,
            queued: EventQueue::new(),
        }
    }

    fn slot_of(&self, id: u8) -> Option<usize> {
        self.fingers.iter().position(|f| f.id == Some(id))
    }

    fn free_slot(&self) -> Option<usize> {
        self.fingers.iter().position(|f| f.id.is_none())
    }

    fn is_phantom(&self, x: u16, y: u16, now_ms: u64) -> bool {
        let reach_sq = u32::from(PHANTOM_RADIUS_PX) * u32::from(PHANTOM_RADIUS_PX);
        self.fingers.iter().any(|f| {
            f.pressed
                && f.down_at_ms
                    .is_some_and(|at| now_ms.saturating_sub(at) <= PHANTOM_WINDOW_MS)
                && sq_dist(f.down_x, f.down_y, x, y) <= reach_sq
        })
    }

    fn forward(
        &mut self,
        fresh: &mut [Option<InputEvent>; 4],
        fallback: Option<InputEvent>,
    ) -> Option<InputEvent> {
        if let Some(older) = self.queued.pop() {
            for event in fresh.iter().flatten() {
                self.queued.push(*event);
            }
            if let Some(frame) = fallback
                && fresh.iter().all(Option::is_none)
            {
                self.queued.push(frame);
            }
            return Some(older);
        }
        let mut events = fresh.iter().flatten().copied();
        match events.next() {
            Some(first) => {
                for event in events {
                    self.queued.push(event);
                }
                Some(first)
            }
            None => fallback,
        }
    }

    fn classify(&mut self, sample: &TouchEvent, now_ms: u64, out: &mut [Option<InputEvent>; 4]) {
        let mut k = 0usize;
        let mut push = |event: InputEvent, out: &mut [Option<InputEvent>; 4]| {
            if k < out.len() {
                out[k] = Some(event);
            }
            k = k.saturating_add(1);
        };

        let mut saw_phantom = false;
        for i in 0..sample.len {
            let point = sample.points[usize::from(i)];
            match point.status {
                TouchStatus::Down | TouchStatus::Contact => {
                    let slot = if let Some(slot) = self.slot_of(point.id) {
                        Some(slot)
                    } else if self.is_phantom(point.x, point.y, now_ms) {
                        saw_phantom = true;
                        None
                    } else if let Some(slot) = self.free_slot() {
                        self.fingers[slot].id = Some(point.id);
                        Some(slot)
                    } else {
                        None
                    };
                    if let Some(slot) = slot {
                        let armed = !self.fingers[slot].pressed;
                        if armed {
                            self.fingers[slot].pressed = true;
                            self.fingers[slot].down_at_ms = Some(now_ms);
                            self.fingers[slot].down_x = point.x;
                            self.fingers[slot].down_y = point.y;
                            self.fingers[slot].farthest = Some((0, point.x, point.y));
                        }
                        let far = travel_px(
                            self.fingers[slot].down_x,
                            self.fingers[slot].down_y,
                            point.x,
                            point.y,
                        );
                        if self.fingers[slot].farthest.is_none_or(|(d, _, _)| far > d) {
                            self.fingers[slot].farthest = Some((far, point.x, point.y));
                        }
                        if armed {
                            push(
                                InputEvent::Gesture(GestureEvent::Press {
                                    id: point.id,
                                    x: point.x,
                                    y: point.y,
                                }),
                                out,
                            );
                        }
                    }
                }
                TouchStatus::Release => {
                    let Some(slot) = self.slot_of(point.id) else {
                        continue;
                    };
                    let sibling_lift = (0..sample.len).any(|j| {
                        let p = sample.points[usize::from(j)];
                        p.status == TouchStatus::Release && p.id != point.id
                    });
                    if let Some(event) = self.resolve_release(slot, point, now_ms, sibling_lift) {
                        push(event, out);
                    }
                }
            }
        }

        if !saw_phantom {
            self.ghosted = false;
        } else if !self.ghosted {
            self.ghosted = true;
            push(InputEvent::Gesture(GestureEvent::Ghost), out);
        }
    }

    fn resolve_release(
        &mut self,
        slot: usize,
        point: TouchPoint,
        now_ms: u64,
        sibling_lift: bool,
    ) -> Option<InputEvent> {
        let (id, down_x, down_y, held, held_ms, farthest) = {
            let f = &mut self.fingers[slot];
            let id = f.id?;
            f.pressed = false;
            let down_at = f.down_at_ms.take()?;
            let held = now_ms.saturating_sub(down_at);
            let held_ms = u16::try_from(held.min(u64::from(u16::MAX))).unwrap_or(u16::MAX);
            let far = travel_px(f.down_x, f.down_y, point.x, point.y);
            if f.farthest.is_none_or(|(d, _, _)| far > d) {
                f.farthest = Some((far, point.x, point.y));
            }
            Some((id, f.down_x, f.down_y, held, held_ms, f.farthest))
        }?;
        self.fingers[slot].id = None;

        if let Some((_, end_x, end_y)) = farthest
            && let Some((direction, distance_px)) = swipe_classify(down_x, down_y, end_x, end_y)
        {
            return Some(InputEvent::Gesture(GestureEvent::Swipe {
                id,
                direction,
                x: down_x,
                y: down_y,
                end_x,
                end_y,
                held_ms,
                distance_px,
            }));
        }
        if held >= LONG_PRESS_MS && !sibling_lift && !self.fingers.iter().any(|f| f.pressed) {
            return Some(InputEvent::Gesture(GestureEvent::LongPress {
                id,
                x: down_x,
                y: down_y,
                held_ms,
            }));
        }

        Some(InputEvent::Gesture(GestureEvent::Tap {
            id,
            x: down_x,
            y: down_y,
            held_ms,
            at_ms: u32::try_from(now_ms).unwrap_or(u32::MAX),
        }))
    }
}

impl Default for TouchGestures {
    fn default() -> Self {
        Self::new()
    }
}

impl EventAggregator for TouchGestures {
    fn feed(&mut self, event: InputEvent, now_ms: u64) -> Option<InputEvent> {
        let InputEvent::Touch(sample) = event else {
            return Some(event);
        };
        let mut backlog = [None; 4];
        self.classify(&sample, now_ms, &mut backlog);
        self.forward(&mut backlog, Some(InputEvent::Touch(sample)))
    }

    fn tick(&mut self, _now_ms: u64) -> Option<InputEvent> {
        self.queued.pop()
    }
}

pub struct DoubleClickAggregator {
    window_ms: u64,
    first_click_at_ms: Option<u64>,
    second_click_at_ms: Option<u64>,
    queued: EventQueue,
}

impl DoubleClickAggregator {
    pub const fn new() -> Self {
        Self::with_window_ms(DOUBLE_CLICK_WINDOW_MS)
    }

    pub const fn with_window_ms(window_ms: u64) -> Self {
        Self {
            window_ms,
            first_click_at_ms: None,
            second_click_at_ms: None,
            queued: EventQueue::new(),
        }
    }

    fn pending_event(&self) -> Option<InputEvent> {
        self.first_click_at_ms?;
        Some(InputEvent::Button(if self.second_click_at_ms.is_some() {
            ButtonEvent::DoubleClick
        } else {
            ButtonEvent::Click
        }))
    }

    fn clear_pending(&mut self) {
        self.first_click_at_ms = None;
        self.second_click_at_ms = None;
    }

    fn expire(&mut self, now_ms: u64) {
        let deadline = self.second_click_at_ms.or(self.first_click_at_ms);
        let Some(deadline) = deadline else {
            return;
        };
        if now_ms.saturating_sub(deadline) > self.window_ms
            && let Some(event) = self.pending_event()
        {
            self.clear_pending();
            self.queued.push(event);
        }
    }
}

impl Default for DoubleClickAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl EventAggregator for DoubleClickAggregator {
    fn feed(&mut self, event: InputEvent, now_ms: u64) -> Option<InputEvent> {
        self.expire(now_ms);
        let generated = match event {
            InputEvent::Button(ButtonEvent::Click) => match self.first_click_at_ms {
                None => {
                    self.first_click_at_ms = Some(now_ms);
                    None
                }
                Some(_) if self.second_click_at_ms.is_some() => {
                    if now_ms.saturating_sub(self.second_click_at_ms.unwrap_or(now_ms))
                        <= self.window_ms
                    {
                        self.clear_pending();
                        Some(InputEvent::Button(ButtonEvent::TripleClick))
                    } else {
                        self.first_click_at_ms = Some(now_ms);
                        Some(InputEvent::Button(ButtonEvent::DoubleClick))
                    }
                }
                Some(first) => {
                    if now_ms.saturating_sub(first) <= self.window_ms {
                        self.second_click_at_ms = Some(now_ms);
                        None
                    } else {
                        self.first_click_at_ms = Some(now_ms);
                        Some(InputEvent::Button(ButtonEvent::Click))
                    }
                }
            },
            _ => {
                if let Some(pending) = self.pending_event() {
                    self.clear_pending();
                    self.queued.push(pending);
                }
                Some(event)
            }
        };
        if let Some(event) = generated {
            self.queued.push(event);
        }
        self.queued.pop()
    }

    fn tick(&mut self, now_ms: u64) -> Option<InputEvent> {
        self.expire(now_ms);
        self.queued.pop()
    }
}

/// Tracks raw per-sample contact lists into id-stable `TouchEvent`s by nearest-neighbor id
/// matching, so event order can shuffle without breaking identity.
#[derive(Debug, Clone, Copy)]
pub struct TouchContinuity {
    points: [TrackerPoint; MAX_TOUCH_POINTS],
    next_id: u8,
    absent: u8,
}

#[derive(Debug, Clone, Copy)]
struct TrackerPoint {
    id: u8,
    x: u16,
    y: u16,
    active: bool,
}

impl TouchContinuity {
    /// Farthest a raw point may move between samples to keep its id: wide enough for a
    /// full-cadence fast flick, far below the panel diagonal so a two-finger pinch keeps
    /// the fingers separate.
    pub const MAX_REACH_PX: u16 = 80;

    pub const fn new() -> Self {
        Self {
            points: [TrackerPoint {
                id: 0,
                x: 0,
                y: 0,
                active: false,
            }; MAX_TOUCH_POINTS],
            next_id: 0,
            absent: 0,
        }
    }

    /// Feed one raw sample (framebuffer space); emits only when the contact set or a
    /// tracked position changed.
    pub fn update(&mut self, raw: &[(u16, u16)]) -> Option<TouchEvent> {
        let mut event = TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 0,
                y: 0,
                status: TouchStatus::Down,
            }; MAX_TOUCH_POINTS],
            len: 0,
        };
        let mut consumed = [false; MAX_TOUCH_POINTS];
        let mut changed = false;

        for &(rx, ry) in raw.iter().take(MAX_TOUCH_POINTS) {
            let point = match self.nearest_active(rx, ry, &consumed) {
                Some(i) => {
                    let tracker = &mut self.points[i];
                    changed |= tracker.x != rx || tracker.y != ry;
                    tracker.x = rx;
                    tracker.y = ry;
                    consumed[i] = true;
                    TouchPoint {
                        id: tracker.id,
                        x: rx,
                        y: ry,
                        status: TouchStatus::Contact,
                    }
                }
                None => {
                    let id = self.allocate_id();
                    let Some(slot) = self.points.iter().position(|p| !p.active) else {
                        break;
                    };
                    self.points[slot] = TrackerPoint {
                        id,
                        x: rx,
                        y: ry,
                        active: true,
                    };
                    consumed[slot] = true;
                    changed = true;
                    TouchPoint {
                        id,
                        x: rx,
                        y: ry,
                        status: TouchStatus::Down,
                    }
                }
            };
            event.points[event.len as usize] = point;
            event.len += 1;
        }

        let pending = self
            .points
            .iter()
            .enumerate()
            .filter(|&(i, p)| p.active && !consumed[i])
            .count();
        if pending > 0 {
            self.absent = self.absent.saturating_add(1);
        } else {
            self.absent = 0;
        }

        if self.absent >= RELEASE_CONFIRM_SAMPLES {
            self.absent = 0;
            for (i, tracker) in self.points.iter_mut().enumerate() {
                if tracker.active && !consumed[i] {
                    if (event.len as usize) < MAX_TOUCH_POINTS {
                        event.points[event.len as usize] = TouchPoint {
                            id: tracker.id,
                            x: tracker.x,
                            y: tracker.y,
                            status: TouchStatus::Release,
                        };
                        event.len += 1;
                    }
                    tracker.active = false;
                    changed = true;
                }
            }
        }

        changed.then_some(event)
    }

    fn nearest_active(&self, x: u16, y: u16, consumed: &[bool; MAX_TOUCH_POINTS]) -> Option<usize> {
        let reach_sq = (Self::MAX_REACH_PX as u32) * (Self::MAX_REACH_PX as u32);
        let mut best = None;
        let mut best_sq = u32::MAX;
        for (i, tracker) in self.points.iter().enumerate() {
            if !tracker.active || consumed[i] {
                continue;
            }
            let dist_sq = sq_dist(tracker.x, tracker.y, x, y);
            if dist_sq <= reach_sq && dist_sq < best_sq {
                best_sq = dist_sq;
                best = Some(i);
            }
        }
        best
    }

    fn allocate_id(&mut self) -> u8 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);
            if !self.points.iter().any(|p| p.active && p.id == id) {
                return id;
            }
        }
    }
}

impl Default for TouchContinuity {
    fn default() -> Self {
        Self::new()
    }
}

/// A polled input source scheduled at its own cadence, paired with the aggregator that
/// turns its raw samples into the events [`crate::intent::recognize`] packages.
pub struct PollEntry {
    source_id: u8,
    key: DemandKey,
    source: Box<dyn InputSource + Send>,
    aggregator: Box<dyn EventAggregator + Send>,
    board_cadence_ms: u64,
    cadence_ms: u64,
    next_at_ms: u64,
    active: bool,
}

impl PollEntry {
    pub fn new(
        source_id: u8,
        key: DemandKey,
        source: Box<dyn InputSource + Send>,
        aggregator: Box<dyn EventAggregator + Send>,
        cadence_ms: u64,
    ) -> Self {
        Self {
            source_id,
            key,
            source,
            aggregator,
            board_cadence_ms: cadence_ms,
            cadence_ms,
            next_at_ms: cadence_ms,
            active: true,
        }
    }

    /// A board button wired as an interaction source: debounced clicks folded into
    /// double- and triple-clicks at the shared button cadence.
    pub fn button(source_id: u8, button: impl Button + Send + 'static) -> Self {
        Self::new(
            source_id,
            DemandKey::INTERACTION,
            Box::new(ButtonScanner::new(button)),
            Box::new(DoubleClickAggregator::new()),
            BUTTON_SCAN_MS,
        )
    }

    /// Whether the demand wants this source polled.
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Turns polling on or off.
    pub fn set_active(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.next_at_ms = 0;
        }
    }

    /// Moves the source's cadence, which is where a latency budget lands.
    ///
    /// The board's wiring is the floor, in either direction, since a plan is derived from
    /// [`Self::board_cadence_ms`] each time. Below the floor is refused rather than clamped:
    /// a caller that ignored the floor must not silently shorten a source whose debounce
    /// windows were measured against it.
    ///
    /// The reading already scheduled stays where it is, so a stretch costs no reading that
    /// was due and a relaxation costs no wait that was not.
    pub fn set_cadence_ms(&mut self, cadence_ms: u64) {
        if cadence_ms < self.board_cadence_ms || cadence_ms == self.cadence_ms {
            return;
        }
        self.cadence_ms = cadence_ms;
    }

    /// Passes the switch on to the source behind it.
    pub fn set_collecting(&mut self, now_ms: u64, collecting: bool) {
        self.source.set_demand_collecting(now_ms, collecting);
    }

    /// Whether a latency budget may stretch this source's cadence.
    pub fn accepts_slower_poll(&self) -> bool {
        self.source.accepts_slower_poll()
    }

    pub fn configure_for(
        &mut self,
        wanted: Option<MotionConfig>,
    ) -> Result<Option<MotionConfig>, MotionConfig> {
        self.source.configure_for(wanted)
    }

    /// The wiring-order identity the board assigned this input, driving intent routing (a
    /// button on source `n` targets light instance `n`).
    pub fn source_id(&self) -> u8 {
        self.source_id
    }

    pub const fn key(&self) -> DemandKey {
        self.key
    }

    /// The name the source behind this entry answers to in diagnostics.
    pub fn name(&self) -> &'static str {
        self.source.name()
    }

    /// A whole multiple of the shared base tick, which a demand may have stretched.
    pub fn cadence_ms(&self) -> u64 {
        self.cadence_ms
    }

    /// The floor a demand cannot cross, and the value a plan is derived from.
    pub fn board_cadence_ms(&self) -> u64 {
        self.board_cadence_ms
    }

    /// When this source is next due on the shared grid.
    pub fn next_at_ms(&self) -> u64 {
        self.next_at_ms
    }

    /// Poll the source, then fold the raw sample (or the tick that stands in for it)
    /// through the aggregator.
    pub fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        match self.source.poll(now_ms) {
            Some(event) => self.aggregator.feed(event, now_ms),
            None => self.aggregator.tick(now_ms),
        }
    }

    pub fn advance_past(&mut self, now_ms: u64) {
        if self.next_at_ms > now_ms {
            return;
        }
        if self.cadence_ms == 0 {
            self.next_at_ms = now_ms.wrapping_add(1);
            return;
        }
        let behind = now_ms - self.next_at_ms;
        let cells = behind / self.cadence_ms + 1;
        self.next_at_ms = self
            .next_at_ms
            .wrapping_add(cells.wrapping_mul(self.cadence_ms));
    }

    /// Whether the control behind this source is down right now, which the poll cadence
    /// cannot answer between two due readings.
    pub fn held(&self) -> bool {
        self.source.held()
    }
}

/// Debounced button scanner, distinguishing clicks from long-presses by hold duration.
pub struct ButtonScanner<B: Button> {
    button: B,
    pressed: bool,
    held_since_ms: u64,
    same_count: u32,
}

impl<B: Button> ButtonScanner<B> {
    pub fn new(button: B) -> Self {
        Self {
            button,
            pressed: false,
            held_since_ms: 0,
            same_count: 0,
        }
    }

    fn read(&mut self, now_ms: u64) -> Option<ButtonEvent> {
        let raw = self.button.is_pressed();

        if raw == self.pressed {
            self.same_count = 0;
            return None;
        }

        self.same_count += 1;
        if self.same_count < DEBOUNCE_SAMPLES {
            return None;
        }

        self.same_count = 0;
        self.pressed = raw;

        if raw {
            self.held_since_ms = now_ms;
            None
        } else {
            let held = now_ms.saturating_sub(self.held_since_ms);
            Some(if held >= LONG_PRESS_MS {
                ButtonEvent::LongPress
            } else {
                ButtonEvent::Click
            })
        }
    }
}

impl<B: Button> InputSource for ButtonScanner<B> {
    fn name(&self) -> &'static str {
        "button"
    }

    fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        self.read(now_ms).map(InputEvent::Button)
    }

    fn held(&self) -> bool {
        self.pressed || self.button.is_pressed()
    }
}

#[cfg(test)]
mod tests;

use alloc::boxed::Box;

/// Active-low contract for a physical button: returns `true` when pressed.
pub trait Button {
    fn is_pressed(&self) -> bool;
}

/// A button gesture after debounce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Click,
    DoubleClick,
    LongPress,
}

/// A discrete touch gesture folded from the raw contact stream by
/// [`TouchGestures`]. Contact gestures carry a stable per-finger `id` plus the
/// gesture origin and classifier-measured hold; two-point gestures (double-tap,
/// swipe) add the trailing `end_x`/`end_y`; a swipe adds its dominant octant
/// and Euclidean travel; `Ghost` is a coordinate-less input-path anomaly pulse
/// (rejected phantom, or a driver I2C failure).
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
    },
    DoubleTap {
        id: u8,
        x: u16,
        y: u16,
        end_x: u16,
        end_y: u16,
        held_ms: u16,
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

/// Dominant travel direction of a resolved [`GestureEvent::Swipe`]: the slide
/// vector's octant, with a ±22.5° diagonal band (`|dx|*12 >= |dy|*5`) read as
/// diagonal and everything steeper folding to the nearest straight axis.
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
    /// Numpad digit for the on-panel readout (8/2/4/6 axes, 7/9/1/3 the
    /// diagonals; `5` reserved); the layout is a display contract.
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
    /// Raw `GESTURE_ID` register a touch controller surfaced (e.g. an FT5x06's
    /// built-in slide detector), pulsed on change so a debug overlay can compare
    /// the chip's engine against the software classifier. Purely diagnostic;
    /// never folds into a light-mode transition.
    ChipGesture(u8),
}

pub const BUTTON_SCAN_MS: u64 = 10;

/// Shared base tick of the app's input loop. Every source cadence is a whole
/// multiple, so each device polls on a stable grid regardless of the clock.
pub const INPUT_BASE_MS: u64 = 5;

/// Minimum hold before a lift may read as [`GestureEvent::LongPress`]: the
/// press must have held the panel *alone* and never travelled to
/// [`SWIPE_MIN_DISTANCE_PX`].
pub const LONG_PRESS_MS: u64 = 600;

/// How long a first click stays pending while the driver waits for a second
/// click. Must be well below `LONG_PRESS_MS` so a held press classifies
/// cleanly instead of being buffered as the first half of a double click.
pub const DOUBLE_CLICK_WINDOW_MS: u64 = 300;

/// Minimum press travel ever observed (px, Euclidean) that reads as a
/// [`GestureEvent::Swipe`] — a quarter of the 240-px panel, so a tap's drift
/// never reaches it. Measured to the farthest *observed* point, never the
/// stale coarse-refresh release.
pub const SWIPE_MIN_DISTANCE_PX: u16 = 60;

/// Smallest dominant-direction travel (px) that re-reads as live movement
/// while a finger is down; below it the per-finger direction row keeps its
/// last axis instead of flickering with a stationary contact's micro-drift.
pub const MOVE_DEADBAND_PX: u16 = 2;

/// Farthest a second contact may land from an already-down finger and still
/// read as a self-cap phantom instead of a genuine second finger: a duplicate
/// point the controller echoes next to the real contact on press-in.
pub const PHANTOM_RADIUS_PX: u16 = 60;

/// How recently a finger's down edge may be for a nearby second contact to
/// count as its phantom echo — one touch-scan cadence: a genuine second finger
/// lands later or farther, a self-cap echo duplicates the same frame's
/// contact.
pub const PHANTOM_WINDOW_MS: u64 = TOUCH_SCAN_MS;

/// Farthest a second short lift may sit from the chambered first tap and still
/// pair into a [`GestureEvent::DoubleTap`]; deliberately above
/// [`PHANTOM_RADIUS_PX`] so a real double tap and a phantom echo never rule in
/// the same way.
pub const PENDING_PAIR_PX: u16 = 100;

/// How many fingers the touch path tracks side-by-side: the FT6336
/// self-capacitance panel exposes exactly two, and the live-point/last-gesture/
/// run-counter tuple is dimensioned to that ceiling.
pub const MAX_TRACKED_POINTS: usize = 2;

/// Gesture kind a finger most recently resolved, stamped into
/// [`FingerLast::kind`]; `0` = none yet. Codes match the on-panel corner
/// readout.
pub const FINGER_NONE: u8 = 0;
pub const FINGER_TAP: u8 = 1;
pub const FINGER_DOUBLE_TAP: u8 = 2;
pub const FINGER_LONG_PRESS: u8 = 3;
pub const FINGER_SWIPE: u8 = 4;

/// The gesture one tracked finger last resolved, so the overlay can answer per
/// finger what it did: `kind` the `FINGER_*` code, `dir` the swipe's
/// `SwipeDirection::code` (`0` otherwise), `value` the measured hold (ms) or
/// swipe travel (px).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FingerLast {
    pub kind: u8,
    pub dir: u8,
    pub value: u16,
}

impl FingerLast {
    pub const fn empty() -> Self {
        Self {
            kind: FINGER_NONE,
            dir: 0,
            value: 0,
        }
    }
}

/// Consecutive identical samples needed to confirm a state change.
const DEBOUNCE_SAMPLES: u32 = 3;

/// Maximum simultaneous contacts a touch event can carry, aligned with
/// Android's `MotionEvent.MAX_POINTERS` ceiling: 10-point controllers can
/// occasionally report an extra contact, and the unused slots cost nothing
/// because a `TouchEvent` is a transient `Copy` value.
pub const MAX_TOUCH_POINTS: usize = 16;

/// Touch poll cadence, a whole multiple of [`INPUT_BASE_MS`] so the touch
/// source lands on the same shared grid. 10 ms keeps ~2 catch windows within a
/// 20 ms threshold dropout for marginal contacts.
pub const TOUCH_SCAN_MS: u64 = 10;

/// Consecutive absent samples that confirm a contact release, so a
/// single-frame dropout of a held contact does not read as a lift.
pub const RELEASE_CONFIRM_SAMPLES: u8 = 3;

/// Per-contact lifecycle: appeared / tracked / left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchStatus {
    Down,
    Contact,
    Release,
}

/// A tracked contact. `id` is stable per finger from `Down` until its
/// `Release`; consumers must key on `id`, never on array index, since point
/// order within an event is undefined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
    pub status: TouchStatus,
}

/// Snapshot of every contact in one sample. A released point stays in
/// `points` for the single sample that observed its `Release`, then
/// disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchEvent {
    pub points: [TouchPoint; MAX_TOUCH_POINTS],
    pub len: u8,
    /// Raw controller contact count (`TD_STATUS`); can transiently exceed
    /// `len` while the chip still counts a release in flight.
    pub contacts: u8,
}

/// Maps raw touch-panel coordinates into framebuffer space. Defaults to
/// identity; boards calibrate per module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchMap {
    pub swap_xy: bool,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl TouchMap {
    /// Raw coordinates are already in framebuffer space.
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

/// `XH[7:6]` event bits of an FT5x06-family (FT6336) touch point: `0b00` the
/// down edge and `0b10` a held contact both carry live coordinates; `0b01` is
/// the lift frame. Filtering these wrongly would read every hold as an
/// immediate lift and the long-press as a short tap.
pub fn ft6x06_point_is_live(xh: u8) -> bool {
    matches!((xh >> 6) & 0x03, 0b00 | 0b10)
}

fn dominant_axis(dx: i32, dy: i32) -> SwipeDirection {
    let ax = dx.unsigned_abs();
    let ay = dy.unsigned_abs();
    let (max, min) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    if min * 12 >= max * 5 {
        // Diagonal band: the octant a ±22.5°-wide 45° line leaves ties toward.
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

/// Direction of the latest frame-to-frame move while a finger is down, for the
/// live per-finger rows: `None` within `deadband_px` (jitter reads as holding
/// still), otherwise the same answer as a resolved [`GestureEvent::Swipe`] so
/// the live arrow and the slide digit never disagree. Pure so the device
/// manager and classifier share one answer.
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

/// Euclidean travel between two positions in pixels, threshold not applied —
/// the running farthest tracker compares every observed point, not just the
/// final release.
fn travel_px(from_x: u16, from_y: u16, to_x: u16, to_y: u16) -> u16 {
    swipe_distance(sq_dist(from_x, from_y, to_x, to_y))
}

/// Classifies a displacement into a swipe: `None` under
/// [`SWIPE_MIN_DISTANCE_PX`], else the dominant destination octant and travel
/// in pixels. Pure so the classifier and the on-panel distance digit share one
/// answer.
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

/// Euclidean length of a displacement for the swipe distance digit; nothing
/// panel-sized can overflow `u16`, so saturate rather than wrap.
fn swipe_distance(sq: u32) -> u16 {
    u16::try_from(isqrt_u32(sq)).unwrap_or(u16::MAX)
}

fn sq_dist(x0: u16, y0: u16, x1: u16, y1: u16) -> u32 {
    let dx = (x0 as i32 - x1 as i32).unsigned_abs();
    let dy = (y0 as i32 - y1 as i32).unsigned_abs();
    dx * dx + dy * dy
}

/// Whether two lift positions belong to the same physical tap cadence.
fn pair_close(x0: u16, y0: u16, x1: u16, y1: u16) -> bool {
    sq_dist(x0, y0, x1, y1) <= u32::from(PENDING_PAIR_PX) * u32::from(PENDING_PAIR_PX)
}

/// Floor of the square root of a `u32` via Newton iteration. Integer-only so
/// the classifier runs on the same fixed-point policy as the rest of `no_std`.
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

/// Poll-based input source. The input task polls this at a fixed cadence,
/// packaging each event with its wiring-order source via
/// [`crate::intent::recognize`].
pub trait InputSource {
    fn poll(&mut self, now_ms: u64) -> Option<InputEvent>;
}

/// Combines/coalesces raw input events into higher-level gestures.
/// Implementations can accumulate state (e.g. multi-press counters, hold
/// durations) and suppress intermediate events.
pub trait EventAggregator {
    fn feed(&mut self, event: InputEvent, now_ms: u64) -> Option<InputEvent>;

    /// Advance the aggregator's clock without a raw sample, so time-held
    /// gestures (e.g. a pending double-click window) can expire deterministically
    /// even when the source stays quiet. Default: no time-sensitivity.
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

/// Folds per-finger touch into `Press` down-edges, deferred `Tap`, in-window
/// `DoubleTap` (paired by position alone — hardware hands the second physical
/// tap a fresh tracker id), `Swipe` when a press ever travelled
/// [`SWIPE_MIN_DISTANCE_PX`] (measured to the farthest published point, since
/// the coarse panel leaves the release stale), and lone `LongPress`. A second
/// contact landing within [`PHANTOM_WINDOW_MS`]/[`PHANTOM_RADIUS_PX`] of a
/// down finger is a self-cap phantom echo, rejected and pulsed as `Ghost` once
/// per run.
///
/// Each contact owns a `FingerState` slot keyed by tracker `id`. Samples that
/// resolve no gesture forward the raw snapshot unchanged; extra resolved
/// gestures buffer for the next poll. `Press` fires whatever its lift's fate,
/// so physical presses tally even when nothing resolves; non-touch pulses pass
/// through untouched.
pub struct TouchGestures {
    /// One state machine per finger slot, keyed by the tracker `id`.
    fingers: [FingerState; MAX_TOUCH_POINTS],
    /// A short lift awaiting its second tap, paired by
    /// [`DOUBLE_CLICK_WINDOW_MS`]/[`PENDING_PAIR_PX`] — never by id, which
    /// hardware reuses across taps.
    chamber: Option<PendingTap>,
    /// Previous sample contained a phantom rejection; a run pulses once.
    ghosted: bool,
    /// Gestures a prior sample resolved beyond the single-event channel; FIFO.
    queued: [Option<InputEvent>; 2],
}

/// Per-finger gesture state: the owned tracker `id`, the armed down edge, and
/// [`FingerState::farthest`] — how far this press was ever observed from its
/// origin, the slide measure, since a release point is a stale coarse-refresh
/// snapshot. Loneliness is read at resolution from the live slots, not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FingerState {
    id: Option<u8>,
    pressed: bool,
    /// When the current contact went down, so a lift can measure its hold.
    down_at_ms: Option<u64>,
    /// Down-edge position, fixed so it anchors the slide measure as later
    /// published coordinates drift past it.
    down_x: u16,
    down_y: u16,
    /// Farthest observed `(distance_px, x, y)` from the origin — a slide that
    /// briefly crossed [`SWIPE_MIN_DISTANCE_PX`] and then released near its
    /// origin still reads as a swipe.
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

/// A short-lift tap parked until its double-click window closes, carrying the
/// finger that parked it so a lone release reports at its own position/id even
/// after the slot was freed for a newer finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingTap {
    owner_id: u8,
    first_at_ms: u64,
    x: u16,
    y: u16,
    held_ms: u16,
}

impl TouchGestures {
    pub fn new() -> Self {
        Self {
            fingers: [FingerState::free(); MAX_TOUCH_POINTS],
            chamber: None,
            ghosted: false,
            queued: [None, None],
        }
    }

    fn slot_of(&self, id: u8) -> Option<usize> {
        self.fingers.iter().position(|f| f.id == Some(id))
    }

    fn free_slot(&self) -> Option<usize> {
        self.fingers.iter().position(|f| f.id.is_none())
    }

    /// Whether a fresh contact lands close enough and soon enough after an
    /// already-down finger to be that finger's self-cap phantom echo.
    fn is_phantom(&self, x: u16, y: u16, now_ms: u64) -> bool {
        let reach_sq = u32::from(PHANTOM_RADIUS_PX) * u32::from(PHANTOM_RADIUS_PX);
        self.fingers.iter().any(|f| {
            f.pressed
                && f.down_at_ms
                    .is_some_and(|at| now_ms.saturating_sub(at) <= PHANTOM_WINDOW_MS)
                && sq_dist(f.down_x, f.down_y, x, y) <= reach_sq
        })
    }

    /// Append a resolved event, dropping the oldest once full.
    fn queue_event(&mut self, event: InputEvent) {
        if self.queued[0].is_none() {
            self.queued[0] = Some(event);
            return;
        }
        if self.queued[1].is_none() {
            self.queued[1] = Some(event);
            return;
        }
        self.queued[0] = self.queued[1].take();
        self.queued[1] = Some(event);
    }

    /// FIFO pop of the pending buffer.
    fn pop_event(&mut self) -> Option<InputEvent> {
        let first = self.queued[0].take();
        self.queued[0] = self.queued[1].take();
        first
    }

    /// Drain the event channel: a buffered event emits first (the fresh batch
    /// queues behind it), else the batch's first event — queueing the rest.
    /// `fallback` fills in when the batch is empty, so a quiet sample still
    /// lets the empty raw snapshot through.
    fn forward(
        &mut self,
        fresh: &mut [Option<InputEvent>; 4],
        fallback: Option<InputEvent>,
    ) -> Option<InputEvent> {
        if let Some(older) = self.pop_event() {
            for event in fresh.iter().flatten() {
                self.queue_event(*event);
            }
            if let Some(fill) = fallback
                && fresh.iter().all(Option::is_none)
            {
                self.queue_event(fill);
            }
            return Some(older);
        }
        let mut events = fresh.iter().flatten().copied();
        match events.next() {
            Some(first) => {
                for event in events {
                    self.queue_event(event);
                }
                Some(first)
            }
            None => fallback,
        }
    }

    /// Classify one sample, appending every resolved gesture in point order
    /// (then any phantom `Ghost`) to `out`.
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
                        // A genuine new finger (or a phantom that outlived its
                        // down window) starts its own state machine.
                        self.fingers[slot].id = Some(point.id);
                        Some(slot)
                    } else {
                        None
                    };
                    if let Some(slot) = slot {
                        let armed = !self.fingers[slot].pressed;
                        if armed {
                            // The rise into a held state is the press: anchor
                            // the down edge and start hold clock + slide
                            // measure at the origin.
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
                        // A phantom's release (never tracked) or a duplicate
                        // lift: nothing to pair with.
                        continue;
                    };
                    // Two lifts in one sample resolve in id order, so by the
                    // second the panel already looks empty; such a paired lift
                    // is this panel's mirror ghost breaking with its real
                    // finger — never a deliberate long-press — so bar it from
                    // the long-press check entirely.
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

        // Phantom runs collapse to one pulse: only the rise into the run emits.
        if !saw_phantom {
            self.ghosted = false;
        } else if !self.ghosted {
            self.ghosted = true;
            push(InputEvent::Gesture(GestureEvent::Ghost), out);
        }
    }

    /// Resolve a finger's lift into its gesture (or park its tap in the
    /// chamber), then free its slot.
    fn resolve_release(
        &mut self,
        slot: usize,
        point: TouchPoint,
        now_ms: u64,
        sibling_lift: bool,
    ) -> Option<InputEvent> {
        // Snapshot what the lift needs before the chamber helper borrows
        // `&mut self` again below.
        let (id, down_x, down_y, held, held_ms, farthest) = {
            let f = &mut self.fingers[slot];
            let id = f.id?;
            f.pressed = false;
            let down_at = f.down_at_ms.take()?;
            let held = now_ms.saturating_sub(down_at);
            let held_ms = u16::try_from(held.min(u64::from(u16::MAX))).unwrap_or(u16::MAX);
            // The release snapshot is one more observation: fold its travel in
            // so a slide crossing the threshold on its final publication still
            // reads as a swipe.
            let far = travel_px(f.down_x, f.down_y, point.x, point.y);
            if f.farthest.is_none_or(|(d, _, _)| far > d) {
                f.farthest = Some((far, point.x, point.y));
            }
            Some((id, f.down_x, f.down_y, held, held_ms, f.farthest))
        }?;
        let free_slot = |slots: &mut [FingerState; MAX_TOUCH_POINTS]| slots[slot].id = None;
        // A lift ever observed at [`SWIPE_MIN_DISTANCE_PX`] is a swipe and
        // closes this finger's cadence.
        if let Some((distance_px, end_x, end_y)) = farthest
            && distance_px >= SWIPE_MIN_DISTANCE_PX
        {
            let direction = dominant_axis(
                i32::from(end_x) - i32::from(down_x),
                i32::from(end_y) - i32::from(down_y),
            );
            self.close_own_chamber(id, down_x, down_y);
            free_slot(&mut self.fingers);
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
            // A lone, stationary press is a long-press; "lone" is read at this
            // lift's own resolution, so a live sibling or same-sample lift
            // (the parked mirror/companion) bars it.
            self.close_own_chamber(id, down_x, down_y);
            free_slot(&mut self.fingers);
            return Some(InputEvent::Gesture(GestureEvent::LongPress {
                id,
                x: down_x,
                y: down_y,
                held_ms,
            }));
        }
        // A short lift is a tap candidate, paired by window and proximity
        // alone; a faraway concurrent finger flushes the chamber as a lone tap
        // and parks in its place.
        match self.chamber.take() {
            None => {
                self.chamber = Some(PendingTap {
                    owner_id: id,
                    first_at_ms: now_ms,
                    x: point.x,
                    y: point.y,
                    held_ms,
                });
                free_slot(&mut self.fingers);
                None
            }
            Some(pending)
                if now_ms.saturating_sub(pending.first_at_ms) <= DOUBLE_CLICK_WINDOW_MS
                    && pair_close(pending.x, pending.y, point.x, point.y) =>
            {
                free_slot(&mut self.fingers);
                Some(InputEvent::Gesture(GestureEvent::DoubleTap {
                    id,
                    x: pending.x,
                    y: pending.y,
                    end_x: point.x,
                    end_y: point.y,
                    held_ms,
                }))
            }
            // The chamber outlived its window (a poll-cadence gap) or this
            // lift is a different finger elsewhere: release the chamber alone
            // and park this lift as its successor.
            Some(pending) => {
                self.chamber = Some(PendingTap {
                    owner_id: id,
                    first_at_ms: now_ms,
                    x: point.x,
                    y: point.y,
                    held_ms,
                });
                free_slot(&mut self.fingers);
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: pending.owner_id,
                    x: pending.x,
                    y: pending.y,
                    held_ms: pending.held_ms,
                }))
            }
        }
    }

    /// A resolved swipe/long-press ends the cadence this finger started, so
    /// the *next* tap must not pair with the abandoned one. A faraway other
    /// finger's swipe leaves the chamber standing.
    fn close_own_chamber(&mut self, id: u8, x: u16, y: u16) {
        if let Some(pending) = self.chamber {
            let owner = pending.owner_id == id;
            let same_spot = pair_close(pending.x, pending.y, x, y);
            if owner || same_spot {
                self.chamber = None;
            }
        }
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
            // Diagnostics pulses (driver I2C ghosts, chip read-backs) are not
            // contact frames: forward untouched, never drop.
            return Some(event);
        };
        let mut backlog = [None; 4];
        self.classify(&sample, now_ms, &mut backlog);
        self.forward(&mut backlog, Some(InputEvent::Touch(sample)))
    }

    fn tick(&mut self, now_ms: u64) -> Option<InputEvent> {
        // A chambered tap whose window outlived it releases as a lone tap.
        let mut expired = [None; 4];
        if let Some(pending) = self.chamber
            && now_ms.saturating_sub(pending.first_at_ms) > DOUBLE_CLICK_WINDOW_MS
        {
            self.chamber = None;
            expired[0] = Some(InputEvent::Gesture(GestureEvent::Tap {
                id: pending.owner_id,
                x: pending.x,
                y: pending.y,
                held_ms: pending.held_ms,
            }));
        }
        self.forward(&mut expired, None)
    }
}

/// Coalesces two clicks within [`DOUBLE_CLICK_WINDOW_MS`] into a single
/// `DoubleClick`, holding a lone first click until the window closes before
/// releasing it as `Click`. A `LongPress` or any non-click event flushes the
/// pending click immediately.
pub struct DoubleClickAggregator {
    first_click_at_ms: Option<u64>,
}

impl DoubleClickAggregator {
    pub const fn new() -> Self {
        Self {
            first_click_at_ms: None,
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
        match event {
            InputEvent::Button(ButtonEvent::Click) => match self.first_click_at_ms {
                None => {
                    self.first_click_at_ms = Some(now_ms);
                    None
                }
                Some(first) if now_ms.saturating_sub(first) <= DOUBLE_CLICK_WINDOW_MS => {
                    self.first_click_at_ms = None;
                    Some(InputEvent::Button(ButtonEvent::DoubleClick))
                }
                // The pending click outlived the window without a tick (e.g. a
                // cadence gap); release it now and buffer this one as the new
                // first click.
                Some(_) => {
                    self.first_click_at_ms = Some(now_ms);
                    Some(InputEvent::Button(ButtonEvent::Click))
                }
            },
            _ => {
                self.first_click_at_ms = None;
                Some(event)
            }
        }
    }

    fn tick(&mut self, now_ms: u64) -> Option<InputEvent> {
        match self.first_click_at_ms {
            Some(first) if now_ms.saturating_sub(first) > DOUBLE_CLICK_WINDOW_MS => {
                self.first_click_at_ms = None;
                Some(InputEvent::Button(ButtonEvent::Click))
            }
            _ => None,
        }
    }
}

/// Tracks raw per-sample contact lists into id-stable `TouchEvent`s by
/// nearest-neighbor id matching, so event order can shuffle without breaking
/// identity. A release confirms only after [`RELEASE_CONFIRM_SAMPLES`]
/// consecutive absent samples — a polled FT6336 drops a held contact for a
/// frame or two — and unchanged samples emit nothing.
#[derive(Debug, Clone, Copy)]
pub struct TouchContinuity {
    points: [TrackerPoint; MAX_TOUCH_POINTS],
    next_id: u8,
    /// Consecutive samples in which at least one active tracker went unconsumed.
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
    /// Farthest a raw point may move between samples to keep its id: wide
    /// enough for a full-cadence fast flick, far below the panel diagonal so a
    /// two-finger pinch keeps the fingers separate.
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

    /// Feed one raw sample (framebuffer space) plus the controller's own
    /// contact count; emits only when the contact set or a tracked position
    /// changed.
    pub fn update(&mut self, raw: &[(u16, u16)], contacts: u8) -> Option<TouchEvent> {
        let mut event = TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 0,
                y: 0,
                status: TouchStatus::Down,
            }; MAX_TOUCH_POINTS],
            len: 0,
            contacts,
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

        // A tracker missing this sample is a candidate release; confirmed only
        // after [`RELEASE_CONFIRM_SAMPLES`] consecutive absences, so an
        // intermittent FT6336 dropout resumes the same tracker (and id).
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
            // Trackers free even when the event array is full, so ids recycle
            // on the next sample.
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

/// A polled input source scheduled at its own cadence, paired with the
/// aggregator that turns its raw samples into the events
/// [`crate::intent::recognize`] packages. The input task polls it on the
/// shared base tick, gating each source to its own cadence. `source_id` is
/// the wiring-order identity the board assigned (button 0, button 1, …); the
/// interpreter's [`crate::intent::translate`] maps it onto the light instance
/// that source drives.
pub struct PollEntry {
    source_id: u8,
    source: Box<dyn InputSource>,
    aggregator: Box<dyn EventAggregator>,
    cadence_ms: u64,
    next_at_ms: u64,
}

impl PollEntry {
    pub fn new(
        source_id: u8,
        source: Box<dyn InputSource>,
        aggregator: Box<dyn EventAggregator>,
        cadence_ms: u64,
    ) -> Self {
        // First sample lands on the cadence grid, so debounce windows stay
        // uniform straight from boot.
        Self {
            source_id,
            source,
            aggregator,
            cadence_ms,
            next_at_ms: cadence_ms,
        }
    }

    /// The wiring-order identity the board assigned this input, driving intent
    /// routing (a button on source `n` targets light instance `n`).
    pub fn source_id(&self) -> u8 {
        self.source_id
    }

    /// The source's cadence, a whole multiple of the shared base tick.
    pub fn cadence_ms(&self) -> u64 {
        self.cadence_ms
    }

    /// When this source is next due on the shared grid.
    pub fn next_at_ms(&self) -> u64 {
        self.next_at_ms
    }

    /// Poll the source, then fold the raw sample (or the tick that stands in
    /// for it) through the aggregator.
    pub fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        match self.source.poll(now_ms) {
            Some(event) => self.aggregator.feed(event, now_ms),
            // No raw sample this pass; still let time-based aggregators expire
            // held gestures on their cadence grid.
            None => self.aggregator.tick(now_ms),
        }
    }

    /// Advance the source's schedule past `now_ms`, keeping it on its own
    /// cadence grid with no catch-up burst.
    pub fn advance_past(&mut self, now_ms: u64) {
        while self.next_at_ms <= now_ms {
            self.next_at_ms = self.next_at_ms.wrapping_add(self.cadence_ms);
        }
    }
}

/// Debounced button scanner, distinguishing clicks from long-presses by
/// hold duration.
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
    fn poll(&mut self, now_ms: u64) -> Option<InputEvent> {
        self.read(now_ms).map(InputEvent::Button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod button {
        use super::*;
        use core::cell::Cell;

        struct FakeButton<'a> {
            pressed: &'a Cell<bool>,
        }

        impl Button for FakeButton<'_> {
            fn is_pressed(&self) -> bool {
                self.pressed.get()
            }
        }

        /// Press the button, let it debounce to held at t=20, then release and
        /// confirm a released edge after `hold` ms. Returns the resulting
        /// gesture.
        fn hold_and_release(hold_ms: u64) -> Option<ButtonEvent> {
            let pressed = Cell::new(false);
            let btn = FakeButton { pressed: &pressed };
            let mut scanner = ButtonScanner::new(btn);

            // Press down; debounce confirms press at t=20 (held_since=20).
            pressed.set(true);
            for ms in 0..=30 {
                scanner.poll(ms * 10);
            }

            // Release starting at t=hold; debounce confirms the released edge at
            // hold+20 (3 consecutive released samples), computing held=hold_ms.
            pressed.set(false);
            let mut outcome = None;
            for ms in 0..=20 {
                if let Some(InputEvent::Button(e)) = scanner.poll(hold_ms + ms * 10) {
                    outcome = Some(e);
                }
            }
            outcome
        }

        #[test]
        fn debounce_confirms_a_click_only_after_n_consecutive_samples() {
            // A released edge confirmed only after DEBOUNCE_SAMPLES consecutive
            // released samples reads as a click; the raw press follows the
            // same rule.
            let pressed = Cell::new(false);
            let mut scanner = ButtonScanner::new(FakeButton { pressed: &pressed });
            for ms in [0, 10, 20] {
                pressed.set(true);
                assert!(scanner.poll(ms).is_none());
            }
            pressed.set(true);
            assert!(scanner.poll(30).is_none());
            pressed.set(false);
            for ms in [40, 50] {
                assert!(scanner.poll(ms).is_none());
            }
            assert_eq!(
                scanner.poll(60),
                Some(InputEvent::Button(ButtonEvent::Click))
            );
            // A glitch shorter than the confirm window never flips the held
            // state: the scanner still reads released at the end.
            let pressed = Cell::new(false);
            let mut scanner = ButtonScanner::new(FakeButton { pressed: &pressed });
            pressed.set(true);
            assert!(scanner.poll(0).is_none());
            assert!(scanner.poll(10).is_none());
            pressed.set(false);
            assert!(scanner.poll(20).is_none());
            assert!(scanner.poll(30).is_none());
            assert!(!scanner.pressed, "should still be released");
        }

        #[test]
        fn long_press_threshold_splits_hold_duration_on_release() {
            // LONG_PRESS_MS = 600 is the inclusive boundary: a 600 ms hold is
            // a long-press, a 599 ms hold is still a click.
            assert_eq!(hold_and_release(599), Some(ButtonEvent::Click));
            assert_eq!(hold_and_release(600), Some(ButtonEvent::LongPress));
            assert_eq!(hold_and_release(610), Some(ButtonEvent::LongPress));
            assert_eq!(hold_and_release(800), Some(ButtonEvent::LongPress));
            assert_eq!(LONG_PRESS_MS, 600);
        }

        #[test]
        fn button_cadence_and_windows_align_on_the_base_grid() {
            assert_eq!(BUTTON_SCAN_MS, 10);
            const _: () = assert!(BUTTON_SCAN_MS.is_multiple_of(INPUT_BASE_MS));
            const _: () = assert!(DOUBLE_CLICK_WINDOW_MS < LONG_PRESS_MS);
            const _: () = assert!(DOUBLE_CLICK_WINDOW_MS.is_multiple_of(INPUT_BASE_MS));
        }

        #[test]
        fn non_click_events_pass_through_immediately() {
            let mut passthrough = PassThrough;
            let click = InputEvent::Button(ButtonEvent::Click);
            assert_eq!(passthrough.feed(click, 100), Some(click));
            let mut agg = DoubleClickAggregator::new();
            let long = InputEvent::Button(ButtonEvent::LongPress);
            assert_eq!(
                agg.feed(long, 100),
                Some(long),
                "a long-press flushes and forwards"
            );
            assert_eq!(agg.tick(1000), None);
        }

        #[test]
        fn double_click_buffers_pairs_and_flushes() {
            let click = InputEvent::Button(ButtonEvent::Click);
            // A first click parks; a second inside the window pairs.
            let mut agg = DoubleClickAggregator::new();
            assert_eq!(agg.feed(click, 100), None, "first click buffers");
            assert_eq!(
                agg.feed(click, 100 + DOUBLE_CLICK_WINDOW_MS),
                Some(InputEvent::Button(ButtonEvent::DoubleClick))
            );
            // A lone click waits out the window, then the tick flushes it.
            let mut agg = DoubleClickAggregator::new();
            assert_eq!(agg.feed(click, 100), None);
            assert_eq!(
                agg.tick(100 + DOUBLE_CLICK_WINDOW_MS),
                None,
                "window still open at the boundary"
            );
            assert_eq!(
                agg.tick(100 + DOUBLE_CLICK_WINDOW_MS + 1),
                Some(InputEvent::Button(ButtonEvent::Click)),
                "the tick past the window releases the lone click"
            );
            // A stale first click outlives an unticked gap: the new click
            // flushes it as a lone Click and parks itself as the new first.
            let mut agg = DoubleClickAggregator::new();
            assert_eq!(agg.feed(click, 100), None);
            assert_eq!(
                agg.feed(click, 100 + DOUBLE_CLICK_WINDOW_MS + 1),
                Some(InputEvent::Button(ButtonEvent::Click)),
                "stale pending click released"
            );
            assert_eq!(
                agg.tick(100 + 2 * (DOUBLE_CLICK_WINDOW_MS + 1)),
                Some(InputEvent::Button(ButtonEvent::Click)),
                "the new buffer expires as a lone click"
            );
            // A long-press between two clicks flushes the pending first click.
            let mut agg = DoubleClickAggregator::new();
            let long = InputEvent::Button(ButtonEvent::LongPress);
            assert_eq!(agg.feed(click, 100), None);
            assert_eq!(agg.feed(long, 100 + DOUBLE_CLICK_WINDOW_MS - 1), Some(long));
            assert_eq!(agg.tick(100 + 2 * DOUBLE_CLICK_WINDOW_MS), None);
        }

        #[test]
        fn poll_entry_anchors_the_first_sample_on_the_cadence_grid() {
            // Leak an owned cell so the boxed, `'static` input source can share
            // the press state with the test drive loop.
            let pressed: &'static Cell<bool> = Box::leak(Box::new(Cell::new(false)));
            let mut entry = PollEntry::new(
                0,
                Box::new(ButtonScanner::new(FakeButton { pressed })),
                Box::new(PassThrough),
                BUTTON_SCAN_MS,
            );
            assert_eq!(entry.next_at_ms(), BUTTON_SCAN_MS);
            entry.advance_past(BUTTON_SCAN_MS);
            assert_eq!(entry.next_at_ms(), 2 * BUTTON_SCAN_MS);
            entry.advance_past(2 * BUTTON_SCAN_MS - 1);
            assert_eq!(entry.next_at_ms(), 2 * BUTTON_SCAN_MS);
            entry.advance_past(25);
            assert_eq!(entry.next_at_ms(), 3 * BUTTON_SCAN_MS);
        }

        #[test]
        fn poll_entry_wires_scanner_into_the_double_click() {
            // PollEntry schedules and steps the source at its cadence; a plain
            // click forwards as Click once debounce confirms the release.
            let pressed: &'static Cell<bool> = Box::leak(Box::new(Cell::new(false)));
            let mut entry = PollEntry::new(
                0,
                Box::new(ButtonScanner::new(FakeButton { pressed })),
                Box::new(PassThrough),
                BUTTON_SCAN_MS,
            );
            for ms in [0, 10, 20] {
                pressed.set(true);
                assert!(entry.poll(ms).is_none());
            }
            pressed.set(false);
            assert!(entry.poll(40).is_none());
            assert!(entry.poll(50).is_none());
            assert_eq!(entry.poll(60), Some(InputEvent::Button(ButtonEvent::Click)));
            // Two clicks inside the window collapse through the aggregator.
            let pressed: &'static Cell<bool> = Box::leak(Box::new(Cell::new(false)));
            let mut entry = PollEntry::new(
                0,
                Box::new(ButtonScanner::new(FakeButton { pressed })),
                Box::new(DoubleClickAggregator::new()),
                BUTTON_SCAN_MS,
            );
            // First click: pressed 0-20 (debounce confirms press at 20),
            // released 30-40 (release confirmed at 50) → Click buffered.
            for ms in [0, 10, 20] {
                pressed.set(true);
                assert!(entry.poll(ms).is_none());
            }
            pressed.set(false);
            assert!(entry.poll(30).is_none());
            assert!(entry.poll(40).is_none());
            assert!(entry.poll(50).is_none());
            // Second click: pressed 60-80, released 90-100 → confirmed at 110,
            // still inside the window of the first one (t=50).
            for ms in [60, 70, 80] {
                pressed.set(true);
                assert!(entry.poll(ms).is_none());
            }
            pressed.set(false);
            assert!(entry.poll(90).is_none());
            assert!(entry.poll(100).is_none());
            assert_eq!(
                entry.poll(110),
                Some(InputEvent::Button(ButtonEvent::DoubleClick))
            );
        }
    }

    mod tracker {
        use super::*;

        #[test]
        fn tracked_points_upsert_move_and_stay_quiet() {
            let mut c = TouchContinuity::new();
            let ev = c.update(&[(100, 200)], 1).unwrap();
            assert_eq!(ev.len, 1);
            assert_eq!(ev.contacts, 1);
            assert_eq!(
                ev.points[0],
                TouchPoint {
                    id: 0,
                    x: 100,
                    y: 200,
                    status: TouchStatus::Down,
                }
            );
            // A move keeps the id and upgrades to Contact.
            let ev = c.update(&[(120, 210)], 1).unwrap();
            assert_eq!(
                ev.points[0],
                TouchPoint {
                    id: 0,
                    x: 120,
                    y: 210,
                    status: TouchStatus::Contact,
                }
            );
            // An identical sample changes nothing.
            assert!(c.update(&[(120, 210)], 1).is_none());
        }

        #[test]
        fn release_confirms_only_after_a_dropout_run_and_recycles_ids() {
            // One absent sample is a dropout, not a lift; two absences still
            // sit inside the confirm window, so the contact resumes the same
            // tracker and id.
            let mut c = TouchContinuity::new();
            c.update(&[(100, 200)], 1);
            assert!(c.update(&[], 0).is_none(), "one dropout is not a lift");
            assert!(c.update(&[], 0).is_none());
            let ev = c.update(&[(101, 201)], 1).unwrap();
            assert_eq!(ev.points[0].status, TouchStatus::Contact);
            assert_eq!(ev.points[0].id, 0);
            // A full confirm run is a release: one emission, then quiet.
            let mut c = TouchContinuity::new();
            c.update(&[(100, 200)], 1);
            for _ in 0..RELEASE_CONFIRM_SAMPLES - 1 {
                assert!(c.update(&[], 0).is_none());
            }
            let ev = c.update(&[], 0).unwrap();
            assert_eq!(
                ev.points[0],
                TouchPoint {
                    id: 0,
                    x: 100,
                    y: 200,
                    status: TouchStatus::Release,
                }
            );
            assert!(c.update(&[], 0).is_none());
            // The next contact after a confirmed release is a fresh Down.
            let mut c = TouchContinuity::new();
            c.update(&[(5, 5)], 1);
            for _ in 0..RELEASE_CONFIRM_SAMPLES {
                c.update(&[], 0);
            }
            let ev = c.update(&[(7, 7)], 1).unwrap();
            assert_eq!(ev.points[0].status, TouchStatus::Down);
        }

        #[test]
        fn ids_follow_fingers_across_index_shuffles() {
            let mut c = TouchContinuity::new();
            let first = c.update(&[(10, 10), (300, 300)], 2).unwrap();
            assert_eq!(first.points[0].id, 0);
            assert_eq!(first.points[1].id, 1);
            // Register order swaps; ids must follow the fingers, not the
            // indices.
            let ev = c.update(&[(300, 301), (11, 12)], 2).unwrap();
            assert_eq!(ev.points[0].id, 1, "far point keeps its id at index 0");
            assert_eq!(ev.points[0].status, TouchStatus::Contact);
            assert_eq!(ev.points[1].id, 0, "near point keeps its id at index 1");
            assert_eq!(ev.points[1].status, TouchStatus::Contact);
        }

        #[test]
        fn multi_contact_samples_keep_releases_and_raw_counts() {
            // A single sample can lift every tracked point at once.
            let mut c = TouchContinuity::new();
            c.update(&[(5, 5), (6, 6)], 2);
            for _ in 0..RELEASE_CONFIRM_SAMPLES - 1 {
                assert!(c.update(&[], 0).is_none());
            }
            let ev = c.update(&[], 0).unwrap();
            assert_eq!(ev.len, 2);
            assert!(
                ev.points[..ev.len as usize]
                    .iter()
                    .all(|p| p.status == TouchStatus::Release)
            );
            // The raw class count can outlive the parsed points during a
            // release.
            let mut c = TouchContinuity::new();
            let ev = c.update(&[(5, 5)], 2).unwrap();
            assert_eq!(ev.len, 1);
            assert_eq!(ev.contacts, 2);
        }

        #[test]
        fn touch_map_identity_swap_and_mirror() {
            assert_eq!(TouchMap::IDENTITY.map((10, 20), 240, 320), (10, 20));
            let swap = TouchMap {
                swap_xy: true,
                mirror_x: false,
                mirror_y: false,
            };
            assert_eq!(swap.map((10, 20), 240, 320), (20, 10));
            let mirror = TouchMap {
                swap_xy: false,
                mirror_x: true,
                mirror_y: true,
            };
            assert_eq!(mirror.map((0, 0), 240, 320), (239, 319));
            let both = TouchMap {
                swap_xy: true,
                mirror_x: true,
                mirror_y: true,
            };
            assert_eq!(both.map((10, 20), 240, 320), (219, 309));
        }

        #[test]
        fn tracker_and_gesture_constants_align_on_the_touch_grid() {
            // Cadence and windows are whole multiples of the shared base tick,
            // and the release-confirm span sits cleanly below the double-click
            // window.
            const _: () = assert!(TOUCH_SCAN_MS.is_multiple_of(INPUT_BASE_MS));
            const _: () = assert!(DOUBLE_CLICK_WINDOW_MS.is_multiple_of(TOUCH_SCAN_MS));
            const _: () = assert!(LONG_PRESS_MS.is_multiple_of(TOUCH_SCAN_MS));
            const _: () = assert!(LONG_PRESS_MS > DOUBLE_CLICK_WINDOW_MS);
            assert_eq!(RELEASE_CONFIRM_SAMPLES, 3);
            const _: () = assert!(RELEASE_CONFIRM_SAMPLES as u64 * TOUCH_SCAN_MS == 30);
            const _: () =
                assert!((RELEASE_CONFIRM_SAMPLES as u64 * TOUCH_SCAN_MS) < DOUBLE_CLICK_WINDOW_MS);
            const _: () = assert!(MAX_TOUCH_POINTS == 16);
            // FT6x06 `XH[7:6]` event bits: down edges and held contacts carry
            // live coordinates; lift and reserved frames do not.
            assert!(ft6x06_point_is_live(0x00), "down edge");
            assert!(ft6x06_point_is_live(0x0F), "down with X high bits set");
            assert!(ft6x06_point_is_live(0x80), "contact with X=0");
            assert!(ft6x06_point_is_live(0x8F), "contact frame of a held finger");
            assert!(!ft6x06_point_is_live(0x40), "up/lift frame");
            assert!(!ft6x06_point_is_live(0x4F), "up with X high bits set");
            assert!(!ft6x06_point_is_live(0xC0), "reserved event");
            assert!(
                !ft6x06_point_is_live(0xFF),
                "reserved event with X high bits set"
            );
        }
    }

    mod classifier {
        use super::*;

        /// Builds a multi-point sample from `(status, id, x, y)` tuples, with
        /// `contacts` the raw controller class count.
        fn sample(points: &[(TouchStatus, u8, u16, u16)], contacts: u8) -> InputEvent {
            let mut pts = [TouchPoint {
                status: TouchStatus::Release,
                id: 0,
                x: 0,
                y: 0,
            }; MAX_TOUCH_POINTS];
            for (i, &(status, id, x, y)) in points.iter().take(MAX_TOUCH_POINTS).enumerate() {
                pts[i] = TouchPoint { status, id, x, y };
            }
            InputEvent::Touch(TouchEvent {
                points: pts,
                len: points.len().min(MAX_TOUCH_POINTS) as u8,
                contacts,
            })
        }

        /// A single-finger sample on id 0.
        fn touch(status: TouchStatus, x: u16, y: u16) -> InputEvent {
            sample(&[(status, 0, x, y)], 1)
        }

        fn touch_contacts_dirty(status: TouchStatus, x: u16, y: u16) -> InputEvent {
            // Raw TD_STATUS reporting 2 while only one point is parsed: a
            // release (or floating second contact) counted in-flight by the
            // chip.
            let mut ev = match touch(status, x, y) {
                InputEvent::Touch(ev) => ev,
                _ => unreachable!(),
            };
            ev.contacts = 2;
            InputEvent::Touch(ev)
        }

        #[test]
        fn press_fires_once_on_the_rise_edge() {
            // Only the transition into a held contact counts as a press;
            // contact follow-ups are silent and the lift defers the tap.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(touch(TouchStatus::Down, 10, 10), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 10,
                    y: 10
                }))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Contact, 10, 10), 5),
                Some(touch(TouchStatus::Contact, 10, 10))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Contact, 10, 10), 10),
                Some(touch(TouchStatus::Contact, 10, 10))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 15),
                Some(touch(TouchStatus::Release, 10, 10))
            );
            assert_eq!(
                agg.tick(316),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 10,
                    y: 10,
                    held_ms: 15
                }))
            );
            // A panel that drops the Down frame still pulses the press on the
            // first contact it publishes.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(touch(TouchStatus::Contact, 10, 10), 5),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 10,
                    y: 10
                }))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                Some(touch(TouchStatus::Release, 10, 10))
            );
            assert_eq!(
                agg.tick(311),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 10,
                    y: 10,
                    held_ms: 5
                }))
            );
            // A lift that never saw a press parks nothing and forwards the
            // bare snapshot.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                Some(touch(TouchStatus::Release, 10, 10)),
                "an unpaired lift is forwarded as a bare snapshot"
            );
            assert!(
                agg.tick(400).is_none(),
                "no press ever parked a tap chamber"
            );
        }

        #[test]
        fn a_short_lift_is_a_tap_at_its_lift_position() {
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(touch(TouchStatus::Down, 100, 100), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 100,
                    y: 100
                }))
            );
            // A short lift with drift a swipe threshold never sees still
            // defers a tap, reported at the lift position.
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 120, 105), 10),
                Some(touch(TouchStatus::Release, 120, 105)),
                "a lift that parks a pending tap still forwards the raw snapshot"
            );
            assert!(
                agg.tick(310).is_none(),
                "the window stays open at exactly 300ms"
            );
            assert_eq!(
                agg.tick(311),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 120,
                    y: 105,
                    held_ms: 10
                }))
            );
            // A dirty raw class count (2) with one parsed contact never
            // cancels the tap.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(touch_contacts_dirty(TouchStatus::Down, 10, 10), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 10,
                    y: 10
                }))
            );
            assert_eq!(
                agg.feed(touch_contacts_dirty(TouchStatus::Release, 10, 10), 10),
                Some(touch_contacts_dirty(TouchStatus::Release, 10, 10))
            );
            assert_eq!(
                agg.tick(311),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 10,
                    y: 10,
                    held_ms: 10
                }))
            );
        }

        #[test]
        fn hold_gap_monotonic_to_the_long_press_threshold() {
            // A lone stationary press sweeps the hold-time gap monotonically:
            // every gap below 600 ms is a deferred tap, 600 ms and beyond is a
            // long-press — the lift's own resolution, never the tick's.
            for gap in [300, 400, 500, 599] {
                let mut agg = TouchGestures::new();
                assert!(
                    agg.feed(touch(TouchStatus::Down, 100, 100), 0).is_some(),
                    "gap {gap}: down edge pulses Press"
                );
                assert_eq!(
                    agg.feed(touch(TouchStatus::Release, 100, 100), gap),
                    Some(touch(TouchStatus::Release, 100, 100)),
                    "gap {gap}: the short lift defers its tap"
                );
                assert_eq!(
                    agg.tick(gap + 311),
                    Some(InputEvent::Gesture(GestureEvent::Tap {
                        id: 0,
                        x: 100,
                        y: 100,
                        held_ms: gap as u16
                    })),
                    "gap {gap}: the tick past the window releases the tap"
                );
            }
            for gap in [600, 610, 800] {
                let mut agg = TouchGestures::new();
                assert!(agg.feed(touch(TouchStatus::Down, 100, 100), 0).is_some());
                assert_eq!(
                    agg.feed(touch(TouchStatus::Release, 100, 100), gap),
                    Some(InputEvent::Gesture(GestureEvent::LongPress {
                        id: 0,
                        x: 100,
                        y: 100,
                        held_ms: gap as u16
                    })),
                    "gap {gap}: a hold to the threshold is a long-press on release"
                );
            }
        }

        #[test]
        fn any_press_that_ever_traveled_the_slide_threshold_is_a_swipe() {
            // A quick flick crosses SWIPE_MIN_DISTANCE_PX on its release
            // publication.
            let mut agg = TouchGestures::new();
            assert!(
                agg.feed(touch(TouchStatus::Down, 20, 120), 0).is_some(),
                "down edge pulses Press"
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 140, 125), 90),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    direction: SwipeDirection::Right,
                    x: 20,
                    y: 120,
                    end_x: 140,
                    end_y: 125,
                    held_ms: 90,
                    distance_px: 120,
                }))
            );
            assert!(agg.tick(400).is_none(), "no tap is buffered after a swipe");
            // A slow slide that outlives the long-press window is still a
            // swipe: the ever-travelled measure decides, not the hold.
            let mut agg = TouchGestures::new();
            assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 110, 10), LONG_PRESS_MS + 1),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    direction: SwipeDirection::Right,
                    x: 10,
                    y: 10,
                    end_x: 110,
                    end_y: 10,
                    held_ms: LONG_PRESS_MS as u16 + 1,
                    distance_px: 100,
                }))
            );
            // The one far coordinate published mid-hold decides even when the
            // lift returns to the origin.
            let mut agg = TouchGestures::new();
            assert!(agg.feed(touch(TouchStatus::Down, 20, 120), 0).is_some());
            let mid = agg.feed(touch(TouchStatus::Contact, 140, 125), 400);
            assert!(
                !matches!(mid, Some(InputEvent::Gesture(_))),
                "a mid-hold position is a silent Contact: {mid:?}"
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 20, 120), LONG_PRESS_MS + 100),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    direction: SwipeDirection::Right,
                    x: 20,
                    y: 120,
                    end_x: 140,
                    end_y: 125,
                    held_ms: LONG_PRESS_MS as u16 + 100,
                    distance_px: 120,
                }))
            );
        }

        #[test]
        fn swipe_classify_reads_axes_bands_and_none_below_threshold() {
            // Straight axes read their cardinal direction and Euclidean
            // travel.
            for (x0, y0, x1, y1, direction, px) in [
                (10, 100, 90, 100, SwipeDirection::Right, 80),
                (90, 100, 10, 100, SwipeDirection::Left, 80),
                (100, 200, 100, 120, SwipeDirection::Up, 80),
                (100, 120, 100, 200, SwipeDirection::Down, 80),
            ] {
                assert_eq!(swipe_classify(x0, y0, x1, y1), Some((direction, px)));
            }
            // The ±22.5° diagonal band resolves to the diagonal octant; a 3:1
            // rake (≈71.6°) is steeper than the 67.5° edge and folds straight.
            for (x0, y0, x1, y1, direction) in [
                (0, 0, 60, 60, SwipeDirection::DownRight),
                (0, 60, 60, 0, SwipeDirection::UpRight),
                (60, 0, 0, 60, SwipeDirection::DownLeft),
                (60, 60, 0, 0, SwipeDirection::UpLeft),
            ] {
                assert_eq!(swipe_classify(x0, y0, x1, y1), Some((direction, 84)));
            }
            let (direction, _) = swipe_classify(0, 0, 20, 60).unwrap();
            assert_eq!(direction, SwipeDirection::Down);
            let (direction, distance) = swipe_classify(10, 10, 80, 80).unwrap();
            assert_eq!(direction, SwipeDirection::DownRight);
            assert_eq!(distance, 98, "sqrt(70²+70²) floors to 98");
            assert_eq!(swipe_classify(0, 0, 50, 0), None);
            assert_eq!(swipe_classify(0, 0, 0, 59), None);
        }

        #[test]
        fn swipe_threshold_straddles_slop_monotonically() {
            // On the 240-px-wide panel a quarter is 60 px: the threshold never
            // exceeds it so a deliberate flick always crosses.
            const QUARTER_PANEL: u16 = 240 / 4;
            const _: () = assert!(SWIPE_MIN_DISTANCE_PX <= QUARTER_PANEL);
            // Every octant classifies monotonically around the slop: 1 px under
            // the threshold is still no swipe, exactly at the threshold the
            // travel (60 px) reads its direction, and a far slide keeps the
            // same direction.
            for (label, (x0, y0), (x_below, y_below), (x_at, y_at), (x_far, y_far), direction) in [
                (
                    "right",
                    (0, 100),
                    (59, 100),
                    (60, 100),
                    (80, 100),
                    SwipeDirection::Right,
                ),
                (
                    "left",
                    (80, 100),
                    (21, 100),
                    (20, 100),
                    (0, 100),
                    SwipeDirection::Left,
                ),
                (
                    "down",
                    (100, 0),
                    (100, 59),
                    (100, 60),
                    (100, 80),
                    SwipeDirection::Down,
                ),
                (
                    "up",
                    (100, 80),
                    (100, 21),
                    (100, 20),
                    (100, 0),
                    SwipeDirection::Up,
                ),
                (
                    "down-right",
                    (0, 0),
                    (50, 32),
                    (50, 34),
                    (80, 80),
                    SwipeDirection::DownRight,
                ),
                (
                    "up-right",
                    (0, 80),
                    (50, 48),
                    (50, 46),
                    (80, 0),
                    SwipeDirection::UpRight,
                ),
                (
                    "down-left",
                    (80, 0),
                    (30, 32),
                    (30, 34),
                    (0, 80),
                    SwipeDirection::DownLeft,
                ),
                (
                    "up-left",
                    (80, 80),
                    (30, 48),
                    (30, 46),
                    (0, 0),
                    SwipeDirection::UpLeft,
                ),
            ] {
                assert_eq!(
                    swipe_classify(x0, y0, x_below, y_below),
                    None,
                    "{label}: 59 px of travel is still a tap"
                );
                let (dir, px) = swipe_classify(x0, y0, x_at, y_at)
                    .expect("{label}: 60 px crosses the slop threshold");
                assert_eq!(dir, direction);
                assert_eq!(px, 60);
                let (dir, px) = swipe_classify(x0, y0, x_far, y_far).unwrap();
                assert_eq!(dir, direction, "{label}: the far slide keeps its direction");
                assert!(px > 60, "{label}: travel keeps growing past the threshold");
            }
        }

        #[test]
        fn direction_between_reads_axes_and_respects_the_deadband() {
            assert_eq!(
                direction_between(100, 200, 100, 120, MOVE_DEADBAND_PX),
                Some(SwipeDirection::Up)
            );
            assert_eq!(
                direction_between(100, 120, 100, 200, MOVE_DEADBAND_PX),
                Some(SwipeDirection::Down)
            );
            assert_eq!(
                direction_between(120, 100, 30, 100, MOVE_DEADBAND_PX),
                Some(SwipeDirection::Left)
            );
            assert_eq!(
                direction_between(30, 100, 90, 100, MOVE_DEADBAND_PX),
                Some(SwipeDirection::Right)
            );
            assert_eq!(
                direction_between(100, 100, 50, 150, MOVE_DEADBAND_PX),
                Some(SwipeDirection::DownLeft)
            );
            assert_eq!(
                direction_between(100, 150, 160, 60, MOVE_DEADBAND_PX),
                Some(SwipeDirection::UpRight)
            );
            assert_eq!(
                direction_between(10, 10, 80, 80, MOVE_DEADBAND_PX),
                Some(SwipeDirection::DownRight)
            );
            // Micro-drift within the deadband keeps the last axis instead of
            // flickering with a stationary contact.
            assert_eq!(direction_between(10, 10, 11, 10, MOVE_DEADBAND_PX), None);
            assert_eq!(direction_between(10, 10, 10, 11, MOVE_DEADBAND_PX), None);
            assert_eq!(direction_between(10, 10, 11, 11, MOVE_DEADBAND_PX), None);
        }

        #[test]
        fn in_window_lifts_pair_into_a_double_tap() {
            // Two short lifts in the same spot inside the window pair into a
            // double-tap — keyed by proximity, never by id, since hardware
            // hands the second physical tap a fresh tracker id.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Down, 12, 40, 60)], 1), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 12,
                    x: 40,
                    y: 60
                }))
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 12, 40, 60)], 1), 10),
                Some(sample(&[(TouchStatus::Release, 12, 40, 60)], 1)),
                "the first lift parks the chamber and forwards its raw snapshot"
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Down, 7, 45, 65)], 1), 90),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 7,
                    x: 45,
                    y: 65
                })),
                "the second physical tap is a fresh tracker, never the old id"
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 7, 45, 65)], 1), 100),
                Some(InputEvent::Gesture(GestureEvent::DoubleTap {
                    id: 7,
                    x: 40,
                    y: 60,
                    end_x: 45,
                    end_y: 65,
                    held_ms: 10
                })),
                "the same-spot lift pairs by position, not by id"
            );
            // The same id works too, mirroring the button's double-click.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(touch(TouchStatus::Down, 10, 10), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 10,
                    y: 10
                }))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                Some(touch(TouchStatus::Release, 10, 10))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Down, 30, 30), 100),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 30,
                    y: 30
                }))
            );
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 40, 30), 170),
                Some(InputEvent::Gesture(GestureEvent::DoubleTap {
                    id: 0,
                    x: 10,
                    y: 10,
                    end_x: 40,
                    end_y: 30,
                    held_ms: 70
                }))
            );
        }

        #[test]
        fn double_tap_gap_sweeps_the_chamber_window() {
            // Two short lifts separated by ≤ DOUBLE_CLICK_WINDOW_MS (300, the
            // inclusive boundary) pair; one past it releases as two taps, in
            // lift order.
            for gap in [200_u64, 290, 300] {
                let mut agg = TouchGestures::new();
                assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
                assert_eq!(
                    agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                    Some(touch(TouchStatus::Release, 10, 10))
                );
                assert!(agg.feed(touch(TouchStatus::Down, 20, 20), gap).is_some());
                assert!(
                    matches!(
                        agg.feed(touch(TouchStatus::Release, 20, 20), gap + 10),
                        Some(InputEvent::Gesture(GestureEvent::DoubleTap {
                            id: 0,
                            x: 10,
                            y: 10,
                            ..
                        }))
                    ),
                    "gap {gap}: the second lift inside the window pairs"
                );
                assert!(
                    agg.tick(400).is_none(),
                    "gap {gap}: nothing remains pending after the pair"
                );
            }
            for gap in [301_u64, 400, 599] {
                let mut agg = TouchGestures::new();
                assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
                assert_eq!(
                    agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                    Some(touch(TouchStatus::Release, 10, 10))
                );
                assert!(agg.feed(touch(TouchStatus::Down, 20, 20), gap).is_some());
                assert!(
                    matches!(
                        agg.feed(touch(TouchStatus::Release, 20, 20), gap + 10),
                        Some(InputEvent::Gesture(GestureEvent::Tap {
                            id: 0,
                            x: 10,
                            y: 10,
                            ..
                        }))
                    ),
                    "gap {gap}: the lapsed pending tap surfaces at its own lift first"
                );
                assert!(
                    matches!(
                        agg.tick(gap + 311),
                        Some(InputEvent::Gesture(GestureEvent::Tap { id: 0, x: 20, .. }))
                    ),
                    "gap {gap}: the new tap parks and expires via tick"
                );
            }
        }

        #[test]
        fn separated_lifts_release_as_independent_taps() {
            let mut agg = TouchGestures::new();
            assert!(agg.tick(100).is_none(), "a quiet clock parks nothing");
            // Lifts of the same finger more than the window apart are
            // independent taps, each released by a tick once its own window
            // closes.
            assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                Some(touch(TouchStatus::Release, 10, 10))
            );
            assert_eq!(
                agg.tick(311),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 10,
                    y: 10,
                    held_ms: 10
                }))
            );
            assert!(agg.feed(touch(TouchStatus::Down, 20, 20), 400).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 20, 20), 410),
                Some(touch(TouchStatus::Release, 20, 20))
            );
            assert_eq!(
                agg.tick(711),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 20,
                    y: 20,
                    held_ms: 10
                }))
            );
        }

        #[test]
        fn an_end_gesture_discards_the_pending_tap() {
            // Tap once, then slide: the swipe wins and drops the pending tap
            // instead of double-pairing with it.
            let mut agg = TouchGestures::new();
            assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                Some(touch(TouchStatus::Release, 10, 10))
            );
            assert!(agg.feed(touch(TouchStatus::Down, 10, 200), 400).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 100), 480),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    direction: SwipeDirection::Up,
                    x: 10,
                    y: 200,
                    end_x: 10,
                    end_y: 100,
                    held_ms: 80,
                    distance_px: 100,
                }))
            );
            assert!(
                agg.tick(800).is_none(),
                "the swipe dropped the first tap's pending window"
            );
            // The same suite, ending in a long-press: the pending tap is
            // dropped too.
            let mut agg = TouchGestures::new();
            assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 10, 10), 10),
                Some(touch(TouchStatus::Release, 10, 10))
            );
            assert!(agg.feed(touch(TouchStatus::Down, 20, 20), 400).is_some());
            assert_eq!(
                agg.feed(touch(TouchStatus::Release, 20, 20), 1000),
                Some(InputEvent::Gesture(GestureEvent::LongPress {
                    id: 0,
                    x: 20,
                    y: 20,
                    held_ms: 600
                }))
            );
            assert!(
                agg.tick(1300).is_none(),
                "the long-press dropped the pending tap instead of double-pairing"
            );
        }

        #[test]
        fn phantom_echoes_pulse_once_and_leave_the_finger_armed() {
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Down, 0, 100, 100)], 1), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 100,
                    y: 100
                }))
            );
            // A second contact inside the first finger's down window and
            // within PHANTOM_RADIUS_PX is that finger's self-cap echo:
            // rejected, pulsed once as Ghost, and given no tracker of its own
            // — so a duplicate at press-in never pairs into a double-tap nor
            // collides with a genuinely concurrent second finger.
            assert_eq!(
                agg.feed(
                    sample(
                        &[
                            (TouchStatus::Contact, 0, 100, 100),
                            (TouchStatus::Down, 1, 110, 105)
                        ],
                        2,
                    ),
                    10,
                ),
                Some(InputEvent::Gesture(GestureEvent::Ghost))
            );
            // The echo's own lift reaches no tracker; the sole real finger
            // stays armed exactly as a single-finger press would.
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 1, 110, 105)], 1), 20),
                Some(sample(&[(TouchStatus::Release, 1, 110, 105)], 1)),
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 0, 100, 100)], 1), 30),
                Some(sample(&[(TouchStatus::Release, 0, 100, 100)], 1)),
            );
            assert_eq!(
                agg.tick(331),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 100,
                    y: 100,
                    held_ms: 30
                }))
            );
            // A second echo still inside the same down window collapses to the
            // same run: no re-pulse, and the untouched finger just forwards its
            // snapshot; a clean run closes it, so a fresh finger plus its echo
            // is a new Ghost.
            let mut agg = TouchGestures::new();
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 0, 30, 30)], 1), 0)
                    .is_some()
            );
            assert_eq!(
                agg.feed(
                    sample(
                        &[
                            (TouchStatus::Contact, 0, 30, 30),
                            (TouchStatus::Down, 1, 35, 35)
                        ],
                        2
                    ),
                    5
                ),
                Some(InputEvent::Gesture(GestureEvent::Ghost))
            );
            assert_eq!(
                agg.feed(
                    sample(
                        &[
                            (TouchStatus::Contact, 0, 30, 30),
                            (TouchStatus::Down, 2, 36, 36)
                        ],
                        2
                    ),
                    9
                ),
                Some(sample(
                    &[
                        (TouchStatus::Contact, 0, 30, 30),
                        (TouchStatus::Down, 2, 36, 36)
                    ],
                    2
                )),
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Contact, 0, 30, 30)], 1), 50),
                Some(sample(&[(TouchStatus::Contact, 0, 30, 30)], 1)),
                "a clean one-contact sample closes the run"
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Down, 3, 200, 200)], 1), 60),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 3,
                    x: 200,
                    y: 200
                }))
            );
            assert_eq!(
                agg.feed(
                    sample(
                        &[
                            (TouchStatus::Contact, 3, 200, 200),
                            (TouchStatus::Down, 4, 205, 205)
                        ],
                        2
                    ),
                    65
                ),
                Some(InputEvent::Gesture(GestureEvent::Ghost)),
                "a genuinely new finger's echo is a fresh run, not a continuation"
            );
        }

        #[test]
        fn concurrent_fingers_never_pair_into_one_gesture() {
            // Finger 0 lands first; finger 1 joins far away: two trackers own
            // two independent state machines. The slide resolves the moment
            // finger 0 lifts — even while finger 1 is still down — and finger
            // 1's short lift is its own tap.
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Down, 0, 20, 120)], 1), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 0,
                    x: 20,
                    y: 120
                }))
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Down, 1, 150, 200)], 1), 0),
                Some(InputEvent::Gesture(GestureEvent::Press {
                    id: 1,
                    x: 150,
                    y: 200
                })),
                "a second faraway finger is genuine concurrency, not a phantom"
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 0, 140, 125)], 1), 90),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    direction: SwipeDirection::Right,
                    x: 20,
                    y: 120,
                    end_x: 140,
                    end_y: 125,
                    held_ms: 90,
                    distance_px: 120,
                }))
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Contact, 1, 150, 200)], 1), 95),
                Some(sample(&[(TouchStatus::Contact, 1, 150, 200)], 1)),
                "finger 1's hold is silent; only its snapshot passes through"
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 1, 150, 200)], 1), 100),
                Some(sample(&[(TouchStatus::Release, 1, 150, 200)], 1)),
            );
            assert_eq!(
                agg.tick(401),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 1,
                    x: 150,
                    y: 200,
                    held_ms: 100
                })),
                "finger 1's tap did not pair with finger 0's swipe"
            );
            // Two taps far apart are two taps: the double-click pairing is
            // keyed by proximity, so a finger elsewhere never pairs into the
            // chambered tap.
            let mut agg = TouchGestures::new();
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 0, 10, 90)], 1), 0)
                    .is_some()
            );
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 1, 150, 90)], 1), 0)
                    .is_some()
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 0, 10, 90)], 1), 10),
                Some(sample(&[(TouchStatus::Release, 0, 10, 90)], 1)),
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 1, 150, 90)], 1), 20),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: 10,
                    y: 90,
                    held_ms: 10
                })),
                "the faraway finger flushes the chambered tap as its own lone Tap"
            );
            assert_eq!(agg.tick(311), None);
            assert_eq!(
                agg.tick(321),
                Some(InputEvent::Gesture(GestureEvent::Tap {
                    id: 1,
                    x: 150,
                    y: 90,
                    held_ms: 20
                }))
            );
        }

        #[test]
        fn same_sample_lifts_buffer_the_second_in_fifo() {
            // Both genuine (far-apart) fingers slide and lift in one frame;
            // two swipes resolve, the classifier returns the first and buffers
            // the second on the single-event channel, drained FIFO by the next
            // poll.
            let mut agg = TouchGestures::new();
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 0, 20, 120)], 1), 0)
                    .is_some()
            );
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 1, 150, 200)], 1), 0)
                    .is_some()
            );
            let both_lift = sample(
                &[
                    (TouchStatus::Release, 0, 140, 125),
                    (TouchStatus::Release, 1, 80, 170),
                ],
                1,
            );
            assert_eq!(
                agg.feed(both_lift, 90),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 0,
                    direction: SwipeDirection::Right,
                    x: 20,
                    y: 120,
                    end_x: 140,
                    end_y: 125,
                    held_ms: 90,
                    distance_px: 120,
                }))
            );
            assert_eq!(
                agg.tick(100),
                Some(InputEvent::Gesture(GestureEvent::Swipe {
                    id: 1,
                    direction: SwipeDirection::UpLeft,
                    x: 150,
                    y: 200,
                    end_x: 80,
                    end_y: 170,
                    held_ms: 90,
                    distance_px: 76,
                })),
                "the second event waited in FIFO, not dropped"
            );
        }

        #[test]
        fn companion_lift_never_cycles_a_clean_hold_still_does() {
            // This panel's self-cap parks a far, mirror-ish companion contact
            // while a real finger is down; it sits still well past
            // LONG_PRESS_MS then lifts. Its lift must never cycle the mode.
            // But the *real* finger's own later lift — a clean hold the panel
            // ended as a single contact — is a deliberate long-press and must
            // still cycle. Only the lift moment decides: transient press-in
            // companions must not veto a hold that then carried the panel
            // alone.
            let mut agg = TouchGestures::new();
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 0, 20, 120)], 2), 0)
                    .is_some()
            );
            assert!(
                agg.feed(sample(&[(TouchStatus::Down, 1, 150, 150)], 2), 100)
                    .is_some(),
                "a far second contact is genuine concurrency, not a phantom"
            );
            let ghost_lift = agg.feed(sample(&[(TouchStatus::Release, 1, 150, 150)], 2), 900);
            assert!(
                !matches!(
                    ghost_lift,
                    Some(InputEvent::Gesture(GestureEvent::LongPress { .. }))
                ),
                "the parked companion's 900 ms lift never reads as a long-press: {ghost_lift:?}"
            );
            assert_eq!(
                agg.feed(sample(&[(TouchStatus::Release, 0, 20, 120)], 2), 1100),
                Some(InputEvent::Gesture(GestureEvent::LongPress {
                    id: 0,
                    x: 20,
                    y: 120,
                    held_ms: 1100,
                })),
                "the real finger's clean hold, ended alone, is the deliberate long-press"
            );
        }

        #[test]
        fn isqrt_u32_floors_perfect_and_imperfect_squares() {
            assert_eq!(isqrt_u32(0), 0);
            assert_eq!(isqrt_u32(1), 1);
            assert_eq!(isqrt_u32(4), 2);
            assert_eq!(isqrt_u32(65536), 256);
            assert_eq!(isqrt_u32(9800), 98);
            assert_eq!(isqrt_u32(u32::MAX), 65535);
        }

        #[test]
        fn chip_gestures_pass_through_unmodified() {
            let mut agg = TouchGestures::new();
            assert_eq!(
                agg.feed(InputEvent::ChipGesture(0x10), 0),
                Some(InputEvent::ChipGesture(0x10))
            );
        }
    }
}

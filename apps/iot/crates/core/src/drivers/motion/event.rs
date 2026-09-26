use super::MotionSample;

/// A tilt direction relative to gravity. Roll drives left/right, pitch up/down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiltDir {
    Left,
    Right,
    Up,
    Down,
}

/// A hardware-independent motion semantic. Sources differ wildly in how they
/// detect these (QMI8658A engines, an LSM6DSR FSM, a core-side classifier), so
/// the contract is the meaning, never the register or the threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionEvent {
    /// A mechanical knock resolved by the core-side recognizer, with the
    /// multi-knock count its gesture window produced. Not a screen touch: the
    /// panel's `GestureEvent::Tap` names a finger on glass and carries a
    /// position and a hold time, so the two may describe one act without being
    /// one event. The planes are never folded together — arbitration runs
    /// within a plane, so a knock and a finger tap both survive.
    Tap {
        count: u8,
    },
    Still,
    Moving,
    Activity,
    Step,
    TiltEnter(TiltDir),
    TiltExit(TiltDir),
    Shake,
    Lift,
    Place,
    Portrait,
    Landscape,
}

/// Mutually exclusive semantic families. Two candidates from the same family
/// describe one physical act, so arbitration keeps at most one per family
/// instead of forwarding both to the business layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionGroup {
    State,
    Pose,
    Action,
}

impl MotionEvent {
    pub const fn group(self) -> MotionGroup {
        match self {
            Self::Still | Self::Moving | Self::Activity | Self::Step => MotionGroup::State,
            Self::TiltEnter(_) | Self::TiltExit(_) | Self::Portrait | Self::Landscape => {
                MotionGroup::Pose
            }
            Self::Tap { .. } | Self::Shake | Self::Lift | Self::Place => MotionGroup::Action,
        }
    }

    /// Ranks candidates inside a family. A shake subsumes the tilt chatter it
    /// physically causes, and a lift/place outranks the orientation change that
    /// comes with it, so the most specific description of one act survives.
    pub const fn priority(self) -> u8 {
        match self {
            Self::Shake => 100,
            Self::Lift | Self::Place => 80,
            Self::TiltEnter(_) | Self::TiltExit(_) => 60,
            Self::Portrait | Self::Landscape => 50,
            Self::Tap { .. } => 40,
            Self::Step => 35,
            Self::Activity => 30,
            Self::Moving | Self::Still => 20,
        }
    }
}

/// How many times each semantic has been raised since boot, saturating. This
/// is the tuning counterpart of a capability set: a threshold is verified by
/// watching the bucket it feeds, so a semantic the board cannot report must
/// stay visibly distinct from one whose threshold simply never fires.
///
/// Counts are `u16` rather than the `u8` the touch counters use because a
/// shake runs near 3/s while its 300 ms refractory period allows it — 255
/// would be gone in well over a minute of handling, too short to calibrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionCounts {
    pub taps: u16,
    pub double_taps: u16,
    pub triple_taps: u16,
    pub still: u16,
    pub moving: u16,
    pub activity: u16,
    pub steps: u16,
    /// Tilt entries summed over all four directions.
    pub tilt_enters: u16,
    /// Tilt exits summed over all four directions.
    pub tilt_exits: u16,
    pub shakes: u16,
    pub lifts: u16,
    pub places: u16,
    pub portraits: u16,
    pub landscapes: u16,
}

impl MotionCounts {
    /// A zeroed tally, for the const-built boot state.
    pub const ZERO: Self = Self {
        taps: 0,
        double_taps: 0,
        triple_taps: 0,
        still: 0,
        moving: 0,
        activity: 0,
        steps: 0,
        tilt_enters: 0,
        tilt_exits: 0,
        shakes: 0,
        lifts: 0,
        places: 0,
        portraits: 0,
        landscapes: 0,
    };

    /// Files one semantic under its bucket. The two groupings a raw enum
    /// variant cannot express live here: a tap splits by the knock count its
    /// recognizer window resolved, and the four tilt directions collapse into
    /// one entry and one exit, since a direction is a detail of the same
    /// transition.
    pub fn bump(&mut self, event: MotionEvent) {
        let counter = match event {
            MotionEvent::Tap { count: 1 } => &mut self.taps,
            MotionEvent::Tap { count: 2 } => &mut self.double_taps,
            MotionEvent::Tap { .. } => &mut self.triple_taps,
            MotionEvent::Still => &mut self.still,
            MotionEvent::Moving => &mut self.moving,
            MotionEvent::Activity => &mut self.activity,
            MotionEvent::Step => &mut self.steps,
            MotionEvent::TiltEnter(_) => &mut self.tilt_enters,
            MotionEvent::TiltExit(_) => &mut self.tilt_exits,
            MotionEvent::Shake => &mut self.shakes,
            MotionEvent::Lift => &mut self.lifts,
            MotionEvent::Place => &mut self.places,
            MotionEvent::Portrait => &mut self.portraits,
            MotionEvent::Landscape => &mut self.landscapes,
        };
        *counter = counter.saturating_add(1);
    }
}

const GROUP_COUNT: usize = 3;

/// What a motion source can actually report. Declared, never switched: the
/// data plane ships unconditionally, so a source that omits a semantic simply
/// never raises it rather than gating the whole stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionCapabilities(u64);

impl MotionCapabilities {
    pub const EMPTY: Self = Self(0);
    pub const TELEMETRY: Self = Self(1 << 0);
    pub const TAP: Self = Self(1 << 1);
    pub const STEP: Self = Self(1 << 2);
    pub const STILL: Self = Self(1 << 3);
    pub const MOVING: Self = Self(1 << 4);
    pub const ACTIVITY: Self = Self(1 << 5);
    pub const TILT: Self = Self(1 << 6);
    pub const SHAKE: Self = Self(1 << 7);
    pub const LIFT_PLACE: Self = Self(1 << 8);
    pub const POSTURE: Self = Self(1 << 9);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Raw mask, for logging a build's declaration without a formatter.
    pub const fn bits(self) -> u64 {
        self.0
    }
}

/// What the core-side classifier derives from any source's data plane. Held
/// apart from a driver's own declarations so a semantic is never attributed to
/// hardware that never computed it, and so the two layers can be unioned into
/// the capability set a product actually advertises. Tap is core-side: the
/// QMI8658A tap engine this product abandoned never resolved a gesture from its
/// enable transient on, so the knock contract belongs to the recognizer here.
pub const RECOGNIZER_CAPABILITIES: MotionCapabilities = MotionCapabilities::TAP
    .union(MotionCapabilities::TILT)
    .union(MotionCapabilities::SHAKE)
    .union(MotionCapabilities::LIFT_PLACE)
    .union(MotionCapabilities::POSTURE);

/// A bounded set of semantics from one poll. The slot count has to hold the
/// *pre*-arbitration union, since a hardware engine flag and a core-side
/// classifier only meet here: one hardware engine (No-Motion) plus the five
/// core classifiers (tap, tilt, posture, shake, rest) is six, and arbitration
/// then collapses the set to at most one per family.
pub const MOTION_EVENT_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionEvents {
    slots: [Option<MotionEvent>; MOTION_EVENT_CAPACITY],
}

impl MotionEvents {
    pub const fn new() -> Self {
        Self {
            slots: [None; MOTION_EVENT_CAPACITY],
        }
    }

    /// Returns false when the set is full, so a caller can tell a dropped
    /// semantic from an absent one instead of silently losing it.
    pub fn push(&mut self, event: MotionEvent) -> bool {
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(event);
                return true;
            }
        }
        false
    }

    /// Appends another set, reporting how many semantics the capacity refused
    /// so a caller can surface a full set rather than a silent loss.
    pub fn extend_from(&mut self, other: MotionEvents) -> usize {
        let mut dropped = 0;
        for event in other.iter() {
            if !self.push(event) {
                dropped += 1;
            }
        }
        dropped
    }

    pub fn iter(&self) -> impl Iterator<Item = MotionEvent> + '_ {
        self.slots.iter().filter_map(|slot| *slot)
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    pub fn latest(&self) -> Option<MotionEvent> {
        self.slots.iter().rev().find_map(|slot| *slot)
    }

    fn retain(&mut self, mut keep: impl FnMut(MotionEvent) -> bool) {
        for slot in &mut self.slots {
            if let Some(event) = *slot
                && !keep(event)
            {
                *slot = None;
            }
        }
    }

    /// Drops everything but the highest-priority member of each family, so one
    /// physical act reaches the business layer as a single description.
    fn keep_best_per_group(&mut self) {
        let mut best: [Option<MotionEvent>; GROUP_COUNT] = [None; GROUP_COUNT];
        for event in self.iter() {
            let slot = &mut best[group_index(event.group())];
            if slot.is_none_or(|kept| event.priority() > kept.priority()) {
                *slot = Some(event);
            }
        }
        self.retain(|event| best[group_index(event.group())] == Some(event));
    }
}

const fn group_index(group: MotionGroup) -> usize {
    match group {
        MotionGroup::State => 0,
        MotionGroup::Pose => 1,
        MotionGroup::Action => 2,
    }
}

/// One poll's worth of motion: the data plane every consumer may read, plus the
/// semantics this poll raised. Both ship together — the data plane backs
/// diagnostics and any app-side algorithm, the semantic plane backs intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionBatch {
    pub sample: MotionSample,
    pub events: MotionEvents,
}

impl MotionBatch {
    pub const fn new(sample: MotionSample) -> Self {
        Self {
            sample,
            events: MotionEvents::new(),
        }
    }
}

/// Suppresses a repeated semantic from the same family. Without it a single
/// shake would raise `TiltEnter`/`TiltExit`/`Moving` on consecutive 20 ms
/// polls and the business layer would see one act as a burst of distinct acts.
pub const MOTION_COOLDOWN_MS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionArbiter {
    last_fired_ms: [Option<u64>; GROUP_COUNT],
}

impl MotionArbiter {
    pub const fn new() -> Self {
        Self {
            last_fired_ms: [None; GROUP_COUNT],
        }
    }

    /// Reduces a poll's candidates to at most one semantic per family, then
    /// drops any family still inside its cooldown. Events arriving from a
    /// hardware engine and from the core-side classifier pass through the same
    /// funnel, so the two can never describe one act twice.
    pub fn resolve(&mut self, now_ms: u64, events: &mut MotionEvents) {
        events.keep_best_per_group();
        events.retain(|event| {
            let slot = &mut self.last_fired_ms[group_index(event.group())];
            let cooled = slot.is_none_or(|last| now_ms.saturating_sub(last) >= MOTION_COOLDOWN_MS);
            if cooled {
                *slot = Some(now_ms);
            }
            cooled
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(events: &[MotionEvent]) -> MotionEvents {
        let mut set = MotionEvents::new();
        for event in events {
            assert!(
                set.push(*event),
                "capacity {MOTION_EVENT_CAPACITY} must hold the fixture"
            );
        }
        set
    }

    #[test]
    fn a_full_set_refuses_silently_oversized_input() {
        let mut set = MotionEvents::new();
        for count in 0..MOTION_EVENT_CAPACITY {
            assert!(set.push(MotionEvent::Tap { count: count as u8 }));
        }
        assert_eq!(set.len(), MOTION_EVENT_CAPACITY);
        assert!(!set.push(MotionEvent::Shake), "a full set reports the drop");
    }

    #[test]
    fn families_keep_their_most_specific_member() {
        let mut events = list(&[
            MotionEvent::TiltEnter(TiltDir::Left),
            MotionEvent::Shake,
            MotionEvent::Moving,
            MotionEvent::TiltExit(TiltDir::Left),
        ]);
        events.keep_best_per_group();
    }

    #[test]
    fn a_family_inside_its_cooldown_is_suppressed() {
        let mut arbiter = MotionArbiter::new();
        let mut first = list(&[MotionEvent::Shake, MotionEvent::Moving]);
        arbiter.resolve(1_000, &mut first);
        assert_eq!(
            first.iter().collect::<Vec<_>>(),
            [MotionEvent::Shake, MotionEvent::Moving]
        );

        let mut repeat = list(&[MotionEvent::Shake]);
        arbiter.resolve(1_000 + MOTION_COOLDOWN_MS - 1, &mut repeat);
        assert!(
            repeat.is_empty(),
            "the same act cannot fire twice in one window"
        );

        let mut settled = list(&[MotionEvent::Shake]);
        arbiter.resolve(1_000 + MOTION_COOLDOWN_MS, &mut settled);
        assert_eq!(settled.iter().collect::<Vec<_>>(), [MotionEvent::Shake]);
    }

    #[test]
    fn cooldowns_are_tracked_per_family() {
        let mut arbiter = MotionArbiter::new();
        let mut first = list(&[MotionEvent::Shake]);
        arbiter.resolve(0, &mut first);

        let mut next = list(&[MotionEvent::Shake, MotionEvent::Landscape]);
        arbiter.resolve(10, &mut next);
        assert_eq!(
            next.iter().collect::<Vec<_>>(),
            [MotionEvent::Landscape],
            "a spent action cooldown must not silence the pose family"
        );
    }

    #[test]
    fn capabilities_compose_and_answer_membership() {
        let caps = MotionCapabilities::TELEMETRY.union(MotionCapabilities::TAP);
        assert!(caps.contains(MotionCapabilities::TELEMETRY));
        assert!(caps.contains(MotionCapabilities::TAP));
        assert!(!caps.contains(MotionCapabilities::SHAKE));
        assert!(MotionCapabilities::EMPTY.union(caps) == caps);
    }
}

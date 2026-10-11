//! Demand: what the product wants of each data plane, and when.
//!
//! A claim is a *temporal contract*: whether a plane runs ([`Mode`]) and how often
//! ([`PlaneDemand::period_ms`]). [`Sensitivity`] asks how stale a reading may be, and this board
//! cannot act on it: batching means "sample on schedule, report later", which needs a hardware
//! FIFO to hold the event meanwhile, and the IMU here has none — so a late report can only be
//! bought by sampling less often, which is `period_ms` said out loud.
//!
//! How a plane is wired to meet that — its registers, its transport, its on-chip engines —
//! is an acquisition *configuration* ([`crate::config`]), chosen by the driver and never by
//! a page. Keeping the two apart is what stops "low fidelity at high cost" from being
//! expressible: a page cannot ask for a worse picture to save power, because a page does not
//! name a configuration at all.
//!
//! A demand is a map keyed by [`PlaneKey`], and that is the whole reason the machinery below
//! never names a device: `plane`, `without`, `aggregate` and [`Override::apply`] all work by
//! lookup, so a board adding a sensor declares one more key and touches none of them.

use alloc::collections::BTreeMap;

/// What a plane is asked to produce, in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Mode {
    /// Not produced.
    #[default]
    Off,
    /// Produced when it changes.
    OnChange,
    /// Produced on a clock.
    Continuous,
}

impl Mode {
    pub const fn is_collecting(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// The cadence a thumbnail request asks for, and the point the camera's cost row keys on: at
/// or slower than this the consumer's repaint gate makes it a thumbnail, faster a preview.
pub const CAMERA_THUMBNAIL_PERIOD_MS: u32 = 200;

/// How wrong a stale reading may be before it matters. Moves no cadence here; it breaks
/// arbitration ties and names the claim in `[DEMAND]`/`[DISPLAY]`.
///
/// Never assign these to the interaction path — a stale button reading is a dead product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sensitivity {
    /// Something on screen reads it and a stale value is a visibly wrong UI.
    Primary,
    /// Worth having; a slightly stale value reads as history.
    Secondary,
    /// Collected for later. Nothing on screen reads it.
    Background,
}

/// What one plane is asked to produce. `period_ms` is the requested cadence and is a floor
/// like any other: the board's own is clamped on top, so a claim can only ever ask for a
/// *slower* stream than the wiring gives.
///
/// A plane missing from the map reads as `off` rather than as an error: a plane nobody
/// declared is a plane nobody can poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneDemand {
    pub mode: Mode,
    /// The requested cadence in milliseconds; `0` means the board's own rate.
    pub period_ms: u32,
    pub class: Sensitivity,
}

impl PlaneDemand {
    pub const fn new(mode: Mode, period_ms: u32, class: Sensitivity) -> Self {
        Self {
            mode,
            period_ms,
            class,
        }
    }

    /// Nothing asked of the plane, and no reading anyone is waiting for.
    pub const fn off() -> Self {
        Self {
            mode: Mode::Off,
            period_ms: 0,
            class: Sensitivity::Background,
        }
    }

    /// Narrow this claim to a mode and period, keeping the class: the class is a property of
    /// the claim rather than of the spelling, so an override changes what is asked and not
    /// who is asking.
    pub const fn with(self, mode: Mode, period_ms: u32) -> Self {
        Self {
            mode,
            period_ms,
            class: self.class,
        }
    }

    /// Two claims on one component: the more demanding wins, and the class follows it so the
    /// latency cannot be widened past the demand that justifies it.
    ///
    /// More demanding is the higher [`Mode`], then the shorter period, then the fresher
    /// class; a claim on `Auto` (`period_ms = 0`) outranks any specific slower one.
    pub fn max(self, other: Self) -> Self {
        if other.demands_more_than(self) {
            other
        } else {
            self
        }
    }

    fn demands_more_than(self, other: Self) -> bool {
        (
            self.mode,
            core::cmp::Reverse(self.period_ms),
            core::cmp::Reverse(self.class),
        ) > (
            other.mode,
            core::cmp::Reverse(other.period_ms),
            core::cmp::Reverse(other.class),
        )
    }
}

/// One device type, independent of how many of them are wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DemandField {
    /// The interaction path, never gated: a scenario is chosen by touching or pressing
    /// something, so gating this would remove the only way to ungate it.
    Interaction,
    Motion,
    Audio,
    Camera,
    Display,
}

/// One plane: a device type plus which of them is meant, so two of a kind can be demanded
/// apart.
///
/// `instance` is 0 for every plane a board wires once. It exists because two sensors of one
/// type are two planes the product can name separately, and a key that cannot say *which*
/// would have to either collapse them into one claim or give up per-instance demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlaneKey {
    field: DemandField,
    instance: u8,
}

impl PlaneKey {
    pub const fn of(field: DemandField) -> Self {
        Self { field, instance: 0 }
    }

    /// The same device type, a specific unit of it.
    pub const fn instance(field: DemandField, instance: u8) -> Self {
        Self { field, instance }
    }

    pub const fn field(self) -> DemandField {
        self.field
    }

    pub const fn instance_index(self) -> u8 {
        self.instance
    }

    pub const MOTION: Self = Self::of(DemandField::Motion);
    pub const AUDIO: Self = Self::of(DemandField::Audio);
    pub const CAMERA: Self = Self::of(DemandField::Camera);
    pub const DISPLAY: Self = Self::of(DemandField::Display);
    /// The interaction path answers as always-continuous: nothing may gate it.
    pub const INTERACTION: Self = Self::of(DemandField::Interaction);
}

/// What the device is asked for, plane by plane.
///
/// A map, not a struct with one field per device, so that the machinery here — `plane`,
/// `without`, `aggregate`, [`Override::apply`] — is generic over the key. A board that wires
/// a sensor nobody else has adds a key and changes none of them; only the board's own list
/// and the page that claims it grow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveDemand {
    planes: BTreeMap<PlaneKey, PlaneDemand>,
}

impl Default for EffectiveDemand {
    /// The panel lit and nothing else asked for, rather than [`Self::none`].
    ///
    /// An unset demand is not a dark device: it is a device nobody has asked a question of
    /// yet.
    fn default() -> Self {
        let mut planes = BTreeMap::new();
        planes.insert(
            PlaneKey::DISPLAY,
            PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
        );
        Self { planes }
    }
}

impl Default for PlaneDemand {
    fn default() -> Self {
        Self::off()
    }
}

impl core::fmt::Display for PlaneDemand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.period_ms == 0 {
            write!(f, "{:?}@{:?}", self.mode, self.class)
        } else {
            write!(f, "{:?}/{}ms@{:?}", self.mode, self.period_ms, self.class)
        }
    }
}

impl core::fmt::Display for EffectiveDemand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, (key, demand)) in self.planes.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{} {}", key, demand)?;
        }
        Ok(())
    }
}

impl core::fmt::Display for PlaneKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Lowercase, because these land in a log line next to `[DEMAND]` and `[DISPLAY]`
        // rather than in a type position.
        let name = match self.field {
            DemandField::Interaction => "input",
            DemandField::Motion => "motion",
            DemandField::Audio => "audio",
            DemandField::Camera => "camera",
            DemandField::Display => "display",
        };
        match self.instance {
            0 => f.write_str(name),
            instance => write!(f, "{name}{instance}"),
        }
    }
}

/// A sparse patch over a demand: what one build or configuration page asks beyond what the
/// page would. Absent keys claim nothing, which is what lets one page override one plane and
/// leave the rest of the product's demand standing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Override {
    named: BTreeMap<PlaneKey, (Mode, u32)>,
}

impl Override {
    pub fn none() -> Self {
        Self {
            named: BTreeMap::new(),
        }
    }

    /// What this override says about one plane, or `None` if it says nothing.
    pub fn named_mode(&self, key: PlaneKey) -> Option<(Mode, u32)> {
        self.named.get(&key).copied()
    }

    pub fn set(&mut self, key: PlaneKey, mode: Mode, period_ms: u32) {
        self.named.insert(key, (mode, period_ms));
    }

    /// Applies the modes this override names, in place, keeping each claim's class.
    pub fn apply(self, demand: &mut EffectiveDemand) {
        for (key, (mode, period_ms)) in self.named {
            let current = demand.plane(key);
            demand.set(key, current.with(mode, period_ms));
        }
    }

    /// Whether anything was named, so a caller can say "no override" instead of republishing
    /// an unchanged demand.
    pub fn names_anything(&self) -> bool {
        !self.named.is_empty()
    }
}

/// The plane names an override spells, as `[probe][name]`.
const FIELD_NAMES: &[(&str, DemandField)] = &[
    ("input", DemandField::Interaction),
    ("motion", DemandField::Motion),
    ("audio", DemandField::Audio),
    ("camera", DemandField::Camera),
    ("display", DemandField::Display),
];

/// What one mode name asks of one plane, or `None` for a name this plane does not answer to.
/// `on` and `continuous` are the same request; `thumb`(nail) is the camera's slow cadence.
///
/// A name may carry its own period as `mode:ms` (`motion=on:30000`), which is how a build
/// asks for a slow stream now that no policy table stands between the request and the cadence.
fn mode_for(field: DemandField, name: &str) -> Option<(Mode, u32)> {
    let (name, asked_period_ms) = match name.split_once(':') {
        Some((mode, period)) => (mode, Some(period.parse().ok()?)),
        None => (name, None),
    };
    if field == DemandField::Interaction {
        return None;
    }
    let (mode, default_period_ms) = match (field, name) {
        (_, "off") => (Mode::Off, 0),
        (_, "on-change") | (_, "on_change") => (Mode::OnChange, 0),
        (_, "on") | (_, "continuous") => (Mode::Continuous, 0),
        (DemandField::Camera, "thumb") | (DemandField::Camera, "thumbnail") => {
            (Mode::Continuous, CAMERA_THUMBNAIL_PERIOD_MS)
        }
        _ => return None,
    };
    Some((mode, asked_period_ms.unwrap_or(default_period_ms)))
}

/// Parses one `field=mode` term, or `None` if it is not one. Every failure yields `None`
/// rather than an error: a build-time mistake should produce firmware that says what it did
/// not understand, not firmware that fails to link.
fn parse_term(term: &str) -> Option<(PlaneKey, (Mode, u32))> {
    let (field_name, mode_name) = term.split_once('=')?;
    let wanted = mode_name.trim().to_ascii_lowercase();
    let field = FIELD_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == field_name.trim().to_ascii_lowercase())
        .map(|(_, field)| *field)?;
    let mode = mode_for(field, wanted.as_str())?;
    Some((PlaneKey::of(field), mode))
}

/// Parses a comma-separated list of `field=mode` terms. Comma-separated rather than a
/// repeated variable, which the shell would collapse to its last value.
pub fn parse_override(text: &str) -> Override {
    let mut combined = Override::none();
    for term in text.split(',') {
        if let Some((key, (mode, period))) = parse_term(term) {
            // Two terms naming the same plane: the later one wins.
            combined.set(key, mode, period);
        }
    }
    combined
}

/// What one polled source should do, given the demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollPlan {
    pub active: bool,
    /// Its poll cadence, never below the board's.
    pub cadence_ms: u64,
}

/// Derives one source's plan from the effective demand.
///
/// The interaction path gets no batching at all: a stale button reading is a device that
/// cannot be steered off the page it is on. For everything else the cadence only ever moves
/// **outward** — the board's cadence is a floor the source's own debounce windows are
/// measured against, and a claim's requested period is a floor it cannot shorten.
///
/// This is the demand's half of [`InputLimiter`], which is the whole: what a source may not
/// be polled less often for is the source's own knowledge, and it only reaches a schedule
/// here.
pub fn poll_plan_of(key: PlaneKey, demand: &EffectiveDemand, board_cadence_ms: u64) -> PollPlan {
    if key.field() == DemandField::Interaction {
        return PollPlan {
            active: true,
            cadence_ms: board_cadence_ms,
        };
    }
    let plane = demand.plane(key);
    if !plane.mode.is_collecting() {
        return PollPlan {
            active: false,
            cadence_ms: board_cadence_ms,
        };
    }
    PollPlan {
        active: true,
        cadence_ms: board_cadence_ms.max(u64::from(plane.period_ms)),
    }
}

/// How far the controller is allowed to stop asking.
///
/// The limiter is self-restraint and never fails: every answer it gives is a schedule the
/// loop can actually run. What the *parts* cannot do is a different question, and it is
/// answered where the part is — a device limitation travels as [`crate::config::Unreachable`]
/// through the configurator rather than being folded into a cadence here.
pub struct InputLimiter;

impl InputLimiter {
    /// The schedule for one source: the demand's floor, the board's own, and the rate past
    /// which this particular source would stop working.
    ///
    /// The three are ordered rather than combined, because which of them is binding is the
    /// answer. A source that cannot go slower folds back to the board's cadence and gives up
    /// the stretch the demand asked for — and the schedule reports that value, so a reader
    /// sees the rate in force rather than the one requested.
    ///
    /// `poll_plan_of` is what the demand alone implies; the fold-back is what the source
    /// allows. Both are needed, and neither is the limiter without the other.
    pub fn plan(
        key: PlaneKey,
        demand: &EffectiveDemand,
        board_cadence_ms: u64,
        accepts_slower_poll: bool,
    ) -> PollPlan {
        let mut plan = poll_plan_of(key, demand, board_cadence_ms);
        if plan.cadence_ms > board_cadence_ms && !accepts_slower_poll {
            plan.cadence_ms = board_cadence_ms;
        }
        plan
    }
}

/// The one executor number worth bounding, and why it is not a percentage.
///
/// A percentage would constrain what share of a second a *preemptive* scheduler spends
/// running tasks. This executor is fair per poll, so no task is starved by volume; what it
/// cannot do is interrupt, so one non-yielding poll holds it for that poll's whole worst-case
/// execution time. That is a per-operation bound, and the thing to bound is the **tick** —
/// every source polled at a tick runs inside one non-awaiting `for`.
///
/// The bound is the measured worst-case tick plus one poll period; the measurements are in
/// [`architecture`](../../../../../docs/content/development/iot/architecture.md).
pub const MAX_SINGLE_HOLD_MS: u32 = 30;

/// Every plane this crate names. A board declares a shorter list of what it actually wires;
/// this is what a caller asks about when it means the product rather than one board.
pub const ALL_PLANES: &[PlaneKey] = &[
    PlaneKey::MOTION,
    PlaneKey::AUDIO,
    PlaneKey::CAMERA,
    PlaneKey::DISPLAY,
];

impl EffectiveDemand {
    /// Nothing demanded. What a device holds before any scenario claims it.
    pub const fn none() -> Self {
        Self {
            planes: BTreeMap::new(),
        }
    }

    /// What one plane is asked for. The interaction path is answered as always-continuous:
    /// nothing may gate it, and this is what tells a poller so.
    pub fn plane(&self, key: PlaneKey) -> PlaneDemand {
        if key.field() == DemandField::Interaction {
            return PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary);
        }
        self.planes.get(&key).copied().unwrap_or_default()
    }

    /// Records what one plane is asked for, replacing whatever it was asked before.
    pub fn set(&mut self, key: PlaneKey, demand: PlaneDemand) {
        self.planes.insert(key, demand);
    }

    /// Every plane this demand says something about, in key order.
    pub fn keys(&self) -> impl Iterator<Item = &PlaneKey> {
        self.planes.keys()
    }

    /// The same demand with one plane dropped, for a part that cannot meet what was asked of
    /// it.
    ///
    /// Dropping rather than substituting: a device that cannot produce change events should
    /// stop claiming it will, not carry on polling and delivering something else under the
    /// name that was asked for. The interaction path is untouched — nothing a demand says may
    /// switch it off, and that holds for a refusal as much as for a page.
    pub fn without(&self, key: PlaneKey) -> Self {
        let mut demand = self.clone();
        if key.field() != DemandField::Interaction {
            demand.planes.insert(key, PlaneDemand::off());
        }
        demand
    }

    /// The arbitration rule: per plane, the most demanding claim wins.
    ///
    /// Not a priority order. Android's sensor stack documents both halves of why: a consumer
    /// asking for zero latency puts every other consumer into continuous mode, and one asking
    /// for a slow rate still receives events at the fastest requester's rate.
    pub fn aggregate(claims: &[Self]) -> Self {
        let mut out = Self::none();
        for (key, demand) in claims.iter().flat_map(|claim| claim.planes.iter()) {
            let merged = out.plane(*key).max(*demand);
            out.planes.insert(*key, merged);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_planes() -> EffectiveDemand {
        {
            let mut demand = EffectiveDemand::none();
            demand.set(
                PlaneKey::MOTION,
                PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Secondary),
            );
            demand.set(
                PlaneKey::AUDIO,
                PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Secondary),
            );
            demand
        }
    }

    #[test]
    fn off_is_the_only_mode_that_stops_a_plane() {
        assert!(!Mode::Off.is_collecting());
        assert!(Mode::OnChange.is_collecting());
        assert!(Mode::Continuous.is_collecting());
    }

    #[test]
    fn a_claim_wins_on_mode_then_period_in_either_argument_order() {
        let off = PlaneDemand::new(Mode::Off, 0, Sensitivity::Primary);
        let continuous = PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Background);
        assert_eq!(off.max(continuous), continuous);
        assert_eq!(continuous.max(off), continuous);

        let auto = PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Background);
        let slow = PlaneDemand::new(Mode::Continuous, 500, Sensitivity::Primary);
        assert_eq!(
            auto.max(slow),
            auto,
            "a faster request outranks a slower one"
        );
        assert_eq!(slow.max(auto), auto);
    }

    #[test]
    fn aggregation_is_per_component_not_whole_vector() {
        let camera_only = {
            let mut demand = EffectiveDemand::none();
            demand.set(
                PlaneKey::CAMERA,
                PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
            );
            demand
        };
        let motion_only = {
            let mut demand = EffectiveDemand::none();
            demand.set(
                PlaneKey::MOTION,
                PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Secondary),
            );
            demand
        };

        let merged = EffectiveDemand::aggregate(&[camera_only, motion_only]);

        assert_eq!(merged.plane(PlaneKey::CAMERA).mode, Mode::Continuous);
        assert_eq!(merged.plane(PlaneKey::MOTION).mode, Mode::Continuous);
        assert_eq!(merged.plane(PlaneKey::DISPLAY).mode, Mode::Off);
        assert_eq!(merged.plane(PlaneKey::AUDIO).mode, Mode::Off);
    }

    #[test]
    fn a_fast_consumer_lifts_every_other_consumer_of_that_component() {
        // The winning class is the one attached to the winning claim, not the slowest.
        let eager = PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary);
        let patient = PlaneDemand::new(Mode::OnChange, 0, Sensitivity::Background);

        let winner = patient.max(eager);

        assert_eq!(winner.mode, Mode::Continuous);
        assert_eq!(winner.class, Sensitivity::Primary);
    }

    #[test]
    fn the_interaction_path_is_never_gated() {
        let off = EffectiveDemand::none();
        let plan = poll_plan_of(PlaneKey::INTERACTION, &off, 10);
        assert!(
            plan.active,
            "a demand of nothing must still leave the input readable"
        );
        assert_eq!(plan.cadence_ms, 10);
    }

    #[test]
    fn a_plane_demanded_at_off_is_not_polled_at_all() {
        let off = EffectiveDemand::none();
        for key in [PlaneKey::MOTION, PlaneKey::AUDIO] {
            let plan = poll_plan_of(key, &off, 20);
            assert!(!plan.active, "{key} demanded at Off must not be polled");
            assert_eq!(plan.cadence_ms, 20);
        }
    }

    #[test]
    fn a_requested_period_is_a_floor_the_cadence_cannot_shorten() {
        let mut demand = live_planes();
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::new(Mode::Continuous, 500, Sensitivity::Secondary),
        );
        let plan = poll_plan_of(PlaneKey::AUDIO, &demand, 20);
        assert_eq!(plan.cadence_ms, 500, "a slow request must not be sped up");
    }

    #[test]
    fn a_source_that_cannot_go_slower_folds_back_to_the_board_cadence() {
        // The fold-back used to live inline in the apply loop, which meant the limiter had no
        // single place a reader could check it. It is the same rule either way: a stretch the
        // source cannot survive is given up rather than scheduled.
        let mut demand = live_planes();
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::new(Mode::Continuous, 2_000, Sensitivity::Secondary),
        );

        assert_eq!(
            InputLimiter::plan(PlaneKey::AUDIO, &demand, 20, false).cadence_ms,
            20,
            "a source whose clock dies when polled slower keeps the board's own cadence"
        );
        assert_eq!(
            InputLimiter::plan(PlaneKey::AUDIO, &demand, 20, true).cadence_ms,
            2_000,
            "one that only goes stale takes the stretch the demand asked for"
        );
    }

    #[test]
    fn the_interaction_path_takes_no_plan_at_all_even_from_a_source_that_cannot_go_slower() {
        let off = EffectiveDemand::none();
        let plan = InputLimiter::plan(PlaneKey::INTERACTION, &off, 10, false);
        assert!(
            plan.active,
            "a stale button reading is a device that cannot be steered off its page"
        );
        assert_eq!(plan.cadence_ms, 10);
    }

    #[test]
    fn a_class_moves_no_cadence_because_there_is_no_fifo_to_hold_an_event() {
        let mut demand = live_planes();
        demand.set(
            PlaneKey::MOTION,
            PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
        );
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Background),
        );

        let eager = poll_plan_of(PlaneKey::MOTION, &demand, 20);
        let patient = poll_plan_of(PlaneKey::AUDIO, &demand, 20);
        assert_eq!(eager.cadence_ms, patient.cadence_ms);
    }

    #[test]
    fn aggregating_nothing_yields_nothing_demanded() {
        assert_eq!(EffectiveDemand::aggregate(&[]), EffectiveDemand::none());
        assert_eq!(
            EffectiveDemand::none().plane(PlaneKey::DISPLAY).mode,
            Mode::Off
        );
        assert_eq!(
            EffectiveDemand::default().plane(PlaneKey::DISPLAY).mode,
            Mode::Continuous,
            "an unset demand is not a dark device"
        );
    }

    #[test]
    fn a_cost_report_can_name_every_plane_and_its_class() {
        use alloc::format;
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::MOTION,
            PlaneDemand::new(Mode::OnChange, 0, Sensitivity::Secondary),
        );
        demand.set(PlaneKey::AUDIO, PlaneDemand::off());
        demand.set(
            PlaneKey::CAMERA,
            PlaneDemand::new(
                Mode::Continuous,
                CAMERA_THUMBNAIL_PERIOD_MS,
                Sensitivity::Primary,
            ),
        );
        demand.set(
            PlaneKey::DISPLAY,
            PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
        );

        assert_eq!(
            format!("{demand}"),
            "motion OnChange@Secondary audio Off@Background camera Continuous/200ms@Primary \
             display Continuous@Primary"
        );
    }

    #[test]
    fn an_override_names_a_plane_and_a_mode_and_leaves_the_rest_alone() {
        let parsed = parse_override("audio=off");
        assert!(parsed.names_anything());
        assert_eq!(parsed.named_mode(PlaneKey::AUDIO), Some((Mode::Off, 0)));
        assert_eq!(parsed.named_mode(PlaneKey::MOTION), None);

        let mut demand = live_planes();
        let before = demand.plane(PlaneKey::MOTION);
        parsed.apply(&mut demand);

        assert_eq!(demand.plane(PlaneKey::AUDIO).mode, Mode::Off);
        assert_eq!(demand.plane(PlaneKey::MOTION), before);
        assert_eq!(
            demand.plane(PlaneKey::AUDIO).class,
            Sensitivity::Secondary,
            "narrowing a mode must not re-price the plane under another budget"
        );
    }

    #[test]
    fn several_terms_can_be_named_at_once() {
        let parsed = parse_override("motion=on-change,display=off");
        assert_eq!(
            parsed.named_mode(PlaneKey::MOTION),
            Some((Mode::OnChange, 0))
        );
        assert_eq!(parsed.named_mode(PlaneKey::DISPLAY), Some((Mode::Off, 0)));
        assert_eq!(parsed.named_mode(PlaneKey::AUDIO), None);
    }

    #[test]
    fn a_camera_override_can_ask_for_a_thumbnail() {
        let parsed = parse_override("camera=thumb");
        assert_eq!(
            parsed.named_mode(PlaneKey::CAMERA),
            Some((Mode::Continuous, CAMERA_THUMBNAIL_PERIOD_MS))
        );
    }

    #[test]
    fn a_build_can_name_its_own_cadence_instead_of_asking_a_policy_for_one() {
        // What a removed policy table used to do to a `Secondary` claim: stretch it to
        // 30 s. The build now says so outright, and it lands on the same cadence.
        let parsed = parse_override("motion=on:30000,audio=off");
        assert_eq!(
            parsed.named_mode(PlaneKey::MOTION),
            Some((Mode::Continuous, 30_000))
        );
        assert_eq!(parsed.named_mode(PlaneKey::AUDIO), Some((Mode::Off, 0)));

        let mut demand = live_planes();
        parsed.apply(&mut demand);
        assert_eq!(
            poll_plan_of(PlaneKey::MOTION, &demand, 20).cadence_ms,
            30_000
        );
    }

    #[test]
    fn a_named_cadence_that_is_not_a_number_names_nothing() {
        for text in [
            "motion=on:fast",
            "motion=on:",
            "motion=:-5",
            "input=on:30000",
        ] {
            assert!(
                !parse_override(text).names_anything(),
                "{text:?} must not parse into a demand"
            );
        }
    }

    #[test]
    fn a_later_term_wins_for_the_same_plane() {
        let parsed = parse_override("display=off,display=on");
        assert_eq!(
            parsed.named_mode(PlaneKey::DISPLAY),
            Some((Mode::Continuous, 0))
        );
    }

    #[test]
    fn a_plane_nobody_named_reads_as_off_rather_than_as_a_missing_key() {
        // Absent and Off have to mean the same thing, because the board's plane list and the
        // page's claim are written independently and a reader cannot tell which one left a
        // key out.
        let empty = EffectiveDemand::none();
        for key in ALL_PLANES.iter().copied() {
            assert_eq!(empty.plane(key), PlaneDemand::off(), "{key}");
            assert_eq!(empty.plane(key).mode, Mode::Off);
        }
    }

    #[test]
    fn an_override_leaves_a_plane_it_says_nothing_about_alone() {
        // The whole point of a sparse patch: naming one plane must not disturb the rest of
        // what the product asked for.
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::new(Mode::Continuous, 500, Sensitivity::Secondary),
        );
        demand.set(
            PlaneKey::DISPLAY,
            PlaneDemand::new(Mode::Continuous, 0, Sensitivity::Primary),
        );

        let mut override_ = Override::none();
        override_.set(PlaneKey::AUDIO, Mode::Off, 0);
        override_.apply(&mut demand);

        assert_eq!(demand.plane(PlaneKey::AUDIO).mode, Mode::Off);
        assert_eq!(
            demand.plane(PlaneKey::DISPLAY).mode,
            Mode::Continuous,
            "naming the audio plane must not have repainted the panel's demand"
        );
    }

    #[test]
    fn a_term_this_firmware_does_not_recognise_costs_that_term_and_not_the_rest() {
        for text in [
            "audoi=off", // misspelled plane
            "audio=of",  // misspelled mode
            "input=off", // a plane no demand may switch off
            "audio",     // no `=`
            "=off",      // no plane
            "",          // nothing at all
        ] {
            assert!(
                !parse_override(text).names_anything(),
                "{text:?} names nothing"
            );
        }
        let partial = parse_override("audio=off, motion=on, nonsense");
        assert_eq!(partial.named_mode(PlaneKey::AUDIO), Some((Mode::Off, 0)));
        assert_eq!(
            partial.named_mode(PlaneKey::MOTION),
            Some((Mode::Continuous, 0))
        );
    }

    #[test]
    fn a_separator_at_either_end_costs_nothing() {
        for text in [",audio=off", "audio=off,", ", audio=off ,", " audio = off "] {
            assert_eq!(
                parse_override(text).named_mode(PlaneKey::AUDIO),
                Some((Mode::Off, 0)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_interaction_path_cannot_be_named_because_nothing_may_switch_it_off() {
        assert!(!parse_override("input=off").names_anything());
    }

    #[test]
    fn an_override_on_the_camera_page_can_reach_off() {
        let mut demand = crate::state::DisplayPage::Camera.demand(ALL_PLANES);
        assert_eq!(demand.plane(PlaneKey::CAMERA).mode, Mode::Continuous);

        parse_override("camera=off").apply(&mut demand);

        assert_eq!(demand.plane(PlaneKey::CAMERA).mode, Mode::Off);
    }

    /// The strings the shipped build tasks pass as `IOT_DEMAND`. An unrecognised term costs
    /// only that term, so a preset that drifts out of the vocabulary still builds and still
    /// flashes — it just quietly stops being the preset it claims to be.
    #[test]
    fn every_shipped_preset_spells_modes_this_firmware_has() {
        // `apps/iot/moon.yml`: build-s3-thrift
        let thrift = parse_override("audio=off,display=on-change");
        assert_eq!(thrift.named_mode(PlaneKey::AUDIO), Some((Mode::Off, 0)));
        assert_eq!(
            thrift.named_mode(PlaneKey::DISPLAY),
            Some((Mode::OnChange, 0))
        );
    }
}

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
//! never names a device: `plane`, `without` and [`Override::apply`] all work by lookup, so a
//! board adding a sensor declares one more key and touches none of them.

use alloc::collections::BTreeMap;

/// How fast the chip has to run for one piece of work to meet its deadline.
///
/// Chip-independent on purpose: `CpuClock` lives in esp-hal and this crate does
/// not depend on it, so `iot-chip-esp` is what turns these into clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Speed {
    Slow,
    Normal,
    /// The default, because every deadline this board has measured needs the
    /// fastest clock it has. That is a measurement, not a preference: see
    /// [`SpeedTable::speed_for`].
    #[default]
    Fast,
}

/// What a page asks for, before anything is arbitrated.
///
/// `Auto` is the only variant that consults [`SpeedTable::speed_for`]. The other three are
/// the page's decision and are taken as given, so a page that says
/// [`Speed::Slow`] gets `Slow` even where a deadline says otherwise — and
/// [`speed_shortfall`] names what that costs, so the breakage points at the page
/// that asked for it rather than at nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeedRequest {
    #[default]
    Auto,
    /// As named.
    Fixed(Speed),
}

/// What one device type needs while it is collecting, in this build's terms.
///
/// The numbers behind this live in the chip layer, because they are worst cases
/// measured on one part and mean nothing on another: the same deadline on a
/// faster core is a shorter hold. What stays here is the shape — which plane
/// needs which — so a board adding a device declares one row and the chip
/// decides what it costs.
pub trait SpeedTable {
    /// What one plane needs while it is collecting.
    fn speed_for(field: DemandField) -> Speed;
}

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
/// `without`, [`Override::apply`] — is generic over the key. A board that wires
/// a sensor nobody else has adds a key and changes none of them; only the board's own list
/// and the page that claims it grow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveDemand {
    planes: BTreeMap<PlaneKey, PlaneDemand>,
    speed: SpeedRequest,
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
        Self {
            planes,
            speed: SpeedRequest::Auto,
        }
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
    speed: SpeedRequest,
}

impl Override {
    pub fn none() -> Self {
        Self {
            named: BTreeMap::new(),
            speed: SpeedRequest::Auto,
        }
    }

    /// What this override asks for, or [`SpeedRequest::Auto`] if it says nothing.
    pub fn speed(&self) -> SpeedRequest {
        self.speed
    }

    pub fn with_speed(mut self, speed: SpeedRequest) -> Self {
        self.speed = speed;
        self
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
        demand.speed = self.speed;
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

fn speed_for_name(name: &str) -> SpeedRequest {
    match name.trim().to_ascii_lowercase().as_str() {
        "slow" => SpeedRequest::Fixed(Speed::Slow),
        "normal" => SpeedRequest::Fixed(Speed::Normal),
        "fast" => SpeedRequest::Fixed(Speed::Fast),
        // Anything unrecognised, `auto` included: a speed nobody asked for is not
        // a speed the product should invent one for.
        _ => SpeedRequest::Auto,
    }
}

/// Parses a comma-separated list of `field=mode` terms, plus an optional
/// `speed=` term naming the clock the whole demand wants. Comma-separated rather than a
/// repeated variable, which the shell would collapse to its last value.
pub fn parse_override(text: &str) -> Override {
    let mut combined = Override::none();
    for term in text.split(',') {
        // `speed=` names no plane, so it is read before the plane parser turns
        // a term with an unknown field into nothing.
        if let Some(wanted) = term.strip_prefix("speed=") {
            combined = combined.with_speed(speed_for_name(wanted));
            continue;
        }
        if let Some((key, (mode, period))) = parse_term(term) {
            // Two terms naming the same plane: the later one wins.
            combined.set(key, mode, period);
        }
    }
    combined
}

/// The clock the chip should run at for this demand.
///
/// A page's explicit choice is taken as given — that is the whole point of
/// letting it state one. Only [`SpeedRequest::Auto`] reads [`SpeedTable::speed_for`], and it
/// reads the collecting planes and takes the highest.
///
/// Downclocking stretches durations and changes nothing else: the peripheral bus
/// keeps its own clock and the system timer runs from XTAL, so the cost of a
/// wrong answer here is a missed deadline and not a broken device. That is why
/// this does not second-guess an explicit request, and why
/// [`speed_shortfall`] exists to report instead. Which preset a [`Speed`] is on
/// this part is the chip layer's, behind [`SpeedTable`].
pub fn resolve_speed<T: SpeedTable>(demand: &EffectiveDemand) -> Speed {
    match demand.speed {
        SpeedRequest::Fixed(speed) => speed,
        SpeedRequest::Auto => demand
            .keys()
            .filter(|key| demand.plane(**key).mode.is_collecting())
            .map(|key| T::speed_for(key.field()))
            .max()
            .unwrap_or_default(),
    }
}

/// The collecting plane whose requirement `resolve_speed` is currently
/// undercut, and what it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortfall {
    /// The plane being asked to collect below its own speed.
    pub plane: PlaneKey,
    /// What that plane needs, so a report can name the requirement beside the
    /// plane rather than making the reader look it up.
    pub needed: Speed,
}

/// The collecting plane whose requirement `resolve_speed` is currently
/// undercut, if a page asked for something below it.
///
/// Named rather than counted so the report can point at the plane rather than at
/// a number with no owner.
pub fn speed_shortfall<T: SpeedTable>(demand: &EffectiveDemand) -> Option<Shortfall> {
    let SpeedRequest::Fixed(asked) = demand.speed else {
        return None;
    };
    demand
        .keys()
        .filter(|key| demand.plane(**key).mode.is_collecting())
        .find_map(|key| {
            let needed = T::speed_for(key.field());
            (needed > asked).then_some(Shortfall {
                plane: *key,
                needed,
            })
        })
}

/// The part a resolved speed is applied to, and what that part achieves.
///
/// The table comes from the sink rather than being named beside it because the two
/// are the same measurement: the rows say how long a plane holds **on that part**,
/// so pairing one part's rows with another's setter would compute a deadline for a
/// chip that is not running. Making it an associated type makes that pairing a
/// compile error instead of a runtime surprise.
pub trait ClockSink {
    /// What each plane needs on the part this sink drives.
    type Table: SpeedTable;

    /// Applies `speed` and returns the speed the part is actually running.
    ///
    /// Answering with what was reached rather than what was asked is what keeps a
    /// part with fewer presets than the vocabulary has — the C6 has no third — from
    /// being believed to have reached the top one.
    fn set_speed(&mut self, speed: Speed) -> Speed;
}

/// What one call to [`ClockGovernor::apply`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// What the demand resolved to.
    pub asked: Speed,
    /// What the part is running, which is below [`asked`](Self::asked) on a part
    /// with fewer presets than the vocabulary has.
    pub running: Speed,
    /// The plane `asked` is undercut, when a page named the speed itself.
    pub undercuts: Option<Shortfall>,
}

/// Holds a part at whatever the current demand resolves to, moving only when the
/// answer moves.
///
/// Named rather than folded into a free function because the answer has to be
/// remembered somewhere, and a `static` would make two devices in one process
/// answer for each other.
///
/// Nothing here names a clock, so what the three speeds are on any given part stays
/// in the chip layer.
pub struct ClockGovernor {
    asked: Option<Speed>,
}

impl Default for ClockGovernor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockGovernor {
    pub const fn new() -> Self {
        Self { asked: None }
    }

    /// Puts `sink` at the speed this demand resolves to, and reports what it did.
    ///
    /// `None` when the answer has not moved since last time. The switch itself
    /// would be cheap every frame — a few microseconds against a 20 ms one — but
    /// the line that announces it is not, and a caller that reports on change
    /// should be able to say nothing while a page sits still.
    ///
    /// This returns what happened rather than logging it, leaving the output channel
    /// to the layer that owns one.
    pub fn apply<C: ClockSink>(
        &mut self,
        demand: &EffectiveDemand,
        sink: &mut C,
    ) -> Option<Applied> {
        let asked = resolve_speed::<C::Table>(demand);
        if self.asked == Some(asked) {
            return None;
        }
        self.asked = Some(asked);
        Some(Applied {
            asked,
            running: sink.set_speed(asked),
            undercuts: speed_shortfall::<C::Table>(demand),
        })
    }
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

/// Every plane this crate names, for tests that mean the product rather than one board.
///
/// A board declares the shorter list it actually wires; this walks all of them so a page
/// that forgets one is caught.
#[cfg(test)]
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
            speed: SpeedRequest::Auto,
        }
    }

    /// What this demand asks the clock for, before anything is arbitrated.
    pub const fn speed(&self) -> SpeedRequest {
        self.speed
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
}

#[cfg(test)]
mod tests {

    /// The S3's rows, so these tests pin the table that ships rather than a
    /// stand-in. The real one lives in `iot-chip-esp` and `iot-core` cannot see
    /// it, which is the point: these numbers belong to a chip, so the chip's copy
    /// is the one that matters and these only keep the *mechanism* honest.
    struct S3;

    impl SpeedTable for S3 {
        fn speed_for(field: DemandField) -> Speed {
            match field {
                DemandField::Interaction | DemandField::Motion => Speed::Slow,
                DemandField::Audio | DemandField::Camera | DemandField::Display => Speed::Fast,
            }
        }
    }

    /// Reaches whatever it is asked for, the way a part whose presets cover the
    /// whole vocabulary does.
    #[derive(Default)]
    struct Reaching {
        asked: Vec<Speed>,
    }

    impl ClockSink for Reaching {
        type Table = S3;

        fn set_speed(&mut self, speed: Speed) -> Speed {
            self.asked.push(speed);
            speed
        }
    }

    /// A part whose top preset sits below the top of the vocabulary — one with 80
    /// and 160 and no third.
    #[derive(Default)]
    struct Capped {
        asked: Vec<Speed>,
    }

    impl ClockSink for Capped {
        type Table = S3;

        fn set_speed(&mut self, speed: Speed) -> Speed {
            self.asked.push(speed);
            match speed {
                Speed::Fast => Speed::Normal,
                reached => reached,
            }
        }
    }

    use super::*;
    use alloc::vec::Vec;

    fn collecting(key: PlaneKey) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        demand.set(key, PlaneDemand::off().with(Mode::Continuous, 0));
        demand
    }

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
    fn an_unset_demand_is_not_a_dark_device() {
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

    #[test]
    fn auto_reads_the_collecting_planes_and_takes_the_highest() {
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::CAMERA,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        assert_eq!(resolve_speed::<S3>(&demand), Speed::Fast);

        demand.set(PlaneKey::CAMERA, PlaneDemand::off());
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::default(),
            "nothing collecting leaves the default, which is what a device that is not wired needs"
        );
    }

    #[test]
    fn an_explicit_request_is_taken_as_given_rather_than_arbitrated() {
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        demand.speed = SpeedRequest::Fixed(Speed::Slow);
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::Slow,
            "a page that named a speed gets it; the point of letting one is that it can be wrong"
        );
    }

    #[test]
    fn a_forced_speed_names_the_plane_it_is_undercutting() {
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        demand.set(PlaneKey::CAMERA, PlaneDemand::off());

        demand.speed = SpeedRequest::Fixed(Speed::Slow);
        assert_eq!(
            speed_shortfall::<S3>(&demand),
            Some(Shortfall {
                plane: PlaneKey::AUDIO,
                needed: Speed::Fast,
            }),
            "the report names the plane and what it needs, so neither has to be looked up"
        );

        demand.speed = SpeedRequest::Auto;
        assert_eq!(
            speed_shortfall::<S3>(&demand),
            None,
            "Auto is arbitrated, so nothing is being undercut by it"
        );
    }

    #[test]
    fn a_table_that_makes_a_choice_splits_the_fields_around_it() {
        // This pins that a table with more than one row is honoured per field, not
        // collapsed to one answer. The values themselves are the S3's and are
        // copied; whether they are still the shipped ones is `iot-chip-esp`'s to say.
        assert_eq!(S3::speed_for(DemandField::Motion), Speed::Slow);
        assert_eq!(S3::speed_for(DemandField::Interaction), Speed::Slow);
        assert_eq!(S3::speed_for(DemandField::Camera), Speed::Fast);
        assert_eq!(S3::speed_for(DemandField::Display), Speed::Fast);
        assert_eq!(S3::speed_for(DemandField::Audio), Speed::Fast);
    }

    #[test]
    fn auto_over_motion_alone_drops_below_fast_and_a_page_can_override_that() {
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::MOTION,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::Slow,
            "motion is the row that makes the table a choice rather than a constant"
        );

        demand.speed = SpeedRequest::Fixed(Speed::Fast);
        assert_eq!(resolve_speed::<S3>(&demand), Speed::Fast);
    }

    #[test]
    fn the_highest_of_several_collecting_planes_wins() {
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::MOTION,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::Fast,
            "one plane at the top of the table lifts every other, which is what makes \
             Auto a max rather than a first-match"
        );
    }

    #[test]
    fn a_speed_term_in_the_build_time_override_reaches_the_demand() {
        let mut demand = EffectiveDemand::none();
        parse_override("display=continuous,speed=slow").apply(&mut demand);
        assert_eq!(demand.speed(), SpeedRequest::Fixed(Speed::Slow));
        assert_eq!(
            parse_override("display=continuous").speed(),
            SpeedRequest::Auto
        );
        assert_eq!(parse_override("speed=nonsense").speed(), SpeedRequest::Auto);
    }

    #[test]
    fn an_override_carries_its_speed_onto_the_demand() {
        let mut demand = EffectiveDemand::none();
        assert_eq!(demand.speed(), SpeedRequest::Auto);
        Override::none()
            .with_speed(SpeedRequest::Fixed(Speed::Normal))
            .apply(&mut demand);
        assert_eq!(demand.speed(), SpeedRequest::Fixed(Speed::Normal));
    }

    #[test]
    fn what_comes_back_is_what_the_part_reached() {
        let mut sink = Reaching::default();
        let mut governor = ClockGovernor::new();
        assert_eq!(
            governor.apply(&collecting(PlaneKey::MOTION), &mut sink),
            Some(Applied {
                asked: Speed::Slow,
                running: Speed::Slow,
                undercuts: None,
            }),
            "a page collecting nothing but motion is under the line, so it sits slow"
        );
        assert_eq!(sink.asked, [Speed::Slow]);
    }

    #[test]
    fn an_answer_that_has_not_moved_is_not_asked_for_again() {
        let mut sink = Reaching::default();
        let mut governor = ClockGovernor::new();
        let demand = collecting(PlaneKey::MOTION);
        governor.apply(&demand, &mut sink);
        assert_eq!(
            governor.apply(&demand, &mut sink),
            None,
            "the answer has not moved, and a line per frame costs more than the switch \
             it would announce"
        );
        assert_eq!(sink.asked, [Speed::Slow], "asked once, not once a frame");
    }

    #[test]
    fn a_part_that_cannot_reach_the_top_of_the_vocabulary_says_so() {
        let mut sink = Capped::default();
        let mut governor = ClockGovernor::new();
        assert_eq!(
            governor.apply(&collecting(PlaneKey::AUDIO), &mut sink),
            Some(Applied {
                asked: Speed::Fast,
                running: Speed::Normal,
                undercuts: None,
            }),
            "a part that cannot reach the top has to report the gap, or the caller names \
             a clock it never got"
        );
        assert_eq!(sink.asked, [Speed::Fast], "asked for the top anyway");
    }

    #[test]
    fn a_page_that_named_its_own_speed_is_told_which_plane_it_is_undercutting() {
        let mut sink = Reaching::default();
        let mut governor = ClockGovernor::new();
        let mut demand = collecting(PlaneKey::MOTION);
        demand.set(
            PlaneKey::AUDIO,
            PlaneDemand::off().with(Mode::Continuous, 0),
        );
        governor.apply(&demand, &mut sink);
        demand.speed = SpeedRequest::Fixed(Speed::Slow);
        assert_eq!(
            governor.apply(&demand, &mut sink),
            Some(Applied {
                asked: Speed::Slow,
                running: Speed::Slow,
                undercuts: Some(Shortfall {
                    plane: PlaneKey::AUDIO,
                    needed: Speed::Fast,
                }),
            }),
            "the page's choice is applied as given, and the plane it costs is named"
        );
    }
}

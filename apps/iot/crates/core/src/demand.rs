//! Demand: what the product wants of each data plane, and when.

use alloc::collections::BTreeMap;

/// How fast the chip has to run for one piece of work to meet its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Speed {
    Slow,
    Normal,
    #[default]
    Fast,
}

/// What a page asks for, before anything is arbitrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeedRequest {
    #[default]
    Auto,
    Fixed(Speed),
}

pub trait SpeedTable {
    fn speed_for(kind: DemandKind) -> Speed;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Mode {
    #[default]
    Off,
    OnChange,
    Continuous,
}

impl Mode {
    pub const fn is_collecting(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// The cadence a thumbnail request asks for, and the point the camera's cost row keys on:
/// at or slower than this the consumer's repaint gate makes it a thumbnail, faster a
/// preview.
pub const CAMERA_THUMBNAIL_PERIOD_MS: u32 = 200;

/// How wrong a stale reading may be before it matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sensitivity {
    Primary,
    Secondary,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub mode: Mode,
    /// The requested cadence in milliseconds; `0` means the board's own rate.
    pub period_ms: u32,
    pub class: Sensitivity,
}

impl Claim {
    pub const fn new(mode: Mode, period_ms: u32, class: Sensitivity) -> Self {
        Self {
            mode,
            period_ms,
            class,
        }
    }

    pub const fn off() -> Self {
        Self {
            mode: Mode::Off,
            period_ms: 0,
            class: Sensitivity::Background,
        }
    }

    /// Narrow this claim to a mode and period, keeping the class: the class is a property
    /// of the claim rather than of the spelling, so an override changes what is asked and
    /// not who is asking.
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
pub enum DemandKind {
    Interaction,
    Motion,
    Audio,
    Camera,
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DemandKey {
    kind: DemandKind,
    instance: u8,
}

impl DemandKey {
    pub const fn of(kind: DemandKind) -> Self {
        Self { kind, instance: 0 }
    }

    /// The same device type, a specific unit of it.
    pub const fn instance(kind: DemandKind, instance: u8) -> Self {
        Self { kind, instance }
    }

    pub const fn kind(self) -> DemandKind {
        self.kind
    }

    pub const MOTION: Self = Self::of(DemandKind::Motion);
    pub const AUDIO: Self = Self::of(DemandKind::Audio);
    pub const CAMERA: Self = Self::of(DemandKind::Camera);
    pub const DISPLAY: Self = Self::of(DemandKind::Display);
    /// The interaction path answers as always-continuous: nothing may gate it.
    pub const INTERACTION: Self = Self::of(DemandKind::Interaction);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveDemand {
    claims: BTreeMap<DemandKey, Claim>,
    speed: SpeedRequest,
}

impl Default for EffectiveDemand {
    fn default() -> Self {
        let mut claims = BTreeMap::new();
        claims.insert(
            DemandKey::DISPLAY,
            Claim::new(Mode::Continuous, 0, Sensitivity::Primary),
        );
        Self {
            claims,
            speed: SpeedRequest::Auto,
        }
    }
}

impl Default for Claim {
    fn default() -> Self {
        Self::off()
    }
}

impl core::fmt::Display for Claim {
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
        for (index, (key, demand)) in self.claims.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{} {}", key, demand)?;
        }
        Ok(())
    }
}

impl core::fmt::Display for DemandKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self.kind {
            DemandKind::Interaction => "input",
            DemandKind::Motion => "motion",
            DemandKind::Audio => "audio",
            DemandKind::Camera => "camera",
            DemandKind::Display => "display",
        };
        match self.instance {
            0 => f.write_str(name),
            instance => write!(f, "{name}{instance}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Override {
    named: BTreeMap<DemandKey, (Mode, u32)>,
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

    pub fn named_mode(&self, key: DemandKey) -> Option<(Mode, u32)> {
        self.named.get(&key).copied()
    }

    pub fn set(&mut self, key: DemandKey, mode: Mode, period_ms: u32) {
        self.named.insert(key, (mode, period_ms));
    }

    /// Applies the modes this override names, in place, keeping each claim's class.
    pub fn apply(self, demand: &mut EffectiveDemand) {
        demand.speed = self.speed;
        for (key, (mode, period_ms)) in self.named {
            let current = demand.claim(key);
            demand.set(key, current.with(mode, period_ms));
        }
    }

    /// Whether anything was named, so a caller can say "no override" instead of
    /// republishing an unchanged demand.
    pub fn names_anything(&self) -> bool {
        !self.named.is_empty()
    }
}

const KIND_NAMES: &[(&str, DemandKind)] = &[
    ("input", DemandKind::Interaction),
    ("motion", DemandKind::Motion),
    ("audio", DemandKind::Audio),
    ("camera", DemandKind::Camera),
    ("display", DemandKind::Display),
];

fn mode_for(kind: DemandKind, name: &str) -> Option<(Mode, u32)> {
    let (name, asked_period_ms) = match name.split_once(':') {
        Some((mode, period)) => (mode, Some(period.parse().ok()?)),
        None => (name, None),
    };
    if kind == DemandKind::Interaction {
        return None;
    }
    let (mode, default_period_ms) = match (kind, name) {
        (_, "off") => (Mode::Off, 0),
        (_, "on-change") | (_, "on_change") => (Mode::OnChange, 0),
        (_, "on") | (_, "continuous") => (Mode::Continuous, 0),
        (DemandKind::Camera, "thumb") | (DemandKind::Camera, "thumbnail") => {
            (Mode::Continuous, CAMERA_THUMBNAIL_PERIOD_MS)
        }
        _ => return None,
    };
    Some((mode, asked_period_ms.unwrap_or(default_period_ms)))
}

fn parse_term(term: &str) -> Option<(DemandKey, (Mode, u32))> {
    let (kind_name, mode_name) = term.split_once('=')?;
    let wanted = mode_name.trim().to_ascii_lowercase();
    let kind = KIND_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == kind_name.trim().to_ascii_lowercase())
        .map(|(_, kind)| *kind)?;
    let mode = mode_for(kind, wanted.as_str())?;
    Some((DemandKey::of(kind), mode))
}

fn speed_for_name(name: &str) -> SpeedRequest {
    match name.trim().to_ascii_lowercase().as_str() {
        "slow" => SpeedRequest::Fixed(Speed::Slow),
        "normal" => SpeedRequest::Fixed(Speed::Normal),
        "fast" => SpeedRequest::Fixed(Speed::Fast),
        _ => SpeedRequest::Auto,
    }
}

pub fn parse_override(text: &str) -> Override {
    let mut combined = Override::none();
    for term in text.split(',') {
        if let Some(wanted) = term.strip_prefix("speed=") {
            combined = combined.with_speed(speed_for_name(wanted));
            continue;
        }
        if let Some((key, (mode, period))) = parse_term(term) {
            combined.set(key, mode, period);
        }
    }
    combined
}

pub fn resolve_speed<T: SpeedTable>(demand: &EffectiveDemand) -> Speed {
    match demand.speed {
        SpeedRequest::Fixed(speed) => speed,
        SpeedRequest::Auto => demand
            .keys()
            .filter(|key| demand.claim(**key).mode.is_collecting())
            .map(|key| T::speed_for(key.kind()))
            .max()
            .unwrap_or_default(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortfall {
    pub key: DemandKey,
    pub needed: Speed,
}

pub fn speed_shortfall<T: SpeedTable>(demand: &EffectiveDemand) -> Option<Shortfall> {
    let SpeedRequest::Fixed(asked) = demand.speed else {
        return None;
    };
    demand
        .keys()
        .filter(|key| demand.claim(**key).mode.is_collecting())
        .find_map(|key| {
            let needed = T::speed_for(key.kind());
            (needed > asked).then_some(Shortfall { key: *key, needed })
        })
}

pub trait ClockSink {
    type Table: SpeedTable;

    fn set_speed(&mut self, speed: Speed) -> Speed;
}

/// What one call to [`ClockGovernor::apply`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// What the demand resolved to.
    pub asked: Speed,
    /// What the part is running, which is below [`asked`](Self::asked) on a part with fewer
    /// presets than the vocabulary has.
    pub running: Speed,
    pub undercuts: Option<Shortfall>,
}

/// Holds a part at whatever the current demand resolves to, moving only when the answer
/// moves.
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
pub fn poll_plan_of(key: DemandKey, demand: &EffectiveDemand, board_cadence_ms: u64) -> PollPlan {
    if key.kind() == DemandKind::Interaction {
        return PollPlan {
            active: true,
            cadence_ms: board_cadence_ms,
        };
    }
    let claim = demand.claim(key);
    if !claim.mode.is_collecting() {
        return PollPlan {
            active: false,
            cadence_ms: board_cadence_ms,
        };
    }
    PollPlan {
        active: true,
        cadence_ms: board_cadence_ms.max(u64::from(claim.period_ms)),
    }
}

/// How far the controller is allowed to stop asking.
pub struct InputLimiter;

impl InputLimiter {
    /// The schedule for one source: the demand's floor, the board's own, and the rate past
    /// which this particular source would stop working.
    pub fn plan(
        key: DemandKey,
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
pub const MAX_SINGLE_HOLD_MS: u32 = 30;

#[cfg(test)]
pub const ALL_KEYS: &[DemandKey] = &[
    DemandKey::MOTION,
    DemandKey::AUDIO,
    DemandKey::CAMERA,
    DemandKey::DISPLAY,
];

impl EffectiveDemand {
    /// Nothing demanded.
    pub const fn none() -> Self {
        Self {
            claims: BTreeMap::new(),
            speed: SpeedRequest::Auto,
        }
    }

    /// What this demand asks the clock for, before anything is arbitrated.
    pub const fn speed(&self) -> SpeedRequest {
        self.speed
    }

    pub fn claim(&self, key: DemandKey) -> Claim {
        if key.kind() == DemandKind::Interaction {
            return Claim::new(Mode::Continuous, 0, Sensitivity::Primary);
        }
        self.claims.get(&key).copied().unwrap_or_default()
    }

    pub fn set(&mut self, key: DemandKey, demand: Claim) {
        self.claims.insert(key, demand);
    }

    pub fn keys(&self) -> impl Iterator<Item = &DemandKey> {
        self.claims.keys()
    }

    pub fn without(&self, key: DemandKey) -> Self {
        let mut demand = self.clone();
        if key.kind() != DemandKind::Interaction {
            demand.claims.insert(key, Claim::off());
        }
        demand
    }
}

#[cfg(test)]
mod tests {

    struct S3;

    impl SpeedTable for S3 {
        fn speed_for(kind: DemandKind) -> Speed {
            match kind {
                DemandKind::Interaction | DemandKind::Motion => Speed::Slow,
                DemandKind::Audio | DemandKind::Camera | DemandKind::Display => Speed::Fast,
            }
        }
    }

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

    fn collecting(key: DemandKey) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        demand.set(key, Claim::off().with(Mode::Continuous, 0));
        demand
    }

    fn live_claims() -> EffectiveDemand {
        {
            let mut demand = EffectiveDemand::none();
            demand.set(
                DemandKey::MOTION,
                Claim::new(Mode::Continuous, 0, Sensitivity::Secondary),
            );
            demand.set(
                DemandKey::AUDIO,
                Claim::new(Mode::Continuous, 0, Sensitivity::Secondary),
            );
            demand
        }
    }

    #[test]
    fn off_is_the_only_mode_that_stops_a_device() {
        assert!(!Mode::Off.is_collecting());
        assert!(Mode::OnChange.is_collecting());
        assert!(Mode::Continuous.is_collecting());
    }

    #[test]
    fn an_unset_demand_is_not_a_dark_device() {
        assert_eq!(
            EffectiveDemand::none().claim(DemandKey::DISPLAY).mode,
            Mode::Off
        );
        assert_eq!(
            EffectiveDemand::default().claim(DemandKey::DISPLAY).mode,
            Mode::Continuous,
            "an unset demand is not a dark device"
        );
    }

    #[test]
    fn the_interaction_path_is_never_gated() {
        let off = EffectiveDemand::none();
        let plan = poll_plan_of(DemandKey::INTERACTION, &off, 10);
        assert!(
            plan.active,
            "a demand of nothing must still leave the input readable"
        );
        assert_eq!(plan.cadence_ms, 10);
    }

    #[test]
    fn a_device_demanded_at_off_is_not_polled_at_all() {
        let off = EffectiveDemand::none();
        for key in [DemandKey::MOTION, DemandKey::AUDIO] {
            let plan = poll_plan_of(key, &off, 20);
            assert!(!plan.active, "{key} demanded at Off must not be polled");
            assert_eq!(plan.cadence_ms, 20);
        }
    }

    #[test]
    fn a_requested_period_is_a_floor_the_cadence_cannot_shorten() {
        let mut demand = live_claims();
        demand.set(
            DemandKey::AUDIO,
            Claim::new(Mode::Continuous, 500, Sensitivity::Secondary),
        );
        let plan = poll_plan_of(DemandKey::AUDIO, &demand, 20);
        assert_eq!(plan.cadence_ms, 500, "a slow request must not be sped up");
    }

    #[test]
    fn a_source_that_cannot_go_slower_folds_back_to_the_board_cadence() {
        let mut demand = live_claims();
        demand.set(
            DemandKey::AUDIO,
            Claim::new(Mode::Continuous, 2_000, Sensitivity::Secondary),
        );

        assert_eq!(
            InputLimiter::plan(DemandKey::AUDIO, &demand, 20, false).cadence_ms,
            20,
            "a source whose clock dies when polled slower keeps the board's own cadence"
        );
        assert_eq!(
            InputLimiter::plan(DemandKey::AUDIO, &demand, 20, true).cadence_ms,
            2_000,
            "one that only goes stale takes the stretch the demand asked for"
        );
    }

    #[test]
    fn the_interaction_path_takes_no_plan_at_all_even_from_a_source_that_cannot_go_slower() {
        let off = EffectiveDemand::none();
        let plan = InputLimiter::plan(DemandKey::INTERACTION, &off, 10, false);
        assert!(
            plan.active,
            "a stale button reading is a device that cannot be steered off its page"
        );
        assert_eq!(plan.cadence_ms, 10);
    }

    #[test]
    fn a_class_moves_no_cadence_because_there_is_no_fifo_to_hold_an_event() {
        let mut demand = live_claims();
        demand.set(
            DemandKey::MOTION,
            Claim::new(Mode::Continuous, 0, Sensitivity::Primary),
        );
        demand.set(
            DemandKey::AUDIO,
            Claim::new(Mode::Continuous, 0, Sensitivity::Background),
        );

        let eager = poll_plan_of(DemandKey::MOTION, &demand, 20);
        let patient = poll_plan_of(DemandKey::AUDIO, &demand, 20);
        assert_eq!(eager.cadence_ms, patient.cadence_ms);
    }

    #[test]
    fn a_cost_report_can_name_every_device_and_its_class() {
        use alloc::format;
        let mut demand = EffectiveDemand::none();
        demand.set(
            DemandKey::MOTION,
            Claim::new(Mode::OnChange, 0, Sensitivity::Secondary),
        );
        demand.set(DemandKey::AUDIO, Claim::off());
        demand.set(
            DemandKey::CAMERA,
            Claim::new(
                Mode::Continuous,
                CAMERA_THUMBNAIL_PERIOD_MS,
                Sensitivity::Primary,
            ),
        );
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(Mode::Continuous, 0, Sensitivity::Primary),
        );

        assert_eq!(
            format!("{demand}"),
            "motion OnChange@Secondary audio Off@Background camera Continuous/200ms@Primary \
             display Continuous@Primary"
        );
    }

    #[test]
    fn an_override_names_a_device_and_a_mode_and_leaves_the_rest_alone() {
        let parsed = parse_override("audio=off");
        assert!(parsed.names_anything());
        assert_eq!(parsed.named_mode(DemandKey::AUDIO), Some((Mode::Off, 0)));
        assert_eq!(parsed.named_mode(DemandKey::MOTION), None);

        let mut demand = live_claims();
        let before = demand.claim(DemandKey::MOTION);
        parsed.apply(&mut demand);

        assert_eq!(demand.claim(DemandKey::AUDIO).mode, Mode::Off);
        assert_eq!(demand.claim(DemandKey::MOTION), before);
        assert_eq!(
            demand.claim(DemandKey::AUDIO).class,
            Sensitivity::Secondary,
            "narrowing a mode must not re-price the device under another budget"
        );
    }

    #[test]
    fn several_terms_can_be_named_at_once() {
        let parsed = parse_override("motion=on-change,display=off");
        assert_eq!(
            parsed.named_mode(DemandKey::MOTION),
            Some((Mode::OnChange, 0))
        );
        assert_eq!(parsed.named_mode(DemandKey::DISPLAY), Some((Mode::Off, 0)));
        assert_eq!(parsed.named_mode(DemandKey::AUDIO), None);
    }

    #[test]
    fn a_camera_override_can_ask_for_a_thumbnail() {
        let parsed = parse_override("camera=thumb");
        assert_eq!(
            parsed.named_mode(DemandKey::CAMERA),
            Some((Mode::Continuous, CAMERA_THUMBNAIL_PERIOD_MS))
        );
    }

    #[test]
    fn a_build_can_name_its_own_cadence_instead_of_asking_a_policy_for_one() {
        let parsed = parse_override("motion=on:30000,audio=off");
        assert_eq!(
            parsed.named_mode(DemandKey::MOTION),
            Some((Mode::Continuous, 30_000))
        );
        assert_eq!(parsed.named_mode(DemandKey::AUDIO), Some((Mode::Off, 0)));

        let mut demand = live_claims();
        parsed.apply(&mut demand);
        assert_eq!(
            poll_plan_of(DemandKey::MOTION, &demand, 20).cadence_ms,
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
    fn a_later_term_wins_for_the_same_device() {
        let parsed = parse_override("display=off,display=on");
        assert_eq!(
            parsed.named_mode(DemandKey::DISPLAY),
            Some((Mode::Continuous, 0))
        );
    }

    #[test]
    fn a_device_nobody_named_reads_as_off_rather_than_as_a_missing_key() {
        let empty = EffectiveDemand::none();
        for key in ALL_KEYS.iter().copied() {
            assert_eq!(empty.claim(key), Claim::off(), "{key}");
            assert_eq!(empty.claim(key).mode, Mode::Off);
        }
    }

    #[test]
    fn an_override_leaves_a_device_it_says_nothing_about_alone() {
        let mut demand = EffectiveDemand::none();
        demand.set(
            DemandKey::AUDIO,
            Claim::new(Mode::Continuous, 500, Sensitivity::Secondary),
        );
        demand.set(
            DemandKey::DISPLAY,
            Claim::new(Mode::Continuous, 0, Sensitivity::Primary),
        );

        let mut override_ = Override::none();
        override_.set(DemandKey::AUDIO, Mode::Off, 0);
        override_.apply(&mut demand);

        assert_eq!(demand.claim(DemandKey::AUDIO).mode, Mode::Off);
        assert_eq!(
            demand.claim(DemandKey::DISPLAY).mode,
            Mode::Continuous,
            "naming the audio device must not have repainted the panel's demand"
        );
    }

    #[test]
    fn a_term_this_firmware_does_not_recognise_costs_that_term_and_not_the_rest() {
        for text in [
            "audoi=off", // misspelled device
            "audio=of",  // misspelled mode
            "input=off", // a device no demand may switch off
            "audio",     // no `=`
            "=off",      // no device
            "",          // nothing at all
        ] {
            assert!(
                !parse_override(text).names_anything(),
                "{text:?} names nothing"
            );
        }
        let partial = parse_override("audio=off, motion=on, nonsense");
        assert_eq!(partial.named_mode(DemandKey::AUDIO), Some((Mode::Off, 0)));
        assert_eq!(
            partial.named_mode(DemandKey::MOTION),
            Some((Mode::Continuous, 0))
        );
    }

    #[test]
    fn a_separator_at_either_end_costs_nothing() {
        for text in [",audio=off", "audio=off,", ", audio=off ,", " audio = off "] {
            assert_eq!(
                parse_override(text).named_mode(DemandKey::AUDIO),
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
        let mut demand = crate::state::DisplayPage::Camera.demand(ALL_KEYS);
        assert_eq!(demand.claim(DemandKey::CAMERA).mode, Mode::Continuous);

        parse_override("camera=off").apply(&mut demand);

        assert_eq!(demand.claim(DemandKey::CAMERA).mode, Mode::Off);
    }

    #[test]
    fn every_shipped_preset_spells_modes_this_firmware_has() {
        let thrift = parse_override("audio=off,display=on-change");
        assert_eq!(thrift.named_mode(DemandKey::AUDIO), Some((Mode::Off, 0)));
        assert_eq!(
            thrift.named_mode(DemandKey::DISPLAY),
            Some((Mode::OnChange, 0))
        );
    }

    #[test]
    fn auto_reads_the_collecting_devices_and_takes_the_highest() {
        let mut demand = EffectiveDemand::none();
        demand.set(DemandKey::CAMERA, Claim::off().with(Mode::Continuous, 0));
        assert_eq!(resolve_speed::<S3>(&demand), Speed::Fast);

        demand.set(DemandKey::CAMERA, Claim::off());
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::default(),
            "nothing collecting leaves the default, which is what a device that is not wired needs"
        );
    }

    #[test]
    fn an_explicit_request_is_taken_as_given_rather_than_arbitrated() {
        let mut demand = EffectiveDemand::none();
        demand.set(DemandKey::AUDIO, Claim::off().with(Mode::Continuous, 0));
        demand.speed = SpeedRequest::Fixed(Speed::Slow);
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::Slow,
            "a page that named a speed gets it; the point of letting one is that it can be wrong"
        );
    }

    #[test]
    fn a_forced_speed_names_the_device_it_is_undercutting() {
        let mut demand = EffectiveDemand::none();
        demand.set(DemandKey::AUDIO, Claim::off().with(Mode::Continuous, 0));
        demand.set(DemandKey::CAMERA, Claim::off());

        demand.speed = SpeedRequest::Fixed(Speed::Slow);
        assert_eq!(
            speed_shortfall::<S3>(&demand),
            Some(Shortfall {
                key: DemandKey::AUDIO,
                needed: Speed::Fast,
            }),
            "the report names the device and what it needs, so neither has to be looked up"
        );

        demand.speed = SpeedRequest::Auto;
        assert_eq!(
            speed_shortfall::<S3>(&demand),
            None,
            "Auto is arbitrated, so nothing is being undercut by it"
        );
    }

    #[test]
    fn a_table_that_makes_a_choice_splits_the_keys_around_it() {
        assert_eq!(S3::speed_for(DemandKind::Motion), Speed::Slow);
        assert_eq!(S3::speed_for(DemandKind::Interaction), Speed::Slow);
        assert_eq!(S3::speed_for(DemandKind::Camera), Speed::Fast);
        assert_eq!(S3::speed_for(DemandKind::Display), Speed::Fast);
        assert_eq!(S3::speed_for(DemandKind::Audio), Speed::Fast);
    }

    #[test]
    fn auto_over_motion_alone_drops_below_fast_and_a_page_can_override_that() {
        let mut demand = EffectiveDemand::none();
        demand.set(DemandKey::MOTION, Claim::off().with(Mode::Continuous, 0));
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::Slow,
            "motion is the row that makes the table a choice rather than a constant"
        );

        demand.speed = SpeedRequest::Fixed(Speed::Fast);
        assert_eq!(resolve_speed::<S3>(&demand), Speed::Fast);
    }

    #[test]
    fn the_highest_of_several_collecting_devices_wins() {
        let mut demand = EffectiveDemand::none();
        demand.set(DemandKey::MOTION, Claim::off().with(Mode::Continuous, 0));
        demand.set(DemandKey::AUDIO, Claim::off().with(Mode::Continuous, 0));
        assert_eq!(
            resolve_speed::<S3>(&demand),
            Speed::Fast,
            "one device at the top of the table lifts every other, which is what makes \
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
            governor.apply(&collecting(DemandKey::MOTION), &mut sink),
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
        let demand = collecting(DemandKey::MOTION);
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
            governor.apply(&collecting(DemandKey::AUDIO), &mut sink),
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
    fn a_page_that_named_its_own_speed_is_told_which_device_it_is_undercutting() {
        let mut sink = Reaching::default();
        let mut governor = ClockGovernor::new();
        let mut demand = collecting(DemandKey::MOTION);
        demand.set(DemandKey::AUDIO, Claim::off().with(Mode::Continuous, 0));
        governor.apply(&demand, &mut sink);
        demand.speed = SpeedRequest::Fixed(Speed::Slow);
        assert_eq!(
            governor.apply(&demand, &mut sink),
            Some(Applied {
                asked: Speed::Slow,
                running: Speed::Slow,
                undercuts: Some(Shortfall {
                    key: DemandKey::AUDIO,
                    needed: Speed::Fast,
                }),
            }),
            "the page's choice is applied as given, and the device it costs is named"
        );
    }
}

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Ticker};
use iot_core::demand::{DemandKind, EffectiveDemand, InputLimiter, MAX_SINGLE_HOLD_MS, PollPlan};
use iot_core::drivers::input::{INPUT_BASE_MS, PollEntry};
use iot_core::intent::{BusinessIntent, Intent, PageIntent, recognize};

use iot_core::config::{Configuration, Configurator, Unreachable};

pub const INTENT_SLOTS: usize = 8;

/// Intent channel between the input task and the render loop.
pub type IntentBus = Channel<CriticalSectionRawMutex, Intent, INTENT_SLOTS>;

pub static INTENT_BUS: IntentBus = IntentBus::new();

const HOLD_REPORT_MS: u64 = 5_000;

/// The lock-side half of a tick: reads the demand and works out what each source should do.
/// It touches no driver, logs nothing, and allocates nothing — the feed interrupt takes the
/// same lock, so anything slow here delays the ring.
fn decide(
    sources: &[PollEntry],
    demand: &EffectiveDemand,
    configurator: &Configurator,
    plans: &mut Vec<PollPlan>,
) -> Result<Configuration, Unreachable> {
    let configuration = configurator.configuration_for(demand);
    plans.clear();
    plans.extend(sources.iter().map(|entry| {
        InputLimiter::plan(
            entry.key(),
            demand,
            entry.board_cadence_ms(),
            entry.accepts_slower_poll(),
        )
    }));
    configuration
}

/// Carries a decision to the drivers: the I2C configuration, the schedule bookkeeping, and
/// the one log line a change is worth. This runs outside the lock.
fn apply_decision(
    sources: &mut [PollEntry],
    decision: Result<Configuration, Unreachable>,
    plans: &[PollPlan],
    now_ms: u64,
) -> bool {
    debug_assert_eq!(
        plans.len(),
        sources.len(),
        "a plan per source, no more, no less"
    );
    let configuration = match decision {
        Ok(configuration) => configuration,
        Err(refusal) => {
            log::warn!(
                "[CONFIG] {refusal} on {}, so the part stays where it is",
                refusal.key
            );
            Configuration::default()
        }
    };
    for entry in sources.iter_mut() {
        if entry.key().kind() == DemandKind::Motion {
            let _ = entry.configure_for(configuration.motion);
        }
    }
    let mut moved = false;
    for (entry, plan) in sources.iter_mut().zip(plans.iter().copied()) {
        let before = (entry.is_active(), entry.cadence_ms());
        entry.set_collecting(now_ms, plan.active);
        entry.set_active(plan.active);
        entry.set_cadence_ms(plan.cadence_ms);
        moved |= (entry.is_active(), entry.cadence_ms()) != before;
    }
    if moved {
        use core::fmt::Write;
        let mut report = alloc::string::String::new();
        for (index, entry) in sources.iter().enumerate() {
            let separator = if index == 0 { "" } else { ", " };
            let name = entry.name();
            if entry.is_active() {
                let _ = write!(report, "{separator}{name} on {} ms", entry.cadence_ms());
            } else {
                let _ = write!(report, "{separator}{name} off");
            }
        }
        let motion_config = match configuration.motion {
            Some(config) => format!("{config:?}"),
            None => String::from("none"),
        };
        log::info!("[DEMAND] schedule: {report} | motion config {motion_config}");
    }
    moved
}

#[cfg(test)]
fn apply(
    sources: &mut [PollEntry],
    demand: &EffectiveDemand,
    configurator: &Configurator,
    now_ms: u64,
) -> bool {
    let mut plans = Vec::with_capacity(sources.len());
    let decision = decide(sources, demand, configurator, &mut plans);
    apply_decision(sources, decision, &plans, now_ms)
}

/// Drains every input source at the shared base tick and forwards recognized operations.
pub async fn input_task(
    intent_bus: &'static IntentBus,
    mut sources: Vec<PollEntry>,
    configurator: &Configurator,
    input: crate::workers::Input<'_>,
) -> ! {
    let mut ticker = Ticker::every(Duration::from_millis(INPUT_BASE_MS));
    let mut now_ms: u64 = 0;
    let mut worst_us = alloc::vec![0u64; sources.len()];
    let mut polled = alloc::vec![false; sources.len()];
    let mut window_hold_us: u64 = 0;
    let mut window_worst_hold_ms: u64 = 0;
    let mut window_ticks: u32 = 0;
    let mut last_report = Instant::now();
    let mut splash_open = true;
    let mut plans: Vec<PollPlan> = Vec::with_capacity(sources.len());
    loop {
        ticker.next().await;
        now_ms = now_ms.wrapping_add(INPUT_BASE_MS);
        let loop_start = Instant::now();
        let decision = input
            .to
            .lock(|current| decide(&sources, &current.borrow(), configurator, &mut plans));
        apply_decision(&mut sources, decision, &plans, now_ms);
        for (index, entry) in sources.iter_mut().enumerate() {
            if !entry.is_active() || entry.next_at_ms() > now_ms {
                continue;
            }
            let poll_start = Instant::now();
            if let Some(event) = entry.poll(now_ms) {
                let intent = Intent::Operation(recognize(event, entry.source_id()));
                let _ = intent_bus.try_send(intent);
            }
            entry.advance_past(now_ms);
            let took_us = poll_start.elapsed().as_micros();
            if let Some(slot) = worst_us.get_mut(index) {
                *slot = (*slot).max(took_us);
            }
            if let Some(seen) = polled.get_mut(index) {
                *seen = true;
            }
        }
        if splash_open
            && now_ms >= iot_core::page::SPLASH_MS
            && !sources.iter().any(|entry| entry.held())
            && intent_bus
                .try_send(Intent::Business(BusinessIntent::Page(
                    PageIntent::SplashOver,
                )))
                .is_ok()
        {
            splash_open = false;
        }
        let loop_hold_us = loop_start.elapsed().as_micros();
        window_hold_us += loop_hold_us;
        window_worst_hold_ms = window_worst_hold_ms.max(loop_hold_us / 1_000);
        window_ticks += 1;

        let now = Instant::now();
        let window_ms = now.saturating_duration_since(last_report).as_millis();
        if window_ms >= HOLD_REPORT_MS {
            let mut parts = alloc::string::String::new();
            use core::fmt::Write;
            for (entry, (seen, worst)) in sources.iter().zip(polled.iter().zip(worst_us.iter())) {
                if !seen {
                    continue;
                }
                let _ = write!(
                    parts,
                    "{}{} {} ms",
                    if parts.is_empty() { "" } else { ", " },
                    entry.name(),
                    worst / 1_000,
                );
            }
            let over_bound = window_worst_hold_ms >= u64::from(MAX_SINGLE_HOLD_MS);
            log::log!(
                if over_bound {
                    log::Level::Warn
                } else {
                    log::Level::Debug
                },
                "[INPUT] held the executor {} ms of {} ms over {} ticks; worst single hold {} ms (bound {} ms); worst source: {}",
                window_hold_us / 1_000,
                window_ms,
                window_ticks,
                window_worst_hold_ms,
                MAX_SINGLE_HOLD_MS,
                parts
            );
            window_hold_us = 0;
            window_worst_hold_ms = 0;
            window_ticks = 0;
            worst_us.fill(0);
            polled.fill(false);
            last_report = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use iot_core::config::MotionConfig;
    use iot_core::demand::{Claim, DemandKey, EffectiveDemand, Mode, Sensitivity};
    use iot_core::drivers::input::{InputSource, PassThrough};

    struct Recording {
        collecting: Option<bool>,
        polls: u32,
        batchable: bool,
        told: Rc<RefCell<Option<MotionConfig>>>,
    }

    impl Recording {
        fn new(batchable: bool) -> Self {
            Self::with_tell_to(batchable, Rc::new(RefCell::new(None)))
        }

        fn told_to(told: &Rc<RefCell<Option<MotionConfig>>>) -> Self {
            Self::with_tell_to(true, Rc::clone(told))
        }

        fn with_tell_to(batchable: bool, told: Rc<RefCell<Option<MotionConfig>>>) -> Self {
            Self {
                collecting: None,
                polls: 0,
                batchable,
                told,
            }
        }
    }

    impl InputSource for Recording {
        fn name(&self) -> &'static str {
            "recording"
        }

        fn poll(&mut self, _now_ms: u64) -> Option<iot_core::drivers::input::InputEvent> {
            self.polls += 1;
            None
        }

        fn set_demand_collecting(&mut self, _now_ms: u64, collecting: bool) {
            self.collecting = Some(collecting);
        }

        fn accepts_slower_poll(&self) -> bool {
            self.batchable
        }

        fn configure_for(
            &mut self,
            wanted: Option<MotionConfig>,
        ) -> Result<Option<MotionConfig>, MotionConfig> {
            *self.told.borrow_mut() = wanted;
            Ok(wanted)
        }
    }

    fn configurable_entry(
        key: DemandKey,
        cadence_ms: u64,
    ) -> (PollEntry, Rc<RefCell<Option<MotionConfig>>>) {
        let told = Rc::new(RefCell::new(None));
        let entry = PollEntry::new(
            0,
            key,
            Box::new(Recording::told_to(&told)),
            Box::new(PassThrough),
            cadence_ms,
        );
        (entry, told)
    }

    fn entry(key: DemandKey, cadence_ms: u64) -> PollEntry {
        entry_with(key, cadence_ms, true)
    }

    fn entry_with(key: DemandKey, cadence_ms: u64, batchable: bool) -> PollEntry {
        PollEntry::new(
            0,
            key,
            Box::new(Recording::new(batchable)),
            Box::new(PassThrough),
            cadence_ms,
        )
    }

    fn configurator() -> Configurator {
        Configurator::new(&[MotionConfig::PolledRegisters])
    }

    fn demand_on_period(period_ms: u32) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        for key in [DemandKey::MOTION, DemandKey::AUDIO] {
            demand.set(
                key,
                Claim::new(Mode::Continuous, period_ms, Sensitivity::Secondary),
            );
        }
        demand
    }

    #[test]
    fn the_configuration_the_configurator_decides_reaches_the_part() {
        let (entry, told) = configurable_entry(DemandKey::MOTION, MOTION_CADENCE_MS);
        let mut sources = alloc::vec![entry];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(
            *told.borrow(),
            Some(MotionConfig::PolledRegisters),
            "a configuration that never reaches the part has configured nothing"
        );
    }

    #[test]
    fn a_device_with_no_configuration_is_not_told_about_one() {
        let (entry, told) = configurable_entry(DemandKey::AUDIO, CAPTURE_CADENCE_MS);
        let mut sources = alloc::vec![entry];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(*told.borrow(), None);
    }

    #[test]
    fn a_board_that_cannot_enter_what_a_mode_needs_keeps_the_part_where_it_is() {
        let (entry, told) = configurable_entry(DemandKey::MOTION, MOTION_CADENCE_MS);
        let mut sources = alloc::vec![entry];

        apply(
            &mut sources,
            &demand_on_period(30_000),
            &Configurator::new(&[]),
            0,
        );

        assert_eq!(
            *told.borrow(),
            None,
            "a part that could not enter the configuration must not be moved to it"
        );
    }

    #[test]
    fn an_unchanged_demand_moves_nothing_and_so_says_nothing() {
        let mut sources = alloc::vec![entry(DemandKey::MOTION, MOTION_CADENCE_MS)];
        let demand = demand_on_period(30_000);

        assert!(
            apply(&mut sources, &demand, &configurator(), 0),
            "the first application moves the cadence and is worth reporting"
        );
        assert!(
            !apply(&mut sources, &demand, &configurator(), 10),
            "the same demand again moves nothing, so there is nothing new to say"
        );
    }

    #[test]
    fn a_demand_that_relaxes_moves_the_schedule_again() {
        let mut sources = alloc::vec![entry(DemandKey::MOTION, MOTION_CADENCE_MS)];

        assert!(apply(
            &mut sources,
            &demand_on_period(30_000),
            &configurator(),
            0
        ));
        assert!(
            apply(&mut sources, &demand_on_period(0), &configurator(), 10),
            "giving the period back is a real change even though the demand was heard before"
        );
    }

    #[test]
    fn a_requested_period_actually_stretches_the_poll_schedule() {
        let mut sources = alloc::vec![entry(DemandKey::MOTION, MOTION_CADENCE_MS)];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(
            sources[0].cadence_ms(),
            30_000,
            "the period the claim asked for is what the loop now polls at"
        );
        assert_eq!(
            sources[0].board_cadence_ms(),
            MOTION_CADENCE_MS,
            "and the board's own cadence is the floor it came from"
        );
    }

    #[test]
    fn dropping_the_period_puts_the_cadence_back_in_one_update() {
        let mut sources = alloc::vec![entry(DemandKey::MOTION, MOTION_CADENCE_MS)];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);
        apply(&mut sources, &demand_on_period(0), &configurator(), 0);

        assert_eq!(
            sources[0].cadence_ms(),
            MOTION_CADENCE_MS,
            "a device asked for no period of its own is read at the board's rate again"
        );
    }

    #[test]
    fn a_device_the_demand_drops_is_stopped_both_as_a_schedule_and_at_the_driver() {
        let mut sources = alloc::vec![entry(DemandKey::MOTION, MOTION_CADENCE_MS)];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);
        assert!(sources[0].is_active());

        apply(&mut sources, &EffectiveDemand::none(), &configurator(), 0);

        assert!(!sources[0].is_active(), "the loop stops asking");
        assert_eq!(
            sources[0].cadence_ms(),
            MOTION_CADENCE_MS,
            "and its cadence is left where the board wired it, not zeroed"
        );
    }

    #[test]
    fn a_device_whose_driver_needs_its_poll_is_not_stretched() {
        let mut sources = alloc::vec![entry_with(DemandKey::AUDIO, CAPTURE_CADENCE_MS, false)];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(
            sources[0].cadence_ms(),
            CAPTURE_CADENCE_MS,
            "the board's cadence is the only one this source can take"
        );
        assert!(
            sources[0].is_active(),
            "and it stays on: this device may be off, never rare"
        );
    }

    #[test]
    fn the_interaction_path_takes_no_plan_at_all() {
        let mut sources = alloc::vec![entry(DemandKey::INTERACTION, 10)];

        for demand in [EffectiveDemand::none(), demand_on_period(30_000)] {
            apply(&mut sources, &demand, &configurator(), 0);
            assert!(sources[0].is_active(), "the input path was gated");
            assert_eq!(sources[0].cadence_ms(), 10, "the input path was stretched");
        }
    }

    const MOTION_CADENCE_MS: u64 = iot_core::drivers::motion::MOTION_SCAN_MS;

    const CAPTURE_CADENCE_MS: u64 = iot_core::drivers::audio::CAPTURE_MS;
}

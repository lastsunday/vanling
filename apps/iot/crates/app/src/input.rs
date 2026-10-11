use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Ticker};
use iot_core::demand::{DemandField, EffectiveDemand, InputLimiter, MAX_SINGLE_HOLD_MS};
use iot_core::drivers::input::{INPUT_BASE_MS, PollEntry};
use iot_core::intent::{Intent, recognize};

use crate::demand;
use iot_core::config::{Configuration, Configurator};

/// Intent channel: input task writes recognized operations, the render loop
/// drains and interprets.
pub type IntentBus = Channel<CriticalSectionRawMutex, Intent, 8>;

/// Intent bus storage; passed as `&'static` into the tasks so no consumer
/// reaches the global by name.
pub static INTENT_BUS: IntentBus = IntentBus::new();

/// Source names by wiring order, for the executor-hold report. Index matches the
/// order `take_input`/`take_motion`/`take_audio` push into the vector.
const SOURCE_NAMES: [&str; 4] = ["button", "touch", "motion", "capture"];

/// Wall-clock window between executor-hold reports, matching the playback
/// side so the two reports line up on one clock.
const HOLD_REPORT_MS: u64 = 5_000;

/// Applies one demand to the parts and then to the poll schedule.
/// Applies `demand` to the schedule, answering whether anything moved.
///
/// Replaying the same demand every tick is free — every setter here is idempotent — but
/// *reporting* it every tick is not, so the schedule itself is the change detector: the
/// setters already refuse a value they are already in, and this reports the ones that
/// actually took.
fn apply(
    sources: &mut [PollEntry],
    demand: &EffectiveDemand,
    configurator: &Configurator,
    now_ms: u64,
) -> bool {
    // The configurator sits here because the parts are here: it needs the board's declared
    // capabilities to resolve a mode, and it reports a refusal against the part that refused.
    // Deciding it anywhere else would mean shipping a decision to whoever owns the hardware.
    let configuration = match configurator.configuration_for(demand) {
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
        if entry.key().field() == DemandField::Motion {
            let _ = entry.configure_for(configuration.motion);
        }
    }
    let mut moved = false;
    let mut report = alloc::string::String::new();
    use core::fmt::Write;
    for (index, entry) in sources.iter_mut().enumerate() {
        // The floor is what the board wired, not the cadence in force: a plan derived
        // from an already-stretched cadence could never be relaxed.
        let board_cadence_ms = entry.board_cadence_ms();
        let plan = InputLimiter::plan(
            entry.key(),
            demand,
            board_cadence_ms,
            entry.accepts_slower_poll(),
        );
        // Told separately from the schedule: a plane that is off is not polled, so
        // cannot learn of it from the next poll.
        let before = (entry.is_active(), entry.cadence_ms());
        entry.set_collecting(now_ms, plan.active);
        entry.set_active(plan.active);
        entry.set_cadence_ms(plan.cadence_ms);
        moved |= (entry.is_active(), entry.cadence_ms()) != before;
        // The plane rather than the source id: this line is read by someone who wants to
        // know what stopped.
        let _ = write!(
            report,
            "{}{} {} {} ms",
            if index == 0 { "" } else { ", " },
            entry.key(),
            if plan.active { "on" } else { "off" },
            plan.cadence_ms
        );
    }
    let motion_config = match configuration.motion {
        Some(config) => format!("{config:?}"),
        None => String::from("none"),
    };
    if moved {
        log::info!("[DEMAND] {report} | motion config {motion_config}");
    }
    moved
}

/// Drains every input source at the shared base tick, gating each source to
/// its own cadence, and forwards recognized operations onto the intent bus.
/// `try_send` drops intents when the bus is full: dropping a press is preferable
/// to blocking the scan loop, which would skew debounce timing.
pub async fn input_task(
    intent_bus: &'static IntentBus,
    mut sources: Vec<PollEntry>,
    configurator: &Configurator,
) -> ! {
    let mut ticker = Ticker::every(Duration::from_millis(INPUT_BASE_MS));
    let mut now_ms: u64 = 0;
    // Before the first demand the schedule stays as the board wired it, the only plan that
    // needs no demand to be true — which is what the state's boot value already says.
    // Per-source worst hold seen in the current window, in microseconds, plus
    // whether that source was ever polled this window.
    let mut worst_us = [0u64; 4];
    let mut polled = [false; 4];
    let mut window_hold_us: u64 = 0;
    let mut window_worst_hold_ms: u64 = 0;
    let mut window_ticks: u32 = 0;
    let mut last_report = Instant::now();
    loop {
        // Ticks stay anchored to the base clock even when a source takes
        // inconsistent time to sample, so slow devices never alias.
        ticker.next().await;
        now_ms = now_ms.wrapping_add(INPUT_BASE_MS);
        let loop_start = Instant::now();
        demand::with(|current| apply(&mut sources, current, configurator, now_ms));
        for (index, entry) in sources.iter_mut().enumerate() {
            if !entry.is_active() || entry.next_at_ms() > now_ms {
                continue;
            }
            let poll_start = Instant::now();
            // Recognition is stateless: it packages the raw event plus the
            // wiring-order source, never reading device state. The render loop
            // interprets the operation against the freshest snapshot, so
            // nothing stale ever gets re-applied.
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
        // The whole `for` body runs without awaiting, so its wall time is
        // exactly how long the other tasks on the shared executor wait.
        let loop_hold_us = loop_start.elapsed().as_micros();
        window_hold_us += loop_hold_us;
        window_worst_hold_ms = window_worst_hold_ms.max(loop_hold_us / 1_000);
        window_ticks += 1;

        let now = Instant::now();
        let window_ms = now.saturating_duration_since(last_report).as_millis();
        if window_ms >= HOLD_REPORT_MS {
            let mut parts = alloc::string::String::new();
            use core::fmt::Write;
            for (index, seen) in polled.iter().enumerate() {
                if !seen {
                    continue;
                }
                let _ = write!(
                    parts,
                    "{}{} {} ms",
                    if parts.is_empty() { "" } else { ", " },
                    SOURCE_NAMES.get(index).copied().unwrap_or("?"),
                    worst_us[index] / 1_000,
                );
            }
            log::warn!(
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
            for slot in worst_us.iter_mut() {
                *slot = 0;
            }
            for seen in polled.iter_mut() {
                *seen = false;
            }
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
    use iot_core::demand::{EffectiveDemand, Mode, PlaneDemand, PlaneKey, Sensitivity};
    use iot_core::drivers::input::{InputSource, PassThrough};

    /// A source that records what it was told, so a test can see the demand reach the
    /// driver.
    struct Recording {
        collecting: Option<bool>,
        polls: u32,
        batchable: bool,
        /// What it was last told to be configured as. Shared rather than owned because the
        /// source is behind a trait object and the entry cannot be asked for it.
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
        fn poll(&mut self, _now_ms: u64) -> Option<iot_core::drivers::input::InputEvent> {
            self.polls += 1;
            None
        }

        fn set_plane_collecting(&mut self, _now_ms: u64, collecting: bool) {
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

    /// An entry whose source reports what it was told to be configured as, so a test can see
    /// the decision reach the part.
    fn configurable_entry(
        key: PlaneKey,
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

    fn entry(key: PlaneKey, cadence_ms: u64) -> PollEntry {
        entry_with(key, cadence_ms, true)
    }

    fn entry_with(key: PlaneKey, cadence_ms: u64, batchable: bool) -> PollEntry {
        PollEntry::new(
            0,
            key,
            Box::new(Recording::new(batchable)),
            Box::new(PassThrough),
            cadence_ms,
        )
    }

    /// The configurator the tests resolve demands against: what this board can enter.
    fn configurator() -> Configurator {
        Configurator::new(&[MotionConfig::PolledRegisters])
    }

    /// The motion and audio planes both asked to poll every `period_ms`.
    fn demand_on_period(period_ms: u32) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        for key in [PlaneKey::MOTION, PlaneKey::AUDIO] {
            demand.set(
                key,
                PlaneDemand::new(Mode::Continuous, period_ms, Sensitivity::Secondary),
            );
        }
        demand
    }

    #[test]
    fn the_configuration_the_configurator_decides_reaches_the_part() {
        // The configurator sits here because the parts are here, so this hop is the one that
        // decides whether the decision does anything at all.
        let (entry, told) = configurable_entry(PlaneKey::MOTION, MOTION_CADENCE_MS);
        let mut sources = alloc::vec![entry];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(
            *told.borrow(),
            Some(MotionConfig::PolledRegisters),
            "a configuration that never reaches the part has configured nothing"
        );
    }

    #[test]
    fn a_plane_with_no_configuration_is_not_told_about_one() {
        // Only motion has an acquisition choice. Asking the others about it would be a claim
        // that they have one.
        let (entry, told) = configurable_entry(PlaneKey::AUDIO, CAPTURE_CADENCE_MS);
        let mut sources = alloc::vec![entry];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(*told.borrow(), None);
    }

    #[test]
    fn a_board_that_cannot_enter_what_a_mode_needs_keeps_the_part_where_it_is() {
        // Refusal has to be visible where it happens: the part is here, so a configurator that
        // cannot be satisfied must not leave the schedule looking configured.
        let (entry, told) = configurable_entry(PlaneKey::MOTION, MOTION_CADENCE_MS);
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
        // The demand is replayed every tick because shared state needs no change detection.
        // That is only free while the report stays tied to the schedule moving — a line per
        // tick would drown the very log it reports in.
        let mut sources = alloc::vec![entry(PlaneKey::MOTION, MOTION_CADENCE_MS)];
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
        // The other half: "unchanged" is a comparison against the schedule in force, not
        // against the previous demand, so a stretch followed by a relaxation reports both.
        let mut sources = alloc::vec![entry(PlaneKey::MOTION, MOTION_CADENCE_MS)];

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
        let mut sources = alloc::vec![entry(PlaneKey::MOTION, MOTION_CADENCE_MS)];

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
        let mut sources = alloc::vec![entry(PlaneKey::MOTION, MOTION_CADENCE_MS)];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);
        apply(&mut sources, &demand_on_period(0), &configurator(), 0);

        assert_eq!(
            sources[0].cadence_ms(),
            MOTION_CADENCE_MS,
            "a plane asked for no period of its own is read at the board's rate again"
        );
    }

    #[test]
    fn a_plane_the_demand_drops_is_stopped_both_as_a_schedule_and_at_the_driver() {
        // Both halves answer different questions: the schedule decides whether the loop
        // asks, and `set_collecting` is the only way a peripheral that has to *stop* hears
        // about it.
        let mut sources = alloc::vec![entry(PlaneKey::MOTION, MOTION_CADENCE_MS)];

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
    fn a_plane_whose_driver_needs_its_poll_is_not_stretched() {
        // A slow period on such a source does not make it staler, it makes its clock die.
        let mut sources = alloc::vec![entry_with(PlaneKey::AUDIO, CAPTURE_CADENCE_MS, false)];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(
            sources[0].cadence_ms(),
            CAPTURE_CADENCE_MS,
            "the board's cadence is the only one this source can take"
        );
        assert!(
            sources[0].is_active(),
            "and it stays on: this plane may be off, never rare"
        );
    }

    #[test]
    fn the_interaction_path_takes_no_plan_at_all() {
        // Under any demand: a stale button reading is a device nobody can steer off the page
        // it is on.
        let mut sources = alloc::vec![entry(PlaneKey::INTERACTION, 10)];

        for demand in [EffectiveDemand::none(), demand_on_period(30_000)] {
            apply(&mut sources, &demand, &configurator(), 0);
            assert!(sources[0].is_active(), "the input path was gated");
            assert_eq!(sources[0].cadence_ms(), 10, "the input path was stretched");
        }
    }

    /// The board's motion cadence, which the plan's floor is compared against.
    const MOTION_CADENCE_MS: u64 = iot_core::drivers::motion::MOTION_SCAN_MS;

    /// The one a driver that needs its poll refuses to give up.
    const CAPTURE_CADENCE_MS: u64 = iot_core::drivers::audio::CAPTURE_MS;
}

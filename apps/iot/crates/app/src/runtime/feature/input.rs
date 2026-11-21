use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::shared::intent::{BusinessIntent, Intent, NavigationIntent, recognize};
use crate::state::PageId;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, ReceiveReadyFuture, TryReceiveError, TrySendError};
use embassy_time::{Duration, Instant, Ticker};
use iot_core::demand::{EffectiveDemand, MAX_SINGLE_HOLD_MS, PollPlan, plan_poll};
use iot_core::drivers::input::{GestureEvent, INPUT_BASE_MS, InputEvent, PollEntry};

use iot_core::config::{Configuration, Configurator, Unreachable};

pub const INTENT_SLOTS: usize = 8;

/// Most events one source can resolve from one sample: the event the poll returns plus the
/// four the aggregator may still hold.
const EVENT_BATCH: usize = 5;

/// The ring between the workers and the coordinator, and the count of what it refused.
///
/// The count lives here because the ring is what turns an intent away: a full ring cannot
/// refuse silently, and a test sees only the drops it caused.
pub struct IntentBus {
    ring: Channel<CriticalSectionRawMutex, Intent, INTENT_SLOTS>,
    /// Refused intents since boot, saturating at [`u32::MAX`].
    dropped: AtomicU32,
}

impl IntentBus {
    pub const fn new() -> Self {
        Self {
            ring: Channel::new(),
            dropped: AtomicU32::new(0),
        }
    }

    /// Queues one intent, waiting for room: the one copy a retry cannot replace.
    pub async fn send(&self, intent: Intent) {
        self.ring.send(intent).await;
    }

    /// Queues one intent if there is room, counting the refusal when there is not.
    pub fn try_send(&self, intent: Intent) -> Result<(), TrySendError<Intent>> {
        let queued = self.ring.try_send(intent);
        if queued.is_err() {
            let _ = self
                .dropped
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                    (count < u32::MAX).then(|| count + 1)
                });
        }
        queued
    }

    pub fn try_receive(&self) -> Result<Intent, TryReceiveError> {
        self.ring.try_receive()
    }

    /// Resolves when an intent is waiting.
    pub fn ready_to_receive(
        &self,
    ) -> ReceiveReadyFuture<'_, CriticalSectionRawMutex, Intent, INTENT_SLOTS> {
        self.ring.ready_to_receive()
    }

    /// How many intents this ring refused since boot.
    pub fn dropped_count(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Default for IntentBus {
    fn default() -> Self {
        Self::new()
    }
}

const HOLD_REPORT_MS: u64 = 5_000;

/// How long the boot transient stays up, read from its route so the timer and the page cannot
/// disagree. A splash that stops being transient fails the build here rather than hanging.
const SPLASH_TIMEOUT_MS: u64 = PageId::Splash
    .timeout_ms()
    .expect("the splash route is a transient with a lifetime");

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
        plan_poll(
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
        let _ = entry.configure_for(configuration.for_kind(entry.key().kind()));
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

/// Whether this event is a transition rather than a position: a page that misses it either
/// never starts a gesture or never ends one.
fn must_arrive(event: &InputEvent) -> bool {
    matches!(
        event,
        InputEvent::Button(_)
            | InputEvent::Gesture(
                GestureEvent::Press { .. }
                    | GestureEvent::Swipe { .. }
                    | GestureEvent::Tap { .. }
                    | GestureEvent::LongPress { .. }
            )
    )
}

/// Drains every input source at the shared base tick and forwards recognized operations.
pub async fn input_task(
    intent_bus: &'static IntentBus,
    mut sources: Vec<PollEntry>,
    configurator: &Configurator,
    input: crate::runtime::workers::Input<'_>,
    second_core_peak: fn() -> Option<(usize, usize)>,
) -> ! {
    let mut ticker = Ticker::every(Duration::from_millis(INPUT_BASE_MS));
    let mut now_ms: u64 = 0;
    let mut worst_us = alloc::vec![0u64; sources.len()];
    let mut polled = alloc::vec![false; sources.len()];
    let mut window_hold_us: u64 = 0;
    let mut window_worst_hold_ms: u64 = 0;
    let mut window_ticks: u32 = 0;
    let mut window_base_dropped = intent_bus.dropped_count();
    let mut last_report = Instant::now();
    let mut stack_peak = 0usize;
    let mut splash_open = true;
    let mut plans: Vec<PollPlan> = Vec::with_capacity(sources.len());
    loop {
        ticker.next().await;
        now_ms = now_ms.wrapping_add(INPUT_BASE_MS);
        let loop_start = Instant::now();
        let decision = input
            .to
            .lock(|current| decide(&sources, &current.get(), configurator, &mut plans));
        apply_decision(&mut sources, decision, &plans, now_ms);
        for (index, entry) in sources.iter_mut().enumerate() {
            if !entry.is_active() || entry.next_at_ms() > now_ms {
                continue;
            }
            let poll_start = Instant::now();
            // One sample can resolve more than one gesture: two fingers move together, so
            // both drags belong to this tick rather than to the next one.
            let mut batch = [None; EVENT_BATCH];
            let mut len = 0;
            entry.poll_all(now_ms, &mut |event| {
                if let Some(slot) = batch.get_mut(len) {
                    *slot = Some(event);
                    len += 1;
                }
            });
            for event in batch.into_iter().flatten() {
                let intent = Intent::Input(recognize(event, entry.source_id()));
                // A press or a lift is a transition the page cannot reconstruct from a later
                // sample, so it waits for room; a drag or a raw sample is superseded by the
                // next poll and may be refused.
                if must_arrive(&event) {
                    intent_bus.send(intent).await;
                } else {
                    let _ = intent_bus.try_send(intent);
                }
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
            && now_ms >= SPLASH_TIMEOUT_MS
            && !sources.iter().any(|entry| entry.held())
            && intent_bus
                .try_send(Intent::Business(BusinessIntent::Navigation(
                    NavigationIntent::SplashOver,
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
            let window_dropped = intent_bus.dropped_count() - window_base_dropped;
            log::log!(
                if over_bound {
                    log::Level::Warn
                } else {
                    log::Level::Debug
                },
                "[INPUT] held the executor {} ms of {} ms over {} ticks; worst single hold {} ms (bound {} ms); worst source: {}; dropped {} intents",
                window_hold_us / 1_000,
                window_ms,
                window_ticks,
                window_worst_hold_ms,
                MAX_SINGLE_HOLD_MS,
                parts,
                window_dropped
            );
            window_hold_us = 0;
            window_worst_hold_ms = 0;
            window_ticks = 0;
            window_base_dropped = intent_bus.dropped_count();
            worst_us.fill(0);
            polled.fill(false);
            last_report = now;
            report_second_core_stack(&mut stack_peak, second_core_peak);
        }
    }
}

/// One line per window, at info only when the peak moved a kilobyte or more: the number is
/// monotone, so repeating it at debug is enough and an info line means "look at the budget".
fn report_second_core_stack(peak: &mut usize, second_core_peak: fn() -> Option<(usize, usize)>) {
    let Some((high, total)) = second_core_peak() else {
        return;
    };
    let grew = high >= *peak + 1_024;
    log::log!(
        if grew {
            log::Level::Info
        } else {
            log::Level::Debug
        },
        "[STACK] core 1 peak {high} B of {total} B",
    );
    *peak = high;
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use core::cell::RefCell;
    use embassy_sync::blocking_mutex::Mutex;
    use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
    use iot_core::config::MotionConfig;
    use iot_core::demand::{Claim, DemandKey, EffectiveDemand, Mode, Priority};
    use iot_core::drivers::input::{InputSource, PassThrough};

    /// The shared cell a double needs now that `InputSource` carries `Send`: an `Rc`/`RefCell`
    /// pair would make the stand-in ineligible for the same bundle as a real driver.
    type Shared<T> = Arc<Mutex<CriticalSectionRawMutex, RefCell<Option<T>>>>;

    struct Recording {
        collecting: Option<bool>,
        polls: u32,
        batchable: bool,
        told: Shared<MotionConfig>,
    }

    impl Recording {
        fn new(batchable: bool) -> Self {
            Self::with_tell_to(batchable, Arc::new(Mutex::new(RefCell::new(None))))
        }

        fn told_to(told: &Shared<MotionConfig>) -> Self {
            Self::with_tell_to(true, Arc::clone(told))
        }

        fn with_tell_to(batchable: bool, told: Shared<MotionConfig>) -> Self {
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
            self.told.lock(|told| *told.borrow_mut() = wanted);
            Ok(wanted)
        }
    }

    fn configurable_entry(key: DemandKey, cadence_ms: u64) -> (PollEntry, Shared<MotionConfig>) {
        let told = Arc::new(Mutex::new(RefCell::new(None)));
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
                Claim::new(Mode::Continuous, period_ms, Priority::Secondary),
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
            told.lock(|told| *told.borrow()),
            Some(MotionConfig::PolledRegisters),
            "a configuration that never reaches the part has configured nothing"
        );
    }

    #[test]
    fn a_device_with_no_configuration_is_not_told_about_one() {
        let (entry, told) = configurable_entry(DemandKey::AUDIO, CAPTURE_CADENCE_MS);
        let mut sources = alloc::vec![entry];

        apply(&mut sources, &demand_on_period(30_000), &configurator(), 0);

        assert_eq!(told.lock(|told| *told.borrow()), None);
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
            told.lock(|told| *told.borrow()),
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

    #[test]
    fn a_full_ring_counts_what_it_refuses_and_the_count_saturates() {
        let bus = IntentBus::new();
        for _ in 0..INTENT_SLOTS {
            assert!(
                bus.try_send(Intent::Business(BusinessIntent::Navigation(
                    NavigationIntent::NextPage
                )))
                .is_ok(),
                "the empty ring takes a full pass"
            );
        }

        assert!(
            bus.try_send(Intent::Business(BusinessIntent::Navigation(
                NavigationIntent::NextPage
            )))
            .is_err(),
            "the full ring refuses the next one"
        );
        assert_eq!(
            bus.dropped_count(),
            1,
            "a refused intent is counted, not lost silently"
        );

        bus.dropped.store(u32::MAX, Ordering::Relaxed);
        let _ = bus.try_send(Intent::Business(BusinessIntent::Navigation(
            NavigationIntent::NextPage,
        )));
        assert_eq!(
            bus.dropped_count(),
            u32::MAX,
            "a count that reached the ceiling stays there instead of wrapping to zero"
        );
    }

    #[test]
    fn a_transition_waits_for_room_while_a_position_may_be_refused() {
        use iot_core::drivers::input::{
            ButtonEvent, SwipeDirection, TouchEvent, TouchPoint, TouchStatus,
        };

        for event in [
            InputEvent::Button(ButtonEvent::Click),
            InputEvent::Gesture(GestureEvent::Press { id: 0, x: 1, y: 2 }),
            InputEvent::Gesture(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Up,
                x: 0,
                y: 0,
                end_x: 0,
                end_y: 60,
                held_ms: 100,
                distance_px: 60,
                at_ms: 100,
            }),
            InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x: 0,
                y: 0,
                held_ms: 10,
                at_ms: 10,
            }),
            InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x: 0,
                y: 0,
                held_ms: 600,
            }),
        ] {
            assert!(must_arrive(&event), "{event:?} may not be dropped");
        }

        let touch = TouchEvent {
            points: [TouchPoint {
                id: 0,
                x: 0,
                y: 0,
                status: TouchStatus::Contact,
            }; iot_core::drivers::input::MAX_TOUCH_POINTS],
            len: 1,
        };
        for event in [
            InputEvent::Touch(touch),
            InputEvent::Gesture(GestureEvent::Drag {
                id: 0,
                x: 0,
                y: 0,
                at_ms: 10,
            }),
            InputEvent::Gesture(GestureEvent::Ghost),
            InputEvent::ChipGesture(0x10),
        ] {
            assert!(
                !must_arrive(&event),
                "{event:?} is superseded by the next poll"
            );
        }
    }

    const MOTION_CADENCE_MS: u64 = iot_core::drivers::motion::MOTION_SCAN_MS;

    const CAPTURE_CADENCE_MS: u64 = iot_core::drivers::audio::CAPTURE_MS;

    /// A full ring delays the hand-over rather than refusing it: a panel question is asked once,
    /// and a refusal would leave it unanswered.
    #[test]
    fn a_full_ring_delays_a_send_instead_of_losing_it() {
        let bus = IntentBus::new();
        for _ in 0..INTENT_SLOTS {
            assert!(
                bus.try_send(Intent::Business(BusinessIntent::Navigation(
                    NavigationIntent::NextPage
                )))
                .is_ok(),
                "the ring takes a full pass"
            );
        }

        let mut waiting = Box::pin(bus.send(Intent::Business(BusinessIntent::Navigation(
            NavigationIntent::NextPage,
        ))));
        assert!(
            poll_once(waiting.as_mut()).is_pending(),
            "a full ring must wait, not refuse"
        );

        assert!(bus.try_receive().is_ok(), "the ring is still full");
        assert!(
            poll_once(waiting.as_mut()).is_ready(),
            "the freed slot takes the waiting intent"
        );
        assert!(
            bus.try_receive().is_ok(),
            "and the intent is the one that was waiting"
        );
    }

    fn poll_once<F: core::future::Future + ?Sized>(
        future: core::pin::Pin<&mut F>,
    ) -> core::task::Poll<F::Output> {
        let waker = core::task::Waker::noop();
        future.poll(&mut core::task::Context::from_waker(waker))
    }
}

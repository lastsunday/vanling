use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Ticker};
use iot_core::demand::{DemandField, EffectiveDemand, InputLimiter, MAX_SINGLE_HOLD_MS};
use iot_core::drivers::audio::{AudioSample, CaptureSink};
use iot_core::drivers::input::{INPUT_BASE_MS, PollEntry};
use iot_core::intent::{BusinessIntent, Intent, recognize};

use crate::SharedDemand;
use iot_core::config::{Configuration, Configurator};

/// Slots on the intent ring.
///
/// Sized for the events whose order matters: three sources can report on one tick, and the
/// render loop drains what it finds. A slot costs the widest message the ring can carry, so
/// nothing that is a window rather than an event belongs on it — see [`CapturePlane`].
pub const INTENT_SLOTS: usize = 8;

/// Intent channel: input task writes recognized operations, the render loop
/// drains and interprets.
pub type IntentBus = Channel<CriticalSectionRawMutex, Intent, INTENT_SLOTS>;

/// Intent bus storage; passed as `&'static` into the tasks so no consumer
/// reaches the global by name.
pub static INTENT_BUS: IntentBus = IntentBus::new();

/// The capture's data plane: the newest poll's window, and the wake that says there is one.
///
/// One slot, rather than a queue, and off the intent ring rather than on it. A capture poll is
/// a window — only its newest value means anything to a scope, so a stale one is worth nothing
/// — and it is a kilobyte and a half wide, where every slot of the ring costs the widest
/// message that ring can carry. Carrying polls there would therefore price the ring at eight
/// windows for the one the state draws from. The board is handed the plane by
/// [`take_audio`](iot_core::drivers::board::HasAudio::take_audio) and publishes into it, the
/// render loop takes from it, and the state keeps what it took.
pub struct CapturePlane(Channel<CriticalSectionRawMutex, AudioSample, 1>);

impl CapturePlane {
    /// Take the window the input task published, or `None` when nothing has arrived since the
    /// last take.
    pub fn take(&self) -> Option<AudioSample> {
        self.0.try_receive().ok()
    }

    /// Ready when a window is waiting. The render loop parks on this, so the wake does not
    /// consume the window — the take at the top of the next pass does.
    pub fn ready(&self) -> impl Future<Output = ()> {
        self.0.ready_to_receive()
    }
}

impl CaptureSink for CapturePlane {
    fn publish(&self, sample: AudioSample) {
        // Refused when the render loop has not taken the last window, and nothing is lost by
        // it: the envelope is the driver's rolling one, so the next poll re-sends what this one
        // held plus the columns it added. The panel runs one poll behind at worst.
        let _ = self.0.try_send(sample);
    }
}

/// Capture plane storage; the board publishes into it and the render loop draws from it.
pub static CAPTURE: CapturePlane = CapturePlane(Channel::new());

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
        // The source rather than the plane: two sources share the interaction plane, and a
        // report naming the plane twice (`input on 10 ms, input on 10 ms`) could not say
        // which of them stopped. The source is also the unit the schedule applies to. A
        // source that is off has no cadence in force, so it prints none.
        let separator = if index == 0 { "" } else { ", " };
        let name = SOURCE_NAMES.get(index).copied().unwrap_or("?");
        if plan.active {
            let _ = write!(report, "{separator}{name} on {} ms", plan.cadence_ms);
        } else {
            let _ = write!(report, "{separator}{name} off");
        }
    }
    let motion_config = match configuration.motion {
        Some(config) => format!("{config:?}"),
        None => String::from("none"),
    };
    if moved {
        log::info!("[DEMAND] schedule: {report} | motion config {motion_config}");
    }
    moved
}

/// How long the splash page stays up after the input clock starts.
///
/// Long enough for the finger that just let go of reset to reach the button, short enough that a
/// device nobody asked to configure lands on its page quickly: the page is a splash, not a menu.
/// A hold that begins inside the window keeps the page up until it is let go, so the window is
/// "begin the hold within this", not "finish it within this".
///
/// Measured from this task's clock, which is the one the button's own events are clocked against:
/// the page's first reader is this loop, so "since boot" and "since the first tick" are the same
/// window to a finger.
const SPLASH_MS: u64 = 1_500;

/// Drains every input source at the shared base tick, gating each source to
/// its own cadence, and forwards recognized operations onto the intent bus.
/// `try_send` drops intents when the bus is full: dropping a press is preferable
/// to blocking the scan loop, which would skew debounce timing.
pub async fn input_task(
    intent_bus: &'static IntentBus,
    mut sources: Vec<PollEntry>,
    configurator: &Configurator,
    demand: &SharedDemand,
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
    // Whether the splash page is still up, so the one report that ends it is sent once. The page
    // itself is the state's (see [`iot_core::state::DisplayPage::Splash`]); what this task owns is
    // its clock — this is the task that has one, and the hold the page answers is its own event.
    let mut splash_open = true;
    loop {
        // Ticks stay anchored to the base clock even when a source takes
        // inconsistent time to sample, so slow devices never alias.
        ticker.next().await;
        now_ms = now_ms.wrapping_add(INPUT_BASE_MS);
        let loop_start = Instant::now();
        demand.lock(|current| apply(&mut sources, &current.borrow(), configurator, now_ms));
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
        // The splash is over `SPLASH_MS` after the clock starts — or as soon as the press in
        // progress is let go, if one is. Deferring on a held button is what keeps a hold that began
        // inside the window (press at 2.9 s, lift at 3.6 s) a press on the splash page: this check
        // runs after the poll above, so the event lands before the report that ends the page.
        if splash_open
            && now_ms >= SPLASH_MS
            && !sources.iter().any(|entry| entry.held())
            && intent_bus
                .try_send(Intent::Business(BusinessIntent::SplashOver))
                .is_ok()
        {
            splash_open = false;
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
            // A healthy window is telemetry; a hold past the bound is the fault this report
            // exists for and the only shape worth waking a reader with.
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

    #[test]
    fn a_capture_window_costs_one_slot_beside_the_ring() {
        // What a ring costs is its slot count times its widest message, so a poll's 1.7 KB
        // window on the intent ring would have set every one of its slots to that width — the
        // memory the capture plane exists to keep off it. These are the two sides of that
        // trade, and both fail if a window moves back onto the ring.
        let ring = core::mem::size_of::<Intent>() * INTENT_SLOTS;
        let window = core::mem::size_of::<AudioSample>();
        assert!(
            ring < window,
            "a capture window is sizing the intent ring again: {ring} B of ring against a \
             {window} B window"
        );
        assert!(
            core::mem::size_of::<CapturePlane>() < 2 * window,
            "the capture plane holds one window, not a queue of them"
        );
    }

    /// The board's motion cadence, which the plan's floor is compared against.
    const MOTION_CADENCE_MS: u64 = iot_core::drivers::motion::MOTION_SCAN_MS;

    /// The one a driver that needs its poll refuses to give up.
    const CAPTURE_CADENCE_MS: u64 = iot_core::drivers::audio::CAPTURE_MS;
}

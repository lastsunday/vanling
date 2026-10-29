//! The motion demand matrix, in its own module so the board that wires the IMU gates it
//! once here. The playback probe alongside it is board-agnostic and must keep compiling
//! on a board with no IMU.

use embassy_time::{Duration, Instant, Timer};
use iot_bsp_esp::components::qmi8658::{Qmi8658, SUPPORTED_CONFIGS};
use iot_core::config::MotionConfig;
use iot_core::demand::{Claim, DemandKey, EffectiveDemand, Mode, Sensitivity, poll_plan_of};
use iot_core::drivers::motion::{MOTION_SCAN_MS, MotionSource};

pub type MotionDevice = iot_bsp_esp::SharedI2cDevice;

const BOARD_CADENCE_MS: u64 = MOTION_SCAN_MS;

/// Polls discarded after a configuration write, enough for the engine windows the driver just
/// programmed to fill once.
const SETTLE_POLLS: u32 = 50;

/// Polls measured in the reference cell.
const MEASURE_POLLS: u32 = 200;

/// Wall clock one cell may take. The whole matrix has to finish inside a coffee break, so
/// a cadence is measured rather than derived only when enough of it fits.
const CELL_BUDGET_MS: u64 = 20_000;

/// Fewest polls a cell is measured over; below this the per-poll figure is a timer
/// artefact rather than a measurement.
const MIN_CELL_POLLS: u32 = 20;

const CLASS: Sensitivity = Sensitivity::Secondary;

/// The configurations in order, so the log reads low to high and shows the ones with no
/// encoding between the ones that have one.
const CONFIGS: [MotionConfig; 4] = [
    MotionConfig::WakeDetector,
    MotionConfig::PolledRegisters,
    MotionConfig::Fifo,
    MotionConfig::FilteredRegisters,
];

/// The cadences measured, slowest first. The three are the ones this probe has always
/// walked, so the rows stay comparable with what it already recorded.
const CADENCES_MS: [u64; 3] = [30_000, 200, BOARD_CADENCE_MS];

/// One cadence's cost, measured over the polls that ran at it. Per second is a measurement
/// wherever the cadence allows one, because "a cadence changes how often this happens and
/// not what one of them costs" is an assumption a probe exists to test.
#[derive(Default)]
struct Cell {
    polls: u32,
    transactions: u32,
    occupied_us: u64,
    /// What turns a count of polls into a rate at a stretched cadence.
    window_us: u64,
}

impl Cell {
    /// Wall time one poll occupies the executor.
    fn microseconds(&self) -> u64 {
        self.occupied_us / u64::from(self.polls.max(1))
    }

    fn transactions(&self) -> u32 {
        self.transactions / self.polls.max(1)
    }

    /// Measured occupancy as a rate. The window rather than the poll count, so this stays
    /// right at a cadence whose polls do not land exactly on schedule.
    fn per_second(&self) -> u64 {
        self.occupied_us * 1_000_000 / self.window_us.max(1)
    }

    fn transactions_per_second(&self) -> u64 {
        u64::from(self.transactions) * 1_000_000 / self.window_us.max(1)
    }

    /// The same per-poll cost at a cadence this cell did not run at. The log says when it uses
    /// this: a derived figure is a claim, not a reading.
    fn derived_per_second(&self, cadence_ms: u64) -> u64 {
        self.microseconds() * 1_000 / cadence_ms.max(1)
    }

    fn derived_transactions_per_second(&self, cadence_ms: u64) -> u64 {
        u64::from(self.transactions()) * 1_000 / cadence_ms.max(1)
    }
}

/// How many polls a cadence can be measured over inside [`CELL_BUDGET_MS`], or `None` when
/// measuring it would take minutes.
fn affordable_polls(cadence_ms: u64) -> Option<u32> {
    let polls = (CELL_BUDGET_MS / cadence_ms.max(1)).min(u32::MAX as u64) as u32;
    if polls >= MIN_CELL_POLLS {
        Some(polls.min(MEASURE_POLLS))
    } else {
        None
    }
}

/// Walks the (configuration × cadence) matrix and reports what each cell costs.
///
/// Each cell is polled at its own cadence where the window allows one, and the log says which
/// cells were measured and which were scaled. That distinction is the point: the cost table
/// turns a per-poll cost into a rate by dividing by a cadence, which only holds if a cadence
/// changes how often a poll happens and not what one costs. 200 ms is measured directly, so
/// the assumption is tested; 30 s would take minutes per cell and is reported as derived.
///
/// **Bus transactions** come from the driver's own counter, so changing its sequence moves the
/// number. **Executor occupancy** is the wall time inside the poll.
///
/// It cannot report current, and configurations with no register encoding are printed
/// unreachable rather than skipped, so the log says what was not measured as loudly as what
/// was.
pub async fn run(mut driver: Qmi8658<MotionDevice>) -> ! {
    log::info!(
        "[PROBE] motion demand matrix: configurations {CONFIGS:?}, driver configures {SUPPORTED_CONFIGS:?}"
    );
    log::info!("[PROBE] current is not reported: it needs a meter across the board");
    log::info!(
        "[PROBE] each cell is polled at its own cadence where {CELL_BUDGET_MS} ms of window \
         allows; anything longer is reported as derived"
    );

    // The driver's `now_ms` is boot-relative and differenced from internally, while
    // `Instant::elapsed` measures from the value it is called on — microseconds for a fresh
    // `Instant::now()`.
    let boot = Instant::now();
    let mut delay = esp_hal::delay::Delay::new();
    for config in CONFIGS {
        // `enter` rather than `set_config`: the probe exists to measure what the product does,
        // and the product switches configuration through `enter`, which does not pay for a
        // soft reset a configuration change does not need. Measuring the reset path instead
        // would price a cell the product never runs.
        if let Err(refused) = driver.enter(config, &mut delay) {
            for cadence_ms in CADENCES_MS {
                log::warn!(
                    "[DEMAND] {config:?}/{cadence_ms}ms unreachable: {refused:?} — no register \
                     encoding for it, so there is nothing to measure"
                );
            }
            continue;
        }

        // Settled before measured: the first window after a configuration write is the one
        // where the engine windows it just programmed have not filled once.
        discard(&mut driver, boot, BOARD_CADENCE_MS, SETTLE_POLLS).await;

        // The reference every derived cell scales from.
        let mut reference = Cell::default();
        measure(
            &mut driver,
            boot,
            BOARD_CADENCE_MS,
            &mut reference,
            MEASURE_POLLS,
        )
        .await;

        for requested_ms in CADENCES_MS {
            let mut demand = EffectiveDemand::none();
            demand.set(
                DemandKey::MOTION,
                Claim::new(Mode::Continuous, requested_ms as u32, CLASS),
            );
            let plan = poll_plan_of(DemandKey::MOTION, &demand, BOARD_CADENCE_MS);
            let cadence_ms = plan.cadence_ms;
            match affordable_polls(cadence_ms) {
                Some(polls) if cadence_ms != BOARD_CADENCE_MS => {
                    let mut stretched = Cell::default();
                    measure(&mut driver, boot, cadence_ms, &mut stretched, polls).await;
                    log::info!(
                        "[DEMAND] {config:?}/{cadence_ms}ms cadence {cadence_ms} ms: MEASURED \
                         over {} polls in {} ms, {} us/s of executor ({:.2}%), {} txn/s ({} per \
                         poll), per poll {} us against the board cadence's {}, now {:?}",
                        stretched.polls,
                        stretched.window_us / 1_000,
                        stretched.per_second(),
                        stretched.per_second() as f32 / 10_000.0,
                        stretched.transactions_per_second(),
                        stretched.transactions(),
                        stretched.microseconds(),
                        reference.microseconds(),
                        driver.config(),
                    );
                }
                Some(_) => log::info!(
                    "[DEMAND] {config:?}/{cadence_ms}ms cadence {cadence_ms} ms: MEASURED over {} \
                     polls, {} us/s of executor ({:.2}%), {} txn/s ({} per poll), now {:?}",
                    reference.polls,
                    reference.per_second(),
                    reference.per_second() as f32 / 10_000.0,
                    reference.transactions_per_second(),
                    reference.transactions(),
                    driver.config(),
                ),
                None => log::info!(
                    "[DEMAND] {config:?}/{cadence_ms}ms cadence {cadence_ms} ms: DERIVED from the \
                     board cadence — a cell this long would need minutes, so {} us/s of executor \
                     ({:.2}%) and {} txn/s ({} per poll) are the measured per-poll cost scaled, \
                     not a reading at this cadence",
                    reference.derived_per_second(cadence_ms),
                    reference.derived_per_second(cadence_ms) as f32 / 10_000.0,
                    reference.derived_transactions_per_second(cadence_ms),
                    reference.transactions(),
                ),
            }
        }
    }

    log::info!("[PROBE] matrix walked; the rows are the cost table's measured columns");
    loop {
        Timer::after(Duration::from_secs(10)).await;
    }
}

/// Polls at a cadence, throwing the numbers away.
async fn discard(driver: &mut Qmi8658<MotionDevice>, boot: Instant, cadence_ms: u64, polls: u32) {
    let mut due = Instant::now();
    for _ in 0..polls {
        Timer::at(due).await;
        let _ = driver.sample(now_ms(boot));
        due += Duration::from_millis(cadence_ms);
    }
}

/// Polls at a cadence and accumulates what each one cost. Occupancy is measured off the poll
/// start, which is what the shared executor feels; `now_ms` comes off `boot`.
async fn measure(
    driver: &mut Qmi8658<MotionDevice>,
    boot: Instant,
    cadence_ms: u64,
    cell: &mut Cell,
    polls: u32,
) {
    let mut due = Instant::now();
    let mut last_transactions = driver.transactions();
    let window_start = Instant::now();

    for _ in 0..polls {
        Timer::at(due).await;
        let poll_start = Instant::now();
        let _ = driver.sample(now_ms(boot));
        cell.occupied_us += poll_start.elapsed().as_micros();
        cell.polls = cell.polls.saturating_add(1);
        let after = driver.transactions();
        cell.transactions = cell.transactions.saturating_add(after - last_transactions);
        last_transactions = after;
        due += Duration::from_millis(cadence_ms);
    }
    cell.window_us = Instant::now()
        .saturating_duration_since(window_start)
        .as_micros();
}

/// Milliseconds since the anchor. Saturating: an instant taken before it is not a fault
/// worth unwinding a probe over.
fn now_ms(boot: Instant) -> u64 {
    Instant::now().saturating_duration_since(boot).as_millis()
}

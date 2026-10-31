//! The motion demand matrix, in its own module so the board that wires the IMU gates it once.

use embassy_time::{Duration, Instant, Timer};
use iot_bsp_esp::components::qmi8658::{Qmi8658, SUPPORTED_CONFIGS};
use iot_core::config::MotionConfig;
use iot_core::demand::{Claim, DemandKey, EffectiveDemand, Mode, Priority, poll_plan_of};
use iot_core::drivers::motion::{MOTION_SCAN_MS, MotionSource};

pub type MotionDevice = iot_bsp_esp::SharedI2cDevice;

const BOARD_CADENCE_MS: u64 = MOTION_SCAN_MS;

const SETTLE_POLLS: u32 = 50;

const MEASURE_POLLS: u32 = 200;

const CELL_BUDGET_MS: u64 = 20_000;

const MIN_CELL_POLLS: u32 = 20;

const PRIORITY: Priority = Priority::Secondary;

const CONFIGS: [MotionConfig; 4] = [
    MotionConfig::WakeDetector,
    MotionConfig::PolledRegisters,
    MotionConfig::Fifo,
    MotionConfig::FilteredRegisters,
];

const CADENCES_MS: [u64; 3] = [30_000, 200, BOARD_CADENCE_MS];

#[derive(Default)]
struct Cell {
    polls: u32,
    transactions: u32,
    occupied_us: u64,
    window_us: u64,
}

impl Cell {
    fn microseconds(&self) -> u64 {
        self.occupied_us / u64::from(self.polls.max(1))
    }

    fn transactions(&self) -> u32 {
        self.transactions / self.polls.max(1)
    }

    fn per_second(&self) -> u64 {
        self.occupied_us * 1_000_000 / self.window_us.max(1)
    }

    fn transactions_per_second(&self) -> u64 {
        u64::from(self.transactions) * 1_000_000 / self.window_us.max(1)
    }

    fn derived_per_second(&self, cadence_ms: u64) -> u64 {
        self.microseconds() * 1_000 / cadence_ms.max(1)
    }

    fn derived_transactions_per_second(&self, cadence_ms: u64) -> u64 {
        u64::from(self.transactions()) * 1_000 / cadence_ms.max(1)
    }
}

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
/// 200 ms is measured, so the per-poll-cost × cadence assumption is tested; 30 s would take
/// minutes and is reported as derived. Occupancy is wall time inside the poll, transactions the
/// driver's own counter; a configuration with no register encoding is printed unreachable.
pub async fn run(mut driver: Qmi8658<MotionDevice>) -> ! {
    log::info!(
        "[PROBE] motion demand matrix: configurations {CONFIGS:?}, driver configures {SUPPORTED_CONFIGS:?}"
    );
    log::info!("[PROBE] current is not reported: it needs a meter across the board");
    log::info!(
        "[PROBE] each cell is polled at its own cadence where {CELL_BUDGET_MS} ms of window \
         allows; anything longer is reported as derived"
    );

    let boot = Instant::now();
    let mut delay = esp_hal::delay::Delay::new();
    for config in CONFIGS {
        if let Err(refused) = driver.enter(config, &mut delay) {
            for cadence_ms in CADENCES_MS {
                log::warn!(
                    "[DEMAND] {config:?}/{cadence_ms}ms unreachable: {refused:?} — no register \
                     encoding for it, so there is nothing to measure"
                );
            }
            continue;
        }

        discard(&mut driver, boot, BOARD_CADENCE_MS, SETTLE_POLLS).await;

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
                Claim::new(Mode::Continuous, requested_ms as u32, PRIORITY),
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

async fn discard(driver: &mut Qmi8658<MotionDevice>, boot: Instant, cadence_ms: u64, polls: u32) {
    let mut due = Instant::now();
    for _ in 0..polls {
        Timer::at(due).await;
        let _ = driver.sample(now_ms(boot));
        due += Duration::from_millis(cadence_ms);
    }
}

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

fn now_ms(boot: Instant) -> u64 {
    Instant::now().saturating_duration_since(boot).as_millis()
}

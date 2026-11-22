//! The liveness watchdog: the one reset a hung executor cannot suppress.
//!
//! esp-hal's `init` disables the RWDT, so this arms it: a boot-sized timeout before board
//! bring-up, then the product timeout once the coordinator can feed it. The feeder is a task
//! on that executor, so anything that stops yielding also stops feeding.
//!
//! `ResetCore`, not `ResetSystem`: the RTC domain holds [`crate::boot`]'s crash count, and
//! both this and the panic path must leave it intact for safe mode.

use core::cell::RefCell;

use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::RTC_TIMER;
use esp_hal::rtc_cntl::{Rtc, Rwdt, RwdtStage, RwdtStageAction};
use esp_hal::time::Duration as HalDuration;
use iot_core::boot::Readiness;

/// Covers the slowest legitimate blocking section, and is short enough that a hang is visible
/// before a person gives up on the panel.
const TIMEOUT_MS: u64 = 5_000;

/// Three feeds per timeout, so one late wake cannot reset a healthy device.
const FEED_INTERVAL_MS: u64 = 1_000;

/// The timeout while board bring-up owns the part: generous for probes and driver retries.
/// `start_watchdog` tightens it to [`TIMEOUT_MS`].
const BOOT_TIMEOUT_MS: u64 = 30_000;

const _: () = assert!(
    FEED_INTERVAL_MS * 3 <= TIMEOUT_MS,
    "the feeder needs room for a missed wake inside the timeout"
);

static WATCHDOG: Mutex<RefCell<Option<Rwdt>>> = Mutex::new(RefCell::new(None));

/// Arms the RWDT before board bring-up, with a timeout sized for the whole boot.
///
/// Call once after `esp_hal::init` (which disables the watchdog) and before any driver that
/// can block for its own timeout. No task feeds it yet; the timeout is the budget.
pub fn arm_boot_watchdog() {
    let mut rwdt = take_rwdt();
    configure(&mut rwdt, BOOT_TIMEOUT_MS);
    store(rwdt);
    log::info!(
        "[WATCHDOG] armed before board bring-up for {BOOT_TIMEOUT_MS} ms; tightened to \
         {TIMEOUT_MS} ms once the coordinator runs"
    );
}

/// Tightens the timeout to the product's and spawns the feeder. Call once, on the
/// coordinator's executor, after the timer driver is up.
pub fn start_watchdog(spawner: Spawner, readiness: &'static Readiness) {
    critical_section::with(|cs| {
        let mut slot = WATCHDOG.borrow_ref_mut(cs);
        let rwdt = slot.get_or_insert_with(take_rwdt);
        configure(rwdt, TIMEOUT_MS);
    });
    spawner.spawn(feed_watchdog(readiness).expect("the watchdog task fits its slot"));
    log::info!(
        "[WATCHDOG] armed for {TIMEOUT_MS} ms, fed every {FEED_INTERVAL_MS} ms from the coordinator"
    );
}

fn configure(rwdt: &mut Rwdt, timeout_ms: u64) {
    rwdt.disable();
    rwdt.set_stage_action(RwdtStage::Stage0, RwdtStageAction::ResetCore);
    rwdt.set_timeout(RwdtStage::Stage0, HalDuration::from_millis(timeout_ms));
    rwdt.enable();
}

fn take_rwdt() -> Rwdt {
    // SAFETY: `Board::new` destructures `Peripherals` with a `..` catch-all, so `RTC_TIMER` is
    // left alone; nothing else in this program names the RTC timer.
    Rtc::new(unsafe { RTC_TIMER::steal() }).rwdt
}

fn store(rwdt: Rwdt) {
    critical_section::with(|cs| {
        WATCHDOG.borrow_ref_mut(cs).replace(rwdt);
    });
}

#[embassy_executor::task]
async fn feed_watchdog(readiness: &'static Readiness) -> ! {
    let mut health_reported = false;
    loop {
        Timer::after(Duration::from_millis(FEED_INTERVAL_MS)).await;
        feed();
        if !health_reported && readiness.is_ready() {
            health_reported = true;
            crate::boot::mark_healthy();
            log::info!(
                "[WATCHDOG] the first frame was shown and the network path settled; this boot \
                 is healthy"
            );
        }
    }
}

fn feed() {
    critical_section::with(|cs| {
        if let Some(watchdog) = WATCHDOG.borrow_ref_mut(cs).as_mut() {
            watchdog.feed();
        }
    });
}

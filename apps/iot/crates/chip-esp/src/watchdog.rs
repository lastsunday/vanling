//! The liveness watchdog: the one reset a hung executor cannot suppress.
//!
//! The RWDT is disabled by esp-hal's own `init`, so this is the only place that arms it.
//! The feeder is an ordinary task on the coordinator's executor, which is exactly the
//! property wanted: a driver or a coordinator that stops yielding also stops feeding, and
//! the part resets instead of parking a dead UI in front of a person.

use core::cell::RefCell;

use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::RTC_TIMER;
use esp_hal::rtc_cntl::{Rtc, Rwdt, RwdtStage, RwdtStageAction};
use esp_hal::time::Duration as HalDuration;

/// Long enough to cover the slowest legitimate blocking section on the product path, short
/// enough that a hang is visible before a person gives up on the panel.
const TIMEOUT_MS: u64 = 5_000;

/// Three feeds per timeout, so one late or missed wake cannot reset a healthy device.
const FEED_INTERVAL_MS: u64 = 1_000;

const _: () = assert!(
    FEED_INTERVAL_MS * 3 <= TIMEOUT_MS,
    "the feeder needs room for a missed wake inside the timeout"
);

static WATCHDOG: Mutex<RefCell<Option<Rwdt>>> = Mutex::new(RefCell::new(None));

/// Arms the RWDT and spawns the task that feeds it.
///
/// Call once, on the executor that runs the coordinator, after the timer driver is up.
pub fn start_watchdog(spawner: Spawner) {
    // SAFETY: `Peripherals` is consumed by the board's `Board::new`, whose destructuring ends
    // in a `..` catch-all and therefore leaves `RTC_TIMER` alone; esp-hal's `init` only
    // reborrows it while setting the chip up. Nothing else in this program names the RTC
    // timer, so this is its sole owner for the rest of the boot.
    let mut rtc = Rtc::new(unsafe { RTC_TIMER::steal() });
    rtc.rwdt.disable();
    rtc.rwdt
        .set_stage_action(RwdtStage::Stage0, RwdtStageAction::ResetSystem);
    rtc.rwdt
        .set_timeout(RwdtStage::Stage0, HalDuration::from_millis(TIMEOUT_MS));
    rtc.rwdt.enable();
    critical_section::with(|cs| WATCHDOG.borrow_ref_mut(cs).replace(rtc.rwdt));
    spawner.spawn(feed_watchdog().expect("the watchdog task fits its slot"));
    log::info!(
        "[WATCHDOG] armed for {TIMEOUT_MS} ms, fed every {FEED_INTERVAL_MS} ms from the coordinator"
    );
}

#[embassy_executor::task]
async fn feed_watchdog() -> ! {
    loop {
        Timer::after(Duration::from_millis(FEED_INTERVAL_MS)).await;
        feed();
    }
}

fn feed() {
    critical_section::with(|cs| {
        if let Some(watchdog) = WATCHDOG.borrow_ref_mut(cs).as_mut() {
            watchdog.feed();
        }
    });
}

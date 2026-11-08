//! The liveness watchdog: the one reset a hung executor cannot suppress.
//!
//! The RWDT is disabled by esp-hal's own `init`, so this is the only place that arms it.
//! It is armed twice: once with a boot-sized timeout right after the chip is up, so a hang
//! in board bring-up still resets, and again with the product timeout once the coordinator
//! owns an executor and can feed it.
//!
//! The feeder is an ordinary task on the coordinator's executor, which is exactly the
//! property wanted: a driver or a coordinator that stops yielding also stops feeding, and
//! the part resets instead of parking a dead UI in front of a person.
//!
//! The reset action is [`RwdtStageAction::ResetCore`], not `ResetSystem`: a system reset
//! clears the RTC domain, which is where [`crate::boot_guard`] keeps the crash count, and
//! the panic path (`software_reset`) is a core reset too. Both kinds then leave the count
//! intact so a boot loop can reach safe mode.

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

/// The timeout the part runs under while board bring-up owns it: generous enough for every
/// probe and driver retry, short enough that a hang is still a reset rather than a dark
/// panel. `start_watchdog` tightens it to [`TIMEOUT_MS`].
const BOOT_TIMEOUT_MS: u64 = 30_000;

const _: () = assert!(
    FEED_INTERVAL_MS * 3 <= TIMEOUT_MS,
    "the feeder needs room for a missed wake inside the timeout"
);

static WATCHDOG: Mutex<RefCell<Option<Rwdt>>> = Mutex::new(RefCell::new(None));

/// Arms the RWDT before board bring-up, with a timeout sized for the whole boot.
///
/// Call once, after `esp_hal::init` (which disables the watchdog) and before anything that
/// can block for a driver's own timeout. No task feeds it yet; the timeout is the budget.
pub fn arm_boot_watchdog() {
    let mut rwdt = take_rwdt();
    configure(&mut rwdt, BOOT_TIMEOUT_MS);
    store(rwdt);
    log::info!(
        "[WATCHDOG] armed before board bring-up for {BOOT_TIMEOUT_MS} ms; tightened to \
         {TIMEOUT_MS} ms once the coordinator runs"
    );
}

/// Tightens the timeout to the product's and spawns the task that feeds it.
///
/// Call once, on the executor that runs the coordinator, after the timer driver is up.
pub fn start_watchdog(spawner: Spawner) {
    critical_section::with(|cs| {
        let mut slot = WATCHDOG.borrow_ref_mut(cs);
        let rwdt = slot.get_or_insert_with(take_rwdt);
        configure(rwdt, TIMEOUT_MS);
    });
    spawner.spawn(feed_watchdog().expect("the watchdog task fits its slot"));
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
    // SAFETY: `Peripherals` is consumed by the board's `Board::new`, whose destructuring ends
    // in a `..` catch-all and therefore leaves `RTC_TIMER` alone; esp-hal's `init` only
    // reborrows it while setting the chip up. Nothing else in this program names the RTC
    // timer, so this is its sole owner for the rest of the boot.
    Rtc::new(unsafe { RTC_TIMER::steal() }).rwdt
}

fn store(rwdt: Rwdt) {
    critical_section::with(|cs| {
        WATCHDOG.borrow_ref_mut(cs).replace(rwdt);
    });
}

#[embassy_executor::task]
async fn feed_watchdog() -> ! {
    let mut healthy = false;
    loop {
        Timer::after(Duration::from_millis(FEED_INTERVAL_MS)).await;
        feed();
        if !healthy && iot_core::boot::boot_ready() {
            healthy = true;
            crate::boot_guard::mark_healthy();
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

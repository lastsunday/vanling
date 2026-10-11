//! The demand state, shared by the two tasks that both need it.
//!
//! It is shared state rather than a message because a demand **is** state: idempotent, with
//! only the latest value meaning anything. Carrying it on a channel would need a one-deep
//! queue, change detection on the writer, and a retry for a refused send — three mechanisms
//! that exist only because a channel cannot hold a value. Intents are the opposite case
//! (`Intent` is an event), which is why those still travel on a bus.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use iot_core::demand::EffectiveDemand;
pub use iot_core::demand::Override;

/// The one demand state a running device has.
///
/// A `static` rather than something `run` builds, because both tasks need it and neither
/// creates it: naming it here keeps both signatures pointing at the same place without either
/// of them owning it. A `RefCell` inside the critical section because embassy-sync's blocking
/// mutex lends out `&T`, and this is the shape the repository already uses for the shared
/// I2C bus.
static CURRENT: Mutex<CriticalSectionRawMutex, RefCell<EffectiveDemand>> =
    Mutex::new(RefCell::new(EffectiveDemand::none()));

/// What the device is currently asked for.
///
/// Borrows rather than returns, because a demand is a map and copying one per tick would
/// allocate 200 times a second to hand out a value the caller only reads. The borrow is
/// inside one critical section, so a poller sees a whole demand and never a torn one.
pub fn with<R>(read: impl FnOnce(&EffectiveDemand) -> R) -> R {
    CURRENT.lock(|demand| read(&demand.borrow()))
}

/// Records what is being asked for. Called wherever the state that derives the demand lives,
/// so a page change reaches the polling loop on that loop's next tick.
pub fn propose(demand: &EffectiveDemand) {
    CURRENT.lock(|current| *current.borrow_mut() = demand.clone());
}

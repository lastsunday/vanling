//! The contract a coordinated system implements.
//!
//! A system owns one boundary: the worker handle it reads and writes, and the state that has
//! to survive a frame. The loop hands it the shared manager and the clock.

use core::any::TypeId;

use crate::state::{Command, DeviceManager};

pub(crate) trait System {
    fn tick(&mut self, manager: &mut DeviceManager, now_ms: u32);

    /// The command type this system takes off the bus. Registration builds the dispatch table
    /// from this, so the system never sees the schedule.
    fn takes(&self) -> Option<TypeId> {
        None
    }

    /// Takes the command declared by [`takes`](Self::takes), leaving one it does not own.
    fn take(&mut self, _command: &mut Option<Command>) {}

    /// Once per applied intent, in registration order: the reaction that owns no command.
    /// The read-only manager is the state its intent already wrote.
    fn intent(&mut self, _manager: &DeviceManager) {}
}

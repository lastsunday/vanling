//! The contract a coordinated system implements, and the handles one frame hands it.
//!
//! A system owns one boundary: the worker handle it reads and writes, and the state that has
//! to survive a frame. The frame hands it only what is shared.

use core::any::TypeId;

use crate::model::state::{Command, DeviceManager};
use crate::runtime::feature::input::IntentBus;

/// Everything a system may touch in one frame. The wall clock rather than the frame counter:
/// a held executor makes a logical clock replay the steps it skipped.
pub(crate) struct Frame<'a> {
    pub(crate) bus: &'a IntentBus,
    pub(crate) manager: &'a mut DeviceManager,
    pub(crate) now_ms: u32,
}

/// One slice of coordination. A system owns one boundary: the worker handle it drives, and the
/// state that must survive a frame. Systems never call each other; whatever they share arrives
/// in the [`Frame`].
pub(crate) trait System {
    fn tick(&mut self, frame: &mut Frame<'_>);

    /// The command type this system takes off the bus. Registration builds the dispatch table
    /// from this, so the system never sees the schedule.
    fn takes(&self) -> Option<TypeId> {
        None
    }

    /// Takes the command declared by [`takes`](Self::takes), leaving one it does not own.
    fn take(&mut self, _command: &mut Option<Command>) {}

    /// Once per applied intent, in registration order: the reaction that owns no command.
    /// Delivery has already happened by the time this runs.
    fn intent(&mut self, _frame: &mut Frame<'_>) {}
}

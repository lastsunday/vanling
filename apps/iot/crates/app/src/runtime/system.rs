//! The contract a coordinated system implements, and the handles one frame hands it.
//!
//! A system owns one boundary: the worker handle it reads and writes, and the state that has
//! to survive a frame. The frame hands it only what is shared.

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

    /// Once per applied intent, in registration order. `command` is what the model asked the
    /// runtime for; the system that owns that boundary takes its own and leaves the rest.
    fn intent(&mut self, _frame: &mut Frame<'_>, _command: &mut Option<Command>) {}
}

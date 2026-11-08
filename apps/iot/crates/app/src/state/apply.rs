//! The input path: write the gesture to the record first, then ask what it means against the
//! state that gesture produced.

use crate::shared::intent::Intent;
use crate::state::{Command, DeviceManager};

use super::translate::translate;

impl DeviceManager {
    /// Route one intent all the way to the state it moves, and answer with what the model
    /// cannot do itself. The input half is written to the record before it is translated, so
    /// the page's answer is read against the state that gesture produced. The caller hands
    /// that answer to the systems in registration order, and the one that owns the boundary
    /// takes it.
    pub fn apply_intent(&mut self, intent: Intent) -> Option<Command> {
        let action = match intent {
            Intent::Input(op) => {
                self.apply_input(op);
                translate(&op, self.state_ref())
            }
            Intent::Action(action) => action,
        };
        self.apply_action(action)
    }
}

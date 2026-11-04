//! The demand the page declares, into the slot the input worker and the feed interrupt read.

use super::{Frame, System};

pub(crate) struct DemandSystem;

impl System for DemandSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        frame.workers.input.publish(frame.manager.demand());
    }
}

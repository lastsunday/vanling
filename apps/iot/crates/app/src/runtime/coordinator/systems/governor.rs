//! The arbiter between what a page asks for and what the parts can run, and the one warning
//! when the two disagree.

use iot_core::demand::{ClockGovernor, ClockSink};

use crate::runtime::system::{Frame, System};

/// The governor is loop-local: a second `run` in one process must not answer for the first.
pub(crate) struct GovernorSystem<'a, C: ClockSink> {
    governor: ClockGovernor,
    sink: &'a mut C,
}

impl<'a, C: ClockSink> GovernorSystem<'a, C> {
    pub(crate) fn new(sink: &'a mut C) -> Self {
        Self {
            governor: ClockGovernor::new(),
            sink,
        }
    }
}

impl<C: ClockSink> System for GovernorSystem<'_, C> {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        if let Some(applied) = self.governor.apply(frame.manager.demand(), self.sink)
            && let Some(short) = applied.undercuts
        {
            log::warn!(
                "[CLOCK] a page asked for {:?}, but {} is running and needs {:?} \
                 — applying the page's choice",
                applied.asked,
                short.key,
                short.needed,
            );
        }
    }
}

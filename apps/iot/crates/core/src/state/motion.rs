//! Motion: the last sample the part handed over, and one tally per semantic. The wiring —
//! whether there is a part at all, and what it declares — is in [`Backed`], because a page
//! asks that question before it asks this one.

use crate::drivers::motion::{MotionBatch, MotionCounts, MotionSample};

/// What a wired motion part has reported since boot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MotionState {
    pub sample: Option<MotionSample>,
    /// How many times each semantic has been raised, arbitrated across the hardware engines
    /// and the core-side classifiers.
    pub counts: MotionCounts,
}

impl MotionState {
    /// One poll: the sample the page draws, and the tallies it fed.
    pub fn apply(&mut self, batch: MotionBatch) {
        self.sample = Some(batch.sample);
        for event in batch.events.iter() {
            self.counts.bump(event);
        }
    }
}

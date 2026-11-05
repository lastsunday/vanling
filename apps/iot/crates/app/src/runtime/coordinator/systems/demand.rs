//! The demand the page declares, into the slot the input worker and the feed interrupt read.

use crate::runtime::system::{Frame, System};
use crate::runtime::workers::Input;

pub(crate) struct DemandSystem<'a> {
    input: Input<'a>,
}

impl<'a> DemandSystem<'a> {
    pub(crate) fn new(input: Input<'a>) -> Self {
        Self { input }
    }
}

impl System for DemandSystem<'_> {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        self.input.publish(frame.manager.demand());
    }
}

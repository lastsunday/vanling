//! The panel replay that stands in for a finger. `BENCH` is a build-time constant, so a
//! product build folds the whole system away instead of paying a call per frame.

use alloc::boxed::Box;

use crate::bench::replay::{BENCH, PanelBench};

use super::{Frame, System};

pub(crate) struct BenchSystem {
    bench: Option<Box<PanelBench>>,
}

impl BenchSystem {
    pub(crate) fn new() -> Self {
        Self {
            bench: BENCH.then(|| Box::new(PanelBench::new())),
        }
    }
}

impl System for BenchSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        if let Some(bench) = self.bench.as_mut() {
            bench.step(frame.manager, frame.bus);
        }
    }
}

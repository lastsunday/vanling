pub mod recorder;

use async_trait::async_trait;
use service::chobits::frame::{Frame, OutputMessage};

pub struct FilterCtx {
    pub session_id: String,
}

pub enum FilterAction<T> {
    Continue(T),
    Consumed,
    Break,
}

#[async_trait]
pub trait InputFilter: Send + Sync {
    async fn process(&self, ctx: &FilterCtx, frame: Frame) -> FilterAction<Frame>;
}

#[async_trait]
pub trait OutputFilter: Send + Sync {
    async fn process(&self, ctx: &FilterCtx, msg: OutputMessage) -> FilterAction<OutputMessage>;
}

pub(crate) enum FilterStep<T> {
    Pass(T),
    Skip,
    Abort,
}

pub(crate) async fn run_input_filters(
    filters: &[Box<dyn InputFilter>],
    ctx: &FilterCtx,
    frame: Frame,
) -> FilterStep<Frame> {
    let mut frame = frame;
    for filter in filters {
        match filter.process(ctx, frame).await {
            FilterAction::Continue(f) => frame = f,
            FilterAction::Consumed => return FilterStep::Skip,
            FilterAction::Break => return FilterStep::Abort,
        }
    }
    FilterStep::Pass(frame)
}

pub(crate) async fn run_output_filters(
    filters: &[Box<dyn OutputFilter>],
    ctx: &FilterCtx,
    msg: OutputMessage,
) -> FilterStep<OutputMessage> {
    let mut msg = msg;
    for filter in filters {
        match filter.process(ctx, msg).await {
            FilterAction::Continue(m) => msg = m,
            FilterAction::Consumed => return FilterStep::Skip,
            FilterAction::Break => return FilterStep::Abort,
        }
    }
    FilterStep::Pass(msg)
}

pub use recorder::*;

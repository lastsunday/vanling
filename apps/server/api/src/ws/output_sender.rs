use async_trait::async_trait;

use service::ws::frame::FrameResult;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::ws::session::round::OutputMessage;

#[async_trait]
pub trait OutputSender: Send {
    async fn recv(&mut self) -> Option<FrameResult>;
}

#[async_trait]
impl OutputSender for UnboundedReceiver<OutputMessage> {
    async fn recv(&mut self) -> Option<FrameResult> {
        Some(self.recv().await?.payload)
    }
}

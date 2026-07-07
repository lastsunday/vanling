use async_trait::async_trait;

use service::chobits::frame::{FrameResult, OutputMessage};
use tokio::sync::mpsc::UnboundedReceiver;

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

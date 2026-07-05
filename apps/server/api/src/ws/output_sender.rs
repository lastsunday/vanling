use async_trait::async_trait;
use axum::extract::ws::Message;
use futures_util::Sink;
use tokio::sync::mpsc::UnboundedReceiver;

use super::write_payload_to_ws;
use crate::ws::session::round::OutputMessage;

#[async_trait]
pub trait OutputSender<W: Sink<Message> + Unpin + Send>: Send {
    async fn recv(&mut self) -> Option<OutputMessage>;
    async fn write(&mut self, write: &mut W, msg: OutputMessage) -> bool;
}

#[async_trait]
impl<W: Sink<Message> + Unpin + Send> OutputSender<W> for UnboundedReceiver<OutputMessage> {
    async fn recv(&mut self) -> Option<OutputMessage> {
        self.recv().await
    }

    async fn write(&mut self, write: &mut W, msg: OutputMessage) -> bool {
        write_payload_to_ws(&msg.payload, write).await
    }
}

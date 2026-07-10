use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use service::chobits::tts::{Tts, TtsPacket};
use std::pin::Pin;
use tokio::sync::mpsc::channel;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

pub struct TtsMute {}

impl TtsMute {
    pub async fn new() -> Result<Self, anyhow::Error> {
        Ok(Self {})
    }
}

#[async_trait]
impl Tts for TtsMute {
    async fn stream(
        &self,
        mut text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>> {
        let (tx, rx) = channel(10);
        tokio::spawn(async move {
            while let Some(text) = text_stream.next().await {
                if cancel.is_cancelled() {
                    break;
                }
                let packet = TtsPacket {
                    audio: vec![],
                    text,
                };
                if tx.send(Ok(packet)).await.is_err() {
                    break;
                }
            }
        });
        Box::pin(ReceiverStream::new(rx))
    }
}

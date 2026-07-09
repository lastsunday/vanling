use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use futures::executor::block_on;
use service::chobits::tts::{Tts, TtsPacket};
use std::pin::Pin;
use std::thread;
use tokio::sync::mpsc::channel;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::error;

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
        thread::spawn(move || {
            block_on(async move {
                while let Some(text) = text_stream.next().await {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let tx = tx.clone();
                    let packet = TtsPacket {
                        audio: vec![],
                        text,
                    };
                    if let Err(e) = tx.send(Ok(packet)).await {
                        error!("output packet error = {}", e);
                        break;
                    }
                }
                drop(tx);
            })
        });
        Box::pin(ReceiverStream::new(rx))
    }
}

use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use service::component::tts::{Tts, TtsPacket};
use service::pipeline::AudioSpec;
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
    fn audio_spec(&self) -> AudioSpec {
        AudioSpec {
            sample_rate: 16000,
            channel: 1,
            frame_duration_ms: 60,
        }
    }

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
                    text: text.into(),
                    ..Default::default()
                };
                if tx.send(Ok(packet)).await.is_err() {
                    break;
                }
            }
        });
        Box::pin(ReceiverStream::new(rx))
    }
}

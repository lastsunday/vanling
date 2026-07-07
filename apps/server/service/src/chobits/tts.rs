use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

pub struct TtsPacket {
    pub text: String,
    pub audio: Vec<Vec<u8>>,
}

#[async_trait]
pub trait Tts: Send + Sync {
    async fn stream(
        &self,
        text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>>;
}

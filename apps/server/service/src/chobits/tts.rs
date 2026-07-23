use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct TtsPacket {
    pub text: Arc<str>,
    pub audio: Vec<Vec<u8>>,
    pub is_first: bool,
    pub is_last: bool,
}

impl Default for TtsPacket {
    fn default() -> Self {
        Self {
            text: Arc::from(""),
            audio: vec![],
            is_first: true,
            is_last: true,
        }
    }
}

#[async_trait]
pub trait Tts: Send + Sync {
    async fn stream(
        &self,
        text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>>;
}

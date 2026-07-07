use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat(
        &self,
        text: String,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send + 'static>>;
}

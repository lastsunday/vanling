use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

use crate::types::{Input, OutputBlock};

#[async_trait]
pub trait Ling: Send + Sync {
    async fn ask(
        &self,
        input: Input,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<OutputBlock, AppError>> + Send + 'static>>;
}

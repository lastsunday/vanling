use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

pub enum ContentBlock {
    Text(String),
}

pub struct Input {
    pub content: Vec<ContentBlock>,
}

impl Input {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text(text.into())],
        }
    }
}

pub enum OutputBlock {
    Text(String),
}

#[async_trait]
pub trait Chii: Send + Sync {
    async fn ask(
        &self,
        input: Input,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<OutputBlock, AppError>> + Send + 'static>>;
}

use crate::common::ModelError;
use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use futures::executor::block_on;
use futures_channel::mpsc::channel;
use futures_util::SinkExt;
use service::chobits::llm::{CompletionEvent, CompletionRequest, ContentPart, Llm};
use std::pin::Pin;
use std::thread;
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(Default, Clone)]
pub struct Echo {}

impl Echo {
    pub fn new() -> core::result::Result<Self, ModelError> {
        Ok(Self {})
    }
}

#[async_trait]
impl Llm for Echo {
    async fn stream(
        &self,
        request: CompletionRequest,
        _cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>> {
        let (mut tx, rx) = channel::<Result<CompletionEvent, AppError>>(10);
        thread::spawn(move || {
            block_on(async move {
                let msg = request.messages.last();
                if let Some(msg) = msg {
                    for part in &msg.parts {
                        if let ContentPart::Text(text) = part
                            && let Err(e) = tx.send(Ok(CompletionEvent::Text(text.clone()))).await
                        {
                            error!(error = %e, "send text error");
                        }
                    }
                }
                let _ = tx
                    .send(Ok(CompletionEvent::Final {
                        prompt_tokens: 0,
                        total_tokens: 0,
                    }))
                    .await;
                drop(tx);
            })
        });
        Box::pin(rx)
    }

    fn calculate_system_prompt_len(&self, _system_prompt: &Option<String>) -> u64 {
        0
    }

    fn calculate_tools_prompt_len(&self, _tools: &[service::chobits::llm::ToolDef]) -> u64 {
        0
    }

    fn calculate_message_prompt_len(&self, _message: &service::chobits::llm::Message) -> u64 {
        0
    }
}

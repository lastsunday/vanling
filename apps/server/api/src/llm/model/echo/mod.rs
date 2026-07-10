use crate::common::ModelError;
use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use service::chobits::llm::{CompletionEvent, CompletionRequest, ContentPart, Llm};
use std::pin::Pin;
use tokio::sync::mpsc::channel;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

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
        let (tx, rx) = channel::<Result<CompletionEvent, AppError>>(10);
        tokio::spawn(async move {
            let msg = request.messages.last();
            if let Some(msg) = msg {
                for part in &msg.parts {
                    if let ContentPart::Text(text) = part
                        && tx
                            .send(Ok(CompletionEvent::Text(text.clone())))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
            }
            let _ = tx
                .send(Ok(CompletionEvent::Final {
                    prompt_tokens: 0,
                    total_tokens: 0,
                }))
                .await;
        });
        Box::pin(ReceiverStream::new(rx))
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

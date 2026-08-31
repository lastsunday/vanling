use crate::common::ModelError;
use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use service::component::llm::{CompletionEvent, CompletionRequest, ContentPart, InputState, Llm};
use service::types::EmptyKind;
use std::pin::Pin;
use tokio::sync::mpsc::channel;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

/// 空输入提示语固定句：按语境（`EmptyKind`）与重试次数（Rule of three）分级。
fn empty_prompt(kind: EmptyKind, count: u32) -> Option<&'static str> {
    match (kind, count) {
        (EmptyKind::Manual, _) => Some("请再说一遍，我没有听清。"),
        (EmptyKind::Wake, 1) => Some("想让我帮你做什么呢？"),
        (EmptyKind::Wake, _) => Some("你可以告诉我你的需求，比如播放音乐或设置提醒。"),
        (EmptyKind::AutoSpoke, 1) => Some("抱歉，我没听清，可以再说一次吗？"),
        (EmptyKind::AutoSpoke, _) => Some("没能听清，请换个说法或说得慢一些。"),
        (EmptyKind::Silence, 1) => Some("我一直在听，你可以尽管说。"),
        (EmptyKind::Silence, _) => Some("请开口告诉我你想做什么。"),
        // 连续监听：静默等待，不反复提示。
        (EmptyKind::Continuing, _) => None,
    }
}

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
        state: InputState,
        _cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>> {
        let (tx, rx) = channel::<Result<CompletionEvent, AppError>>(10);
        tokio::spawn(async move {
            if let InputState::Empty { kind, count } = state {
                if let Some(prompt) = empty_prompt(kind, count)
                    && tx
                        .send(Ok(CompletionEvent::Text(prompt.to_string())))
                        .await
                        .is_err()
                {
                    return;
                }
                let _ = tx
                    .send(Ok(CompletionEvent::Final {
                        prompt_tokens: 0,
                        total_tokens: 0,
                    }))
                    .await;
                return;
            }
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

    fn calculate_tools_prompt_len(&self, _tools: &[service::component::llm::ToolDef]) -> u64 {
        0
    }

    fn calculate_message_prompt_len(&self, _message: &service::component::llm::Message) -> u64 {
        0
    }
}

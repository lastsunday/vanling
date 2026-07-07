use crate::{common::ModelError, llm::Llm};
use async_trait::async_trait;
use futures::{SinkExt, executor::block_on};
use futures_channel::mpsc::channel;
use rig::{
    completion::{CompletionError, CompletionRequest, ToolDefinition},
    message::{Message, UserContent},
    streaming::{RawStreamingChoice, StreamingCompletionResponse},
};
use std::thread;
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
    ) -> Result<
        StreamingCompletionResponse<rig::providers::openai::streaming::StreamingCompletionResponse>,
        CompletionError,
    > {
        let (mut tx, rx) = channel::<
            Result<
                RawStreamingChoice<rig::providers::openai::streaming::StreamingCompletionResponse>,
                CompletionError,
            >,
        >(10);
        thread::spawn(move || {
            block_on(async move {
                let chat_history = request.chat_history;
                let msg = chat_history.last();
                match msg {
                    Message::User { content } => {
                        let user_content = content.last();
                        match user_content {
                            UserContent::Text(text) => {
                                if let Err(e) =
                                    tx.send(Ok(RawStreamingChoice::Message(text.text))).await
                                {
                                    error!(error = %e, "send text error");
                                }
                            }
                            _ => {
                                // TODO: other input,eg audio,document
                            }
                        }
                    }
                    Message::Assistant { .. } => {
                        //skip
                    }
                }
                drop(tx);
            })
        });
        Ok(StreamingCompletionResponse::stream(Box::pin(rx)))
    }

    fn calculate_system_prompt_len(&self, _system_prompt: &Option<String>) -> u64 {
        0
    }

    fn calculate_tools_prompt_len(&self, _tools: &[ToolDefinition]) -> u64 {
        0
    }

    fn calculate_message_prompt_len(&self, _message: &Message) -> u64 {
        0
    }
}

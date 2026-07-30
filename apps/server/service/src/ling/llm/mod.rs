mod token_converter;

pub use token_converter::*;

use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<ContentPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub enum ContentPart {
    Text(String),
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        id: String,
        output: String,
    },
    Reasoning(String),
}

#[derive(Debug, Clone)]
pub enum CompletionEvent {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    Final {
        prompt_tokens: usize,
        total_tokens: usize,
    },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub preamble: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("token convert failure: {0}")]
    TokenConvertFailure(String),
    #[error("chat error: {0}")]
    Chat(String),
    #[error("model inference error: {0}")]
    ModelInferenceError(String),
}

impl From<LlmError> for framework::error::AppError {
    fn from(value: LlmError) -> Self {
        framework::error::AppError::from(anyhow::anyhow!(value.to_string()))
    }
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn stream(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>>;

    fn calculate_system_prompt_len(&self, system_prompt: &Option<String>) -> u64;

    fn calculate_tools_prompt_len(&self, tools: &[ToolDef]) -> u64;

    fn calculate_message_prompt_len(&self, message: &Message) -> u64;
}

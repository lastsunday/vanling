mod token_converter;

pub use token_converter::*;

use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

use crate::types::EmptyKind;

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

/// 输入状态：描述发起 LLM 调用时的用户输入情形（模态无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputState {
    /// 正常的用户输入（语音/文本等）。
    #[default]
    Normal,
    /// 无有效输入（如 push-to-talk 下空识别）：应生成提示语引导用户。
    /// 携带语境（`EmptyKind`）与重试次数（Rule of three），供生成层分级措辞。
    Empty { kind: EmptyKind, count: u32 },
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
        state: InputState,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>>;

    fn calculate_system_prompt_len(&self, system_prompt: &Option<String>) -> u64;

    fn calculate_tools_prompt_len(&self, tools: &[ToolDef]) -> u64;

    fn calculate_message_prompt_len(&self, message: &Message) -> u64;
}

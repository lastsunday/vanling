use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmMessage {
    #[serde(flatten)]
    pub message: Message,
    pub session_id: Option<String>,
    pub emotion: Option<String>,
    pub text: Option<String>,
    #[serde(skip)]
    pub full_text: Option<String>,
}

impl LlmMessage {
    pub fn new(session_id: Option<String>, emotion: Option<String>, text: Option<String>) -> Self {
        Self {
            message: Message { mtype: Type::Llm },
            session_id,
            emotion,
            text,
            full_text: None,
        }
    }
}

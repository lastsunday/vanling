use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SttMessage {
    #[serde(flatten)]
    pub message: Message,
    pub session_id: Option<String>,
    pub text: Option<String>,
}

impl SttMessage {
    pub fn new(session_id: Option<String>, text: Option<String>) -> Self {
        Self {
            message: Message { mtype: Type::Stt },
            session_id,
            text,
        }
    }
}

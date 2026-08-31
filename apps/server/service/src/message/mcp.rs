use super::*;
use rmcp::model::{JsonRpcMessage, JsonRpcRequest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpRequest {
    #[serde(flatten)]
    pub message: Message,
    pub session_id: Option<String>,
    pub payload: JsonRpcRequest,
}

impl McpRequest {
    pub fn new(session_id: Option<String>, payload: JsonRpcRequest) -> Self {
        Self {
            message: Message { mtype: Type::Mcp },
            session_id,
            payload,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpMessage {
    #[serde(flatten)]
    pub message: Message,
    pub payload: JsonRpcMessage,
}

impl McpMessage {
    pub fn new(payload: JsonRpcMessage) -> Self {
        Self {
            message: Message { mtype: Type::Mcp },
            payload,
        }
    }
}

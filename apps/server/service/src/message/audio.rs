use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioMessage {
    pub session_id: Option<String>,
    pub data: Vec<u8>,
}

impl AudioMessage {
    pub fn new(session_id: Option<String>, data: Vec<u8>) -> Self {
        Self { session_id, data }
    }
}

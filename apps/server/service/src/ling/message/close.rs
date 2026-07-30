use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CloseMessage {
    /// The reason as a code.
    pub code: u16,
    /// The reason as text string.
    pub reason: String,
}

impl CloseMessage {
    pub fn new(code: u16, reason: String) -> Self {
        Self { code, reason }
    }
}

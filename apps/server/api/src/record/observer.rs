use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundEndReason {
    Completed,
    Interrupted,
}

pub enum SessionEvent {
    RoundStart {
        round_id: String,
        session_id: Option<String>,
        client_info: Option<JsonValue>,
    },
    LlmDelta {
        round_id: String,
        text: String,
    },
    TtsDelta {
        round_id: String,
        text: String,
    },
    Frame {
        round_id: Option<String>,
        session_id: Option<String>,
        seq: u64,
        direction: FrameDirection,
        detail: String,
        data: Option<Vec<u8>>,
    },
    RoundEnd {
        round_id: String,
        reason: RoundEndReason,
    },
}

use std::fmt;

use serde::Serialize;

use crate::chobits::message::{
    abort::AbortMessage,
    audio::AudioMessage,
    close::CloseMessage,
    hello::HelloMessage,
    listen::{ListenMessage, ListenState},
    llm::LlmMessage,
    mcp::{McpMessage, McpRequest},
    stt::SttMessage,
    tts::TtsMessage,
};

#[derive(Debug, Clone)]
pub enum Frame {
    Hello(HelloMessage),
    Listen(ListenMessage),
    UnknowText { data: Vec<u8> },
    Voice { data: Vec<u8> },
    Abort(AbortMessage),
    Ping { data: Vec<u8> },
    Pong { data: Vec<u8> },
    Close(CloseMessage),
    Mcp(McpMessage),
    Error { code: u32, message: String },
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Frame::Hello(msg) => write!(f, "Hello(session_id={:?})", msg.session_id),
            Frame::Listen(msg) => {
                write!(f, "Listen(state=")?;
                match msg.state {
                    ListenState::Start => write!(f, "Start")?,
                    ListenState::Stop => write!(f, "Stop")?,
                    ListenState::Detect => write!(f, "Detect")?,
                    ListenState::Text => write!(f, "Text")?,
                }
                if let Some(text) = &msg.text {
                    write!(f, ", text=\"{text}\"")?;
                }
                write!(f, ")")
            }
            Frame::Voice { data } => write!(f, "Voice(data_len={})", data.len()),
            Frame::UnknowText { data } => write!(f, "UnknowText(data_len={})", data.len()),
            Frame::Abort(msg) => write!(f, "Abort(reason={:?})", msg.reason),
            Frame::Ping { data } => write!(f, "Ping(data_len={})", data.len()),
            Frame::Pong { data } => write!(f, "Pong(data_len={})", data.len()),
            Frame::Close(msg) => write!(f, "Close(code={}, reason={})", msg.code, msg.reason),
            Frame::Mcp(msg) => write!(
                f,
                "Mcp(payload={})",
                serde_json::to_string(&msg.payload).unwrap_or_default()
            ),
            Frame::Error { code, message } => write!(f, "Error(code={code}, msg={message})"),
        }
    }
}

#[derive(Debug)]
pub enum FrameResult {
    HelloResult(HelloMessage),
    STTResult(SttMessage),
    LLMResult(LlmMessage),
    TTSResult(TtsMessage),
    AudioResult(AudioMessage),
    CloseResult,
    McpResult(McpRequest),
}

impl Serialize for FrameResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::HelloResult(msg) => msg.serialize(serializer),
            Self::STTResult(msg) => msg.serialize(serializer),
            Self::LLMResult(msg) => msg.serialize(serializer),
            Self::TTSResult(msg) => msg.serialize(serializer),
            Self::McpResult(msg) => msg.serialize(serializer),
            Self::AudioResult(msg) => msg.serialize(serializer),
            Self::CloseResult => serializer.serialize_unit(),
        }
    }
}

impl fmt::Display for FrameResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameResult::HelloResult(msg) => {
                write!(f, "HelloResult(session_id={:?})", msg.session_id)
            }
            FrameResult::STTResult(msg) => write!(f, "STTResult(text={:?})", msg.text),
            FrameResult::LLMResult(msg) => write!(
                f,
                "LLMResult(emotion={:?}, text={:?})",
                msg.emotion, msg.text
            ),
            FrameResult::TTSResult(msg) => {
                write!(f, "TTSResult(state={:?}, text={:?})", msg.state, msg.text)
            }
            FrameResult::AudioResult(msg) => write!(
                f,
                "AudioResult(session_id={:?}, data_len={})",
                msg.session_id,
                msg.data.len()
            ),
            FrameResult::CloseResult => write!(f, "CloseResult"),
            FrameResult::McpResult(req) => write!(
                f,
                "McpResult(payload={})",
                serde_json::to_string(&req.payload).unwrap_or_default()
            ),
        }
    }
}

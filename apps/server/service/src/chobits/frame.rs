use std::fmt;

use framework::error::AppError;
use serde::Serialize;
use serde::ser::SerializeMap;

use crate::chobits::message::{
    abort::AbortMessage,
    audio::AudioMessage,
    close::CloseMessage,
    hello::HelloMessage,
    llm::LlmMessage,
    mcp::{McpMessage, McpRequest},
    stt::SttMessage,
    tts::TtsMessage,
};

#[derive(Debug, Clone)]
pub enum Frame {
    Hello(HelloMessage),
    ListenStart { barge_in: bool },
    ListenStop,
    Input { text: String },
    Voice { data: Vec<u8> },
    Abort(AbortMessage),
    Ping { data: Vec<u8> },
    Pong { data: Vec<u8> },
    Close(CloseMessage),
    Mcp(McpMessage),
    Error { code: u32, message: String },
    UnknownText { data: Vec<u8> },
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Frame::Hello(msg) => write!(f, "Hello(session_id={:?})", msg.session_id),
            Frame::ListenStart { barge_in } => {
                write!(f, "ListenStart(barge_in={barge_in})")
            }
            Frame::ListenStop => write!(f, "ListenStop"),
            Frame::Input { text } => write!(f, "Input(text=\"{text}\")"),
            Frame::Voice { data } => write!(f, "Voice(data_len={})", data.len()),
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
            Frame::UnknownText { data } => write!(f, "UnknownText(data_len={})", data.len()),
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
    Error(AppError),
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
            Self::Error(e) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("code", &e.code())?;
                map.serialize_entry("message", &e.message())?;
                map.end()
            }
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
            FrameResult::Error(e) => {
                write!(f, "Error(code={}, msg={})", e.code(), e.message())
            }
        }
    }
}

#[derive(Debug)]
pub struct OutputMessage {
    pub epoch: u64,
    pub round_id: Option<String>,
    pub session_id: String,
    pub payload: FrameResult,
}

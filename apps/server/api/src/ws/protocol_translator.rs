use axum::extract::ws::Message;
use serde::de::DeserializeOwned;
use serde_json::Value;
use service::chobits::frame::{Frame, FrameResult, InputMode};
use service::chobits::message::{
    abort::AbortMessage, close::CloseMessage, hello::HelloMessage, mcp::McpMessage,
};
use tracing::warn;

pub trait ProtocolTranslator: Send + Sync {
    fn input(&self, message: Message) -> Frame;
    fn output(&self, result: FrameResult) -> Message;
}

#[derive(Clone, Copy)]
pub struct XiaozhiProtocolTranslator;

impl ProtocolTranslator for XiaozhiProtocolTranslator {
    fn input(&self, message: Message) -> Frame {
        match message {
            Message::Text(data) => {
                let bytes = data.to_string().into_bytes();
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(json) if json.is_object() => {
                        match json.get("type").and_then(|v| v.as_str()) {
                            Some("hello") => try_parse::<HelloMessage>(bytes, Frame::Hello),
                            Some("listen") => match serde_json::from_slice::<Value>(&bytes) {
                                Ok(json) => {
                                    let state = json.get("state").and_then(|v| v.as_str());
                                    match state {
                                        Some("start") => {
                                            let barge_in = json
                                                .get("mode")
                                                .and_then(|v| v.as_str())
                                                .is_some_and(|m| m != "auto");
                                            let is_voice_break_detect = json
                                                .get("mode")
                                                .and_then(|v| v.as_str())
                                                .is_some_and(|m| m == "auto" || m == "realtime");
                                            Frame::ListenStart {
                                                barge_in,
                                                is_voice_break_detect,
                                            }
                                        }
                                        Some("stop") => Frame::ListenStop,
                                        Some("detect") | Some("text") => {
                                            let text = json
                                                .get("text")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            if text.is_empty() {
                                                Frame::UnknownText { data: bytes }
                                            } else {
                                                if state == Some("detect") {
                                                    Frame::Input {
                                                        text: text.to_string(),
                                                        mode: InputMode::Wake,
                                                    }
                                                } else {
                                                    Frame::Input {
                                                        text: text.to_string(),
                                                        mode: InputMode::Normal,
                                                    }
                                                }
                                            }
                                        }
                                        _ => Frame::UnknownText { data: bytes },
                                    }
                                }
                                Err(_) => Frame::UnknownText { data: bytes },
                            },
                            Some("abort") => try_parse::<AbortMessage>(bytes, Frame::Abort),
                            Some("mcp") => try_parse::<McpMessage>(bytes, Frame::Mcp),
                            _ => {
                                warn!("unknown message type");
                                Frame::UnknownText { data: bytes }
                            }
                        }
                    }
                    Ok(json) => {
                        warn!("unknown json message = {json}");
                        Frame::UnknownText { data: bytes }
                    }
                    Err(_) => Frame::UnknownText { data: bytes },
                }
            }
            Message::Binary(data) => Frame::Voice {
                data: data.to_vec(),
            },
            Message::Close(c) => {
                let (code, reason) = match c {
                    Some(cf) => (cf.code, cf.reason.to_string()),
                    None => (1000, String::new()),
                };
                Frame::Close(CloseMessage::new(code, reason))
            }
            Message::Pong(data) => Frame::Pong {
                data: data.to_vec(),
            },
            Message::Ping(data) => Frame::Ping {
                data: data.to_vec(),
            },
        }
    }

    fn output(&self, result: FrameResult) -> Message {
        match result {
            FrameResult::AudioResult(audio) => Message::Binary(audio.data.into()),
            FrameResult::Error(e) => Message::Text(
                serde_json::json!({
                    "type": "error",
                    "code": e.code(),
                    "message": e.message(),
                })
                .to_string()
                .into(),
            ),
            other => {
                let text = serde_json::to_string(&other).unwrap_or_default();
                Message::Text(text.into())
            }
        }
    }
}

fn try_parse<T: DeserializeOwned>(data: Vec<u8>, make: impl FnOnce(T) -> Frame) -> Frame {
    match serde_json::from_slice::<T>(&data) {
        Ok(msg) => make(msg),
        Err(_) => Frame::UnknownText { data },
    }
}

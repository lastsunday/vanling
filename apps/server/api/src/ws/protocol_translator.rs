use axum::extract::ws::Message;
use serde::de::DeserializeOwned;
use serde_json::Value;
use service::frame::{Frame, FrameResult, InputMode};
use service::message::{
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
                            Some("listen") => {
                                let state = json.get("state").and_then(|v| v.as_str());
                                let mode = json.get("mode").and_then(|v| v.as_str());
                                match state {
                                    Some("start") => Frame::ListenStart {
                                        barge_in: mode.is_some_and(|m| m != "auto"),
                                        is_voice_break_detect: mode
                                            .is_some_and(|m| m == "auto" || m == "realtime"),
                                    },
                                    Some("stop") => Frame::ListenStop,
                                    Some("detect") | Some("text") => {
                                        let text =
                                            json.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                        if text.is_empty() {
                                            Frame::UnknownText { data: bytes }
                                        } else {
                                            let mode = if state == Some("detect") {
                                                InputMode::Wake
                                            } else {
                                                InputMode::Normal
                                            };
                                            Frame::Input {
                                                text: text.to_string(),
                                                mode,
                                            }
                                        }
                                    }
                                    _ => Frame::UnknownText { data: bytes },
                                }
                            }
                            Some("abort") => try_parse::<AbortMessage>(bytes, Frame::Abort),
                            Some("mcp") => try_parse::<McpMessage>(bytes, Frame::Mcp),
                            _ => {
                                warn!(
                                    component = "WS",
                                    event = "unknown_message_type",
                                    "unknown message type"
                                );
                                Frame::UnknownText { data: bytes }
                            }
                        }
                    }
                    Ok(json) => {
                        warn!(component = "WS", event = "unknown_json_message", json = %json, "unknown json message");
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

#[cfg(test)]
mod tests {
    use super::*;
    use service::message::llm::LlmMessage;

    fn text_msg(s: &str) -> Message {
        Message::Text(s.to_string().into())
    }

    #[test]
    fn output_stt_result_serializes_type_stt() {
        let t = XiaozhiProtocolTranslator;
        let stt = service::message::stt::SttMessage::new(Some("s1".into()), Some("你好".into()));
        match t.output(FrameResult::STTResult(stt)) {
            Message::Text(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert_eq!(v["type"], "stt");
                assert_eq!(v["text"], "你好");
                assert_eq!(v["session_id"], "s1");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn output_llm_result_serializes_type_llm() {
        let t = XiaozhiProtocolTranslator;
        let llm = LlmMessage::new(Some("s1".into()), None, Some("回答".into()));
        match t.output(FrameResult::LLMResult(llm)) {
            Message::Text(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert_eq!(v["type"], "llm");
                assert_eq!(v["text"], "回答");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn output_multi_round_stt_no_state_leak() {
        // 连续两轮 STTResult（第二轮末端），验证序列化互不影响、均带 type=stt。
        let t = XiaozhiProtocolTranslator;
        for (sid, text) in [("r1", "第一段"), ("r2", "第二段")] {
            let stt = service::message::stt::SttMessage::new(Some(sid.into()), Some(text.into()));
            let Message::Text(s) = t.output(FrameResult::STTResult(stt)) else {
                panic!("expected Text");
            };
            let v: serde_json::Value = serde_json::from_str(&s).unwrap();
            assert_eq!(v["type"], "stt");
            assert_eq!(v["session_id"], sid);
            assert_eq!(v["text"], text);
        }
    }

    #[test]
    fn input_text_maps_to_normal_input() {
        let t = XiaozhiProtocolTranslator;
        let frame = t.input(text_msg(
            r#"{"type":"listen","state":"text","text":"你好"}"#,
        ));
        assert!(
            matches!(frame, Frame::Input { ref text, mode: InputMode::Normal } if text == "你好"),
            "got {frame}"
        );
    }

    #[test]
    fn input_listen_detect_maps_to_wake_input() {
        let t = XiaozhiProtocolTranslator;
        let frame = t.input(text_msg(
            r#"{"type":"listen","state":"detect","text":"小叽"}"#,
        ));
        assert!(
            matches!(frame, Frame::Input { ref text, mode: InputMode::Wake } if text == "小叽"),
            "got {frame}"
        );
    }

    #[test]
    fn input_listen_start_maps_barge_in_from_manual() {
        let t = XiaozhiProtocolTranslator;
        let frame = t.input(text_msg(
            r#"{"type":"listen","mode":"manual","state":"start"}"#,
        ));
        assert!(
            matches!(
                frame,
                Frame::ListenStart {
                    barge_in: true,
                    is_voice_break_detect: false
                }
            ),
            "got {frame}"
        );
    }

    #[test]
    fn input_binary_maps_to_voice() {
        let t = XiaozhiProtocolTranslator;
        let frame = t.input(Message::Binary(vec![1, 2, 3].into()));
        assert!(
            matches!(frame, Frame::Voice { ref data } if data == &vec![1, 2, 3]),
            "got {frame}"
        );
    }
}

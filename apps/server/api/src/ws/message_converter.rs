use axum::extract::ws::Message;
use serde::de::DeserializeOwned;
use serde_json::Value;
use service::chobits::message::close::CloseMessage;
use service::ws::frame::Frame;
use std::ops::ControlFlow;

fn try_parse<T, F>(data: &[u8], f: F) -> ControlFlow<Option<Frame>, Option<Frame>>
where
    T: DeserializeOwned,
    F: FnOnce(T) -> Frame,
{
    match serde_json::from_slice::<T>(data) {
        Ok(msg) => ControlFlow::Continue(Some(f(msg))),
        Err(_) => ControlFlow::Continue(Some(Frame::UnknowText {
            data: data.to_vec(),
        })),
    }
}

pub fn convert_to_frame(msg: &Message) -> ControlFlow<Option<Frame>, Option<Frame>> {
    match msg {
        Message::Text(data) => {
            let data = data.as_bytes();
            match serde_json::from_slice::<Value>(data) {
                Ok(json) if json.is_object() => match json.get("type").and_then(|v| v.as_str()) {
                    Some("hello") => try_parse(data, Frame::Hello),
                    Some("listen") => try_parse(data, Frame::Listen),
                    Some("abort") => try_parse(data, Frame::Abort),
                    Some("mcp") => try_parse(data, Frame::Mcp),
                    _ => {
                        tracing::warn!("unknown message type");
                        ControlFlow::Continue(None)
                    }
                },
                Ok(json) => {
                    tracing::warn!("unknown json message = {json}");
                    ControlFlow::Continue(None)
                }
                Err(_) => ControlFlow::Continue(Some(Frame::UnknowText {
                    data: data.to_vec(),
                })),
            }
        }

        Message::Binary(data) => ControlFlow::Continue(Some(Frame::Voice {
            data: data.to_vec(),
        })),

        Message::Close(c) => match c {
            Some(cf) => ControlFlow::Break(Some(Frame::Close(CloseMessage::new(
                cf.code,
                cf.reason.to_string(),
            )))),
            None => ControlFlow::Break(None),
        },

        Message::Pong(data) => ControlFlow::Continue(Some(Frame::Pong {
            data: data.to_vec(),
        })),

        Message::Ping(data) => ControlFlow::Continue(Some(Frame::Ping {
            data: data.to_vec(),
        })),
    }
}

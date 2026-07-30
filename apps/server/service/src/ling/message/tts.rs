use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TtsMessage {
    #[serde(flatten)]
    pub message: Message,
    pub session_id: Option<String>,
    pub state: Option<TtsState>,
    pub text: Option<String>,
}

impl TtsMessage {
    pub fn new(session_id: Option<String>, state: Option<TtsState>, text: Option<String>) -> Self {
        Self {
            message: Message { mtype: Type::Tts },
            session_id,
            state,
            text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TtsState {
    Start,
    SentenceStart,
    SentenceEnd,
    Stop,
}

impl<'de> Deserialize<'de> for TtsState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == r#"start"# {
            Ok(TtsState::Start)
        } else if value == r#"stop"# {
            Ok(TtsState::Stop)
        } else if value == r#"sentence_start"# {
            Ok(TtsState::SentenceStart)
        } else if value == r#"sentence_end"# {
            Ok(TtsState::SentenceEnd)
        } else {
            Err(serde::de::Error::custom(
                "Expected start,stop,sentence_start for tts state",
            ))
        }
    }
}

impl Serialize for TtsState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            TtsState::Start => r#"start"#,
            TtsState::Stop => r#"stop"#,
            TtsState::SentenceStart => r#"sentence_start"#,
            TtsState::SentenceEnd => r#"sentence_end"#,
        })
    }
}

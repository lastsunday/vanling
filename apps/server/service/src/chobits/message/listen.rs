use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListenMessage {
    #[serde(flatten)]
    pub message: Message,
    pub session_id: Option<String>,
    pub state: ListenState,
    #[serde(rename = "mode")]
    pub mmod: Option<ListenMode>,
    pub text: Option<String>,
}

impl Default for ListenMessage {
    fn default() -> Self {
        Self {
            message: Message {
                mtype: Type::Listen,
            },
            session_id: Default::default(),
            state: ListenState::Start,
            mmod: Default::default(),
            text: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ListenState {
    Start,
    Stop,
    Detect,
    Text,
}

impl<'de> Deserialize<'de> for ListenState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == r#"start"# {
            Ok(ListenState::Start)
        } else if value == r#"stop"# {
            Ok(ListenState::Stop)
        } else if value == r#"detect"# {
            Ok(ListenState::Detect)
        } else if value == r#"text"# {
            Ok(ListenState::Text)
        } else {
            Err(serde::de::Error::custom(
                "Expected start,stop,detect,text for listen state",
            ))
        }
    }
}

impl Serialize for ListenState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            ListenState::Start => r#"start"#,
            ListenState::Stop => r#"stop"#,
            ListenState::Detect => r#"detect"#,
            ListenState::Text => r#"text"#,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenMode {
    Auto,
    Manual,
    RealTime,
}

impl<'de> Deserialize<'de> for ListenMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == r#"auto"# {
            Ok(Self::Auto)
        } else if value == r#"manual"# {
            Ok(Self::Manual)
        } else if value == r#"realtime"# {
            Ok(Self::RealTime)
        } else {
            Err(serde::de::Error::custom(
                "Expected auto,manual,realtime for listen mode",
            ))
        }
    }
}

impl Serialize for ListenMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Auto => r#"auto"#,
            Self::Manual => r#"manual"#,
            Self::RealTime => r#"realtime"#,
        })
    }
}

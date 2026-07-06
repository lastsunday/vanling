use serde::{Deserialize, Serialize};

pub mod abort;
pub mod audio;
pub mod close;
pub mod hello;
pub mod llm;
pub mod mcp;
pub mod stt;
pub mod tts;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "type")]
    pub mtype: Type,
}

#[derive(Debug, Clone)]
pub enum Type {
    Hello,
    Listen,
    Tts,
    Stt,
    Llm,
    Abort,
    Mcp,
}

impl<'de> Deserialize<'de> for Type {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == r#"hello"# {
            Ok(Self::Hello)
        } else if value == r#"listen"# {
            Ok(Self::Listen)
        } else if value == r#"tts"# {
            Ok(Self::Tts)
        } else if value == r#"stt"# {
            Ok(Self::Stt)
        } else if value == r#"llm"# {
            Ok(Self::Llm)
        } else if value == r#"abort"# {
            Ok(Self::Abort)
        } else if value == r#"mcp"# {
            Ok(Self::Mcp)
        } else {
            Err(serde::de::Error::custom(
                "Expected hello,listen,tts,abort for type",
            ))
        }
    }
}

impl Serialize for Type {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Type::Hello => r#"hello"#,
            Type::Listen => r#"listen"#,
            Type::Tts => r#"tts"#,
            Type::Stt => r#"stt"#,
            Type::Llm => r#"llm"#,
            Type::Abort => r#"abort"#,
            Type::Mcp => r#"mcp"#,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Transport {
    Websocket,
    // TODO: Martix
}

impl<'de> Deserialize<'de> for Transport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == r#"websocket"# {
            Ok(Self::Websocket)
        } else {
            Err(serde::de::Error::custom("Expected websocket for transport"))
        }
    }
}

impl Serialize for Transport {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Websocket => r#"websocket"#,
        })
    }
}

#[derive(Debug, Clone)]
pub enum AudioFormat {
    Opus,
}

impl<'de> Deserialize<'de> for AudioFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == r#"opus"# {
            Ok(Self::Opus)
        } else {
            Err(serde::de::Error::custom("Expected opus for audio format"))
        }
    }
}

impl Serialize for AudioFormat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Opus => r#"opus"#,
        })
    }
}

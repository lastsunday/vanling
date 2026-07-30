use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HelloMessage {
    #[serde(flatten)]
    pub message: Message,
    pub version: Option<i32>,
    pub transport: Option<Transport>,
    pub audio_params: Option<AudioParam>,
    pub features: Option<Feature>,
    pub session_id: Option<String>,
}

impl Default for HelloMessage {
    fn default() -> Self {
        Self {
            message: Message { mtype: Type::Hello },
            version: Default::default(),
            transport: Default::default(),
            audio_params: Default::default(),
            features: Default::default(),
            session_id: Default::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioParam {
    pub format: AudioFormat,
    pub sample_rate: u32,
    pub channels: u32,
    pub frame_duration: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Feature {
    pub mcp: Option<bool>,
    pub aec: Option<bool>,
}

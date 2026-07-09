use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AudioConfig {
    #[serde(default)]
    pub output_sample_rate: Option<u32>,
    #[serde(default)]
    pub output_channel: Option<u32>,
    /// unit: ms
    #[serde(default)]
    pub output_frame_duration: Option<u64>,
}

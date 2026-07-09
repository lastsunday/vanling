use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct VadConfig {
    #[serde(default)]
    pub model: Option<super::VadModel>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub num_threads: Option<i32>,
    #[serde(default)]
    pub threshold: Option<f32>,
    #[serde(default)]
    pub min_silence_duration: Option<f32>,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            model: Default::default(),
            variant: Default::default(),
            path: Default::default(),
            num_threads: Default::default(),
            threshold: Some(0.5),
            min_silence_duration: Some(1000.0),
        }
    }
}

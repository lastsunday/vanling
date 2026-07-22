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
    /// Deactivation threshold for hysteresis. Must be < threshold.
    /// Speech stops when score drops below this value during active speech.
    #[serde(default)]
    pub deactivation_threshold: Option<f32>,
    #[serde(default)]
    pub min_silence_duration: Option<f32>,
    #[serde(default)]
    pub min_speech_duration: Option<f32>,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            model: Default::default(),
            variant: Default::default(),
            path: Default::default(),
            num_threads: Default::default(),
            threshold: Some(0.4),
            deactivation_threshold: Some(0.2),
            min_silence_duration: Some(550.0),
            min_speech_duration: Some(150.0),
        }
    }
}

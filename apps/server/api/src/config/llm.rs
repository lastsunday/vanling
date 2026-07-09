use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct LlmConfig {
    #[serde(default)]
    pub model: Option<super::LlmModel>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

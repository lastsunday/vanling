use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct LlmConfig {
    #[serde(default)]
    pub provider: Option<super::LlmProvider>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

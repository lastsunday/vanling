use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct LingConfig {
    #[serde(default)]
    pub system_prompt: Option<String>,
}

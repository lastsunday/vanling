use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AsrConfig {
    #[serde(default)]
    pub model: Option<super::AsrModel>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

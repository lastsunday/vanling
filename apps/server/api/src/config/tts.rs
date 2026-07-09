use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct TtsConfig {
    #[serde(default)]
    pub model: Option<super::TtsModel>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    //参照音频字幕
    #[serde(default)]
    pub reference_prompt_text: Option<String>,
    //参照音频路径
    #[serde(default)]
    pub reference_prompt_wav_path: Option<String>,
    //模型特有配置
    #[serde(default)]
    pub options: Option<serde_json::Value>,
}

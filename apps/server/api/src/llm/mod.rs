pub mod model;

use crate::config::{self, llm::LlmConfig};
use crate::llm::model::{echo::Echo, openai_compatible::OpenAiCompatible, qwen3::LlmQwen};
use std::sync::{Arc, OnceLock};

pub use service::ling::llm::Llm;

static INSTANCE: OnceLock<LlmManager> = OnceLock::new();

pub struct LlmManager {
    default_llm: Arc<dyn Llm>,
    pub config: Arc<LlmConfig>,
}

impl LlmManager {
    pub fn new(default_llm: Arc<dyn Llm>, config: Arc<LlmConfig>) -> Self {
        Self {
            default_llm,
            config,
        }
    }

    pub async fn init(config: Arc<LlmConfig>) -> &'static Self {
        let llm = LlmManager::create_model(&config);
        INSTANCE.get_or_init(|| Self::new(llm, config))
    }

    pub fn default(&self) -> Arc<dyn Llm> {
        self.default_llm.clone()
    }

    pub fn create_model(config: &LlmConfig) -> Arc<dyn Llm> {
        match config.provider.as_ref().expect("llm provider is empty") {
            config::LlmProvider::LocalQwen3 => {
                Arc::new(LlmQwen::new(config.path.as_ref().expect("llm path is empty")).unwrap())
            }
            config::LlmProvider::LocalEcho => Arc::new(Echo::new().unwrap()),
            config::LlmProvider::RemoteOpenAiCompatible => {
                let api_url = config
                    .api_url
                    .as_deref()
                    .expect("llm.api_url is required for openai_compatible model");
                let api_key = config.api_key.as_deref().unwrap_or("");
                let api_model = config
                    .model
                    .as_deref()
                    .expect("llm.model is required for openai_compatible model");
                Arc::new(
                    OpenAiCompatible::new(api_url, api_key, api_model)
                        .expect("failed to create OpenAiCompatible model"),
                )
            }
        }
    }

    pub fn global() -> &'static LlmManager {
        INSTANCE.get().unwrap()
    }
}

pub mod model;

use crate::config::{self, llm::LlmConfig};
use crate::llm::model::{echo::Echo, qwen3::LlmQwen};
use std::sync::{Arc, OnceLock};

pub use service::chobits::llm::Llm;

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
        match config.model.as_ref().expect("llm model is empty") {
            config::LlmModel::Qwen3 => {
                Arc::new(LlmQwen::new(config.path.as_ref().expect("llm path is empty")).unwrap())
            }
            config::LlmModel::Echo => Arc::new(Echo::new().unwrap()),
        }
    }

    pub fn global() -> &'static LlmManager {
        INSTANCE.get().unwrap()
    }
}

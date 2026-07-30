pub mod model;

use crate::{
    asr::model::{void::AsrVoid, x_asr::AsrXAsr},
    config::{AsrModel, asr::AsrConfig},
};
use service::ling::asr::Asr;
use std::sync::{Arc, OnceLock};

static INSTANCE: OnceLock<AsrManager> = OnceLock::new();

pub struct AsrManager {
    default_instance: Arc<dyn Asr>,
    pub config: Arc<AsrConfig>,
}

impl AsrManager {
    pub fn new(default_instance: Arc<dyn Asr>, config: Arc<AsrConfig>) -> Self {
        Self {
            default_instance,
            config,
        }
    }

    pub async fn init(config: Arc<AsrConfig>) -> &'static Self {
        INSTANCE.get_or_init(|| -> Self { Self::new(Self::create_model(&config).into(), config) })
    }

    pub fn global() -> &'static AsrManager {
        INSTANCE.get().unwrap()
    }

    pub fn default(&self) -> Arc<dyn Asr> {
        self.default_instance.clone()
    }

    pub fn create_model(config: &AsrConfig) -> Box<dyn Asr> {
        let model = config.model.clone().expect("asr model is empty");
        match model {
            AsrModel::Void => Box::new(AsrVoid::new().unwrap()),
            AsrModel::XAsr => {
                let path = config.path.clone().expect("asr path is empty");
                Box::new(AsrXAsr::new(&path).unwrap())
            }
        }
    }
}

pub mod model;

use crate::{
    asr::model::{sense_voice::AsrSenseVoice, void::AsrVoid},
    config::{AsrModel, asr::AsrConfig},
};
use service::chobits::asr::Asr;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

static INSTANCE: OnceLock<AsrFactory> = OnceLock::new();

pub struct AsrFactory {
    default_instance: Arc<Mutex<Box<dyn Asr>>>,
    pub config: Arc<AsrConfig>,
}

impl AsrFactory {
    pub fn new(default_instance: Arc<Mutex<Box<dyn Asr>>>, config: Arc<AsrConfig>) -> Self {
        Self {
            default_instance,
            config,
        }
    }

    pub async fn init(config: Arc<AsrConfig>) -> &'static Self {
        INSTANCE.get_or_init(|| -> Self {
            Self::new(Arc::new(Mutex::new(Self::create_model(&config))), config)
        })
    }

    pub fn global() -> &'static AsrFactory {
        INSTANCE.get().unwrap()
    }

    pub fn default(&self) -> Arc<Mutex<Box<dyn Asr>>> {
        self.default_instance.clone()
    }

    pub fn create_model(config: &AsrConfig) -> Box<dyn Asr> {
        let model = config.model.clone().expect("asr model is empty");
        match model {
            AsrModel::Void => Box::new(AsrVoid::new().unwrap()),
            AsrModel::SenseVoice => {
                let path = config.path.clone().expect("asr path is empty");
                Box::new(AsrSenseVoice::new(&path).unwrap())
            }
        }
    }
}

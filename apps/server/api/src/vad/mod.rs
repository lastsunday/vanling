pub mod model;
use crate::config::VadModel;
use crate::config::vad::VadConfig;
use crate::vad::model::earshot::VadEarshot;
use crate::vad::model::void::VadVoid;
use service::chobits::vad::Vad;
use std::sync::{Arc, OnceLock};

static VAD_INSTANCE: OnceLock<VadManager> = OnceLock::new();

#[derive(Default)]
pub struct VadManager {
    pub config: Arc<VadConfig>,
}

impl VadManager {
    pub fn new(config: Arc<VadConfig>) -> Self {
        Self { config }
    }

    pub async fn init(config: Arc<VadConfig>) -> &'static Self {
        VAD_INSTANCE.get_or_init(|| -> Self { Self::new(config) })
    }

    pub fn global() -> &'static VadManager {
        VAD_INSTANCE.get().unwrap()
    }

    pub fn config() -> Arc<VadConfig> {
        VAD_INSTANCE.get().unwrap().config.clone()
    }

    pub fn create_model(config: &VadConfig) -> Box<dyn Vad> {
        match config.model.as_ref().expect("vad model empty") {
            VadModel::Void => Box::new(VadVoid::new().unwrap()),
            VadModel::Earshot => Box::new(VadEarshot::new(config).unwrap()),
        }
    }
}

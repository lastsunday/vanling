pub mod model;
pub mod opus_encoder;

pub use opus_encoder::StreamingOpusEncoder;

use self::model::matcha::TtsMatcha;
use self::model::mute::TtsMute;
use crate::config;
use crate::config::audio::AudioConfig;
use crate::config::tts::TtsConfig;
use service::component::tts::Tts;
use std::sync::{Arc, OnceLock};

static INSTANCE: OnceLock<TtsManager> = OnceLock::new();

pub struct TtsManager {
    default_instance: Arc<dyn Tts>,
    pub tts_config: Arc<TtsConfig>,
    pub audio_config: Arc<AudioConfig>,
}

impl TtsManager {
    pub fn new(
        default_instance: Arc<dyn Tts>,
        tts_config: Arc<TtsConfig>,
        audio_config: Arc<AudioConfig>,
    ) -> Self {
        Self {
            default_instance,
            tts_config,
            audio_config,
        }
    }

    pub async fn init(
        tts_config: Arc<TtsConfig>,
        audio_config: Arc<AudioConfig>,
    ) -> Result<&'static Self, anyhow::Error> {
        let tts = Self::create_model(&tts_config, &audio_config).await?;
        Ok(
            INSTANCE
                .get_or_init(|| -> Self { Self::new(Arc::from(tts), tts_config, audio_config) }),
        )
    }

    pub fn default(&self) -> Arc<dyn Tts> {
        self.default_instance.clone()
    }

    pub async fn create_model(
        tts_config: &TtsConfig,
        audio_config: &AudioConfig,
    ) -> Result<Box<dyn Tts>, anyhow::Error> {
        match tts_config.model.clone().expect("tts model is empty") {
            config::TtsModel::Mute => Ok(Box::new(TtsMute::new().await?)),
            config::TtsModel::MatchaTts => {
                Ok(Box::new(TtsMatcha::new(tts_config, audio_config).await?))
            }
        }
    }

    pub fn global() -> &'static TtsManager {
        INSTANCE.get().unwrap()
    }
}

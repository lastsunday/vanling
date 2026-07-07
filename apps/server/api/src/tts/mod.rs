pub mod model;

use self::model::matcha::TtsMatcha;
use self::model::mute::TtsMute;
use crate::config;
use crate::config::audio::AudioConfig;
use crate::config::tts::TtsConfig;
use service::chobits::tts::Tts;
use std::sync::{Arc, OnceLock};

static INSTANCE: OnceLock<TtsFactory> = OnceLock::new();

pub struct TtsFactory {
    default_instance: Arc<dyn Tts>,
    pub tts_config: Arc<TtsConfig>,
    pub audio_config: Arc<AudioConfig>,
}

impl TtsFactory {
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

    pub fn global() -> &'static TtsFactory {
        INSTANCE.get().unwrap()
    }
}

pub fn encode_sample_to_tts_packet(
    sample: Vec<f32>,
    encoder: &mut opus::Encoder,
    encode_sample_rate: u32,
    encode_channel: u32,
    encode_frame_duration: u64,
) -> Vec<Vec<u8>> {
    let len = sample.len();
    let size = calcalute_tts_packet_size(encode_sample_rate, encode_channel, encode_frame_duration);
    let count = len.div_ceil(size);
    let mut audio: Vec<Vec<u8>> = Vec::with_capacity(count);
    for n in 0..count {
        let start = n * size;
        let end = std::cmp::min(start + size, len);
        let mut frame: Vec<f32> = sample[start..end].to_vec();
        frame.resize(size, 0.0);
        let packet = encoder.encode_vec_float(&frame, size).unwrap();
        audio.push(packet);
    }
    audio
}

pub fn calcalute_tts_packet_size(sample_rate: u32, channel: u32, delay_millis: u64) -> usize {
    sample_rate as usize * channel as usize * delay_millis as usize / 1000
}

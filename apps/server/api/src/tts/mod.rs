pub mod model;

use self::model::matcha::TtsMatcha;
use self::model::mute::TtsMute;
use crate::config;
use crate::config::audio::AudioConfig;
use crate::config::tts::TtsConfig;
use service::ling::tts::Tts;
use std::sync::{Arc, OnceLock};

const OPUS_MAX_PACKET_MULTIPLIER: usize = 4;
const FADE_OUT_RATIO: usize = 4;

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

pub struct StreamingOpusEncoder {
    encoder: ropus::Encoder,
    frame_size: usize,
    buffer: Vec<f32>,
}

impl StreamingOpusEncoder {
    pub fn new(
        sample_rate: u32,
        channels: ropus::Channels,
        application: ropus::Application,
        frame_duration_ms: u64,
    ) -> Result<Self, anyhow::Error> {
        let encoder = ropus::Encoder::builder(sample_rate, channels, application).build()?;
        let frame_size = sample_rate as usize * frame_duration_ms as usize / 1000;
        Ok(Self {
            encoder,
            frame_size,
            buffer: Vec::new(),
        })
    }

    pub fn push_samples(&mut self, samples: &[f32]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(samples);

        let mut frames = Vec::new();
        while self.buffer.len() >= self.frame_size {
            let mut buf = vec![0u8; self.frame_size * OPUS_MAX_PACKET_MULTIPLIER];
            if let Ok(written) = self
                .encoder
                .encode_float(&self.buffer[..self.frame_size], &mut buf)
            {
                buf.truncate(written);
                frames.push(buf);
            }
            self.buffer.drain(..self.frame_size);
        }
        frames
    }

    pub fn flush(&mut self) -> Vec<Vec<u8>> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let mut frame = std::mem::take(&mut self.buffer);
        frame.resize(self.frame_size, 0.0);

        let fade_samples = self.frame_size / FADE_OUT_RATIO;
        let start = frame.len() - fade_samples;
        for i in 0..fade_samples {
            let fade = 1.0 - (i as f32 / fade_samples as f32);
            frame[start + i] *= fade;
        }

        let mut buf = vec![0u8; self.frame_size * OPUS_MAX_PACKET_MULTIPLIER];
        if let Ok(written) = self.encoder.encode_float(&frame, &mut buf) {
            buf.truncate(written);
            vec![buf]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_encoder() -> StreamingOpusEncoder {
        StreamingOpusEncoder::new(16000, ropus::Channels::Mono, ropus::Application::Audio, 20)
            .unwrap()
    }

    #[test]
    fn test_fade_out_on_flush() {
        let mut enc = make_encoder();
        let samples: Vec<f32> = vec![0.5; 1640];
        enc.push_samples(&samples);

        let flushed = enc.flush();
        assert!(!flushed.is_empty());

        let mut decoder = ropus::Decoder::new(16000, ropus::Channels::Mono).unwrap();
        let mut decoded = Vec::new();
        for pkt in &flushed {
            let mut buf = vec![0f32; 320];
            if let Ok(n) = decoder.decode_float(pkt, &mut buf, ropus::DecodeMode::Normal) {
                decoded.extend_from_slice(&buf[..n]);
            }
        }
        assert!(!decoded.is_empty());
        let tail_start = decoded.len().saturating_sub(160);
        let tail_rms: f32 = (decoded[tail_start..].iter().map(|s| s * s).sum::<f32>()
            / decoded[tail_start..].len() as f32)
            .sqrt();
        let head_rms: f32 = (decoded[..160].iter().map(|s| s * s).sum::<f32>()
            / decoded[..160].len() as f32)
            .sqrt();
        assert!(
            tail_rms < head_rms,
            "tail RMS ({tail_rms}) should be less than head RMS ({head_rms})"
        );
    }

    #[test]
    fn test_empty_flush() {
        let mut enc = make_encoder();
        let flushed = enc.flush();
        assert!(flushed.is_empty());
    }
}

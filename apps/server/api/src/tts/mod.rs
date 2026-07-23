pub mod model;

use self::model::matcha::TtsMatcha;
use self::model::mute::TtsMute;
use crate::config;
use crate::config::audio::AudioConfig;
use crate::config::tts::TtsConfig;
use service::chobits::tts::Tts;
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

pub fn encode_sample_to_tts_packet(
    sample: Vec<f32>,
    encoder: &mut ropus::Encoder,
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
        let mut buf = vec![0u8; size * 4];
        let written = encoder.encode_float(&frame, &mut buf).unwrap();
        buf.truncate(written);
        audio.push(buf);
    }
    audio
}

pub fn calcalute_tts_packet_size(sample_rate: u32, channel: u32, delay_millis: u64) -> usize {
    sample_rate as usize * channel as usize * delay_millis as usize / 1000
}
/// Incrementally accumulates PCM samples and emits Opus frames as they become available.
///
/// Frames are encoded directly from the internal buffer without crossfade —
/// the caller is expected to supply continuous PCM, so frame boundaries are
/// already smooth. A linear fade-out on `flush()` prevents abrupt audio cutoff.
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

    /// Push incremental PCM samples. Returns any complete Opus frames produced.
    pub fn push_samples(&mut self, samples: &[f32]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(samples);

        let mut frames = Vec::new();
        while self.buffer.len() >= self.frame_size {
            let frame: Vec<f32> = self.buffer.drain(..self.frame_size).collect();

            let mut buf = vec![0u8; self.frame_size * 4];
            if let Ok(written) = self.encoder.encode_float(&frame, &mut buf) {
                buf.truncate(written);
                frames.push(buf);
            }
        }
        frames
    }

    /// Flush remaining samples with fade-out applied to prevent abrupt audio cutoff.
    /// Call after generation completes.
    pub fn flush(&mut self) -> Vec<Vec<u8>> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let mut frame = std::mem::take(&mut self.buffer);
        frame.resize(self.frame_size, 0.0);

        // Linear fade-out on the tail to prevent click at sentence end
        let fade_samples = self.frame_size / 4;
        let start = frame.len() - fade_samples;
        for i in 0..fade_samples {
            let fade = 1.0 - (i as f32 / fade_samples as f32);
            frame[start + i] *= fade;
        }

        let mut buf = vec![0u8; self.frame_size * 4];
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
        // Push a non-aligned amount so there are leftover samples for flush()
        let samples: Vec<f32> = vec![0.5; 1640];
        enc.push_samples(&samples);

        let flushed = enc.flush();
        assert!(
            !flushed.is_empty(),
            "flush should produce at least one frame"
        );

        // Decode and verify the tail has fade-out
        let mut decoder = ropus::Decoder::new(16000, ropus::Channels::Mono).unwrap();
        let mut decoded = Vec::new();
        for pkt in &flushed {
            let mut buf = vec![0f32; 320];
            if let Ok(n) = decoder.decode_float(pkt, &mut buf, ropus::DecodeMode::Normal) {
                decoded.extend_from_slice(&buf[..n]);
            }
        }
        assert!(!decoded.is_empty());
        // Last samples should be closer to zero due to fade-out
        let tail_start = decoded.len().saturating_sub(160);
        let tail_rms: f32 = (decoded[tail_start..].iter().map(|s| s * s).sum::<f32>()
            / decoded[tail_start..].len() as f32)
            .sqrt();
        let head_rms: f32 = (decoded[..160].iter().map(|s| s * s).sum::<f32>()
            / decoded[..160].len() as f32)
            .sqrt();
        assert!(
            tail_rms < head_rms,
            "tail RMS ({tail_rms}) should be less than head RMS ({head_rms}) due to fade-out"
        );
    }

    #[test]
    fn test_empty_flush() {
        let mut enc = make_encoder();
        let flushed = enc.flush();
        assert!(flushed.is_empty(), "flush with no data should return empty");
    }

    #[test]
    fn test_encode_sample_to_tts_packet() {
        let mut encoder =
            ropus::Encoder::builder(16000, ropus::Channels::Mono, ropus::Application::Audio)
                .build()
                .unwrap();
        let samples: Vec<f32> = vec![0.1; 320]; // 20ms @ 16kHz
        let packets = encode_sample_to_tts_packet(samples, &mut encoder, 16000, 1, 20);
        assert_eq!(packets.len(), 1);
        assert!(!packets[0].is_empty());
    }
}

use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};
use service::chobits::tts::{Tts, TtsPacket};
use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsMatchaModelConfig,
    OfflineTtsModelConfig,
};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc::channel;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::config::audio::AudioConfig;
use crate::config::tts::TtsConfig;
use crate::tts::encode_sample_to_tts_packet;

pub struct TtsMatcha {
    tts: Arc<OfflineTts>,
    output_sample_rate: u32,
    output_channel: u32,
    output_frame_duration: u64,
    speed: f32,
}

impl TtsMatcha {
    pub async fn new(
        tts_config: &TtsConfig,
        audio_config: &AudioConfig,
    ) -> Result<Self, anyhow::Error> {
        let path = tts_config
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("tts path must be set in TtsConfig"))?;
        if !path.ends_with('/') {
            return Err(anyhow::anyhow!("tts path must end with '/'"));
        }

        let opts = tts_config.options.as_ref();

        let num_threads = opts
            .and_then(|o| o.get("num_threads"))
            .and_then(|v| v.as_i64())
            .unwrap_or(2) as i32;

        let debug = opts
            .and_then(|o| o.get("debug"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let noise_scale = opts
            .and_then(|o| o.get("noise_scale"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.667) as f32;

        let length_scale = opts
            .and_then(|o| o.get("length_scale"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;

        let speed = opts
            .and_then(|o| o.get("speed"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;

        let dict_dir = opts
            .and_then(|o| o.get("dict_dir"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let acoustic_model = opts
            .and_then(|o| o.get("acoustic_model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| auto_discover_onnx(path, "model-steps-3"));

        let acoustic_model_path = acoustic_model.ok_or_else(|| {
            anyhow::anyhow!("Matcha acoustic model file (.onnx) not found in {path}")
        })?;

        let vocoder = opts
            .and_then(|o| o.get("vocoder"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| auto_discover_onnx(path, "vocos-22khz-univ"))
            .or_else(|| auto_discover_onnx(path, "vocos-16khz-univ"));

        let vocoder_path = vocoder
            .ok_or_else(|| anyhow::anyhow!("Matcha vocoder file (.onnx) not found in {path}"))?;

        let tokens = opts
            .and_then(|o| o.get("tokens"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{path}tokens.txt"));

        let lexicon = opts
            .and_then(|o| o.get("lexicon"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{path}lexicon.txt"));

        let data_dir = opts
            .and_then(|o| o.get("data_dir"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                let candidate = format!("{path}espeak-ng-data");
                if std::path::Path::new(&candidate).is_dir() {
                    Some(candidate)
                } else {
                    None
                }
            });

        let rule_fsts = {
            let p = std::path::Path::new(path);
            let mut files: Vec<String> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(p) {
                for entry in entries.flatten() {
                    let ep = entry.path();
                    if ep.extension().is_some_and(|ext| ext == "fst") {
                        files.push(ep.to_string_lossy().into_owned());
                    }
                }
            }
            files.sort_by(|a, b| {
                fn priority(f: &str) -> u8 {
                    if f.contains("phone") {
                        0
                    } else if f.contains("date") {
                        1
                    } else if f.contains("number") {
                        2
                    } else {
                        3
                    }
                }
                priority(a).cmp(&priority(b))
            });
            if files.is_empty() {
                None
            } else {
                Some(files.join(","))
            }
        };

        let rule_fars = {
            let p = std::path::Path::new(path).join("rule.far");
            if p.is_file() {
                Some(p.to_string_lossy().into_owned())
            } else {
                None
            }
        };

        let matcha_config = OfflineTtsMatchaModelConfig {
            acoustic_model: Some(acoustic_model_path),
            vocoder: Some(vocoder_path),
            tokens: Some(tokens),
            lexicon: Some(lexicon),
            data_dir,
            dict_dir,
            noise_scale,
            length_scale,
        };

        let config = OfflineTtsConfig {
            model: OfflineTtsModelConfig {
                matcha: matcha_config,
                num_threads,
                debug,
                ..Default::default()
            },
            rule_fsts,
            rule_fars,
            ..Default::default()
        };

        let tts = OfflineTts::create(&config)
            .ok_or_else(|| anyhow::anyhow!("Failed to create OfflineTts (Matcha)"))?;

        let output_sample_rate = audio_config
            .output_sample_rate
            .ok_or_else(|| anyhow::anyhow!("AudioConfig.output_sample_rate is required"))?;
        let output_channel = audio_config
            .output_channel
            .ok_or_else(|| anyhow::anyhow!("AudioConfig.output_channel is required"))?;
        let output_frame_duration = audio_config
            .output_frame_duration
            .ok_or_else(|| anyhow::anyhow!("AudioConfig.output_frame_duration is required"))?;

        Ok(Self {
            tts: Arc::new(tts),
            output_sample_rate,
            output_channel,
            output_frame_duration,
            speed,
        })
    }
}

#[async_trait]
impl Tts for TtsMatcha {
    async fn stream(
        &self,
        mut text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>> {
        let (tx, rx) = channel::<Result<TtsPacket, AppError>>(64);

        let tts = self.tts.clone();
        let output_sample_rate = self.output_sample_rate;
        let output_channel = self.output_channel;
        let output_frame_duration = self.output_frame_duration;
        let speed = self.speed;

        tokio::spawn(async move {
            let encode_sr = output_sample_rate;
            let channels = match output_channel {
                2 => 2_usize,
                _ => 1_usize,
            };
            let opus_channels = if channels == 2 {
                ropus::Channels::Stereo
            } else {
                ropus::Channels::Mono
            };
            let mut encoder =
                match ropus::Encoder::builder(encode_sr, opus_channels, ropus::Application::Audio)
                    .build()
                {
                    Ok(e) => e,
                    Err(err) => {
                        error!("[MatchaTTS] opus encoder creation error = {}", err);
                        return;
                    }
                };

            while let Some(text) = text_stream.next().await {
                if cancel.is_cancelled() {
                    break;
                }

                let tts_clone = tts.clone();
                let text_clone = text.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let gen_config = GenerationConfig {
                        speed,
                        ..Default::default()
                    };
                    let audio = tts_clone.generate_with_config(
                        &text_clone,
                        &gen_config,
                        None::<fn(&[f32], f32) -> bool>,
                    );
                    match audio {
                        Some(a) => {
                            let samples = a.samples().to_vec();
                            let sr = a.sample_rate();
                            Some((samples, sr))
                        }
                        None => None,
                    }
                })
                .await;

                let (pcm_samples, pcm_sample_rate) = match result {
                    Ok(Some((s, sr))) => (s, sr),
                    _ => {
                        error!("[MatchaTTS] generation failed for text = {}", text);
                        continue;
                    }
                };

                let (opus_pcm, _opus_sr) = if pcm_sample_rate != encode_sr as i32 {
                    let chunk_size = 4096.min(pcm_samples.len());
                    let mut resampler = Fft::<f32>::new(
                        pcm_sample_rate as usize,
                        encode_sr as usize,
                        chunk_size,
                        2,
                        1,
                        FixedSync::Input,
                    )
                    .expect("Failed to create resampler");
                    let input_frames = pcm_samples.len();
                    let input_data = vec![pcm_samples.clone()];
                    let input = SequentialSliceOfVecs::new(&input_data, 1, input_frames)
                        .expect("Failed to create input adapter");
                    let output_len = resampler.process_all_needed_output_len(input_frames);
                    let mut output_data = vec![vec![0.0f32; output_len]; 1];
                    let mut output =
                        SequentialSliceOfVecs::new_mut(&mut output_data, 1, output_len)
                            .expect("Failed to create output adapter");
                    let (_, nbr_out) = resampler
                        .process_all_into_buffer(&input, &mut output, input_frames, None)
                        .expect("Resampling failed");
                    let all_output = output_data[0][..nbr_out].to_vec();
                    (all_output, encode_sr as i32)
                } else {
                    (pcm_samples.clone(), pcm_sample_rate)
                };

                let audio_packets = encode_sample_to_tts_packet(
                    opus_pcm,
                    &mut encoder,
                    encode_sr,
                    output_channel,
                    output_frame_duration,
                );

                let packet = TtsPacket {
                    audio: audio_packets,
                    text,
                };

                if tx.send(Ok(packet)).await.is_err() {
                    break;
                }
            }
        });

        Box::pin(ReceiverStream::new(rx))
    }
}

/// Auto-discover an ONNX file in `dir` matching a known prefix.
fn auto_discover_onnx(dir: &str, prefix: &str) -> Option<String> {
    let p = std::path::Path::new(dir);
    std::fs::read_dir(p).ok().and_then(|mut entries| {
        entries.find_map(|entry| {
            entry.ok().and_then(|e| {
                let path = e.path();
                if path.extension().is_some_and(|ext| ext == "onnx")
                    && path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|stem| stem == prefix)
                {
                    path.to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
    })
}

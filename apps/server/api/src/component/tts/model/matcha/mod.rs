use async_trait::async_trait;
use framework::error::AppError;
use futures::Stream;
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use service::component::tts::{Tts, TtsPacket};
use service::pipeline::AudioSpec;
use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsMatchaModelConfig,
    OfflineTtsModelConfig,
};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::component::tts::StreamingOpusEncoder;
use crate::config::audio::AudioConfig;
use crate::config::tts::TtsConfig;

const RESAMPLER_CHUNK_SIZE: usize = 4096;
const RESAMPLER_SUB_CHUNKS: usize = 1;
const RESAMPLER_CHANNELS: usize = 1;

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
    fn audio_spec(&self) -> AudioSpec {
        AudioSpec {
            sample_rate: self.output_sample_rate,
            channel: self.output_channel,
            frame_duration_ms: self.output_frame_duration,
        }
    }

    async fn stream(
        &self,
        mut text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>> {
        let (tx, rx) = mpsc::channel::<Result<TtsPacket, AppError>>(256);

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

            let mut total_ttfa_ms: u64 = 0;
            let mut total_rtf_sum: f64 = 0.0;
            let mut sentence_count: u32 = 0;
            let pipeline_start = Instant::now();

            while let Some(text) = text_stream.next().await {
                if cancel.is_cancelled() {
                    break;
                }

                let tts_clone = tts.clone();
                let text_clone = text.clone();
                let tx_clone = tx.clone();
                let cancel_clone = cancel.clone();

                let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>(64);

                let gen_handle = tokio::task::spawn_blocking(move || {
                    let gen_config = GenerationConfig {
                        speed,
                        ..Default::default()
                    };
                    let audio = tts_clone.generate_with_config(
                        &text_clone,
                        &gen_config,
                        Some(move |samples: &[f32], _progress: f32| -> bool {
                            if !samples.is_empty() {
                                let _ = sample_tx.blocking_send(samples.to_vec());
                            }
                            true
                        }),
                    );
                    audio.is_some()
                });

                let pcm_sample_rate = tts.sample_rate() as u32;
                let needs_resample = pcm_sample_rate != encode_sr;
                let tts_sentence_start = Instant::now();

                let mut resampler = if needs_resample {
                    Some(
                        Fft::<f32>::new(
                            pcm_sample_rate as usize,
                            encode_sr as usize,
                            RESAMPLER_CHUNK_SIZE,
                            RESAMPLER_SUB_CHUNKS,
                            RESAMPLER_CHANNELS,
                            FixedSync::Input,
                        )
                        .expect("Failed to create resampler"),
                    )
                } else {
                    None
                };

                let mut encoder = match StreamingOpusEncoder::new(
                    encode_sr,
                    opus_channels,
                    ropus::Application::Audio,
                    output_frame_duration,
                ) {
                    Ok(e) => e,
                    Err(err) => {
                        tracing::error!(
                            component = "TTS", event = "encoder_error",
                            error = %err, "opus encoder creation error"
                        );
                        let _ = gen_handle.await;
                        continue;
                    }
                };

                let mut resampler_buf: Vec<f32> = Vec::new();
                let mut sample_rx = sample_rx;
                let mut first_frame_instant: Option<Instant> = None;
                let mut sentence_started = false;
                let mut total_samples: usize = 0;

                while let Some(pcm_samples) = sample_rx.recv().await {
                    if cancel_clone.is_cancelled() {
                        break;
                    }

                    let resampled = if let Some(resampler) = resampler.as_mut() {
                        resampler_buf.extend_from_slice(&pcm_samples);
                        let mut all_resampled = Vec::new();
                        while resampler_buf.len() >= RESAMPLER_CHUNK_SIZE {
                            let chunk: Vec<f32> =
                                resampler_buf.drain(..RESAMPLER_CHUNK_SIZE).collect();
                            let r = process_with_resampler(resampler, &chunk, 1);
                            all_resampled.extend_from_slice(&r);
                        }
                        all_resampled
                    } else {
                        pcm_samples
                    };

                    total_samples += resampled.len();
                    let opus_frames = encoder.push_samples(&resampled);

                    for frame in &opus_frames {
                        if first_frame_instant.is_none() {
                            first_frame_instant = Some(Instant::now());
                        }
                        let is_first = !sentence_started;
                        if is_first {
                            sentence_started = true;
                        }
                        let packet = TtsPacket {
                            text: text.clone().into(),
                            audio: vec![frame.clone()],
                            is_first,
                            is_last: false,
                        };
                        if tx_clone.send(Ok(packet)).await.is_err() {
                            let _ = gen_handle.await;
                            return;
                        }
                    }
                }

                if let Some(resampler) = resampler.as_mut()
                    && !resampler_buf.is_empty()
                {
                    let remaining = std::mem::take(&mut resampler_buf);
                    let resampled = process_with_resampler(resampler, &remaining, 1);
                    total_samples += resampled.len();
                    let opus_frames = encoder.push_samples(&resampled);
                    for frame in &opus_frames {
                        if first_frame_instant.is_none() {
                            first_frame_instant = Some(Instant::now());
                        }
                        let is_first = !sentence_started;
                        if is_first {
                            sentence_started = true;
                        }
                        let packet = TtsPacket {
                            text: text.clone().into(),
                            audio: vec![frame.clone()],
                            is_first,
                            is_last: false,
                        };
                        if tx_clone.send(Ok(packet)).await.is_err() {
                            let _ = gen_handle.await;
                            return;
                        }
                    }
                }

                let gen_time = tts_sentence_start.elapsed();

                let flush_frames = encoder.flush();
                for (i, frame) in flush_frames.iter().enumerate() {
                    if first_frame_instant.is_none() {
                        first_frame_instant = Some(Instant::now());
                    }
                    let is_first = !sentence_started && i == 0;
                    if is_first {
                        sentence_started = true;
                    }
                    let packet = TtsPacket {
                        text: text.clone().into(),
                        audio: vec![frame.clone()],
                        is_first,
                        is_last: true,
                    };
                    if tx_clone.send(Ok(packet)).await.is_err() {
                        let _ = gen_handle.await;
                        return;
                    }
                }
                sentence_started = true;

                // If no frames were produced at all, send an empty packet
                if !sentence_started {
                    let packet = TtsPacket {
                        text: text.clone().into(),
                        audio: vec![],
                        is_first: true,
                        is_last: true,
                    };
                    let _ = tx_clone.send(Ok(packet)).await;
                }

                let text_len = text.len();
                let ttfa_ms = first_frame_instant
                    .map(|t| t.duration_since(tts_sentence_start).as_millis() as u64)
                    .unwrap_or(0);
                let audio_duration_secs = total_samples as f64 / encode_sr as f64;
                let rtf = if audio_duration_secs > 0.0 {
                    gen_time.as_secs_f64() / audio_duration_secs
                } else {
                    0.0
                };

                total_ttfa_ms += ttfa_ms;
                total_rtf_sum += rtf;
                sentence_count += 1;

                info!(
                    component = "TTS",
                    event = "sentence_metrics",
                    ttfa_ms,
                    rtf = format!("{rtf:.3}"),
                    audio_duration_ms = (audio_duration_secs * 1000.0) as u64,
                    gen_time_ms = gen_time.as_millis() as u64,
                    text_len,
                    "TTS sentence metrics"
                );

                let _ = gen_handle.await;
            }

            let total_time = pipeline_start.elapsed();
            let avg_ttfa = if sentence_count > 0 {
                total_ttfa_ms / sentence_count as u64
            } else {
                0
            };
            let avg_rtf = if sentence_count > 0 {
                total_rtf_sum / sentence_count as f64
            } else {
                0.0
            };
            info!(
                component = "TTS",
                event = "pipeline_complete",
                total_time_ms = total_time.as_millis() as u64,
                sentences = sentence_count,
                avg_ttfa_ms = avg_ttfa,
                avg_rtf = format!("{avg_rtf:.3}"),
                "TTS pipeline complete"
            );
        });

        Box::pin(ReceiverStream::new(rx))
    }
}

fn process_with_resampler(resampler: &mut Fft<f32>, chunk: &[f32], channels: usize) -> Vec<f32> {
    if chunk.is_empty() {
        return Vec::new();
    }
    let chunk_size = chunk.len();
    let input_data = vec![chunk.to_vec()];
    let input = SequentialSliceOfVecs::new(&input_data, channels, chunk_size)
        .expect("Failed to create input adapter");

    let chunk_size_out = resampler.output_frames_next();
    let mut output_data = vec![vec![0.0f32; chunk_size_out]; channels];
    let mut output = SequentialSliceOfVecs::new_mut(&mut output_data, channels, chunk_size_out)
        .expect("Failed to create output adapter");

    let indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        active_channels_mask: None,
        partial_len: Some(chunk_size),
    };
    let (_nbr_in, nbr_out) = resampler
        .process_into_buffer(&input, &mut output, Some(&indexing))
        .expect("Resampling failed");
    output_data[0][..nbr_out].to_vec()
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

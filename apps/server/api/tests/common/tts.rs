#![allow(dead_code)]

use std::fmt;
use std::path::Path;
use std::sync::LazyLock;
use std::thread;

use api::config::{audio::AudioConfig, tts::TtsConfig};
use futures::{Stream, executor::block_on};
use oximedia_audio_analysis::{AnalysisConfig, AudioAnalyzer};
use tokio::sync::mpsc::channel;
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tracing::info;

/// Test text covering Chinese, English, and numeric patterns (rule FST + OOV scenarios).
pub const TEST_TTS_TEXT: &str = "2024年5月11号，拨打110或者18920240511，花了99块钱。我在学习machine learning和artificial intelligence。";

/// Weight of `TEST_TTS_TEXT` in the OmniVoice RuleDurationEstimator weight system.
pub static TEST_TTS_TEXT_WEIGHT: LazyLock<f64> = LazyLock::new(|| {
    TEST_TTS_TEXT
        .chars()
        .map(|c| match c as u32 {
            0x30..=0x39 => 3.5,
            0x41..=0x5A | 0x61..=0x7A => 1.0,
            0xC0..=0x024F => 1.0,
            0x20 | 0x3000 => 0.2,
            0x21..=0x2F | 0x3A..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E | 0x3001..=0x303F => 0.5,
            0x3040..=0x309F | 0x30A0..=0x30FF => 2.5,
            0xAC00..=0xD7AF | 0x1100..=0x11FF => 2.2,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF => 3.0,
            _ => 1.0,
        })
        .sum::<f64>()
});

/// Estimate standard speech duration from text content alone.
pub fn estimate_std_duration(text: &str) -> f64 {
    const STANDARD_SPEED_FACTOR: f64 = 12.0;
    let weight: f64 = text
        .chars()
        .map(|c| match c as u32 {
            0x30..=0x39 => 3.5,
            0x41..=0x5A | 0x61..=0x7A => 1.0,
            0xC0..=0x024F => 1.0,
            0x20 | 0x3000 => 0.2,
            0x21..=0x2F | 0x3A..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E | 0x3001..=0x303F => 0.5,
            0x3040..=0x309F | 0x30A0..=0x30FF => 2.5,
            0xAC00..=0xD7AF | 0x1100..=0x11FF => 2.2,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF => 3.0,
            _ => 1.0,
        })
        .sum();
    weight / STANDARD_SPEED_FACTOR
}

/// TTS audio diagnostics.
#[derive(Debug)]
pub struct TtsAudioDiagnostics {
    pub num_samples: usize,
    pub duration_secs: f64,
    pub shimmer_pct: f64,
    pub dynamic_range_db: f64,
    pub gen_elapsed_secs: f64,
    pub rtf: f64,
    pub std_duration_secs: f64,
    pub std_diff_secs: f64,
    pub glitch_count: usize,
    pub clipping_count: usize,
    pub crest_factor_db: f64,
    pub energy_variance: f64,
    pub spectral_centroid_hz: f64,
    pub spectral_flatness: f64,
    pub spectral_rolloff_hz: f64,
    pub zero_crossing_rate: f64,
    pub dc_offset: f64,
    pub snr_db: f64,
    pub leading_silence_ms: f64,
    pub trailing_silence_ms: f64,
}

impl TtsAudioDiagnostics {
    pub fn shimmer_grade(&self) -> &'static str {
        match self.shimmer_pct {
            s if s < 3.81 => "A",
            s if s < 5.0 => "B",
            s if s < 6.0 => "C",
            s if s < 10.0 => "D",
            _ => "F",
        }
    }

    pub fn dr_grade(&self) -> &'static str {
        match self.dynamic_range_db {
            d if d > 20.0 => "A",
            d if d > 15.0 => "C",
            _ => "F",
        }
    }

    pub fn audio_score(&self) -> f64 {
        let s = if self.shimmer_pct < 3.81 {
            100.0
        } else if self.shimmer_pct < 5.0 {
            lerp(100.0, 75.0, (self.shimmer_pct - 3.81) / (5.0 - 3.81))
        } else if self.shimmer_pct < 6.0 {
            lerp(75.0, 50.0, (self.shimmer_pct - 5.0) / (6.0 - 5.0))
        } else if self.shimmer_pct < 10.0 {
            lerp(50.0, 25.0, (self.shimmer_pct - 6.0) / (10.0 - 6.0))
        } else {
            0.0
        };
        let d = if self.dynamic_range_db > 20.0 {
            100.0
        } else if self.dynamic_range_db > 15.0 {
            lerp(0.0, 100.0, (self.dynamic_range_db - 15.0) / (20.0 - 15.0))
        } else {
            0.0
        };
        s * 0.7 + d * 0.3
    }

    pub fn performance_score(&self) -> f64 {
        match self.rtf {
            r if r < 0.1 => 100.0,
            r if r < 0.3 => lerp(100.0, 80.0, (r - 0.1) / 0.2),
            r if r < 0.5 => lerp(80.0, 60.0, (r - 0.3) / 0.2),
            r if r < 1.0 => lerp(60.0, 0.0, (r - 0.5) / 0.5),
            _ => 0.0,
        }
    }

    pub fn timing_score(&self) -> f64 {
        let deviation = (self.std_diff_secs / self.std_duration_secs).abs();
        match deviation {
            d if d < 0.05 => 100.0,
            d if d < 0.20 => lerp(100.0, 80.0, (d - 0.05) / 0.15),
            d if d < 0.50 => lerp(80.0, 40.0, (d - 0.20) / 0.30),
            d if d < 1.00 => lerp(40.0, 0.0, (d - 0.50) / 0.50),
            _ => 0.0,
        }
    }

    pub fn score_grade(score: f64) -> &'static str {
        if score >= 86.0 {
            "A"
        } else if score >= 66.0 {
            "B"
        } else if score >= 41.0 {
            "C"
        } else if score >= 21.0 {
            "D"
        } else {
            "F"
        }
    }

    pub fn audio_grade(&self) -> &'static str {
        Self::score_grade(self.audio_score())
    }

    pub fn performance_grade(&self) -> &'static str {
        Self::score_grade(self.performance_score())
    }

    pub fn timing_grade(&self) -> &'static str {
        Self::score_grade(self.timing_score())
    }

    pub fn verdict(&self) -> &'static str {
        match self.shimmer_pct {
            s if s >= 10.0 => {
                "Unsuitable for daily use — shimmer exceeds algorithm reliability limit"
            }
            s if s >= 6.0 => "Marginal — shimmer in pathological range (>6%), noticeable roughness",
            s if s >= 5.0 => "Marginal — shimmer in warning zone (5–6%), slight tremor",
            _ => match self.dynamic_range_db {
                d if d < 10.0 => "Marginal — dynamic range too low (<10dB), flat audio",
                d if d < 15.0 => "Marginal — dynamic range narrow (10–15dB), compressed sound",
                _ => "Suitable for daily use — all indicators within normal range",
            },
        }
    }
}

fn lerp(start: f64, end: f64, t: f64) -> f64 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

impl fmt::Display for TtsAudioDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dev_pct = self.std_diff_secs / self.std_duration_secs * 100.0;
        write!(
            f,
            "Audio:scr={:.0}({}) Perf:scr={:.0}({}) Timing:scr={:.0}({}) | \
             sh={:.2}%({}) dr={:.1}dB({}) rtf={:.2} gen={:.1}s dur={:.2}s(std={:.1}s{:+.0}%) \
             glitch={} clip={} cf={:.1}dB ev={:.4} sc={:.0}Hz sf={:.4} sr={:.0}Hz \
             zcr={:.4} dc={:.5} snr={:.1}dB lead={:.0}ms trail={:.0}ms {}",
            self.audio_score(),
            self.audio_grade(),
            self.performance_score(),
            self.performance_grade(),
            self.timing_score(),
            self.timing_grade(),
            self.shimmer_pct,
            self.shimmer_grade(),
            self.dynamic_range_db,
            self.dr_grade(),
            self.rtf,
            self.gen_elapsed_secs,
            self.duration_secs,
            self.std_duration_secs,
            dev_pct,
            self.glitch_count,
            self.clipping_count,
            self.crest_factor_db,
            self.energy_variance,
            self.spectral_centroid_hz,
            self.spectral_flatness,
            self.spectral_rolloff_hz,
            self.zero_crossing_rate,
            self.dc_offset,
            self.snr_db,
            self.leading_silence_ms,
            self.trailing_silence_ms,
            self.verdict(),
        )
    }
}

/// 对解码后 PCM 做完整音频诊断。
pub fn analyze_audio(
    samples: &[f32],
    sample_rate: u32,
    gen_elapsed: std::time::Duration,
    std_duration_secs: f64,
) -> TtsAudioDiagnostics {
    let window = 160; // 10ms @ 16kHz
    let mut rms: Vec<f32> = samples
        .chunks(window)
        .map(|chunk| {
            let sq_sum: f32 = chunk.iter().map(|s| s * s).sum();
            (sq_sum / chunk.len() as f32).sqrt()
        })
        .collect();

    let peak = rms.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    rms.retain(|r| *r > peak * 0.05);

    let shimmer_pct = if rms.len() < 2 {
        0.0
    } else {
        let mean = rms.iter().sum::<f32>() / rms.len() as f32;
        if mean < 1e-10 {
            0.0
        } else {
            let sum_diff: f32 = rms.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
            let mean_diff = sum_diff / (rms.len() - 1) as f32;
            (mean_diff / mean * 100.0) as f64
        }
    };

    let dynamic_range_db = if rms.len() < 2 {
        0.0
    } else {
        let max_rms = rms.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min_rms = rms.iter().cloned().fold(f32::INFINITY, f32::min);
        if min_rms < 1e-10 {
            0.0
        } else {
            (20.0 * (max_rms / min_rms).log10()) as f64
        }
    };

    let frame_size = (sample_rate as f32 * 0.020) as usize;
    let energy_var = if samples.len() >= frame_size * 2 {
        let energies: Vec<f32> = samples
            .chunks(frame_size)
            .map(|chunk| chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32)
            .collect();
        let mean = energies.iter().sum::<f32>() / energies.len() as f32;
        energies.iter().map(|e| (*e - mean).powi(2)).sum::<f32>() / energies.len() as f32
    } else {
        0.0
    };

    let sr_f = sample_rate as f32;
    let config = AnalysisConfig::default();
    let analyzer = AudioAnalyzer::new(config);
    let result = analyzer.analyze(samples, sr_f).unwrap_or_else(|_| {
        use oximedia_audio_analysis::*;
        AnalysisResult {
            spectral: spectral::SpectralFeatures {
                centroid: 0.0,
                flatness: 0.0,
                crest: 0.0,
                bandwidth: 0.0,
                rolloff: 0.0,
                flux: 0.0,
                magnitude_spectrum: vec![],
            },
            pitch: pitch::PitchResult {
                estimates: vec![],
                confidences: vec![],
                mean_f0: 0.0,
                voicing_rate: 0.0,
            },
            formants: formant::FormantResult {
                formants: vec![],
                lpc_coefficients: vec![],
            },
            dynamics: dynamics::DynamicsResult {
                peak: 0.0,
                rms: 0.0,
                crest: 0.0,
                dynamic_range_db: 0.0,
                loudness_variation: 0.0,
                rms_over_time: vec![],
            },
            transients: transient::TransientResult {
                transient_times: vec![],
                onset_strength: vec![],
                num_transients: 0,
                avg_strength: 0.0,
            },
            voice: None,
        }
    });

    let spectral_centroid_hz = result.spectral.centroid as f64;
    let spectral_flatness = result.spectral.flatness as f64;
    let spectral_rolloff_hz = result.spectral.rolloff as f64;

    let crest_factor_db = result.dynamics.crest as f64;
    let _ = dynamic_range_db;

    let clipping_result =
        oximedia_audio_analysis::distortion::clipping::detect_clipping(samples, 0.999);
    let clipping_count = clipping_result.clipped_samples;

    let glitch_count = result.transients.num_transients;

    let zcr = oximedia_audio_analysis::energy::zero_crossing_rate(samples);

    let dc_offset = if samples.is_empty() {
        0.0
    } else {
        let sum: f64 = samples.iter().map(|&s| s as f64).sum();
        sum / samples.len() as f64
    };

    let noise_samples = (sample_rate as usize / 10).min(samples.len());
    let signal_start = noise_samples;
    let signal_end = samples.len();
    let snr_db = if signal_start < signal_end {
        let noise_power: f64 = samples[..noise_samples]
            .iter()
            .map(|&s| (s as f64).powi(2))
            .sum::<f64>()
            / noise_samples.max(1) as f64;
        let signal_len = signal_end - signal_start;
        let signal_power: f64 = samples[signal_start..signal_end]
            .iter()
            .map(|&s| (s as f64).powi(2))
            .sum::<f64>()
            / signal_len.max(1) as f64;
        if noise_power > 1e-20 {
            10.0 * (signal_power / noise_power).log10()
        } else {
            100.0 // effectively infinite SNR
        }
    } else {
        0.0
    };

    let silence_regions =
        oximedia_audio_analysis::energy::detect_silence_regions(samples, sample_rate, -40.0, 20);
    let leading_silence_ms = if let Some(&(start, end)) = silence_regions.first() {
        if start == 0 {
            end as f64 / sample_rate as f64 * 1000.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    let trailing_silence_ms = if let Some(&(start, end)) = silence_regions.last() {
        let total_samples = samples.len();
        if end >= total_samples - sample_rate as usize / 100 {
            (total_samples - start) as f64 / sample_rate as f64 * 1000.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    TtsAudioDiagnostics {
        num_samples: samples.len(),
        duration_secs: samples.len() as f64 / sample_rate as f64,
        shimmer_pct,
        dynamic_range_db,
        gen_elapsed_secs: gen_elapsed.as_secs_f64(),
        rtf: gen_elapsed.as_secs_f64() / (samples.len() as f64 / sample_rate as f64),
        std_duration_secs,
        std_diff_secs: samples.len() as f64 / sample_rate as f64 - std_duration_secs,
        glitch_count,
        clipping_count,
        crest_factor_db,
        energy_variance: energy_var as f64,
        spectral_centroid_hz,
        spectral_flatness,
        spectral_rolloff_hz,
        zero_crossing_rate: zcr as f64,
        dc_offset,
        snr_db,
        leading_silence_ms,
        trailing_silence_ms,
    }
}

/// Monorepo root path (3 levels up from `CARGO_MANIFEST_DIR`).
pub fn ws_root() -> &'static std::path::PathBuf {
    static ROOT: LazyLock<std::path::PathBuf> = LazyLock::new(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    &ROOT
}

/// Collect .fst rule files from a model directory, return comma-separated paths (or None).
pub fn collect_rule_fsts(dir: &std::path::Path) -> Option<String> {
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
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
    (!files.is_empty()).then(|| files.join(","))
}

/// Resample → Opus encode → Opus decode → return decoded PCM at `encode_sr`.
pub fn opus_pipeline(samples: &[f32], sample_rate: i32, encode_sr: u32) -> Vec<f32> {
    use rubato::Resampler;
    use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
    let channels = 1_usize;
    let chunk_size = 4096.min(samples.len());

    let (pcm, sr) = if sample_rate as u32 != encode_sr {
        let mut resampler = rubato::Fft::<f32>::new(
            sample_rate as usize,
            encode_sr as usize,
            chunk_size,
            1,
            1,
            rubato::FixedSync::Input,
        )
        .expect("Failed to create resampler");
        let input_frames = samples.len();
        let input_data = vec![samples.to_vec()];
        let input = SequentialSliceOfVecs::new(&input_data, 1, input_frames)
            .expect("Failed to create input adapter");
        let output_len = resampler.process_all_needed_output_len(input_frames);
        let mut output_data = vec![vec![0.0f32; output_len]; 1];
        let mut output = SequentialSliceOfVecs::new_mut(&mut output_data, 1, output_len)
            .expect("Failed to create output adapter");
        let (_, nbr_out) = resampler
            .process_all_into_buffer(&input, &mut output, input_frames, None)
            .expect("Resampling failed");
        let all_output = output_data[0][..nbr_out].to_vec();
        (all_output, encode_sr)
    } else {
        (samples.to_vec(), sample_rate as u32)
    };

    let opus_channels = if channels == 2 {
        ropus::Channels::Stereo
    } else {
        ropus::Channels::Mono
    };
    let mut encoder = ropus::Encoder::builder(sr as u32, opus_channels, ropus::Application::Audio)
        .build()
        .expect("Failed to create Opus encoder");
    let frame_dur = 20u64;
    let packet_size = sr as usize * channels * frame_dur as usize / 1000;
    let count = pcm.len().div_ceil(packet_size);
    let mut packets = Vec::with_capacity(count);
    for n in 0..count {
        let start = n * packet_size;
        let end = std::cmp::min(start + packet_size, pcm.len());
        let mut frame: Vec<f32> = pcm[start..end].to_vec();
        frame.resize(packet_size, 0.0);
        let mut buf = vec![0u8; packet_size * 4];
        let written = encoder.encode_float(&frame, &mut buf).unwrap();
        buf.truncate(written);
        packets.push(buf);
    }

    let mut decoder = ropus::Decoder::new(sr as u32, opus_channels).unwrap();
    let mut decoded = Vec::new();
    for pkt in &packets {
        let mut samples = vec![0f32; packet_size];
        if let Ok(len) = decoder.decode_float(pkt, &mut samples, ropus::DecodeMode::Normal) {
            decoded.extend_from_slice(&samples[..len]);
        }
    }
    decoded
}

/// Standard AudioConfig for TTS tests: 16kHz / mono / 20ms frame duration.
pub fn test_audio_config() -> AudioConfig {
    AudioConfig {
        output_sample_rate: Some(16000),
        output_channel: Some(1),
        output_frame_duration: Some(20),
        ..Default::default()
    }
}

/// Shared TTS test helper: create model → stream inference → Opus decode → write WAV.
pub async fn run_tts_test(
    tts_config: &TtsConfig,
    audio_config: &AudioConfig,
    wav: &str,
) -> anyhow::Result<()> {
    let tts = api::tts::TtsManager::create_model(tts_config, audio_config).await?;
    let text_stream = tts_stream(String::from(TEST_TTS_TEXT));
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut tts_stream = tts.stream(Box::pin(text_stream), cancel).await;

    let gen_start = std::time::Instant::now();
    let mut all_packets: Vec<Vec<u8>> = Vec::new();
    while let Some(data) = tts_stream.next().await {
        match data {
            Ok(data) => {
                info!("text: {}", data.text);
                all_packets.extend(data.audio);
            }
            Err(e) => panic!("{:?}", e),
        }
    }
    let gen_elapsed = gen_start.elapsed();
    anyhow::ensure!(!all_packets.is_empty(), "Expected audio packets from TTS");

    let decode_fs = 320;
    let mut decoder = ropus::Decoder::new(16000, ropus::Channels::Mono).unwrap();
    let mut decoded = Vec::new();
    for packet in &all_packets {
        let mut samples = vec![0f32; decode_fs];
        if let Ok(len) = decoder.decode_float(packet, &mut samples, ropus::DecodeMode::Normal) {
            decoded.extend_from_slice(&samples[..len]);
        }
    }
    anyhow::ensure!(decoded.len() > 1000, "Decoded audio too short");
    let std_dur = estimate_std_duration(TEST_TTS_TEXT);
    let diag = analyze_audio(&decoded, 16000, gen_elapsed, std_dur);
    info!("{diag}");

    assert!(
        diag.glitch_count <= decoded.len() / 16000,
        "Too many glitches: {} (max ~1/sec), audio likely has clicks",
        diag.glitch_count
    );
    assert_eq!(
        diag.clipping_count, 0,
        "Clipping detected — normalization may be broken"
    );
    assert!(
        diag.crest_factor_db > 3.0,
        "Crest factor too low ({:.1} dB) — audio may be flat/compressed",
        diag.crest_factor_db
    );
    assert!(
        diag.energy_variance > 1e-8,
        "Energy variance too low ({:.6}) — audio may be smearing from crossfade artifacts",
        diag.energy_variance
    );
    assert!(
        diag.spectral_centroid_hz > 200.0,
        "Spectral centroid too low ({:.0} Hz) — high-frequency loss, possible electronic artifacts",
        diag.spectral_centroid_hz
    );
    assert!(
        diag.spectral_flatness < 0.5,
        "Spectral flatness too high ({:.4}) — audio may be noise-like rather than speech",
        diag.spectral_flatness
    );
    assert!(
        diag.dc_offset.abs() < 0.01,
        "DC offset too high ({:.5}) — possible DC bias in output",
        diag.dc_offset
    );

    std::fs::create_dir_all("./test_data")?;
    let _ = wavers::write(wav, &decoded, 16000, 1);
    Ok(())
}

/// Create a TTS input stream from a text string.
pub fn tts_stream(text: String) -> impl Stream<Item = String> + Unpin + Send + 'static {
    let (tx, rx) = channel::<String>(10);
    thread::spawn(move || {
        block_on(async move {
            let _ = tx.send(text).await;
            drop(tx);
        })
    });
    ReceiverStream::new(rx)
}

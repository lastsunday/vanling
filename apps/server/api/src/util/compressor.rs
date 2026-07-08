use ebur128::{EbuR128, Mode};
use serde::Deserialize;

/// Soft-knee compressor configuration.
///
/// If `knee_db` is 0, the compressor behaves as a hard-knee (standard)
/// compressor with a sharp transition at the threshold.
///
/// # Example (providing all fields):
/// ```json
/// { "threshold_db": -20.0, "ratio": 4.0, "attack_ms": 2.0, "release_ms": 150.0, "makeup_gain_db": 8.0, "knee_db": 6.0 }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct CompressorConfig {
    /// Threshold in dB. Compression starts when the envelope exceeds this.
    /// Typical: -24.0 to -12.0
    pub threshold_db: f32,

    /// Compression ratio (e.g. 4.0 means 4:1). Higher = more compression.
    /// Typical: 2.0 to 8.0
    pub ratio: f32,

    /// Attack time in milliseconds. How fast the compressor responds.
    /// Typical: 1.0 to 5.0
    pub attack_ms: f32,

    /// Release time in milliseconds. How fast the compressor recovers.
    /// Typical: 50.0 to 200.0
    pub release_ms: f32,

    /// Makeup gain in dB applied after compression.
    /// Typical: 3.0 to 12.0
    pub makeup_gain_db: f32,

    /// Soft knee width in dB. Smooths compression onset around the threshold.
    /// 0 = hard knee (sharp transition). Typical: 3.0 to 6.0
    #[serde(default)]
    pub knee_db: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            threshold_db: -28.0,
            ratio: 6.0,
            attack_ms: 5.0,
            release_ms: 80.0,
            makeup_gain_db: 8.0,
            knee_db: 0.0,
        }
    }
}

/// Apply dynamic range compression to PCM samples.
///
/// Uses a standard feed-forward compressor with a peak-detector envelope
/// follower and optional soft knee. The output is clamped to [-1, 1].
pub fn pcm_compress(samples: &[f32], sample_rate: u32, config: &CompressorConfig) -> Vec<f32> {
    let sample_rate_f = sample_rate.max(1) as f32;
    let attack = (-1.0 / (sample_rate_f * config.attack_ms / 1000.0)).exp();
    let release = (-1.0 / (sample_rate_f * config.release_ms / 1000.0)).exp();
    let makeup_linear = 10.0_f32.powf(config.makeup_gain_db / 20.0);

    let knee_half = config.knee_db / 2.0;
    let slope = 1.0 / config.ratio - 1.0;

    let mut envelope = 0.0_f32;
    let mut output = Vec::with_capacity(samples.len());

    for &sample in samples {
        let input_abs = sample.abs();
        let coeff = if input_abs > envelope {
            attack
        } else {
            release
        };
        envelope += coeff * (input_abs - envelope);

        let gain_db = if envelope > 0.0 {
            let env_db = 20.0 * envelope.log10();
            compute_gain_db(env_db, config.threshold_db, slope, knee_half)
        } else {
            0.0
        };

        let gain_linear = 10.0_f32.powf(gain_db / 20.0);
        let out = sample * gain_linear * makeup_linear;
        output.push(out.clamp(-1.0, 1.0));
    }

    output
}

/// Result of objective audio evaluation using EBU R128 metrics.
#[derive(Debug, Clone)]
pub struct AudioMetrics {
    /// Loudness Range in LU (lower = more consistent volume)
    pub lra: f64,
    /// Integrated loudness in LUFS
    pub lufs: f64,
    /// Crest factor (peak-to-RMS ratio) in dB
    pub crest_factor_db: f32,
}

/// Evaluate compressed audio using EBU R128 metrics.
///
/// Returns (LRA, integrated LUFS, crest factor).
pub fn evaluate_compressed(samples: &[f32], sample_rate: u32) -> anyhow::Result<AudioMetrics> {
    let mut meter = EbuR128::new(1, sample_rate, Mode::I | Mode::LRA | Mode::SAMPLE_PEAK)?;
    meter.set_channel(0, ebur128::Channel::Left)?;
    meter.add_frames_f32(samples)?;

    let lra = meter.loudness_range()?;
    let lufs = meter.loudness_global()?;
    let peak = meter.sample_peak(0)?;

    let rms = (samples.iter().map(|s| (s * s) as f64).sum::<f64>() / samples.len() as f64).sqrt();
    let crest = if rms > 0.0 && peak > 0.0 {
        20.0 * (peak / rms).log10()
    } else {
        0.0
    };

    Ok(AudioMetrics {
        lra,
        lufs,
        crest_factor_db: crest as f32,
    })
}

/// Run a grid search over compressor parameters to find the best combination.
///
/// The scoring minimizes LRA while penalizing over-compression (crest factor < 6 dB).
/// Returns sorted results from best to worst.
pub fn grid_search_compressor(
    samples: &[f32],
    sample_rate: u32,
) -> anyhow::Result<Vec<(CompressorConfig, AudioMetrics)>> {
    let thresholds = [-16.0, -18.0, -20.0, -22.0, -24.0, -26.0, -28.0];
    let ratios = [2.0, 3.0, 4.0, 5.0, 6.0];
    let knees = [0.0, 3.0, 6.0, 9.0];
    let attacks = [1.0, 2.0, 5.0];
    let releases = [80.0, 120.0, 150.0, 200.0];
    let makeups = [8.0];

    let mut results = Vec::new();

    for &threshold in &thresholds {
        for &ratio in &ratios {
            for &knee in &knees {
                for &attack in &attacks {
                    for &release in &releases {
                        for &makeup in &makeups {
                            let cfg = CompressorConfig {
                                threshold_db: threshold,
                                ratio,
                                attack_ms: attack,
                                release_ms: release,
                                makeup_gain_db: makeup,
                                knee_db: knee,
                            };

                            let compressed = pcm_compress(samples, sample_rate, &cfg);
                            match evaluate_compressed(&compressed, sample_rate) {
                                Ok(metrics) => {
                                    results.push((cfg, metrics));
                                }
                                Err(e) => {
                                    tracing::warn!("Evaluation failed for {:?}: {}", cfg, e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Score: lower LRA is better, but penalize crest factor < 6 dB
    results.sort_by(|(_, a), (_, b)| {
        let score_a = if a.crest_factor_db < 6.0 {
            a.lra + 100.0
        } else {
            a.lra
        };
        let score_b = if b.crest_factor_db < 6.0 {
            b.lra + 100.0
        } else {
            b.lra
        };
        score_a
            .partial_cmp(&score_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

/// Adaptive loudness normalization — zero configuration.
///
/// Analyzes the signal's frame-level RMS distribution, then applies a smoothly
/// varying gain envelope so that most frames land near the 30th-percentile RMS
/// level.  This automatically reduces loud sections (e.g. English-mixed second
/// half) and gently boosts quiet ones, without any user-supplied parameters.
///
/// # Algorithm
///
/// 1. Slice audio into 200 ms frames (10 ms hop) and compute the RMS of each.
/// 2. Take the 30th percentile RMS as the **target** level.
/// 3. Derive a per-frame desired gain = target / frame\_rms, capped to ±12 dB.
/// 4. Smooth the gain envelope sample-by-sample with a fast attack (5 ms) and
///    slow release (300 ms) so there is no audible pumping.
/// 5. Clamp the output to [-1, 1] and apply a final soft-limiter at −0.5 dBFS.
pub fn adaptive_normalize(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }

    let frame_len = (sample_rate as f32 * 0.200) as usize; // 200 ms analysis window
    let hop_len = (sample_rate as f32 * 0.010) as usize; // 10 ms step
    let frame_len = frame_len.max(1);
    let hop_len = hop_len.max(1);

    // 1. Compute per-frame RMS
    let num_frames = samples.len().saturating_sub(frame_len) / hop_len + 1;
    let mut frame_rms = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let start = i * hop_len;
        let end = start + frame_len;
        let frame = &samples[start..end];
        let sum_sq = frame.iter().map(|s| s * s).sum::<f32>();
        let rms = (sum_sq / frame.len() as f32).sqrt();
        frame_rms.push(rms);
    }

    // 2. Target = 30th-percentile RMS (so 70 % of frames are at or above it)
    let mut sorted = frame_rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let target_idx = ((sorted.len() as f32) * 0.30) as usize;
    let target_rms = sorted
        .get(target_idx)
        .copied()
        .unwrap_or(1e-10_f32)
        .max(1e-10);

    // 3. Per-frame desired gain, capped to ±12 dB
    let max_boost_linear = 10.0_f32.powf(12.0 / 20.0); // +12 dB
    let max_cut_linear = 10.0_f32.powf(-12.0 / 20.0); // −12 dB

    let mut desired_gains = Vec::with_capacity(num_frames);
    for &rms in &frame_rms {
        let gain = if rms > 1e-10 {
            (target_rms / rms).clamp(max_cut_linear, max_boost_linear)
        } else {
            max_boost_linear
        };
        desired_gains.push(gain);
    }

    // 4. Smooth gain envelope (per-sample)
    let attack_coeff = (-1.0 / (sample_rate as f32 * 0.005)).exp();
    let release_coeff = (-1.0 / (sample_rate as f32 * 0.300)).exp();

    let mut output = Vec::with_capacity(samples.len());
    let mut current_gain = 1.0_f32;

    for (i, sample) in samples.iter().enumerate() {
        // Linearly interpolate desired gain from neighbouring frame centres
        let centre = i as f32 / hop_len as f32 - (frame_len as f32 / hop_len as f32) * 0.5;
        let idx = centre.floor() as isize;
        let frac = centre - idx as f32;

        let desired_gain = if idx >= 0 && (idx as usize) + 1 < desired_gains.len() {
            desired_gains[idx as usize] * (1.0 - frac) + desired_gains[idx as usize + 1] * frac
        } else if idx >= 0 && (idx as usize) < desired_gains.len() {
            desired_gains[idx as usize]
        } else if idx < 0 && num_frames > 0 {
            desired_gains[0] // before first frame centre → use first
        } else if num_frames > 0 {
            desired_gains[num_frames - 1] // after last → use last
        } else {
            1.0
        };

        let coeff = if desired_gain < current_gain {
            attack_coeff
        } else {
            release_coeff
        };
        current_gain += coeff * (desired_gain - current_gain);

        output.push(*sample * current_gain);
    }

    // 5. Global makeup gain: restore overall loudness to original level + 3 dB extra
    let orig_rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32)
        .sqrt()
        .max(1e-10);
    let out_rms = (output.iter().map(|s| s * s).sum::<f32>() / output.len() as f32)
        .sqrt()
        .max(1e-10);
    let extra_gain = 10.0_f32.powf(3.0 / 20.0); // +3 dB
    let global_gain = (orig_rms / out_rms * extra_gain).clamp(0.5, 4.0);

    for s in &mut output {
        *s *= global_gain;
    }

    // 6. Soft limiter at −0.5 dBFS to prevent inter-sample peaks
    let ceiling = 10.0_f32.powf(-0.5 / 20.0); // ≈ 0.944
    for s in &mut output {
        if s.abs() > ceiling {
            *s = s.signum() * ceiling;
        }
    }

    output
}

/// Compute gain reduction in dB using a soft-knee curve.
///
/// - Below `threshold - knee_half`: no reduction (gain = 0 dB).
/// - Above `threshold + knee_half`: full ratio compression.
/// - In between: quadratic interpolation for a smooth transition.
fn compute_gain_db(env_db: f32, threshold_db: f32, slope: f32, knee_half: f32) -> f32 {
    if knee_half <= 0.0 {
        // Hard knee
        if env_db > threshold_db {
            (env_db - threshold_db) * slope
        } else {
            0.0
        }
    } else if env_db < threshold_db - knee_half {
        0.0
    } else if env_db > threshold_db + knee_half {
        (env_db - threshold_db) * slope
    } else {
        // Soft knee region: quadratic interpolation
        let x = env_db - threshold_db + knee_half;
        let curve = x * x / (4.0 * knee_half);
        curve * slope
    }
}

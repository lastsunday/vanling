+++
title = "TTS Audio Quality Evaluation"
weight = 20
[extra]
source_file_hash = "f32b22839058efca6dca3379c2491bcdfc66843c"
translated_at = "2026-10-25T00:00:00Z"
+++

# TTS Audio Quality Evaluation

`api/tests/common/tts.rs` wires "synthesize → decode → score" into one reproducible flow, used to compare models, tune `length_scale`, and judge whether a change hurt quality. Analysis cases need the real model and are `#[ignore]`d by default.

## Shared helpers

| Function | Notes |
| --- | --- |
| `ws_root()` | Monorepo root (`CARGO_MANIFEST_DIR` three levels up) |
| `test_audio_config()` | Standard `AudioConfig` for tests (16000 Hz / mono / 20 ms frames) |
| `run_tts_test(tts_config, audio_config, wav)` | Full flow: build model → streaming inference → Opus decode → write WAV |
| `tts_stream(text)` | Build a TTS input `Stream` from a string |
| `opus_pipeline(samples, sample_rate, encode_sr)` | Round-trip through encode/decode to gauge Opus loss |
| `analyze_audio(...)` | Returns `TtsAudioDiagnostics` (below) |
| `estimate_std_duration(text)` | Estimates standard duration from text weights |
| `collect_rule_fsts(dir)` | Collects the rule FSTs (text normalisation) under a directory |

`run_length_scale_scan(...)` is not a shared helper; it lives in `api/tests/tts_analysis_test.rs`.

## Standard duration

`estimate_std_duration` uses OmniVoice's RuleDurationEstimator weights: every character gets a speech weight by Unicode range, converted at a fixed 12.0 weight/sec (about 4 CJK chars/sec or 150 WPM English). It does not vary with model or `length_scale`, so it is a consistent yardstick across models.

`TEST_TTS_TEXT`: `2024年5月11号，拨打110或者18920240511，花了99块钱。我在学习machine learning和artificial intelligence。`

Standard duration is about 14.1 s (weight ≈ 168.8 / 12.0).

## Three-axis scoring

`TtsAudioDiagnostics` reports three independent axes and **never merges them into one total** — quality, performance and speaking rate are orthogonal problems, and collapsing them hides the cause.

| Axis | Fields | Basis | Grade |
| --- | --- | --- | --- |
| Audio | `shimmer_pct`, `dynamic_range_db` | shimmer 70% + DR 30% | A/B/C/D/F |
| Perf | `rtf` | RTF thresholds | A/B/C/D/F |
| Timing | `duration_secs`, `std_duration_secs`, `std_diff_secs` | Deviation from standard duration | A/B/C/D/F |

Grade mapping: ≥86 → A, 66–85 → B, 41–65 → C, 21–40 → D, <21 → F.

### Audio

Shimmer is the frame-to-frame amplitude variation rate; thresholds come from clinical speech pathology: <3.81% → A (healthy voice), 3.81–5.0 → B, 5.0–6.0 → C, 6.0–10.0 → D, >10 beyond the algorithm's reliable limit → F.

Dynamic range: >20 dB → A (natural speech), 15–20 dB → C (over-compressed), <15 dB → F (flat and dull).

`audio_score = shimmer_score × 0.7 + dr_score × 0.3`, both interpolated linearly: shimmer 3.81→5.0 maps 100→75, 5.0→6.0 maps 75→50, 6.0→10.0 maps 50→25; DR 15→20 dB maps 0→100.

### Perf

RTF = generation time / audio duration. `<0.1` → 100 (A), 0.1–0.3 → 100→80 (B), 0.3–0.5 → 80→60 (C), 0.5–1.0 → 60→0 (D), `>=1.0` → 0 (F).

### Timing

Deviation `|actual - std| / std`. <5% → 100 (A), 5–20% → 100→80 (B), 20–50% → 80→40 (C), 50–100% → 40→0 (D), `>=100%` → 0 (F).

## Output

```
Audio:scr=30(D) Perf:scr=84(B) Timing:scr=74(B) | sh=18.07%(F) dr=25.9dB(A) rtf=0.26 gen=2.8s dur=10.60s(std=14.1s-25%) glitch=3 clip=0 cf=14.2dB ev=0.0012 sc=1850Hz sf=0.08 sr=4200Hz zcr=0.12 dc=0.0003 snr=28.5dB lead=0ms trail=0ms Marginal...
```

The three `scr`/grade pairs come first; raw metrics follow after `|`.

| Field | Notes |
| --- | --- |
| `cf` | Crest factor (peak/RMS); higher means more dynamics |
| `ev` | Inter-frame energy variance; low values usually mean cross-fades smeared the detail |
| `sc` | Spectral centroid in Hz, roughly 1500–3000 Hz for speech |
| `sf` | Spectral flatness 0–1, higher is closer to noise |
| `sr` | Spectral roll-off in Hz (85% energy threshold) |
| `zcr` | Zero-crossing rate, roughly 0.05–0.15 for speech |
| `dc` | DC offset, should be near 0 |
| `snr` | Estimated SNR in dB |
| `lead` / `trail` | Leading / trailing silence in ms |

## Running

```bash
# Scan length_scale to find the speaking-rate calibration point
cargo test --package api --test tts_analysis_test -- test_tts_matcha_zh_en_scan_ls --ignored --nocapture

# End-to-end for a single model, writes a WAV
cargo test --package api --test tts_test -- test_tts_matcha_zh_en --ignored --nocapture
```

`tts_test` is `#[ignore]`d because Matcha's real output occasionally clips and the clipping gate fluctuates with model data, so it is unsuitable for CI.

## Current baseline

`matcha-icefall-zh-en` shows about 17% shimmer, above the 12% reliability limit — that is a property of the model, not something the pipeline introduces.

`length_scale` defaults to 1.0 in the constructor arguments of `api/src/component/tts/model/matcha/mod.rs` (applied when `opts` omits it); calibrating to the 14.1 s standard lands on that same value.

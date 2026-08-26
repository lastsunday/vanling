+++
title = "Audio Debugging"
weight = 401
[extra]
source_file_hash = "2d852ff6fd8d7a2282dedb19f9a1875e4f1b865c"
translated_at = "2026-08-26T00:00:00Z"
+++

# Audio Debugging

## Problem

Real-time WebSocket TTS audio playback: starts fine, but after a few seconds exhibits noticeable **stuttering** and **static noise**.

## Investigation

### 1. Validate server output

- Session integration tests produce WAV files that play cleanly
- **Conclusion: server-side audio pipeline (TTS → resample → Opus → WebSocket) has no quality issues**

### 2. Rule out each tunable

| Change | Effect |
|--------|--------|
| `opus-rs` → `opus` crate (C binding) | No effect |
| Output sample rate 16000 → 24000Hz | No effect |
| `sherpa-onnx::LinearResampler` → `rubato::FftFixedIn` | No effect |
| `Application::LowDelay` → `Application::Audio` + FEC | No effect |
| `Signal::Auto` → `Signal::Voice` + `Bandwidth::Superwideband` | No effect |
| VBR → CBR (`set_vbr(false)`) | No effect |
| Old pacing: elapsed relative time → absolute target time | No effect |

### 3. Client code analysis

Reviewed client code under `apps/server-ui/public/test/device/js/`:

**Opus decoding (synchronous WASM call on main thread):**

```javascript
// player.js
decode: function (opusData) {
    const decodedSamples = mod._opus_decode(    // ← sync WASM
        this.decoderPtr, opusPtr, opusData.length,
        pcmPtr, this.frameSize, 0
    );
    // Blocks main thread until decoding completes
}
```

**Web Audio API chained scheduling (120ms per segment):**

```javascript
// stream-context.js
const startTime = Math.max(this.scheduledEndTime, currentTime);
this.source.start(startTime);
this.scheduledEndTime = startTime + audioBuffer.duration;
```

**Batch frame fetching (inner loop up to 99 packets):**

```javascript
// player.js
const data = await this.queue.dequeue(99, 30);
// 99 packets fed into decode loop at once
```

## Root Cause

### Timeline

```
TTS model inference (~13s)     TTS::Start          AudioResult × N
│────────────────────────────┤─────────────────────►
                             │
audio_start_time = T=0       │
                             │   (all frames arrive at OutputController after 13s)
                             │
Frame 228:                   │
  paced_index = 228 - 10     │
  target = 0 + 218 × 60ms    │
         = 13080ms ≈ 13s     │
  now = 13s + ϵ > target     │
  → sleep(0) → send immediately│
                             ▼
                    228 frames burst-sent (<10ms)
```

### Detailed Mechanism

1. **TTS model** generates continuous PCM for the full sentence (~13 seconds inference time)
2. PCM is split into 228 frames (60ms/frame), all arriving at `OutputController` simultaneously via mpsc channel
3. **Old pacing logic** uses `audio_start_time` (TTS::Start time) as the absolute target baseline:
   ```
   target = audio_start_time + paced_index × 60ms
   ```
   All targets are 13s in the past → `now > target` → never sleeps → **228 frames sent in <10ms burst**
4. **Client** receives 200+ Opus packets instantly → decode loop processes all in a single batch
5. `_opus_decode` in WASM is a **synchronous call**, blocking the JS main thread for >1 second
6. While the main thread is blocked, Web Audio API cannot create the next `AudioBufferSourceNode`
7. `scheduledEndTime` is already past → chain scheduling breaks → audible gaps and noise

### Why Didn't This Affect Offline WAV?

- Offline tests (`test_tts_audio_collect`) decode all Opus frames into continuous PCM then write to file
- No real-time scheduling constraints; WASM decoding blocking has no audible effect
- Offline WAV = seamless concatenation of all frames → perfectly clean

## Fix

### Solution

Use `tokio::time::interval_at` + `MissedTickBehavior::Skip` with **lazy creation**: first frame sent immediately, subsequent frames strictly spaced at 20ms from current time.

### Code

```rust
// output_controller.rs — key change

use tokio::time::{Duration, Instant, MissedTickBehavior, interval_at};

pub struct OutputController {
    interval: Option<tokio::time::Interval>,
    frame_duration: u64,
    // ...
}

impl OutputController {
    async fn pace_audio(&mut self) {
        if let Some(interval) = &mut self.interval {
            interval.tick().await;  // wait 20ms → send → wait 20ms → send...
        } else {
            let start = Instant::now() + Duration::from_millis(self.frame_duration);
            let mut intv = interval_at(start, Duration::from_millis(self.frame_duration));
            intv.set_missed_tick_behavior(MissedTickBehavior::Skip);
            self.interval = Some(intv); // first frame sent immediately, interval from next frame
        }
    }
}
```

### Improved Timeline

```
TTS inference (~13s)      TTS::Start     AudioResult × N
│────────────────────────┤───────────────►
                         │
Frame 1:                 │
  interval = None        │
  → create interval(now+20ms, 20ms)  ← no tick, send immediately
                         │
Frame 2:                 │
  interval.tick().await  │  → wait 20ms → send
Frame 3:                 │
  interval.tick().await  │  → wait 20ms → send
Frame 4:                 │
  interval.tick().await  │  → wait 20ms → send
  ...                    │  strict 20ms/frame
                         ▼
                    Client plays smoothly
```

### Effect Comparison

| Metric | Old Approach | New Approach |
|--------|-------------|-------------|
| Initial burst | 228 frames (<10ms) | 1 frame (first sent immediately) |
| Steady-state interval | None (all burst) | Strict 20ms/frame |
| Client batch decode | 200+ frames/batch | 1 frame/batch |
| Main thread blocking | >1 second | <1ms |
| Web Audio API | Scheduling broken | Continuous scheduling |

## Key Files

| File | Role |
|------|------|
| `apps/server/api/src/ws/session/output_controller.rs` | Frame sending pacing logic (fix location) |
| `apps/server-ui/public/test/device/js/core/audio/player.js` | Client Opus decoding + batch fetching |
| `apps/server-ui/public/test/device/js/core/audio/stream-context.js` | Web Audio API chain scheduling |
| `apps/server/api/src/tts/model/matcha/mod.rs` | MatchaTts model audio generation |
| `apps/server/api/src/ws/session/mod.rs` | WebSocket session frame sending |
| `apps/server/api/src/tts/mod.rs` | `encode_sample_to_tts_packet` frame splitting & encoding |

## TTS Testing Tools

Audio quality analysis tools are in `apps/server/api/tests/common/tts.rs`, TTS integration tests in `apps/server/api/tests/tts_test.rs`.

### Shared Helper Functions

| Function | Description |
|----------|-------------|
| `ws_root()` | Monorepo root path (`CARGO_MANIFEST_DIR` up 3 levels) |
| `test_audio_config()` | Standard test AudioConfig (16000Hz / mono / 20ms frame) |
| `run_tts_test(tts_config, audio_config, wav)` | Full flow: create model → TTS streaming inference → Opus decode → write WAV |
| `run_length_scale_scan(dir, audio_cfg, wav_prefix, ls_values)` | Scan multiple `length_scale` values to find speed calibration point |
| `tts_stream(text)` | Create a TTS input `Stream` from string |
| `analyze_audio(samples, sample_rate, gen_elapsed, std_duration_secs)` | Returns `TtsAudioDiagnostics` diagnostics |
| `estimate_std_duration(text)` | Estimate standard duration based on text content (OmniVoice weight system) |

### Standard Duration Estimation

`estimate_std_duration(text)` uses the OmniVoice RuleDurationEstimator weight system, assigning Unicode range speech weights to text, then calculates standard duration using a fixed speed factor of 12.0 weight/sec (≈4 Chinese characters/sec or 150 WPM English). The result is **independent of model or length_scale**, providing a consistent reference across models.

**TEST_TTS_TEXT**: `2024年5月11号，拨打110或者18920240511，花了99块钱。我在学习machine learning和artificial intelligence。`

Standard duration: ~14.1 seconds (weight ≈168.8 / 12.0).

### Three-Dimensional Scoring

`TtsAudioDiagnostics` independently reports three dimensions, **without aggregating a total score**:

| Dimension | Field | Scoring Basis | Grade |
|-----------|-------|--------------|-------|
| **Audio** (quality) | `shimmer_pct`, `dynamic_range_db` | shimmer 70% + DR 30% | A/B/C/D/F |
| **Perf** (performance) | `rtf` | RTF real-time factor threshold | A/B/C/D/F |
| **Timing** (speed) | `duration_secs`, `std_duration_secs`, `std_diff_secs` | Deviation from standard duration % | A/B/C/D/F |

**Grade letters:**

| Score Range | Grade |
|-------------|-------|
| ≥ 86 | A |
| 66–85 | B |
| 41–65 | C |
| 21–40 | D |
| < 21 | F |

#### Audio Score

Based on shimmer (frame-to-frame amplitude variation rate) and dynamic range.

**Shimmer grades (clinical voice pathology reference):**

| Grade | Shimmer (%) | Meaning |
|-------|-------------|---------|
| A | < 3.81 | Healthy human voice level |
| B | 3.81–5.0 | Normal range, slightly perceptible |
| C | 5.0–6.0 | Warning zone, noticeably unstable |
| D | 6.0–10.0 | Pathological range, rough/jittery |
| F | > 10.0 | Beyond algorithm reliability limit |

**Dynamic Range grades:**

| Grade | dB | Meaning |
|-------|-----|---------|
| A | > 20 | Natural speech level |
| C | 15–20 | Compressed, insufficient dynamics |
| F | < 15 | Noticeably flat/muffled |

**Composite score (weight 70% + 30%, linear interpolation):**

| Shimmer (%) → Score | DR (dB) → Score |
|--------------------|-----------------|
| < 3.81 → 100 | > 20 → 100 |
| 3.81–5.0 → 100→75 linear decrease | 15–20 → 0→100 linear increase |
| 5.0–6.0 → 75→50 linear decrease | < 15 → 0 |
| 6.0–10.0 → 50→25 linear decrease | |
| >= 10.0 → 0 | |

**Formula**: `audio_score = shimmer_score × 0.7 + dr_score × 0.3`

#### Perf Score

Based on RTF (generation time / audio duration):

| RTF | Score | Grade |
|-----|-------|-------|
| < 0.1 | 100 | A |
| 0.1–0.3 | 100→80 | B |
| 0.3–0.5 | 80→60 | C |
| 0.5–1.0 | 60→0 | D |
| >= 1.0 | 0 | F |

#### Timing Score

Based on percentage deviation from standard duration `|actual - std| / std`:

| Deviation | Score | Grade |
|-----------|-------|-------|
| < 5% | 100 | A |
| 5–20% | 100→80 | B |
| 20–50% | 80→40 | C |
| 50–100% | 40→0 | D |
| >= 100% | 0 | F |

### Output Format

```
Audio:scr=30(D) Perf:scr=84(B) Timing:scr=74(B) | sh=18.07%(F) dr=25.9dB(A) rtf=0.26 gen=2.8s dur=10.60s(std=14.1s-25%) glitch=3 clip=0 cf=14.2dB ev=0.0012 sc=1850Hz sf=0.08 sr=4200Hz zcr=0.12 dc=0.0003 snr=28.5dB lead=0ms trail=0ms Marginal...
```

Three sections at a glance: Audio→P, Performance→G, Timing→G, raw metrics after `|`.

**Extended diagnostic fields:**

| Field | Description |
|-------|-------------|
| `cf` | Crest factor in dB (peak-to-RMS ratio, higher = more dynamic) |
| `ev` | Energy variance across 20ms frames (low = crossfade smearing) |
| `sc` | Spectral centroid in Hz (center of mass, speech ~1500–3000 Hz) |
| `sf` | Spectral flatness (0–1, higher = more noise-like) |
| `sr` | Spectral rolloff in Hz (85% energy threshold) |
| `zcr` | Zero-crossing rate (crossings per sample, speech ~0.05–0.15) |
| `dc` | DC offset (should be near 0.0, non-zero = bias) |
| `snr` | Estimated signal-to-noise ratio in dB (higher = cleaner) |
| `lead` | Leading silence in ms |
| `trail` | Trailing silence in ms |

### Parameter Tuning Tests

```bash
# length_scale speed calibration scan
cargo test --package api --test tts_analysis_test -- test_tts_matcha_zh_en_scan_ls --ignored --nocapture
```

### Known Issues

**matcha-icefall-zh-en**: shimmer ~17%, far exceeding the algorithm's reliability limit (12%).

**Default length_scale calibration (to make duration close to standard 14.1s):**

| Model | Default `length_scale` |
|-------|----------------------|
| matcha-icefall-zh-en | **1.0** |

Calibration values are written into `default_length_scale()` (`api/src/tts/mod.rs`), automatically taking effect when `length_scale` is not specified in `tts_options`.

---

## Volume Waveform Normalization

### Problem

TTS audio has inconsistent volume before and after (e.g., in Chinese-English mixed speech, the trailing English part is noticeably louder), causing a jarring listening experience.

### Solution Evolution

#### Approach 1: Fixed-Parameter DRC Compressor (Deprecated)

Traditional feed-forward compressor with peak-detector envelope following and optional soft knee. Optimal parameters found via grid search + EBU R128 objective metrics:

**Grid search range:**

| Parameter | Range | Step |
|-----------|-------|------|
| threshold | -32 ~ -20 dB | 2 dB |
| ratio | 2 ~ 8 | 2 |
| knee | 0 ~ 6 dB | 3 dB |
| attack | 1 ~ 5 ms | 1 ms |
| release | 80 ~ 200 ms | 40 ms |

**Best result:** threshold=-28, ratio=6, knee=0, attack=5ms, release=80ms, makeup=8dB

```
LRA 9.58 → 5.66 LU (↓41%)
Crest 15.4 → 6.2 dB ❌ Sounds muffled
```

Crest Factor compressed to 6.2 dB (too low), perceived as "muffled" — volume is balanced but dynamics are lost.

#### Approach 2: Adaptive RMS Gain Normalization (Current)

`adaptive_normalize()` in `apps/server/api/src/util/compressor.rs`, zero configuration, auto-adjusting.

**Algorithm:**

| Step | Description |
|------|-------------|
| 1. Frame RMS analysis | 200ms window, 10ms step, compute RMS per frame |
| 2. Target loudness | p30 (30th percentile) of all frame RMS values |
| 3. Frame gain | target_rms / frame_rms per frame, clamped to ±12 dB |
| 4. Gain smoothing | attack=5ms / release=300ms per-sample IIR smoothing |
| 5. Global loudness compensation | Match original RMS + extra +3 dB |
| 6. Soft limiting | -0.5 dBFS hard limit to prevent clipping |

**Result comparison:**

| Metric | Raw | Adaptive |
|--------|-----|----------|
| LRA | 9.08 LU | **1.59 LU** (↓82%) |
| LUFS | -26.90 | **-24.14** (~3 dB louder than raw) |
| Crest Factor | 15.3 dB | **14.7 dB** ✅ Dynamics preserved |

Crest Factor maintained at 14.7 dB — nearly the original level, no "muffled" feeling at all.

### EBU R128 Objective Metrics

`evaluate_compressed()` in `apps/server/api/src/util/compressor.rs` reports three metrics:

| Metric | Full Name | Meaning | Target |
|--------|-----------|---------|--------|
| LRA | Loudness Range | Loudness range, lower is more consistent | Significantly reduce |
| LUFS | Loudness Units relative to Full Scale | Overall integrated loudness | Match original +3 dB |
| Crest Factor | Peak-to-RMS Ratio | Dynamic headroom, higher = more punch | Maintain ≥ original |

### Performance Impact

| Stage | Complexity | 10s Audio Time |
|-------|-----------|----------------|
| Frame RMS | O(n) | < 1ms |
| Sort for p30 | O(f log f), ~1000 frames | < 1ms |
| Gain smoothing | O(n), per-sample IIR | < 10ms |
| **Total overhead** | | **~10ms** (TTS inference takes seconds, negligible) |

### Test Commands

```bash
# Comparison test: Raw vs Resample+Opus vs Adaptive Normalize, generates WAV + prints EBU R128 metrics
cargo test --package api --test tts_analysis_test -- test_compare_raw_vs_processed --ignored --nocapture

# Grid search compressor (historical reference retained)
cargo test --package api --test tts_analysis_test -- test_grid_search_compressor --ignored --nocapture
```

### Output Files

| File | Description |
|------|-------------|
| `./test_data/compare_raw.wav` | Raw PCM (sherpa-onnx direct output) |
| `./test_data/compare_processed.wav` | Current pipeline (resample + Opus) |
| `./test_data/compare_adaptive.wav` | After adaptive_normalize processing |

### Key Files

| File | Role |
|------|------|
| `apps/server/api/src/util/compressor.rs` | `adaptive_normalize()`, `evaluate_compressed()`, historical `pcm_compress()` / `grid_search_compressor()` |
| `apps/server/api/src/util/compressor.rs` | `adaptive_normalize()` definition (not currently called by pipeline) |
| `apps/server/api/tests/tts_analysis_test.rs` | `test_compare_raw_vs_processed`, `test_grid_search_compressor` |

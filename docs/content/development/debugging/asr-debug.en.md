+++
title = "ASR Speech Recognition"
weight = 402
[extra]
source_file_hash = "9c2482713d8febed978b63651db96d84523c1e26"
translated_at = "2026-07-11T00:00:00Z"
+++

# ASR Speech Recognition

## Architecture

The ASR module lives at `apps/server/api/src/asr/`, using the **Manager singleton pattern** for model instance management, with **sherpa-onnx**'s `OfflineRecognizer` under the hood.

### Manager Pattern

```rust
AsrManager::init(config).await;                             // Initialize at application startup
let mut model = AsrManager::global().default().lock().await; // Get Arc<Mutex<Box<dyn Asr>>>
let result = model.transcribe(sample_rate, &samples).await;
```

Initialization is at `api/src/lib.rs:121`, consumption points at `ws/session/listener.rs:238` (WebSocket real-time speech) and `matrix/client.rs` (Matrix protocol).

### Asr trait

```rust
#[async_trait]
pub trait Asr: Send + Sync {
    async fn transcribe(
        &mut self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<RecognizerResult, ModelError>;
}
```

All models share `RecognizerResult { text: String, prob: f32 }` as output.

## Model Overview

| Model | Config Name | Model File | Size | Languages | Architecture |
|-------|-------------|------------|------|-----------|-------------|
| **SenseVoice** | `sense_voice` | `model.int8.onnx` | 228MB | zh/en/ja/ko/yue | OfflineSenseVoice |
| **Void** | `void` | None (no-op) | 0 | — | — |

All models come from <https://github.com/k2-fsa/sherpa-onnx>, managed via downloader manifests.

### Downloader Manifests

| Manifest | Model |
|----------|-------|
| `manifests/asr/sense_voice.json` | SenseVoice |

```shell
# Download the currently configured ASR model
moon run server:downloader -- asr

# Download a specific ASR model
moon run server:downloader -- asr sense_voice
```

## Performance Benchmarks

Test conditions: debug mode, MatchaTTS generates ~11.84s Chinese-English mixed audio, looped back into ASR for transcription.

| Model | CER | WER | RTF | ASR Time | Accuracy | Performance | Total | Verdict |
|-------|-----|-----|-----|---------|---------|-------------|-------|---------|
| **SenseVoice** | **0.00%(A)** | 0.00% | 0.110 | 1.3s | 100% | 89.0% | 96.7% | ✅ Suitable for daily use |

**Conclusion: Only SenseVoice currently meets usable quality; it is the default model.**

| Observation | Description |
|-------------|-------------|
| SenseVoice accuracy | TTS loopback CER is 0% — perfect recognition of synthetic speech |
| Performance | RTF < 0.12 (debug mode), faster in release mode |

### TTS-ASR Loopback Test

Synthesize `TEST_TTS_TEXT` via MatchaTTS, then feed into ASR for transcription, computing CER/WER:

| Test Name | ASR Model | Threshold |
|-----------|-----------|-----------|
| `test_tts_asr_loopback` | SenseVoice | CER < 6% (actual 0%) |

**TTS generation time**: ~6.1-6.2s (debug) → equivalent to RTF=0.52 (TTS generation + ASR transcription)

## Testing Framework

### Diagnostic Metrics (`common/asr.rs`)

```
CER=XX.X%(X) WER=XX.X% Acc=XX.X% Perf=XX.X% Total=XX.X% | RTF=X.XXX ASR=X.Xs Audio=X.XXs <verdict>
```

| Metric | Calculation |
|--------|-------------|
| **CER** | Character-level Levenshtein distance / reference text length |
| **WER** | Word-level Levenshtein distance / reference word count |
| **RTF** | ASR time / audio duration |
| **Accuracy** | 1.0 - CER |
| **Score** | Accuracy × 70 + (1 - min(RTF,1)) × 30 |

### CER Grades

| Grade | Range | Meaning |
|-------|-------|---------|
| A | < 3% | Excellent |
| B | 3–6% | Good |
| C | 6–10% | Fair |
| D | 10–20% | Poor |
| F | ≥ 20% | Unusable |

### RTF Grades

| Grade | Range | Meaning |
|-------|-------|---------|
| A | < 0.05 | Very fast |
| B | 0.05–0.10 | Fast |
| C | 0.10–0.20 | Normal |
| D | 0.20–0.50 | Slow |
| F | ≥ 0.50 | Unusable |

## All Tests

| Test File | Test Name | Model | Assertion |
|-----------|-----------|-------|-----------|
| `asr_test.rs` | `test_asr` | SenseVoice | None (diagnostic) |
| `asr_test.rs` | `test_asr_model_void` | Void | text=="" prob==1.0 |
| `asr_test.rs` | `test_asr_with_reference_audio` | SenseVoice | Contains expected substrings (zh/en/yue) |
| `asr_test.rs` | `test_tts_asr_loopback` | SenseVoice | CER < 6% |

### Session Integration Tests

| Test Name | ASR Role | Description |
|-----------|----------|-------------|
| `test_asr_voice_input_manual` | SenseVoice + Echo LLM | Send JFK WAV audio, assert STT text exact match |
| `test_chat_flow_listen_auto` | SenseVoice (full pipeline) | VAD detects speech end → ASR → LLM → TTS |
| `test_chat_flow_listen_realtime` | SenseVoice (full pipeline) | Real-time mode, send audio + Detect text |
| `test_chat_flow_listen_realtime_silent_voice_connection_timeout` | Void (disconnect test) | Silence timeout disconnect, no real ASR needed |
| `test_chat_flow_handle_text_message` | SenseVoice (full pipeline) | Text input → LLM → TTS |
| `test_chat_flow_handle_text_message_multiple_time` | SenseVoice (full pipeline) | 19 rounds of text conversation |
| `test_chat_flow_break` | Void (interruption test) | Interrupt and resume, no real ASR needed |

### Test Commands

```shell
# Void model test (no model file needed, not ignored)
cargo test --package api --test asr_test -- test_asr_model_void --nocapture

# SenseVoice basic transcription
cargo test --package api --test asr_test -- test_asr --ignored --nocapture

# Reference audio test
cargo test --package api --test asr_test -- test_asr_with_reference_audio --ignored --nocapture

# TTS-ASR loopback test
cargo test --package api --test asr_test -- test_tts_asr_loopback --ignored --nocapture

# release mode (faster)
cargo test --package api --test asr_test --release -- test_asr --ignored --nocapture

# All ASR tests
cargo test --package api --test asr_test -- --ignored --nocapture
```

## Adding a New ASR Model

1. **Create manifest**: `apps/server/src/downloader/manifests/asr/<model>.json`
2. **Implement model module**: `apps/server/api/src/asr/model/<model>/mod.rs`, implement the `Asr` trait
3. **Register model**: Add `pub mod <model>` in `api/src/asr/model/mod.rs`
4. **Add enum variant**: Add `<Model>` to the `AsrModel` enum in `api/src/config/mod.rs`
5. **Register Manager**: Add match arm in `create_model()` in `api/src/asr/mod.rs`
6. **Add tests**: Add reference audio and loopback tests in `api/tests/asr_test.rs`

## Key Files

| File | Role |
|------|------|
| `apps/server/api/src/asr/mod.rs` | Asr trait, Manager, RecognizerResult |
| `apps/server/api/src/asr/model/sense_voice/mod.rs` | SenseVoice model implementation |
| `apps/server/api/src/asr/model/void/mod.rs` | Void no-op model |
| `apps/server/api/src/config/asr.rs` | AsrConfig struct |
| `apps/server/api/src/config/mod.rs` | AsrModel enum |
| `apps/server/src/downloader/manifests/asr/` | Download manifests |
| `apps/server/api/tests/asr_test.rs` | 3 ASR integration tests |
| `apps/server/api/tests/common/asr.rs` | AsrDiagnostics system |
| `apps/server/api/tests/session/asr.rs` | Session integration ASR tests |

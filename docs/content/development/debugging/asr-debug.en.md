+++
title = "ASR Speech Recognition"
weight = 402
sort_by = "weight"
[extra]
source_file_hash = "6c63ce613850e8bc6f9f2ae6fbb571e2feed06ab"
translated_at = "2026-10-25T00:00:00Z"
+++

# ASR Speech Recognition

(This page is the recognizer's debugging and benchmark entry point; its place in the chain and turn detection live in [VAD and Turn Detection](@/development/server/vad.en.md).)

## Architecture

The ASR trait lives in `service/src/component/asr.rs`, while the engine implementation and singleton live in `api/src/component/asr/`, backed by **sherpa-onnx**.

```rust
pub trait Asr: Send + Sync {
    fn create_stream(&self) -> Box<dyn AsrStream>;
    async fn transcribe(&self, sample_rate: u32, samples: &[f32])
        -> Result<RecognizerResult, AppError>;
}
```

`RecognizerResult { text: String, prob: f32 }` is the shared output of both paths. The streaming path is `AsrStream`: `accept_waveform` / `decode` / `is_endpoint` / `get_partial` / `finish` / `reset`.

```rust
AsrManager::init(config).await;          // initialise at startup
let model = AsrManager::global().default(); // Arc<dyn Asr>, shareable across turns
let mut stream = model.create_stream();
stream.accept_waveform(&samples);
stream.decode();
```

Initialisation happens in the server startup path; there are two consumers: `AsrNode` (the real-time voice chain) and the Matrix client (whole-utterance transcription).

## Models

| Model | Config | Location | Notes |
| --- | --- | --- | --- |
| **XAsr** | `x_asr` | `data/asr/model/x_asr/default/` | zh/en/ja/ko/yue; usable both streaming and whole-utterance |
| **Void** | `void` | no files | Returns fixed text `"void"` and confidence 1.0 for testing the chain |

Models come from <https://github.com/k2-fsa/sherpa-onnx> and are managed by downloader manifests:

```shell
moon run server:downloader -- asr
moon run server:downloader -- asr x_asr
```

## Benchmarks and diagnostics

`api/tests/common/asr.rs` turns each test's timing into one diagnostic line:

```text
CER=XX.X%(X) WER=XX.X% Acc=XX.X% Perf=XX.X% Total=XX.X% | RTF=X.XXX ASR=X.Xs Audio=X.XXs <verdict>
```

| Metric | Computation |
| --- | --- |
| CER | Character-level Levenshtein distance / reference length |
| WER | Word-level Levenshtein distance / reference word count |
| RTF | ASR time / audio duration |
| Accuracy | 1 − CER |
| Score | Accuracy × 70 + (1 − min(RTF, 1)) × 30 |

| Grade | CER | RTF |
| --- | --- | --- |
| A | < 3% | < 0.05 |
| B | 3–6% | 0.05–0.10 |
| C | 6–10% | 0.10–0.20 |
| D | 10–20% | 0.20–0.50 |
| F | ≥ 20% | ≥ 0.50 |

> The old page's "TTS loopback CER 0%, RTF 0.110" were measurements from before the refactor and were **not re-measured this round**; the commands below are how to find out what they are now.

## Tests

`api/tests/asr_test.rs` has five, all calling `AsrManager::create_model` directly:

| Test | Model | Assertion |
| --- | --- | --- |
| `test_asr_model_void` | Void | `text == "void"`, `prob == 1.0` |
| `test_asr_model_void_stream` | Void | Streaming surface works: no endpoint, no partial, finish returns the same |
| `test_asr_model_x_asr_transcribe` | XAsr | Whole-utterance transcription non-empty, `prob > 0` |
| `test_asr_model_x_asr_streaming` | XAsr | Feed every 100 ms; at least one partial and a non-empty finish |
| `test_asr_transcribe_short_audio` | XAsr | Diagnostic comparison of 1 s truncation, +300 ms silence, and the full file |

The XAsr tests need `data/asr/model/x_asr/default/` downloaded (`moon run server:downloader -- asr`).

```shell
# Void: no model files needed
cargo test --package api --test asr_test -- test_asr_model_void

# XAsr: model required
cargo test --package api --test asr_test -- test_asr_model_x_asr_transcribe --nocapture
cargo test --package api --test asr_test -- test_asr_model_x_asr_streaming --nocapture

# release is faster
cargo test --package api --test asr_test --release -- test_asr_model_x_asr_streaming --nocapture
```

Chain-level regression lives in `api/tests/session/`: `test_chat_flow_listen_manual`, `test_chat_flow_listen_auto`, `test_chat_flow_listen_realtime`, `test_chat_flow_voice_text_voice_round_trips`, `test_chat_flow_resample_48k_multi_round`.

## Adding an ASR model

1. **Manifest**: `apps/server/src/downloader/manifests/asr/<model>.json`
2. **Implementation**: `api/src/component/asr/model/<model>/mod.rs`, implementing `Asr` (and `AsrStream` if streaming)
3. **Register the module**: add `pub mod <model>` in `api/src/component/asr/model/mod.rs`
4. **Enum**: add a variant to `AsrModel` in `api/src/config/mod.rs`
5. **Factory**: add a match arm in `create_model()` in `api/src/component/asr/mod.rs`
6. **Tests**: add whole-utterance and streaming cases in `api/tests/asr_test.rs`, at least asserting non-empty output and a partial

## Key files

| File | Purpose |
| --- | --- |
| `apps/server/service/src/component/asr.rs` | `Asr` / `AsrStream` traits, `RecognizerResult` |
| `apps/server/api/src/component/asr/mod.rs` | Manager singleton and model factory |
| `apps/server/api/src/component/asr/model/x_asr/` | XAsr implementation |
| `apps/server/api/src/component/asr/model/void/` | Void stand-in |
| `apps/server/api/src/config/asr.rs` | `AsrConfig` |
| `apps/server/api/src/config/mod.rs` | `AsrModel` enum |
| `apps/server/src/downloader/manifests/asr/` | Download manifests |
| `apps/server/api/tests/asr_test.rs` | The five tests above |
| `apps/server/api/tests/common/asr.rs` | Diagnostics and scoring |

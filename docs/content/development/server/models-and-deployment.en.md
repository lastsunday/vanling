+++
title = "Models and Deployment"
weight = 203
[extra]
source_file_hash = "f73d4802118254cd043ac51b9fe2bb54f85fdc80"
translated_at = "2026-06-28T18:00:00Z"
+++

# Models and Deployment

## File Structure

The location of each module follows the directory tree in AGENTS.md. Under `api/src/`, modules are organized by function (asr, auth, llm, tts, vad, ws, record, mcp, matrix, etc.), `service/src/` is the business logic layer, `entity/src/` contains Sea-ORM Entities, `migration/src/` contains database migrations, and `web/src/` serves static files.

## Data Flow

See [Dialogue Flow](@/development/server/dialogue-flow.en.md).

## Models

### LLM

| Model                           | Memory   | File Size          | Notes                |
| ------------------------------- | -------- | ------------------ | -------------------- |
| Qwen3-0.6B (Candle-GGUF)        | ~1GB     | 0.6B GGUF          | Qwen3-0.6B-Q4_K_M.gguf |
| Qwen3-1.7B (Candle-GGUF)        | ~2.5GB   | 1.11GB             | Qwen3-1.7B-Q4_K_M.gguf |
| Echo                            | 0        | 0                  | Echo (testing)       |

### ASR

| Model                                   | Memory   | File Size | Language            | CER (TTS loopback)   |
| --------------------------------------- | -------- | --------- | ------------------- | -------------------- |
| SenseVoice (sherpa-onnx)                | ~600MB   | 228MB     | zh/en/ja/ko/yue     | 0.00%(A)             |
| Void                                    | 0        | 0         | —                   | — (testing)          |

### TTS

| Model                           | Memory   | File Size                      | Notes                |
| ------------------------------- | -------- | ------------------------------ | -------------------- |
| MatchaTts (sherpa-onnx)         | ~500MB   | 72MB + 76MB (vocoder)          | Chinese/Chinese-English bilingual |
| Mute                            | 0        | 0                              | Silent (testing)     |

### VAD

| Model                           | Memory   | File Size | Notes                  |
| ------------------------------- | -------- | --------- | ---------------------- |
| Earshot (Silero VAD)            | ~10MB    | Embedded  | Pure Rust, no ONNX     |
| Void                            | 0        | 0         | Always returns voice (testing) |

## Fedora 43 CUDA Installation and Configuration

```shell
sudo sh cuda_12.8.1_570.124.06_linux.run --toolkit --no-drm --silent --override
````

```zshrc
export PATH="/usr/local/cuda/bin:$PATH"
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/usr/local/cuda/lib64
export LIBRARY_PATH=$LIBRARY_PATH:/usr/local/cuda/lib64
```

``` shell
conda create -n cuda
# Install candle build dependencies
conda install conda-forge::gcc==14.3.0
conda install conda-forge::gxx==14.3.0
# Install openssl
conda install anaconda::openssl
conda activate cuda
# Start development...
```

## Reference Guidelines

<https://rust-lang.github.io/api-guidelines/>

<https://rust-coding-guidelines.github.io/rust-coding-guidelines-zh/overview.html>

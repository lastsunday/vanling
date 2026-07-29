+++
title = "Models and Deployment"
weight = 203
[extra]
source_file_hash = "f2355b3553a198a4d94eadfd32fb815de42fdb8c"
translated_at = "2026-07-24T00:00:00Z"
+++

# Models and Deployment

## File Structure

```
apps/server/
├── src/                      # Server entry point (CLI, Downloader, Config loading)
│   ├── main.rs               # Binary entry
│   ├── mod.rs                # run() / async_main()
│   ├── server.rs             # Server lifecycle (wraps api::server::Server)
│   ├── clap.rs               # CLI args: serve / downloader
│   └── downloader/           # Model download system (manifest, checksum, mirror)
├── api/                      # HTTP routes, AI Managers, WebSocket handling
│   └── src/
│       ├── lib.rs            # start() / create_router()
│       ├── config/           # Config definitions (figment layered loading)
│       ├── server.rs         # Server runtime state
│       ├── auth.rs           # Auth routes (login / refresh / reset_password)
│       ├── index.rs          # Health / Version endpoints
│       ├── ota.rs            # OTA firmware update
│       ├── device.rs         # Device management (admin CRUD)
│       ├── ws/               # WebSocket handler (core real-time pipeline)
│       ├── chii/             # ChiiCore: LLM + MCP orchestration
│       ├── asr/              # ASR Manager + model (XAsr)
│       ├── tts/              # TTS Manager + model (MatchaTTS)
│       ├── vad/              # VAD Manager + model (Earshot)
│       ├── llm/              # LLM Manager + model (Qwen3 / Echo)
│       ├── mcp/              # MCP Server + Client (rmcp Streamable HTTP)
│       ├── record/           # Session recording REST API
│       ├── matrix/           # Matrix chat integration
│       └── common/           # Shared helpers (device selection, errors)
├── service/                  # Business logic layer (trait definitions + Session state machine)
│   └── src/chobits/
│       ├── session/          # Session state machine + Round lifecycle
│       ├── frame.rs          # Frame / FrameResult / OutputMessage
│       ├── listener.rs       # Listener trait (VAD + ASR orchestration)
│       ├── chii.rs           # Chii trait (LLM + MCP orchestration)
│       ├── asr.rs / tts.rs / vad.rs  # AI trait definitions
│       ├── llm/              # Llm trait + ChatMessage / ToolDef
│       ├── mcp/              # McpClient trait + McpRegistry
│       └── message/          # Wire protocol message definitions
├── entity/                   # Sea-ORM Entities (user / session / round / round_data / frame / config)
├── migration/                # Database migrations
└── web/                      # Static file serving (SPA)
```

## Data Flow

```
Client → WS → ProtocolTranslator → InputFilters → Session → OutputFilters → ProtocolTranslator → WS
                ┌─────────────────┐     ┌──────────────────┐
                │ McpRouterFilter  │     │ RecorderFilter   │
                │ RecorderFilter   │     │ (frame recording)│
                └─────────────────┘     └──────────────────┘
```

See [Dialogue Flow](@/development/server/dialogue-flow.en.md).

## Configuration System

Configuration uses [figment](https://docs.rs/figment) with layered loading, lowest to highest priority:

1. TOML file pointed to by `CHOBITS_CONFIG` env var
2. CLI `--config` paths (multiple, merged)
3. `CHOBITS_` prefixed environment variables (e.g., `CHOBITS_LLM_MODEL`)
4. CLI `-O key=value` overrides

### Hot Reload

`config::Manager` uses `AtomicPtr<Config>` + thread-local cache (8-slot history) for lock-free hot reload. When config changes, old versions remain safe for in-flight coroutines, and new versions take effect immediately.

### Key Configuration

| Category | Key | Default | Description |
|----------|-----|---------|-------------|
| Server | `address` | `127.0.0.1` | Listen address |
| Server | `port` | `3000` | Listen port |
| Database | `database_url` | `sqlite://db.sqlite?mode=rwc` | SQLite or PostgreSQL URL |
| Auth | `auth_access_token_secret` | `QLjJTeVblAlM47de` | JWT signing secret |
| TTS | `tts_model` | `matcha_tts` | Model selection |
| ASR | `asr_model` | `x_asr` | Model selection |
| LLM | `llm_provider` | `local_qwen3` | Model selection |
| VAD | `vad_model` | `earshot` | Model selection |
| Session | `silence_voice_timeout` | `1200` | Silence timeout (ms) |
| Logging | `log_console_enabled` | `true` | Console logging |
| Matrix | `matrix_enable` | `false` | Matrix integration |

See `application-example.toml` for the full configuration reference.

## Downloader System

The `chobits downloader` subcommand provides model asset downloading:

- **Manifest-driven**: Model definitions in `src/downloader/manifests/` TOML files with URLs, variants, checksums
- **Category installation**: `--category tts|asr|llm|vad|reference`
- **Interactive wizard**: `chobits downloader wizard` for guided installation
- **Mirror support**: `--mirror hf-mirror.com` for users in China
- **Verification**: Automatic SHA256 checksum verification
- **Path derivation**: Installed paths auto-injected via `derive_tts_path` / `derive_asr_path` / `derive_llm_path`

Usage:

```shell
# Run from apps/server/ directory
moon run server:run -- downloader install --data-dir ../../data --all
moon run server:run -- downloader wizard --data-dir ../../data
```

## Models

### LLM

| Model | Memory | File Size | Notes |
|-------|--------|-----------|-------|
| Qwen3-0.6B (Candle-GGUF) | ~1GB | 0.6B GGUF | Qwen3-0.6B-Q4_K_M.gguf |
| Qwen3-1.7B (Candle-GGUF) | ~2.5GB | 1.11GB | Qwen3-1.7B-Q4_K_M.gguf |
| Echo | 0 | 0 | Echo (testing) |

### ASR

| Model | Memory | File Size | Language | CER (TTS loopback) |
|-------|--------|-----------|----------|-------------------|
| XAsr (sherpa-onnx) | ~600MB | ~50MB | zh/en | — |
| Void | 0 | 0 | — | — (testing) |

### TTS

| Model | Memory | File Size | Notes |
|-------|--------|-----------|-------|
| MatchaTts (sherpa-onnx) | ~500MB | 72MB + 76MB (vocoder) | Chinese/Chinese-English bilingual |
| Mute | 0 | 0 | Silent (testing) |

### VAD

| Model | Memory | File Size | Notes |
|-------|--------|-----------|-------|
| Earshot (Silero VAD) | ~10MB | Embedded | Pure Rust, no ONNX |
| Void | 0 | 0 | Always returns voice (testing) |

## Fedora 43 CUDA Installation and Configuration

```shell
sudo sh cuda_12.8.1_570.124.06_linux.run --toolkit --no-drm --silent --override
```

```zshrc
export PATH="/usr/local/cuda/bin:$PATH"
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/usr/local/cuda/lib64
export LIBRARY_PATH=$LIBRARY_PATH:/usr/local/cuda/lib64
```

```shell
conda create -n cuda
conda install conda-forge::gcc==14.3.0
conda install conda-forge::gxx==14.3.0
conda install anaconda::openssl
conda activate cuda
# Start development...
```

## Reference Guidelines

<https://rust-lang.github.io/api-guidelines/>

<https://rust-coding-guidelines.github.io/rust-coding-guidelines-zh/overview.html>

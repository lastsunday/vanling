+++
title = "Model Downloader"
weight = 500
[extra]
source_file_hash = "94fef3d37385474a7a0eaf295ac6159d89f069c8"
translated_at = "2026-07-30T00:00:00Z"
+++

# Model Downloader

The `vanling-server downloader` subsystem handles downloading and managing AI model files. All model metadata is compiled into the binary as JSON manifests, with no runtime external dependencies.

## Directory Structure

Manifest JSON files are located in `apps/server/src/downloader/manifests/{category}/` and embedded into the binary at compile time via `include_dir!`.

## Manifest File Format

Each JSON file describes all variants and file listings for a model.

```json
{
  "config": { "category": "tts", "model": "matcha" },
  "default_variant": "matcha-icefall-zh-en",
  "variants": {
    "matcha-icefall-zh-en": {
      "files": [
        {
          "url": "https://huggingface.co/.../model.onnx",
          "path": "tts/model/matcha/matcha-icefall-zh-en/model.onnx",
          "sha256": null
        }
      ]
    }
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `config.category` | string | Category name, corresponds to `{cat}_model` in config |
| `config.model` | string | Model name, corresponds to the value of `{cat}_model` in config |
| `default_variant` | string / null | Default variant, used when neither `--variant` nor config is specified |
| `variants` | object | Variant name → file list |
| `files[].url` | string | Download URL from Hugging Face or other sources |
| `files[].path` | string | Storage path relative to the data directory |
| `files[].sha256` | string / null | Optional SHA256 checksum; `null` skips verification |

### Archives Format

In addition to `files`, manifests also support an `archives` field for downloading and extracting compressed archives. 3/4 of manifests use this format:

```json
{
  "config": { "category": "asr", "model": "x_asr" },
  "default_variant": "default",
  "variants": {
    "default": {
      "archives": [
        {
          "url": "https://huggingface.co/.../sense-voice-zh-en-ja-ko-yue-2025-03-19.tar.bz2",
          "path": "asr/model/x_asr/default/",
          "sha256": "abc123..."
        }
      ]
    }
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `archives[].url` | string | Archive download URL |
| `archives[].path` | string | Extraction target directory |
| `archives[].sha256` | string / null | Optional SHA256 checksum |

Supported archive formats: `.tar.gz`, `.tar.bz2`, `.zip`. After download, they are automatically extracted to the `path` directory.

Manifests currently using `archives`: `llm/qwen3.json`, `reference/audio.json`. Only `tts/matcha.json` uses the `files` format.

### Path Derivation Rules

The `path` field follows the format `{category}/model/{model}/{variant}/{file}`, all lowercase. For example, the TTS Matcha config file path is `tts/model/matcha/matcha-icefall-zh-en/model.onnx`.

The `*_path` config items (e.g., `tts_path`) are now optional. When omitted, the system automatically derives the path from `{model}+{variant}`.

## CLI Commands

### Downloader

```shell
vanling-server downloader <COMMAND>
```

| Subcommand | Description |
|------------|-------------|
| `install` | Download AI models to the local data directory |
| `wizard` | Interactive wizard mode |
| `list` | List all available models and their variants |
| `update-checksums` | Compute SHA256 of downloaded files and write back to manifest JSON |

Running `vanling-server downloader` (without subcommands) automatically displays help information.

#### install

```shell
vanling-server downloader install [category] [model] [variant] [options]
```

| Argument | Description |
|----------|-------------|
| `category` | Category: `tts`, `asr`, `llm`, `vad`, `reference`; defaults to all |
| `model` | Model name, e.g., `qwen3`, `matcha`, `x_asr` |
| `variant` | Variant name, e.g., `0.5b`, `1.7b`, `tiny`, `default` |
| `--data-dir <path>` | Data directory, defaults to `data` |
| `--quiet` | Silent mode, suppress progress output |
| `--mirror <url>` | Custom mirror domain, can be specified multiple times, replaces the built-in `hf-mirror.com` |
| `--override <path>` | Override file path or URL, JSON format |
| `--all` | Download all files from all manifests (ignores config file) |
| `-c, --config <path>` | Explicitly specify a config file (optional); auto-discovers `application.toml` by default |

**Examples:**

```shell
# Download models required by the current config (auto-discovers application.toml)
vanling-server downloader install

# Download models required by current config, tts category only
vanling-server downloader install tts

# Without config file, use defaults (MatchaTts + Qwen3×2)
vanling-server downloader install

# Explicitly specify config file
vanling-server downloader install --config my-config.toml

# Use custom mirror
vanling-server downloader install --mirror https://my-mirror.example.com

# Download all files from all manifests (ignores config file)
vanling-server downloader install --all
```

#### wizard

```shell
vanling-server downloader wizard [options]
```

| Argument | Description |
|----------|-------------|
| `--data-dir <path>` | Data directory, defaults to `data` |
| `--quiet` | Silent mode, suppress progress output |

#### update-checksums

```shell
vanling-server downloader update-checksums [options]
```

| Argument | Description |
|----------|-------------|
| `--data-dir <path>` | Data directory, defaults to `data` |
| `--quiet` | Silent mode, suppress progress output |

Scans downloaded files in `data_dir`, computes SHA256, and writes back to manifest JSON. Preferentially reads cached SHA from `download-report.json` to avoid re-reading large files.

```shell
# Update SHA for files in the default data directory
vanling-server downloader update-checksums

# Specify a data directory
vanling-server downloader update-checksums --data-dir /path/to/models
```

### List Models

```shell
vanling-server downloader list [category] [--json]
```

Lists all available models and their variants in a tree structure:

```
tts
  └── matcha
      └── matcha-icefall-zh-en (default)
```

**Moon tasks:**

```shell
moon run server:downloader                # Equivalent to vanling-server downloader install
moon run server:downloader -- vad         # Download only VAD
moon run server:downloader -- tts matcha  # Download only TTS Matcha
moon run server:downloader-list           # List all available models
moon run server:downloader-list -- --json # JSON format output
moon run server:download-all-and-checksums # Download all models and update SHA
```

## Download Workflow

### Request Method

Based on `reqwest` + `rustls-tls`, no OpenSSL linkage required, pure Rust TLS stack. Does not depend on the `hf-hub` library.

### Mirror Fallback

For Hugging Face URLs, mirror candidates are automatically added:

1. Original URL
2. Domain replaced with mirror domain (default `hf-mirror.com`)

The mirror list can be fully replaced via the `--mirror` parameter. URLs not from `huggingface.co` are not processed for mirroring.

### Concurrency Control

Uses `tokio::sync::Semaphore` to limit to a maximum of **4** concurrent downloads, with `JoinSet` for ordered result collection.

### Download Flow

```
Client → Check local file (SHA256 match → skip)
       → Create parent directory
       → Generate candidate URL list (original + mirror)
       → Try candidate URLs in sequence (write to .tmp file)
       → SHA256 verification
       → Atomic rename .tmp → target file
       → Return (size, SHA256)
```

### Caching and Verification

- When a local file exists, compute its SHA256:
  - Match → skip (cache hit)
  - Mismatch → delete and re-download
- `sha256: null` means skip verification
- `downloader update-checksums` writes the actual SHA256 of downloaded files back to the manifest file

### Download Report

After each download completes, a `download-report.json` is generated in the data directory, containing the completion time, base path, and status of each file. `update-checksums` preferentially reads SHA cache from this report to avoid re-reading large files:

```json
{
  "completed_at": "2026-06-16T12:00:00Z",
  "base_dir": "data",
  "files": [
    { "path": "tts/model/matcha/matcha-icefall-zh-en/vocos-16khz-univ.onnx", "size": 1024, "sha256": "abc...", "status": "downloaded" }
  ]
}
```

## Configuration Integration

### config_to_targets

Reads the `*_model` / `*_variant` fields from `application.toml` and converts them into a download target list. Correspondences for each model enum:

| Config Field | Possible Values | Download Target |
|-------------|----------------|-----------------|
| `tts_model` | `matcha_tts` / `mute` | `mute` skipped |
| `asr_model` | `x_asr` / `void` | `void` skipped |
| `llm_provider` | `local_qwen3` / `local_echo` | `local_echo` skipped |
| `vad_model` | `earshot` / `void` | Both skipped (earshot embedded, void no-op) |

### Config File Discovery

The `downloader` command shares the same config discovery logic as the server:

- `VANLING_CONFIG` environment variable → `application.toml` in current directory → fallback `application.toml`

Without a config file, `AppConfig` defaults are used:

| Module | Default Model | Description |
|--------|--------------|-------------|
| TTS | MatchaTts (`matcha-icefall-zh-en`) | Downloads `matcha` |
| ASR | XAsr (default variant) | Downloads `x_asr` |
| LLM | Qwen3 (default variant) | Downloads `qwen3` |
| VAD | Earshot | Embedded weights, no download |

### load_selections / upsert_config

- `load_selections`: Parses the `[global]` section of `application.toml`, extracting `*_model` / `*_variant` / `*_path` key-value pairs
- `upsert_config`: Writes new selections to `application.toml`, preserving non-`[global]` sections

## Override System

`--override` accepts a local file path or HTTP URL, JSON format:

```json
{
  "tts/model/matcha/matcha-icefall-zh-en/model.onnx": {
    "url": "http://localhost:8080/model.onnx",
    "sha256": "abc123..."
  },
  "llm/model/qwen3/0.6b/model.gguf": {
    "url": "http://localhost:8080/model.gguf",
    "sha256": null
  }
}
```

Matching rule: exact match by `path` field. Overrides `url`, optionally overrides `sha256` (`null` means no checksum).

## Interactive Wizard

`vanling-server downloader wizard` provides an interactive selection flow:

1. **Find config**: Locate `application.toml`, read existing selections
2. **Show catalog**: List all available models and their variants by category
3. **Cycle selection**: Arrow keys to select category → model → variant
4. **Preview**: Display the final selection set and corresponding config items
5. **Write config**: Write selections to `application.toml`
6. **Auto-download**: Execute downloads one by one in category order

The last item in the category list is `done`; selecting it ends the loop. All interactions use arrow keys for navigation and Enter to confirm.

## Architecture Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| TLS stack | rustls | Pure Rust, no OpenSSL linkage, eliminates build bottlenecks |
| Runtime dependencies | None | Manifests embedded at compile time via `include_dir!`, binary is self-contained |
| Model discovery | JSON manifest | Flexible and extensible, adding a model only requires adding a JSON file |
| Download library | reqwest | Feature-complete, supports streaming downloads, proxies, TLS |
| Concurrency control | Semaphore + JoinSet | Clean async concurrency pattern |
| Cache strategy | SHA256 verification | Avoids redundant downloads, ensures data integrity |
| Path derivation | Automatic fallback | `*_path` is optional, reducing configuration overhead |

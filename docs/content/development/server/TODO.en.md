+++
title = "TODO"
weight = 204
[extra]
source_file_hash = "97e6fbfa04e1eb48e87bba565af23d2bcc1b7045"
translated_at = "2026-07-09T00:00:00Z"
+++

# TODO

A to-do list organized by project directory. Before fixing, please read [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) to understand the development conventions.

## apps/server

### Session

| Module | File | Issue | Status |
|--------|------|-------|--------|
| stop_round race condition | `service/src/chobits/session/round.rs` | Missing synchronization between `llm_tts_handle` and `stop_round`, potential use-after-cancel | 🔴 P0 |
| Opus division by zero | `api/src/ws/default_listener.rs` | Division by zero when channels=0 / sample_rate=0 | 🟡 P1 |
| Clock overflow | `service/src/chobits/session/mod.rs` | `Local::now()` is non-monotonic, subtraction can overflow | 🟡 P1 |

### Protocol

| Module | File | Issue | Status |
|--------|------|-------|--------|
| Message types | `api/src/ws/frame.rs` | Missing `system`, `alert`, `custom` message types (compared to xiaozhi-esp32 spec) | ⚠️ P2 |

### AI Modules

| Module | File | Issue | Status |
|--------|------|-------|--------|
| LLM thread safety | `api/src/llm/model/qwen3/mod.rs` | `thread::spawn` + `block_on`, missing `catch_unwind`, panic silently crashes | 🔴 P0 |
| LLM Echo thread | `api/src/llm/model/echo/mod.rs` | Same issue | 🔴 P0 |
| VAD sample rate | `api/src/vad/` | Hardcoded 16kHz, non-16kHz input silently fails | ⚠️ P2 |
| ASR | `api/src/asr/` | SenseVoice (sherpa-onnx), no `Sync` trait, 16kHz mono only | ⚠️ P2 |
| LLM history blocking | `api/src/llm/model/qwen3/mod.rs` | DB persistence blocks the entire thread | 🟡 P1 |
| describe O(n) | `api/src/llm/model/qwen3/mod.rs` | Rebuilds full message history on every request | 🟢 P3 |
| TTS clone storm | `api/src/tts/` | `Arc<str>` vs `String` clone storm | 🟢 P3 |

### Persistence

| Module | File | Issue | Status |
|--------|------|-------|--------|
| RecordCollector unbounded | `api/src/record/recorder.rs` | `Vec<RecordEntry>` has no size limit, unbounded memory growth under high concurrency | 🟡 P1 |
| Double serialization | `api/src/record/recorder.rs` | Double JSON serialization in record path | 🟢 P3 |

### Security

| Module | File | Issue | Status |
|--------|------|-------|--------|
| WS auth | `api/src/ws/mod.rs` | WS handler has no auth layer, all WS connections unauthenticated | 🔴 P0 |
| ~~JWT secret~~ | ~~`api/src/config/mod.rs`~~ | ~~Hardcoded default secret `chobits-jwt-secret`~~ | ✅ Fixed |
| Token logging | `api/src/auth.rs` | Access token logged in plain text in tracing span | 🟡 P1 |
| Refresh revocation | `api/src/auth.rs` | No revocation mechanism for refresh tokens | 🟡 P1 |

### MCP

| Module | File | Issue | Status |
|--------|------|-------|--------|
| Lock ordering risk | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server lock order ABBA, potential deadlock | 🟡 P1 |
| Missing auth | `api/src/mcp/mod.rs` | `/mcp` endpoint auth is commented out | 🟡 P1 |
| Error handling | `api/src/mcp/` | Incomplete error handling | 🟢 P3 |

### Database

| Module | File | Issue | Status |
|--------|------|-------|--------|
| email constraint | `migration/src/m20241230_000001_init.rs` | Entity annotated with `#[sea_orm(unique)]`, migration does not implement UNIQUE | 🟡 P1 |
| Foreign key constraints | `migration/src/m20241230_000001_init.rs` | Missing FK: `round.session_id`, `round_data.round_id`, `frame.round_id` | 🟡 P1 |
| Timestamp auto-fill | `entity/src/config.rs` | `Config` entity missing `ActiveModelBehavior`, timestamps not auto-filled | 🟢 P3 |

### Performance

| Issue | Location | Description | Status |
|-------|----------|-------------|--------|
| Audio hot path cloning | `api/src/ws/default_listener.rs` | Frequent `data.to_vec()` cloning every 20ms | 🟡 P1 |

## libs

### framework

| Module | File | Issue | Status |
|--------|------|-------|--------|
| signal macro | `framework/src/signal.rs` | Uses non-existent `debug_error!` macro, fails to compile on non-unix | 🔴 P0 |
| Panic handling | `framework/src/panic.rs` | Uses `eprintln!` instead of `tracing::error!`, bypasses Sentry | 🟡 P1 |
| Runtime race condition | `framework/src/runtime.rs` | Race condition in `OnceLock` initialization | 🟡 P1 |
| Graceful shutdown | `framework/src/signal.rs` | Missing shutdown ordering across modules | 🟡 P1 |

## Cross-project

| Issue | Involves | Description | Status |
|-------|----------|-------------|--------|
| Graceful shutdown order | apps/server + libs | Missing shutdown ordering across modules | 🟡 P1 |

---

## Severity Guide

| Level | Meaning | Action |
|-------|---------|--------|
| 🔴 P0 | Must fix immediately | Compile error, no auth, data inconsistency |
| 🟡 P1 | Should fix | Race conditions, memory leaks, security risks |
| ⚠️ P2 | Missing feature | Incomplete protocol, insufficient configurability |
| 🟢 P3 | Optimization | Performance, code quality |

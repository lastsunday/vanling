+++
title = "TODO"
weight = 204
[extra]
source_file_hash = "97e6fbfa04e1eb48e87bba565af23d2bcc1b7045"
translated_at = "2026-06-28T18:00:00Z"
+++

# TODO

A to-do list organized by project directory. Before fixing, please read [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) to understand the development conventions.

## apps/server

### WS Session

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| stop_round race condition | `api/src/ws/session/round.rs` | Missing synchronization between `llm_tts_handle` and `stop_round`, potential use-after-cancel | 🔴 P0 |
| Opus division by zero | `api/src/ws/session/listener.rs` | Division by zero when channels=0 / sample_rate=0 | 🟡 P1 |
| Clock overflow | `api/src/ws/session/` | `Local::now()` is non-monotonic, subtraction can overflow | 🟡 P1 |

### Protocol

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| Message types | `api/src/ws/frame.rs` | Missing `system`, `alert`, `custom` message types (compared to xiaozhi-esp32 spec) | ⚠️ P2 |

### AI Modules

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| LLM thread safety | `api/src/llm/client.rs` | `thread::spawn` + `block_on`, missing `catch_unwind`, panic silently crashes | 🔴 P0 |
| VAD sample rate | `api/src/vad/` | Hardcoded 16kHz, non-16kHz input silently fails | ⚠️ P2 |
| ASR | `api/src/asr/` | SenseVoice (sherpa-onnx), no `Sync` trait, 16kHz mono only | ⚠️ P2 |
| LLM history blocking | `api/src/llm/client.rs` | DB persistence blocks the entire thread | 🟡 P1 |
| describe O(n) | `api/src/llm/client.rs` | Rebuilds full message history on every request | 🟢 P3 |
| TTS clone storm | `api/src/tts/` | `Arc<str>` vs `String` clone storm | 🟢 P3 |

### Persistence

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| on_session_end not called | `api/src/record/observer.rs` | `SessionObserver::on_session_end` is defined but never called, DB session.end_time is always NULL | 🔴 P0 |
| RecordCollector unbounded | `api/src/record/collector.rs` | `Vec<RoundBuffer>` has no size limit, unbounded memory growth under high concurrency | 🟡 P1 |
| Double serialization | `api/src/record/` | Double JSON serialization in record path | 🟢 P3 |

### Security

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| WS auth | `api/src/ws/mod.rs:59` | `// .layer(get_auth_layer())` commented out, all WS connections unauthenticated | 🔴 P0 |
| JWT secret | `api/src/config/mod.rs` | Hardcoded default secret `chobits-jwt-secret` | 🔴 P0 |
| Token logging | `api/src/auth.rs` | Access token logged in plain text in tracing span | 🟡 P1 |
| Refresh revocation | `api/src/auth.rs` | No revocation mechanism for refresh tokens | 🟡 P1 |

### MCP

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| Lock ordering risk | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server lock order ABBA, potential deadlock | 🟡 P1 |
| Missing auth | `api/src/mcp/` | `/mcp` endpoint has no authentication | 🟡 P1 |
| Error handling | `api/src/mcp/` | Incomplete error handling | 🟢 P3 |

### Database

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| email constraint | `entity/src/user.rs` | Entity annotated with `#[sea_orm(unique)]`, migration does not implement UNIQUE | 🟡 P1 |
| Foreign key constraints | `migration/src/m20241230_000001_init.rs` | Missing FK: `round.session_id`, `round_data.round_id`, `frame.round_id` | 🟡 P1 |
| Timestamp auto-fill | `entity/src/` | `Config` entity missing `ActiveModelBehavior`, timestamps not auto-filled | 🟢 P3 |

### Performance

| Issue | Location | Description | Severity |
|-------|----------|-------------|----------|
| Audio hot path cloning | `api/src/ws/session/listener.rs` | Frequent `data.to_vec()` cloning every 20ms | 🟡 P1 |

## libs

### framework

| Module | File | Issue | Severity |
|--------|------|-------|----------|
| signal macro | `framework/src/signal.rs` | Uses non-existent `debug_error!` macro, fails to compile on non-unix | 🔴 P0 |
| Panic handling | `framework/src/panic.rs` | Uses `eprintln!` instead of `tracing::error!`, bypasses Sentry | 🟡 P1 |
| Runtime race condition | `framework/src/runtime.rs` | Race condition in `OnceLock` initialization | 🟡 P1 |
| Graceful shutdown | `framework/src/signal.rs` | Missing shutdown ordering across modules | 🟡 P1 |

## Cross-project

| Issue | Involves | Description | Severity |
|-------|----------|-------------|----------|
| Graceful shutdown order | apps/server + libs | Missing shutdown ordering across modules | 🟡 P1 |

---

## Severity Guide

| Level | Meaning | Action |
|-------|---------|--------|
| 🔴 P0 | Must fix immediately | Compile error, no auth, data inconsistency |
| 🟡 P1 | Should fix | Race conditions, memory leaks, security risks |
| ⚠️ P2 | Missing feature | Incomplete protocol, insufficient configurability |
| 🟢 P3 | Optimization | Performance, code quality |

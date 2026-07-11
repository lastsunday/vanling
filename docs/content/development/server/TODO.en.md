+++
title = "TODO"
weight = 204
[extra]
source_file_hash = "b5f1fa004c9575e6ffcd55bc1c8ffadbcfc8526b"
translated_at = "2026-07-11T00:00:00Z"
+++

# TODO

> **⚠️ Note: This document is an exploratory feature inventory, referencing industry standards (Alexa+, Gemini, Siri AI) and reference projects (xiaozhi-esp32-server, xiaozhi-esp32-server-java, xiaozhi-server-go). Most items will not be implemented — they serve as reference for chobits' positioning and trade-offs. Actual development priorities follow the project Roadmap.**

To-do list organized by functional module. Before fixing, please read [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) to understand the development conventions.

## Security & Authentication

| Priority | Item | Location | Description | Open Source Solution | Status |
|----------|------|----------|-------------|---------------------|--------|
| 🔴 P0 | WS Authentication | `api/src/ws/mod.rs` | WS handler has no auth layer, all WS connections unauthenticated | Suggest: `axum-jwt-auth` — JWT middleware, JWKS caching | 🔴 P0 |
| 🔴 P0 | Invite Code System | New feature | No invite code generation/verification/management, new user registration uncontrolled | Suggest: `nanoid` — short URL-safe ID generator | 🔴 P0 |
| 🟡 P1 | Rate Limiting | `api/src/auth.rs` | No login rate limiting / brute-force protection | Suggest: `tower-governor` + `governor` — GCRA algorithm | 🟡 P1 |
| 🟡 P1 | Refresh Revocation | `api/src/auth.rs` | No revocation mechanism for refresh tokens, logout only clears client-side | Existing: `redis-rs` + `sea-orm` | 🟡 P1 |
| 🟡 P1 | Token Logging | `api/src/auth.rs` | Access token logged in plain text in tracing span | Existing: `tracing` | 🟡 P1 |
| 🟡 P1 | OTA Device Activation | `api/src/ota.rs` | `activate` endpoint is a stub, returns "success" without verifying device, device info not persisted to DB | — | 🟡 P1 |
| 🟡 P1 | MCP Authentication | `api/src/mcp/mod.rs` | `/mcp` endpoint auth is commented out | Existing: `rmcp` | 🟡 P1 |

## Core Features

| Priority | Item | Location | Description | Open Source Solution | Status |
|----------|------|----------|-------------|---------------------|--------|
| 🔴 P0 | stop_round Race Condition | `service/src/chobits/session/round.rs` | Missing synchronization between `llm_tts_handle` and `stop_round`, potential use-after-cancel | — | 🔴 P0 |
| 🔴 P0 | LLM Thread Safety | `api/src/llm/model/qwen3/mod.rs` | `thread::spawn` + `block_on`, missing `catch_unwind`, panic silently crashes | — | 🔴 P0 |
| 🔴 P0 | LLM Echo Thread | `api/src/llm/model/echo/mod.rs` | Same issue | — | 🔴 P0 |
| 🔴 P0 | Wake Word Support | `api/src/ws/` + protocol layer | ESP32 client already has ESP-SR offline wake, server needs to handle `wake_word` message type, current protocol lacks this field. Industry standard (Echo/Google/Xiaozhi all have it), response latency <200ms | — (ESP32 side ESP-SR, server only protocol parsing) | 🔴 P0 |
| 🟡 P1 | Voiceprint Recognition | New feature | Reference project xinnan-tech implements this (3D-Speaker model), processed in parallel with ASR, identifies speaker identity for LLM personalization. sherpa-onnx already supports speaker identification (ECAPA-TDNN/WeSpeaker), no new dependency needed. hackers365 Go version uses Qdrant vector DB + dynamic TTS voice switching, more mature architecture. Needs: registration/management/recognition flow + DB storage of voiceprint vectors + LLM context injection | Existing: `sherpa-onnx` (ECAPA-TDNN/WeSpeaker) | 🟡 P1 |
| 🟡 P1 | Opus Division by Zero | `api/src/ws/default_listener.rs` | Division by zero when channels=0 / sample_rate=0 | — | 🟡 P1 |
| 🟡 P1 | Clock Overflow | `service/src/chobits/session/mod.rs` | `Local::now()` is non-monotonic, subtraction can overflow | Existing: `jiff` (monotonic clock) | 🟡 P1 |
| 🟡 P1 | Device Management | New feature | No device registration/binding/listing, no device persistence after OTA activation. Reference projects have complete device lifecycle management (registration/status/config/OTA/batch operations) | Existing: `sea-orm` | 🟡 P1 |
| 🟡 P1 | Music Playback | MCP tool or standalone | Reference project xinnan-tech has `play_music.py` + `hass_play_music.py`; joey-zhou has `MusicPlayer` with LRC lyrics sync. Industry standard | Suggest: `rodio` — cross-platform audio playback | 🟡 P1 |
| 🟡 P1 | Timer/Reminders/Alarms | MCP tool or standalone | Industry standard ("Alexa, set a timer"), currently no timer/reminder mechanism | Suggest: `tokio-cron-scheduler` — Tokio async cron | 🟡 P1 |
| 🟡 P1 | Continued Conversation | `service/src/chobits/session/` | Microphone should briefly remain open after reply, allowing wake-word-free follow-up. Gemini / Alexa+ both support this | — | 🟡 P1 |
| 🟡 P1 | Emotion Recognition | `api/src/llm/model/` (analyze_emotion) | Current stub returns "happy". Industry approach: audio features (wav2vec2 SER) + text sentiment (GoEmotions) dual-channel fusion, used to adjust TTS tone and reply style | Existing: `sherpa-onnx` (SER model) | 🟡 P1 |
| 🟡 P1 | Personalized Memory / Long-term Preferences | New feature | Currently only chat history, no long-term preference storage. Reference project xinnan-tech has PowerMem (user profiling + Ebbinghaus forgetting curve + vector retrieval); joey-zhou has 3 memory modes (window/summary/long + graph retrieval) | Suggest: `qdrant` + `rig-core` — vector DB + RAG framework | 🟡 P1 |
| 🟡 P1 | RAG Knowledge Base | MCP or built-in module | Reference project xinnan-tech integrates RAGFlow; joey-zhou has EmbeddingModelFactory + graph retrieval. Current MCP framework can connect but has no built-in vector retrieval | Existing: `rig-core` (10+ vector store backends) | 🟡 P1 |
| 🟡 P1 | Multi-language TTS | `api/src/tts/` | Currently single-language TTS voice. ESP32 client already supports 25+ language ASR, TTS side needs to match | Suggest: `sherpa-onnx` (Piper/VITS ONNX models) | 🟡 P1 |
| 🟡 P1 | Home Assistant Integration | MCP or standalone module | Reference project xinnan-tech has 3 HA integration methods (community plugin / HA as LLM tool / HA MCP Server). Smart home is a core voice assistant scenario | Existing: `rmcp` (HA MCP Server) | 🟡 P1 |
| 🟡 P1 | Inter-device Calls | MCP tool or standalone | Reference project xinnan-tech has `call_device.py`, ESP32 devices can call each other like phones, needs MQTT gateway + contacts management | Suggest: `rumqttc` — pure Rust MQTT client | 🟡 P1 |
| 🟡 P1 | Intent Recognition | New feature | Reference project xinnan-tech supports 3 modes: function_call (recommended), intent_llm (dedicated LLM), nointent. chobits currently has no independent intent layer | Existing: `rmcp` (function_call) | 🟡 P1 |
| 🟡 P1 | Audio Hot-path Cloning | `api/src/ws/default_listener.rs` | Frequent `data.to_vec()` cloning every 20ms | — | 🟡 P1 |
| 🟡 P1 | Quick Reply / Pre-acknowledgment | New feature | Play short phrases ("I'm here", "Coming") during LLM inference to reduce perceived latency. Reference project hackers365 Go version implements this, UX critical, simple to implement | — | 🟡 P1 |
| 🟡 P1 | Dynamic TTS Voice Switching | New feature | Automatically switch TTS voice based on voiceprint identification. Reference project hackers365 Go version implements this (sherpa-onnx voiceprint + per-speaker TTS voice), natural extension of voiceprint recognition | Existing: `sherpa-onnx` (voiceprint + TTS switching) | 🟡 P1 |
| 🟡 P1 | Semantic VAD | New feature | Reference project OpenAI Realtime implements model-level turn detection (not pure silence detection), can distinguish coughing from starting a new sentence. Industry gold standard, smarter interruption/turn-taking than traditional VAD | Suggest: `wavekat-vad` — unified trait over WebRTC+Silero | 🟡 P1 |
| 🟡 P1 | LLM History Blocking | `api/src/llm/model/qwen3/mod.rs` | DB persistence blocks the entire thread | — | 🟡 P1 |
| 🟡 P1 | Recorder Unbounded | `api/src/record/recorder.rs` | `Vec<RecordEntry>` has no size limit, unbounded memory growth under high concurrency | — | 🟡 P1 |
| ⚠️ P2 | Message Types | `api/src/ws/frame.rs` | Missing `system`, `alert`, `custom`, `wake_word` message types (compared to xiaozhi-esp32 spec) | — | ⚠️ P2 |
| ⚠️ P2 | Multi ASR/TTS Provider | `api/src/asr/` + `api/src/tts/` | Reference project xinnan-tech supports 12 ASR + 18+ TTS providers (including free EdgeTTS); joey-zhou has 7 STT + 8 TTS. chobits has only 1 ASR + 1 TTS | Existing: `sherpa-onnx` (multi-model switching) | ⚠️ P2 |
| ⚠️ P2 | Plugin System | New feature | Reference project xinnan-tech has 13 built-in plugins + hot-reload. chobits has no plugin architecture, feature extensions require modifying core code | Suggest: `wasmtime` — WASI P2 + Component Model | ⚠️ P2 |
| ⚠️ P2 | MCP Market / Tool Marketplace | New feature | Reference project hackers365 Go version implements MCP tool "app store": aggregates multiple third-party marketplaces (e.g. ModelScope), one-click import of remote MCP services + hot-reload. chobits currently has no MCP tool discovery/aggregation mechanism | Existing: `rmcp` | ⚠️ P2 |
| ⚠️ P2 | MCP Debug Console | New feature | Reference project hackers365 Go version has Agent/Device-level MCP remote debugging: web console generates per-agent MCP endpoints, real-time call testing, supports per-agent tool filtering. chobits currently has no MCP debugging tools | Existing: `rmcp` | ⚠️ P2 |
| ⚠️ P2 | Setup Wizard + Full-chain Testing | New feature | Reference project hackers365 Go version has first-run wizard (OTA/VAD/ASR/LLM/TTS step-by-step config) + per-component latency testing + visualization charts. chobits currently has no deployment wizard | — | ⚠️ P2 |
| ⚠️ P2 | MCP Tool Aggregation | New feature | Reference projects xiaozhi-mcp / yuexianga/xiaozhi-mcp provide pre-built tool libraries (DingTalk/QQ/system monitoring/WebPilot/math), ready to use. chobits has no built-in MCP tool package | Existing: `rmcp` | ⚠️ P2 |
| ⚠️ P2 | MQTT Gateway | New feature | Reference project xinnan-tech/xiaozhi-mqtt-gateway implements MQTT+UDP → WS bridging: distributed deployment + dynamic load balancing + HMAC authentication + MCP command dispatch. chobits currently only supports WS protocol | Suggest: `rumqttc` — pure Rust, tokio-native | ⚠️ P2 |
| ⚠️ P2 | Conversational Prosody TTS | New feature | Reference project Sesame CSM (Apache 2.0 open source) generates breaths/hesitations/laughter for conversational prosody, making speech more human-like. Cartesia Sonic 3.5 uses SSM architecture for <90ms TTS latency | Suggest: `csm.rs` — Rust Sesame CSM (AGPL-3.0) | ⚠️ P2 |
| ⚠️ P2 | Emotion-adaptive Tone | New feature | Reference project Hume AI EVI detects 600+ emotion tags (hesitation/sarcasm/relief), adaptively adjusts TTS tone. MiniMax Speech-2.8 supports 7 emotions + 0-100% intensity control + interjection tags `(laughs)` `(sighs)` | Suggest: `voirs-emotion` — multi-dimensional emotion control | ⚠️ P2 |
| ⚠️ P2 | Agent Task Orchestration | New feature | Reference projects Alexa+ autonomously executes Uber/OpenTable/Grubhub; Rabbit R1 LAM Large Action Model; Doubao Super Mode autonomously decomposes complex tasks. LLM + MCP tool chain for autonomous task execution | Existing: `rig-core` (Agent/Chain/Router) | ⚠️ P2 |
| ⚠️ P2 | VAD Sample Rate | `api/src/vad/` | Hardcoded 16kHz, non-16kHz input silently fails | — | ⚠️ P2 |
| ⚠️ P2 | ASR | `api/src/asr/` | SenseVoice (sherpa-onnx), no `Sync` trait, 16kHz mono only | Existing: `sherpa-onnx` | ⚠️ P2 |
| 🟢 P3 | Vision Perception (VLLM) | New feature | Reference project xinnan-tech supports GLM-4V / Qwen-VL vision models for photo understanding. chobits currently voice-only | Suggest: `reqwest` → Ollama/vLLM (OpenAI API) | 🟢 P3 |
| 🟢 P3 | Proactive Suggestions | New feature | Gemini Daily Brief / Alexa+ proactive alerts for traffic/deals/calendar. Requires scheduled tasks + user context reasoning | Suggest: `tokio-cron-scheduler` | 🟢 P3 |
| 🟢 P3 | Multimodal (Voice + Screen + Video) | New feature | Gemini 2.5 / GPT-Realtime / Siri AI all support camera/screen input. chobits currently voice-only | Suggest: `webrtc-rs` (v0.17.x) | 🟢 P3 |
| 🟢 P3 | Cross-device Continuity | New feature | Alexa+: Echo→phone→PC seamless conversation context switching. Requires session state sync mechanism | — (custom: SQLite + WS delta sync) | 🟢 P3 |
| 🟢 P3 | Speaker Diarization | `api/src/listener/` | Speaker separation, distinguishing different users in multi-person scenarios. sherpa-onnx already supports this (ECAPA-TDNN + AHC clustering) | Existing: `sherpa-onnx` / Suggest: `polyvoice` | 🟢 P3 |
| 🟢 P3 | Voice Cloning | `api/src/tts/` | Reference project xinnan-tech supports Volcengine voice cloning; joey-zhou supports per-role voice cloning. MatchaTTS already supports reference audio, needs exposed config interface | Suggest: `sherpa-onnx` (speaker embedding) | 🟢 P3 |
| 🟢 P3 | Server-side AEC Denoising | `api/src/ws/default_listener.rs` | joey-zhou implements WebRTC AEC3 server-side echo cancellation (with noise suppression + high-pass filter + adaptive gain). chobits only has client-side AEC | Suggest: `aec3` — pure Rust WebRTC AEC3 | 🟢 P3 |
| 🟢 P3 | WebRTC Real-time Audio/Video | New feature | Reference project dairoot/xiaozhi-webrtc implements WebRTC low-latency + Live2D + multimodal vision + MCP. chobits currently only WS protocol | Suggest: `webrtc-rs` (v0.17.x) | 🟢 P3 |
| 🟢 P3 | Audio Normalization Integration | `api/src/util/compressor.rs` → pipeline | `adaptive_normalize()` implemented but not integrated into TTS output pipeline | — | 🟢 P3 |
| 🟢 P3 | Live2D Avatar | Client feature | Reference project Android client (TOM88812) implements: multi-model switching + real-time animation + custom characters + mood mode. chobits Flutter client can reference this | Suggest: `live2d-cubism-core-sys` + `flutter_rust_bridge` | 🟢 P3 |
| 🟢 P3 | Embodied AI / GPIO | New feature | Reference project py-xiaozhi implements: Raspberry Pi/Jetson/STM32 direct hardware control (motors/sensors/LEDs), camera vision understanding. Vertical scenario, not core voice assistant | Suggest: `rppal` (Raspberry Pi) / `gpio-cdev` | 🟢 P3 |
| 🟢 P3 | UGC Character Marketplace | New feature | Reference projects Character.AI has 10M+ user-created characters; Doubao agent platform supports no-code AI character creation. chobits could support user-defined AI personality + voice + backstory | — | 🟢 P3 |
| 🟢 P3 | Session Export/Delete | `api/src/record/` | Sessions can only be viewed, not exported or deleted | — | 🟢 P3 |
| 🟢 P3 | describe O(n) | `api/src/llm/model/qwen3/mod.rs` | Rebuilds full message history on every request | — | 🟢 P3 |
| 🟢 P3 | TTS Clone Storm | `api/src/tts/` | `Arc<str>` vs `String` clone storm | — | 🟢 P3 |
| 🟢 P3 | Double Serialization | `api/src/record/recorder.rs` | Double JSON serialization in record path | — | 🟢 P3 |
| 🟡 P1 | Piper/Kokoro TTS Integration | `api/src/tts/` | Open-source TTS alternatives: Piper (20M params/MIT/CPU 55ms latency/30+ languages) and Kokoro (82M/Apache 2.0/CPU real-time/54 voices). Can replace or supplement current MatchaTTS; Piper suits edge deployment, Kokoro offers best quality-to-size ratio | Suggest: `sherpa-onnx` (Piper ONNX models) | 🟡 P1 |
| ⚠️ P2 | Ambient Listening Mode | New feature | Passive listening mode from healthcare AI (Nuance DAX/Nabla): not wake-word triggered, continuously monitors ambient audio, proactively responds to user needs. Requires privacy architecture (Nabla model: no raw audio storage). Suitable for home/office scenarios | — | ⚠️ P2 |
| 🟢 P3 | Matter/Thread Protocol | New feature | Smart home hub standard protocol (Matter 1.3+Thread 1.4), enabling cross-platform device compatibility (Apple/Google/Amazon/Samsung). ESP32 already has Thread support, needs Matter SDK integration | Suggest: `rs-matter` — only production Rust Matter implementation | 🟢 P3 |
| 🟢 P3 | Guided Reasoning Dialogue | New feature | Educational AI (Zuoyebang/Youdao) guided mode: doesn't give answers directly, step-by-step guides user thinking. Suitable for children/learning scenarios, requires LLM prompt engineering + dialogue state management | — | 🟢 P3 |

## Infrastructure

| Priority | Item | Location | Description | Open Source Solution | Status |
|----------|------|----------|-------------|---------------------|--------|
| 🟡 P1 | Email Constraint | `migration/src/m20241230_000001_init.rs` | Entity annotated with `#[sea_orm(unique)]`, migration does not implement UNIQUE | Existing: `sea-orm` (migration fix) | 🟡 P1 |
| 🟡 P1 | Foreign Key Constraints | `migration/src/m20241230_000001_init.rs` | Missing FK: `round.session_id`, `round_data.round_id`, `frame.round_id` | Existing: `sea-orm` (migration fix) | 🟡 P1 |
| 🟡 P1 | MCP Lock Ordering | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server lock order ABBA, potential deadlock | — | 🟡 P1 |
| 🟡 P1 | Graceful Shutdown Order | `framework/src/signal.rs` + `apps/server` | Missing shutdown ordering across modules | — | 🟡 P1 |
| 🟡 P1 | Panic Handling | `framework/src/panic.rs` | Uses `eprintln!` instead of `tracing::error!`, bypasses Sentry | Existing: `tracing` | 🟡 P1 |
| 🟡 P1 | Runtime Race Condition | `framework/src/runtime.rs` | Race condition in `OnceLock` initialization | — | 🟡 P1 |
| 🔴 P0 | Signal Macro | `framework/src/signal.rs` | Uses non-existent `debug_error!` macro, fails to compile on non-unix | — | 🔴 P0 |
| 🟢 P3 | Timestamp Auto-fill | `entity/src/config.rs` | `Config` entity missing `ActiveModelBehavior`, timestamps not auto-filled | Existing: `sea-orm` | 🟢 P3 |
| 🟢 P3 | MCP Error Handling | `api/src/mcp/` | Incomplete | Existing: `rmcp` | 🟢 P3 |

## Frontend (apps/server-ui)

| Priority | Item | Location | Description | Open Source Solution | Status |
|----------|------|----------|-------------|---------------------|--------|
| 🟡 P1 | Dashboard Page | `routes/_pathlessLayout.admin/index.tsx` | Shell only, renders "Hello", no stats/monitoring content | Existing: `@mantine/core` v9 + `@tanstack/react-query` | 🟡 P1 |
| 🟡 P1 | User CRUD Management UI | New page | No user list/create/delete/role management interface | Existing: `@mantine/core` v9 | 🟡 P1 |
| 🟡 P1 | Multi-user/RBAC Management | New feature | Reference project busy-worker Java management platform has Token usage monitoring + conversation duration + device activity + data visualization + RBAC. chobits Dashboard should include this | Existing: `@mantine/core` v9 | 🟡 P1 |
| 🟢 P3 | System Monitoring | New page | No server health/connection count/resource usage/error rate dashboard | Existing: `@mantine/core` v9 + `@tanstack/react-query` | 🟢 P3 |
| 🟢 P3 | MCP Dashboard | New feature | Reference project xiaozhi-mcphub has React frontend: multi-endpoint management + tool sync + group access control + logs. Reference for chobits MCP management | Existing: `@mantine/core` v9 | 🟢 P3 |

## Mobile (apps/app)

| Priority | Item | Location | Description | Open Source Solution | Status |
|----------|------|----------|-------------|---------------------|--------|
| 🟡 P1 | Flutter WS Integration | `apps/app/` | Flutter app has scaffolding but no WS + auth integration | Suggest: `web_socket_channel` | 🟡 P1 |
| 🟡 P1 | App CI/CD | `.github/workflows/` | No iOS/Android build/sign/publish pipeline | GitHub Actions | 🟡 P1 |

## Testing

| Priority | Item | Location | Description | Open Source Solution | Status |
|----------|------|----------|-------------|---------------------|--------|
| 🔴 P0 | WS Auth Tests | `apps/server/api/tests/` | WS endpoint has zero auth, no corresponding tests | Existing: `axum` test utils + `reqwest` | 🔴 P0 |
| 🔴 P0 | Wake Word Tests | `apps/server/api/tests/` | Wake word message handling has no tests, needs to verify protocol parsing and session wake flow | Existing: `axum` test utils | 🔴 P0 |
| 🟡 P1 | Auto Mode + AEC Tests | `apps/server/api/tests/session/` | Auto mode (barge_in=false) with/without AEC scenarios lacks dedicated tests | Existing: `axum` test utils | 🟡 P1 |
| 🟡 P1 | Button Talk Tests | `apps/server/api/tests/session/` | Manual mode (push-to-talk) lacks end-to-end tests | Existing: `axum` test utils | 🟡 P1 |
| 🟡 P1 | Continued Conversation Tests | `apps/server/api/tests/session/` | Follow-up flow with microphone kept open after reply has no tests | Existing: `axum` test utils | 🟡 P1 |
| 🟡 P1 | Rate Limiting Tests | `apps/server/api/tests/` | Login rate limiting feature does not exist, no tests | Existing: `axum` test utils | 🟡 P1 |
| 🟡 P1 | Emotion Recognition Tests | `apps/server/api/tests/` | analyze_emotion stub has no test coverage | — | 🟡 P1 |
| 🟡 P1 | Voiceprint Tests | `apps/server/api/tests/` | Voiceprint registration/recognition/LLM injection flow has no tests | — | 🟡 P1 |
| 🟡 P1 | Music Playback Tests | `apps/server/api/tests/` | Music playback feature has no tests | Existing: `axum` test utils | 🟡 P1 |

## To Be Confirmed / Exploratory

| Item | Description | Open Source Solution |
|------|-------------|---------------------|
| Voiceprint Solution Choice | 3D-Speaker (used by xinnan-tech) vs sherpa-onnx speaker ID (already in tech stack) vs pyannote. sherpa-onnx is optimal: no new dependency, supports ECAPA-TDNN/WeSpeaker/CAM++ models | Existing: `sherpa-onnx` |
| Plugin Architecture Design | xinnan-tech has 13 built-in plugins + hot-reload. Does chobits need a similar mechanism, or is MCP extension sufficient? | Suggest: `wasmtime` — WASI sandbox + Component Model |
| Multi-Provider Extension Strategy | Reference projects support 12+ ASR / 18+ TTS providers. Does chobits need a multi-provider architecture, or stay lean? | Suggest: trait abstraction (`AsrProvider`/`TtsProvider`) |
| Music Playback | Industry standard (Spotify/Apple Music integration), need to confirm implementation: MCP tool calling external API / built-in audio playback / TTS extension | Suggest: `rodio` |
| ESP32 CI/CD | Separate project (xiaozhi-esp32), outside this repo's scope, chobits provides WS + OTA interface as backend | — |
| Token Persistent Storage | Consider using DB table to store refresh tokens for multi-device management | Existing: `sea-orm` + `redis-rs` |
| Smart Home Integration | Alexa+ / Google Home core capability. Reference projects have 3 HA integration methods, need to evaluate chobits' positioning as hub | Existing: `rmcp` (HA MCP) |
| Agentic Capabilities | Alexa+ "Experts" / Gemini Spark 24/7 agents, can autonomously browse web/forms/orders. LLM + MCP tool calling has foundation, need to evaluate implementation depth | Existing: `rig-core` |
| Emotional TTS | Gemini 2.5 / Cartesia Sonic-3 support adjusting TTS tone based on emotion. Need to evaluate if MatchaTTS supports style/prosody control | Suggest: `voirs-emotion` |
| Local Privacy Processing | Industry trend: Echo/Fire devices migrating to local processing. chobits already has local Qwen3, can extend to full local ASR/TTS pipeline | Existing: `sherpa-onnx` + `candle` |
| Vision Capabilities | Reference project xinnan-tech supports VLLM (GLM-4V/Qwen-VL) for photo understanding. Does chobits need vision capabilities? | Suggest: `reqwest` → Ollama/vLLM |
| MCP Market Architecture Design | Reference project hackers365 Go version implements MCP tool "app store" (multi-market aggregation + hot-reload). Does chobits need MCP tool discovery/aggregation? | Existing: `rmcp` |
| Dynamic TTS Voice Switching | Reference project hackers365 Go version auto-switches TTS voice based on voiceprint. How should chobits link voiceprint recognition to TTS? | Existing: `sherpa-onnx` |
| Setup Wizard Design | Reference project hackers365 Go version has first-run wizard + full-chain latency testing. Does chobits need a deployment wizard to lower the barrier? | — |
| MQTT Gateway Architecture | Distributed deployment requires MQTT+UDP bridging + dynamic load balancing (ref: xinnan-tech/xiaozhi-mqtt-gateway). Does chobits need an MQTT gateway layer? | Suggest: `rumqttc` |
| MCP Tool Aggregation Strategy | Reference projects have pre-built tool libraries (DingTalk/QQ/system monitoring/WebPilot/math, etc.). Does chobits need a built-in MCP tool package? | Existing: `rmcp` |
| End-to-end S2S Architecture | OpenAI/Hume/Sesame use single model for audio input/output (not cascaded STT→LLM→TTS). Should chobits evolve from current cascaded approach? | Suggest: `csm.rs` (AGPL-3.0) or `moshi` (Apache 2.0) |
| Semantic VAD Implementation | OpenAI's model-level turn detection vs traditional VAD — how should chobits implement smarter interruption/turn-taking? | Suggest: `wavekat-vad` |
| Conversational Prosody TTS | Sesame CSM open-source model (Apache 2.0) directly usable — should chobits integrate breaths/hesitations/laughter prosody? | Suggest: `csm.rs` (AGPL-3.0) |
| Privacy Architecture Design | Always-listening device privacy (Bee no-audio-storage / Omi local processing) — how should chobits balance functionality and privacy? | Existing: `sherpa-onnx` (full local pipeline) |
| SSM Architecture TTS | Cartesia uses State Space Model instead of Transformers for <90ms TTS — should chobits consider SSM for TTS? | — (no Rust implementation) |
| Piper/Kokoro Evaluation | Open-source TTS models Piper (20M params/MIT) and Kokoro (82M/Apache 2.0) — are they suitable to replace or supplement current MatchaTTS? Piper suits edge deployment, Kokoro offers best quality-to-size ratio | Suggest: `sherpa-onnx` (Piper ONNX) |
| Step-Audio-TTS-3B Integration | StepFun open-source Chinese TTS (Apache 2.0, emotion control, dialect support) — should this serve as server-side TTS candidate? Need to evaluate GPU requirements and latency | Suggest: `reqwest` → StepFun API |
| Ambient Listening Architecture | Healthcare AI passive listening mode (non-wake-word), should chobits support this? How to ensure privacy? Reference Nabla (no raw audio storage) architecture | — |
| Matter Protocol Support | Smart home hub standard protocol (Matter 1.3+Thread 1.4), ESP32 already has Thread support. Does chobits need full Matter SDK integration for cross-platform device compatibility? | Suggest: `rs-matter` |

---

## Appendix B: Open Source Voice Models

> Open-source TTS/voice models, ranked by quality/size/license. Directly usable by chobits.

| Model | Params | License | Latency | Voice Cloning | Fit for chobits |
|-------|--------|---------|---------|---------------|-----------------|
| [Piper](https://github.com/rhasspy/piper) | ~20M | MIT | 55ms (10s audio) | No | ★★★★★ CPU-only, 30+ languages, edge deployment first choice |
| [Kokoro v1.0](https://github.com/hexgrad/kokoro) | 82M | Apache 2.0 | CPU real-time | No (KokoClone extension supports) | ★★★★★ Best quality-to-size ratio, 54 voices |
| [Step-Audio-TTS-3B](https://github.com/stepfun-ai/Step-Audio-TTS-3B) | 3B | Apache 2.0 | Requires GPU | Yes | ★★★★ Best Chinese voice quality + emotion control + dialects |
| [Coqui XTTS v2](https://github.com/coqui-ai/TTS) | 467M | CPML (non-commercial) | <200ms | Yes (6s sample) | ★★★★ Best cloning, 17 languages, but license restriction |
| [F5-TTS](https://github.com/SWivid/F5-TTS) | 335M | CC-BY-NC | Requires GPU | Yes (zero-shot) | ★★★ Excellent cloning, EN-ZH code-switching, but non-commercial |
| [Orpheus TTS](https://github.com/canopylabs-ai/orpheus-tts) | 3B | — | Requires GPU | No | ★★★ Llama backbone, style control |
| [Bark (Suno)](https://github.com/suno-ai/bark) | — | MIT | 0.8x (too slow) | No | ★★ Expressive (laughter/sighs/music), but poor real-time |

### Recommended Architecture

```
ESP32 (edge)                    Server (chobits)
┌──────────┐                ┌─────────────────────┐
│ VAD      │                │ ASR: sherpa-onnx     │
│ Piper/   │◄── WebSocket ──│ LLM: Qwen3 candle    │
│ Kokoro   │                │ TTS: Kokoro/         │
│ (local   │                │ Step-Audio-TTS-3B    │
│  TTS)    │                └─────────────────────┘
└──────────┘
```

- **Piper or Kokoro** for ESP32 edge TTS (lightweight, fast, CPU-only)
- **Kokoro** for server-side TTS (best quality/size ratio, Apache 2.0)
- **Step-Audio-TTS-3B** for server-side high-quality Chinese voice (emotion control, dialects)

---

## Appendix: Related Projects

> The following are primary reference projects for chobits' positioning and feature trade-offs, organized by category.

### Backend Services (Alternative Implementations)

| Project | Language | Stars | Reference Value for chobits |
|---------|----------|-------|-----------------------------|
| [xinnan-tech/xiaozhi-esp32-server](https://github.com/xinnan-tech/xiaozhi-esp32-server) | Python | 10k+ | Full-feature reference: 12 ASR + 18 TTS + 13 plugins + PowerMem + RAGFlow + VLLM |
| [joey-zhou/xiaozhi-esp32-server-java](https://github.com/joey-zhou/xiaozhi-esp32-server-java) | Java | 1.3k | DDD architecture + WebRTC AEC3 + 3 memory modes + RBAC + A/B device collaboration |
| [AnimeAIChat/xiaozhi-server-go](https://github.com/AnimeAIChat/xiaozhi-server-go) | Go | — | Production-grade: VLLM image security + Quick Reply + role system + UPX compression |
| [hackers365/xiaozhi-esp32-server-golang](https://github.com/hackers365/xiaozhi-esp32-server-golang) | Go | — | MCP Market + OpenClaw + MCP Audio Server + setup wizard + dynamic TTS |
| [78/xiaozhi](https://github.com/78/xiaozhi) | — | 772 | Original official version (deprecated) |
| [mm7h/XiaoZhi.Net](https://github.com/mm7h/XiaoZhi.Net) | C# | 35 | .NET 8 implementation + sherpa-onnx + plugin system |
| [daxpot/xiaozhi-cpp-server](https://github.com/daxpot/xiaozhi-cpp-server) | C++ | 32 | C++20 coroutine architecture + EdgeTTS + Doubao |
| [Hyrsoft/xiaozhi_linux_rs](https://github.com/Hyrsoft/xiaozhi_linux_rs) | Rust | 47 | **First Rust client**: ALSA + Opus + MCP dynamic loading |

### Client Implementations

| Project | Language | Stars | Reference Value |
|---------|----------|-------|-----------------|
| [huangjunsen0406/py-xiaozhi](https://github.com/huangjunsen0406/py-xiaozhi) | Python | 3.4k | Cross-platform AI client: camera vision + GPIO + Live2D + MQTT |
| [TOM88812/xiaozhi-android-client](https://github.com/TOM88812/xiaozhi-android-client) | Flutter | — | Live2D + Mood Mode + HTML preview + chain-of-thought visualization |
| [shenjingnan/xiaozhi-client](https://github.com/shenjingnan/xiaozhi-client) | TS | 282 | MCP CLI bridge: aggregates multiple MCP endpoints → Cursor/Cherry Studio |
| [TOM88812/xiaozhi-web-client](https://github.com/TOM88812/xiaozhi-web-client) | HTML | 184 | Browser-based voice dialogue: WebRTC + AudioWorklet + Opus |
| [SylarLi/xiaozhi-unity](https://github.com/SylarLi/xiaozhi-unity) | C# | 57 | Unity3D + VRM avatar + uLipSync + Mijia control |
| [coloz/xiaozhi-library](https://github.com/coloz/xiaozhi-library) | Arduino | 10+ | Arduino library: 100+ boards + WS/MQTT dual protocol + LVGL + 20 languages |

### MCP Ecosystem Tools

| Project | Language | Reference Value |
|---------|----------|-----------------|
| [xinnan-tech/mcp-endpoint-server](https://github.com/xinnan-tech/mcp-endpoint-server) | Python | Lightweight MCP registry, WebSocket protocol, Docker deployable |
| [huangjunsen0406/xiaozhi-mcphub](https://github.com/huangjunsen0406/xiaozhi-mcphub) | TS | Enterprise MCP management: multi-endpoint + vector routing + RBAC + React dashboard |
| [avxxoo/xiaozhi-mcp](https://github.com/avxxoo/xiaozhi-mcp) | Python | MCP tool aggregation: DingTalk/QQ/system monitoring/WebPilot/math |
| [yuexianga/xiaozhi-mcp](https://github.com/yuexianga/xiaozhi-mcp) | Python | 18-tool MCP server: file/Telegram/system/Git/email/screenshot |
| [mcp2xiaozhi](https://pypi.org/project/mcp2xiaozhi/) | Python | Universal MCP bridge: stdio/SSE/HTTP → WS, PyPI package |
| [ZhongZiTongXue/xiaozhi-MCPTools](https://github.com/ZhongZiTongXue/xiaozhi-MCPTools) | VB6 | GUI MCP deployment: 25+ open API tools + music playback |
| [johz-chen/mcp-bridge](https://github.com/johz-chen/mcp-bridge) | Rust | Rust MCP bridge: WS/MQTT + process management + heartbeat |
| [dsw0000/xiaozhi-openclaw-plugin](https://github.com/dsw0000/xiaozhi-openclaw-plugin) | JS | OpenClaw bidirectional communication: messaging/device control/agent tasks |

### Home Assistant Integration

| Project | Language | Reference Value |
|---------|----------|-----------------|
| [RealDeco/xiaozhi-esphome](https://github.com/RealDeco/xiaozhi-esphome) | ESPHome | 767 stars, 15+ device support, HA voice satellite, no xiaozhi server needed |
| [AleksSem/xiaozhi-assistant](https://github.com/AleksSem/xiaozhi-assistant) | Python | Most complete HA integration: Conversation Agent + STT/TTS + MCP + OTA |
| [c1pher-cn/ha-mcp-for-xiaozhi](https://github.com/c1pher-cn/ha-mcp-for-xiaozhi) | Python | HA as direct MCP Server: WebSocket + multi-entity proxy |
| [mac8005/xiaozhi-mcp-ha](https://github.com/mac8005/xiaozhi-mcp-ha) | Python | HACS MCP proxy: SSE proxy + auto-reconnect |

### MQTT Gateway

| Project | Language | Reference Value |
|---------|----------|-----------------|
| [xinnan-tech/xiaozhi-mqtt-gateway](https://github.com/xinnan-tech/xiaozhi-mqtt-gateway) | Python | MQTT+UDP → WS bridge: dynamic load balancing + HMAC auth + MCP command dispatch |

### Embedded Hardware Adaptations

| Project | Platform | Reference Value |
|---------|----------|-----------------|
| [78/xiaozhi-sf32](https://github.com/78/xiaozhi-sf32) | SiFli SF32 | Bluetooth PAN networking + LCD + AEC + OTA |
| [100askTeam/xiaozhi-linux](https://github.com/100askTeam/xiaozhi-linux) | Embedded Linux | NXP/Allwinner/Canaan/Rockchip/STM32 multi-BSP support |
| [QuecPython/solution-xiaozhiAI](https://github.com/QuecPython/solution-xiaozhiAI) | Quectel 4G | 4G cellular module + voice wake + WebSocket |
| [D-Robotics/xiaozhi-in-rdk](https://github.com/D-Robotics/xiaozhi-in-rdk) | Horizon RDK | Horizon RDK board adaptation + edge AI acceleration |

### Deployment Tools

| Project | Description |
|---------|-------------|
| [haotianshouwang/xiaozhi-server-installer-docker.sh](https://github.com/haotianshouwang/xiaozhi-server-installer-docker.sh) | One-click Docker deploy script + interactive config + 15+ API support |
| [jsntwdj/xiaozhi-esp32-server](https://hub.docker.com/r/jsntwdj/xiaozhi-esp32-server) | ARM64 Docker image (Raspberry Pi, etc.) |
| [78/xiaozhi-assets-generator](https://github.com/78/xiaozhi-assets-generator) | Web asset generator: custom wake words/fonts/emojis/chat backgrounds |

### Management & Monitoring

| Project | Description |
|---------|-------------|
| [busy-worker/xiaozhi-esp32-server-java](https://github.com/busy-worker/xiaozhi-esp32-server-java) | Java management platform: Token usage + conversation duration + device activity + data visualization |
| [joey-zhou/xiaozhi-concurrent](https://github.com/joey-zhou/xiaozhi-concurrent) | WS concurrency load testing tool: metrics dashboard + automated performance reports |

### Commercial Product Reference

> Key technologies and UX patterns from closed-source commercial products, for chobits' trade-off reference.

#### Voice AI Platforms

| Product | Key Technology | Reference Value for chobits |
|---------|---------------|-----------------------------|
| OpenAI Realtime | End-to-end S2S, semantic VAD, WebRTC, ~232ms latency | Semantic VAD is gold standard for turn-taking/interruption; WebRTC transport architecture reference |
| Cartesia Sonic 3.5 | SSM architecture TTS, <90ms latency, 3-second voice cloning | SSM is new direction for TTS, faster than Transformers |
| ElevenLabs | Voice cloning (1-25 samples), Expressive Mode, 11.ai MCP voice assistant | Voice + MCP integration pattern; Agent coaching/evaluation |
| MiniMax Speech-2.8 | Interjection tags `(laughs)` `(sighs)`, 7 emotions + 0-100% intensity, per-sentence emotion control | Simple practical emotion expression, no SSML needed |
| Inworld AI | 15-second voice cloning, text-described voice generation, <130ms TTS | Lowest barrier voice personalization |
| Hume AI EVI | 600+ emotion tags, emotion-adaptive tone, detects hesitation/sarcasm/relief | Emotion detection + adaptive tone is key differentiator |
| Sesame CSM | Conversational prosody (breaths/hesitations/laughter), Apache 2.0 open source | Directly usable open-source model, makes speech more human |

#### Voice AI Infrastructure

| Product | Key Technology | Reference Value for chobits |
|---------|---------------|-----------------------------|
| LiveKit / Pipecat | Open-source SFU architecture, 50+ AI model integration, OpenAI ChatGPT backbone | Voice AI infrastructure standard, usable as transport layer |
| Retell AI | ~600ms end-to-end (no tuning), proprietary turn-taking model, BYO-LLM | Turn detection is core differentiator |
| Vapi | BYO-stack (choose STT/LLM/TTS), A/B testing, 1000+ templates | Composable architecture pattern |

#### Smart Speakers / Phone Assistants

| Product | Key Technology | Reference Value for chobits |
|---------|---------------|-----------------------------|
| Amazon Alexa+ | Autonomous task execution (Uber/OpenTable/Grubhub), model-agnostic routing, cross-device continuity | Agent chain execution pattern; chobits can implement via MCP |
| Google Gemini for Home | Natural language automation creation ("Ask Home"), AI camera understanding | "Describe automation" UX pattern |
| Xiaomi Super XiaoAI | Voiceprint family member identification, AI call proxy, smart bubble visual feedback | Multi-member voiceprint + family features |
| Baidu Xiaodu | Bluetooth Mesh local gateway, offline control, voice personality persistence | Local gateway architecture; voice personality is emotional bond |
| Tmall Genie | "1+3+N" architecture, space intelligent agent, edge scene scheduling | Space intelligent agent: from command-based to context-aware |
| iFlytek Spark | One-sentence voice cloning, 74+ dialects, end-to-end voice translation | One-sentence cloning is killer feature |
| Doubao | Super Mode (autonomous task decomposition), 100M+ DAU, Coze agent platform | Agent task decomposition + UGC character platform |

#### AI Wearables

| Product | Key Technology | Reference Value for chobits |
|---------|---------------|-----------------------------|
| Meta Ray-Ban | 76% market share, audio-first, visual perception, 7+ language real-time translation | Audio-first + visual perception is success model |
| Omi (open source) | $89, MIT license, 250+ community apps, local recording + cloud sync | Closest open-source wearable reference |
| Bee AI (Amazon) | $50, 7-day battery, no-audio-storage privacy policy | Low price + long battery + privacy-first |
| Looki L1 | 30g pendant, 12-hour battery, AI environment perception + life logging | Smallest form factor + perception resonance concept |
| Limitless Pendant | Always-listening, speaker diarization, MCP server integration, 100+ languages | Ambient context capture + MCP integration pattern |

#### AI Companion Apps

| Product | Key Technology | Reference Value for chobits |
|---------|---------------|-----------------------------|
| Character.AI | 10M+ user characters, Lorebook worldbuilding, PipSqueak 2 model | UGC character marketplace + persistent personality |
| Replika 2.0 | Cross-month memory reconstruction, proactive reminders, AR experiences | Proactive memory-driven suggestions is powerful UX |
| ChatGPT Voice | End-to-end audio model, 320ms latency, 22 neural TTS voices | Consumer voice AI benchmark |
| Microsoft Copilot | Work IQ persistent memory, Agent 365 governance, computer-using agents | Most mature enterprise memory system |

#### Key Industry Trends

| Trend | Representative Products | chobits Opportunity |
|-------|------------------------|---------------------|
| Semantic VAD | OpenAI (model-level turn detection) | Smarter interruption than pure VAD |
| Conversational Prosody | Sesame CSM (breaths/hesitations/laughter) | Makes TTS more natural |
| End-to-end S2S | OpenAI/Hume/Sesame | Ultimate goal, current cascaded more practical |
| Emotion-adaptive Tone | Hume AI (detect → adapt tone) | Key voice assistant differentiator |
| SSM Architecture TTS | Cartesia (State Space Model) | Faster TTS inference |
| Persistent Memory | Work IQ / Replika / Character.AI | From chat history to long-term memory |
| MCP Voice Integration | OpenAI + MCP, ElevenLabs 11.ai | Tool calling during voice conversations |
| Voice Cloning Democratization | Cartesia 3s / Inworld 15s / MiniMax 10s | Lowest barrier voice personalization |
| Privacy-first | Bee (no-audio-storage), Omi (local processing) | Always-listening device privacy strategy |

#### Vertical Industry Voice

> Voice AI solutions from automotive, healthcare, education, translation and other verticals, for chobits vertical scenario reference.

| Industry | Representative Products | Key Technology | Reference Value for chobits |
|----------|------------------------|----------------|-----------------------------|
| Automotive Voice | NIO NOMI, XPeng Tianji, SoundHound | Edge offline processing, 14 emotion tones, RAG vehicle manuals, Speech-to-Meaning | Edge offline is critical; emotion tone customization; RAG reference |
| Healthcare Voice | Nuance DAX, Nabla, Abridge, Suki AI | Ambient clinical documentation, privacy-first (no data storage), HIPAA compliance, guided reasoning | Passive listening mode; privacy-first architecture; guided dialogue |
| Education Voice | Duolingo Max, Zuoyebang P50, Youdao Ziyue | Step-by-step guided reasoning, personalized learning paths, gamification, real-time pronunciation correction | Guided dialogue mode; gamification drives engagement |
| Translation Devices | Pocketalk, Timekettle, Vasco | Dual-mic noise cancellation, <1s latency, offline translation packs, dedicated hardware | Dual-mic noise cancellation + low latency is key; offline capability |
| Smart Home Hub | Echo Hub, HomePod, SmartThings | Matter 1.3/Thread 1.4, offline voice (0.2-0.4s local), predictive automation | ESP32 as privacy-first local controller |

#### Chinese AI Startups (2025-2026)

> Voice AI technologies from China's "Six Little Tigers" and other emerging companies, for chobits technology selection reference.

| Company | Key Technology | Reference Value for chobits |
|---------|----------------|-----------------------------|
| [StepFun](https://github.com/stepfun-ai) | Step-Audio 2 (130B S2S), Step-Audio-TTS-3B, Apache 2.0 open source, emotion control + dialects | **Most relevant**: Best open-source voice AI, directly usable |
| [Zhipu AI](https://github.com/THUDM) | GLM-4V vision model, 85% revenue from local deployment, HKEX IPO | Local deployment business model reference; vision capabilities |
| [MiniMax](https://github.com/MiniMaxAI) | Hailuo AI, Xingye, 1.75T weekly token calls, Speech-2.8 | Consumer AI product operations reference; emotion TTS |
| [Moonshot AI / Kimi](https://github.com/MoonshotAI) | Kimi K2.5 + Kimi Claw, valuation jumped $4.3B→$18B in 3 months | Ultra-long context + voice AI combination |
| [Baichuan](https://github.com/baichuan-inc) | Healthcare vertical LLM, 3B RMB cash reserves | Vertical industry deep cultivation model |

---

## Priority Guide

| Level | Meaning | Action |
|-------|---------|--------|
| 🔴 P0 | Must fix immediately | Compile error, no auth, data inconsistency |
| 🟡 P1 | Should fix | Race conditions, memory leaks, security risks, core feature gaps |
| ⚠️ P2 | Missing feature | Incomplete protocol, insufficient configurability |
| 🟢 P3 | Optimization | Performance, code quality, non-core features |

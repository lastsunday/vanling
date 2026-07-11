+++
title = "TODO"
weight = 204

[extra]
translated_at = "2026-07-10T15:30:00+08:00"
source_file_hash = "f8d049863ce0f2969da161a0bd91cb2049cfdc9c"
+++

# TODO

> **⚠️ Note: This is an exploratory feature document. It references industry standards (Alexa+, Gemini, Siri AI) and reference project implementations (xiaozhi-esp32-server, xiaozhi-esp32-server-java, xiaozhi-server-go). Most items will NOT be implemented — this serves as a reference for chobits positioning and trade-offs. Actual development priorities follow the project Roadmap.**

A categorized backlog of TODO items. Before fixing, read [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) for development conventions.

## Security & Authentication

| Priority | Item | Location | Description | Open Source / Libraries |
|----------|------|----------|-------------|------------------------|
| 🔴 P0 | WS Authentication | `api/src/ws/mod.rs` | WS handler has zero auth — all connections are unauthenticated | Suggest: `axum-jwt-auth` — JWT middleware with JWKS caching |
| 🔴 P0 | Invite Code System | New feature | No invite code generation/verification/admin. No access control on new user registration | Suggest: `nanoid` — short, URL-safe ID generator |
| 🟡 P1 | Rate Limiting | `api/src/auth.rs` | No login rate limiting or brute-force protection | Suggest: `tower-governor` + `governor` — GCRA algorithm |
| 🟡 P1 | Refresh Revocation | `api/src/auth.rs` | No refresh token revocation mechanism — logout only clears client-side | Existing: `redis-rs` + `sea-orm` |
| 🟡 P1 | Token Logging | `api/src/auth.rs` | Access tokens logged in plaintext in tracing spans | Existing: `tracing` |
| 🟡 P1 | OTA Device Activation | `api/src/ota.rs` | `activate` endpoint is a stub returning "success" without verifying devices. Device info not persisted to DB | — |
| 🟡 P1 | MCP Authentication | `api/src/mcp/mod.rs` | `/mcp` endpoint auth is commented out | Existing: `rmcp` |

## Core Features

| Priority | Item | Location | Description | Open Source / Libraries |
|----------|------|----------|-------------|------------------------|
| 🔴 P0 | stop_round Race Condition | `service/src/chobits/session/round.rs` | `llm_tts_handle` and `stop_round` lack synchronization — possible use-after-cancel | — |
| 🔴 P0 | LLM Thread Safety | `api/src/llm/model/qwen3/mod.rs` | `thread::spawn` + `block_on` without `catch_unwind` — silent panic crash | — |
| 🔴 P0 | LLM Echo Thread | `api/src/llm/model/echo/mod.rs` | Same as above | — |
| 🔴 P0 | Wake Word Support | `api/src/ws/` + protocol layer | ESP32 client already has ESP-SR offline wake word detection. Server needs to handle `wake_word` message type — protocol currently lacks this field. Industry standard (Echo/Google/Xiaozhi all have it). Response latency <200ms required | — (ESP32-side ESP-SR, server-side protocol parsing only) |
| 🟡 P1 | Speaker Identification (Voiceprint) | New feature | Reference project xinnan-tech implements this (3D-Speaker model). Processed in parallel with ASR to identify speaker identity and pass to LLM for personalized replies. sherpa-onnx already supports speaker identification (ECAPA-TDNN/WeSpeaker) — no new dependencies needed. hacker365 Go version uses Qdrant vector DB + dynamic TTS voice switching for a more mature architecture. Needs: register/manage/identify workflow + DB storage for voiceprint vectors + LLM context injection | Existing: `sherpa-onnx` (ECAPA-TDNN/WeSpeaker) |
| 🟡 P1 | Opus Division by Zero | `api/src/ws/default_listener.rs` | Divide by zero when channels=0 / sample_rate=0 | — |
| 🟡 P1 | Clock Overflow | `service/src/chobits/session/mod.rs` | `Local::now()` is non-monotonic — subtraction can overflow | Existing: `jiff` (monotonic clock) |
| 🟡 P1 | Device Management | New feature | No device registration/binding/listing. After OTA activation, devices are not persisted. Reference projects have full device lifecycle (registration/state/config/OTA/bulk operations) | Existing: `sea-orm` |
| 🟡 P1 | Music Playback | MCP tool or standalone | Reference project xinnan-tech has `play_music.py` + `hass_play_music.py`; joey-zhou has `MusicPlayer` with LRC lyrics sync. Industry standard | Suggest: `rodio` — cross-platform audio playback |
| 🟡 P1 | Timer/Reminders/Alarms | MCP tool or standalone | Industry standard ("Alexa, set a timer"). Currently no timing/reminder mechanism at all | Suggest: `tokio-cron-scheduler` — async Tokio cron |
| 🟡 P1 | Continued Conversation | `service/src/chobits/session/` | After a reply, the microphone should stay open briefly to allow follow-up without wake word. Supported by Gemini / Alexa+ | — |
| 🟡 P1 | Emotion Recognition Completion | `api/src/llm/model/` (analyze_emotion) | Currently stub returning "happy". Industry approach: dual-channel fusion of audio features (wav2vec2 SER) + text sentiment (GoEmotions), used to adjust TTS tone and response style | Existing: `sherpa-onnx` (SER models) |
| 🟡 P1 | Personalized Memory / Long-term Preferences | New feature | Currently only chat history — no long-term preference storage. Reference project xinnan-tech has PowerMem (user profiles + Ebbinghaus forgetting curve + vector retrieval); joey-zhou has 3 memory modes (window/summary/long + graph retrieval) | Suggest: `qdrant` + `rig-core` — vector DB + RAG framework |
| 🟡 P1 | RAG Knowledge Base | MCP or built-in module | Reference project xinnan-tech integrates RAGFlow; joey-zhou has EmbeddingModelFactory + graph retrieval. Current MCP framework can integrate but has no built-in vector retrieval | Existing: `rig-core` (10+ vector storage backends) |
| 🟡 P1 | Multi-language TTS | `api/src/tts/` | Currently single-language TTS voice. ESP32 client already supports 25+ ASR languages — TTS side needs to match | Suggest: `sherpa-onnx` (Piper/VITS ONNX models) |
| 🟡 P1 | Smart Home Integration (Home Assistant) | MCP or standalone | Reference project xinnan-tech has 3 HA integration methods (community plugin / HA as LLM tool / HA MCP Server). Smart home is a core voice assistant scenario | Existing: `rmcp` (HA MCP Server) |
| 🟡 P1 | Cross-device Calling | MCP tool or standalone | Reference project xinnan-tech has `call_device.py` — ESP32 devices can call each other like phone calls. Needs MQTT gateway + contact management | Suggest: `rumqttc` — pure Rust MQTT client |
| 🟡 P1 | Intent Recognition | New feature | Reference project xinnan-tech supports 3 modes: function_call (recommended), intent_llm (dedicated LLM), nointent. chobits currently has no independent intent layer | Existing: `rmcp` (function_call) |
| 🟡 P1 | Audio Hot-path Cloning | `api/src/ws/default_listener.rs` | `data.to_vec()` called every 20ms — frequent cloning | — |
| 🟡 P1 | Quick Reply Pre-response | New feature | Play "I'm here" / "Coming" short phrases during LLM inference to reduce perceived latency. Reference project hacker365 Go version implements this. Critical UX, simple to implement | — |
| 🟡 P1 | Dynamic TTS Voice Switching | New feature | Auto-switch TTS voice based on speaker identification. Reference project hacker365 Go version implements this (sherpa-onnx voiceprint + per-speaker TTS voice). Natural extension of voiceprint recognition | Existing: `sherpa-onnx` (voiceprint + TTS switching) |
| 🟡 P1 | Semantic VAD | New feature | Reference OpenAI Realtime implements model-level turn-taking detection (not pure silence detection) — can distinguish a cough from starting a new sentence. Industry gold standard. More intelligent interruption/turn-taking than traditional VAD | Suggest: `wavekat-vad` — unified trait wrapping WebRTC+Silero |
| 🟡 P1 | LLM History Blocking | `api/src/llm/model/qwen3/mod.rs` | DB write blocks full thread | — |
| 🟡 P1 | Recorder No Limit | `api/src/record/recorder.rs` | `Vec<RecordEntry>` has no size limit — unbounded memory growth under high concurrency | — |
| 🟡 P1 | Piper/Kokoro TTS Integration | `api/src/tts/` | Open-source TTS alternatives: Piper (20M params/MIT/CPU 55ms latency/30+ languages) and Kokoro (82M/Apache 2.0/CPU realtime/54 voices). Can replace or supplement current MatchaTTS. Piper ideal for edge deployment, Kokoro is best quality/size ratio | Suggest: `sherpa-onnx` (Piper ONNX models) |
| ⚠️ P2 | Message Types | `api/src/ws/frame.rs` | Missing `system`, `alert`, `custom`, `wake_word` message types (vs xiaozhi-esp32 spec) | — |
| ⚠️ P2 | Multi ASR/TTS Provider | `api/src/asr/` + `api/src/tts/` | Reference project xinnan-tech supports 12 ASR + 18+ TTS providers (including free EdgeTTS); joey-zhou has 7 STT + 8 TTS. chobits has only 1 ASR + 1 TTS | Existing: `sherpa-onnx` (multi-model switching) |
| ⚠️ P2 | Plugin System | New feature | Reference project xinnan-tech has 13 built-in plugins + hot-reload. chobits has no plugin architecture — feature extensions require modifying core code | Suggest: `wasmtime` — WASI P2 + Component Model |
| ⚠️ P2 | MCP Market / Tool Marketplace | New feature | Reference project hacker365 Go version implements MCP tool "app store": aggregates multiple third-party markets (e.g. ModelScope), one-click import of remote MCP services + hot-reload. chobits currently has no MCP tool discovery/aggregation mechanism | Existing: `rmcp` |
| ⚠️ P2 | MCP Debug Console | New feature | Reference project hacker365 Go version has Agent/Device-level MCP remote debugging: web console generates per-agent independent MCP endpoints, real-time call testing, per-agent tool filtering. chobits currently has no MCP debugging tools | Existing: `rmcp` |
| ⚠️ P2 | Configuration Wizard + E2E Testing | New feature | Reference project hacker365 Go version has first-run wizard (OTA/VAD/ASR/LLM/TTS step-by-step config) + per-component latency tests + visualization charts. chobits currently has no deployment wizard | — |
| ⚠️ P2 | MCP Tool Aggregation | New feature | Reference projects xiaozhi-mcp / yuexianga/xiaozhi-mcp provide pre-built tool libraries (DingTalk/QQ/system monitor/WebPilot/math), ready to use out of the box. chobits has no built-in MCP tool packs | Existing: `rmcp` |
| ⚠️ P2 | MQTT Gateway | New feature | Reference projects xinnan-tech/xiaozhi-mqtt-gateway implement MQTT+UDP → WS bridging: distributed deployment + dynamic load balancing + HMAC auth + MCP command dispatch. chobits currently only supports WS protocol | Suggest: `rumqttc` — pure Rust, tokio native |
| ⚪ P2 | Conversational Prosody TTS | New feature | Reference project Sesame CSM (Apache 2.0 open-source) generates breathing/hesitation/laughter prosody, making speech more human-like. Cartesia Sonic 3.5 uses SSM architecture for <90ms TTS latency | Suggest: `csm.rs` — Rust Sesame CSM (AGPL-3.0) |
| ⚪ P2 | Emotion-adaptive Intonation | New feature | Reference project Hume AI EVI detects 600+ emotion tags (hesitation/sarcasm/relief), adaptively adjusting TTS tone. MiniMax Speech-2.8 supports 7 emotions + 0-100% intensity + insertion tags `(laughs)` `(sighs)` | Suggest: `voirs-emotion` — multi-dimensional emotion control |
| ⚪ P2 | Agent Task Orchestration | New feature | Reference Alexa+ self-executing Uber/OpenTable/Grubhub; Rabbit R1 LAM large action model; Doubao autonomous task decomposition. LLM + MCP tool chain enables autonomous task execution | Existing: `rig-core` (Agent/Chain/Router) |
| ⚠️ P2 | VAD Sample Rate | `api/src/vad/` | Hardcoded 16kHz — silent failure on non-16kHz input | — |
| ⚠️ P2 | ASR | `api/src/asr/` | SenseVoice (sherpa-onnx), lacks `Sync` trait, 16kHz mono only | Existing: `sherpa-onnx` |
| ⚠️ P2 | Ambient Listening Mode | New feature | Medical AI (Nuance DAX/Nabla) passive listening mode: non-wake-word triggered, continuously listens to ambient audio, proactively responds to user needs. Needs privacy architecture (Nabla model: no raw audio storage). Suitable for home/office scenarios | — |
| 🟢 P3 | Vision Perception (VLLM) | New feature | Reference project xinnan-tech supports GLM-4V/Qwen-VL vision models for photo recognition. chobits currently voice-only | Suggest: `reqwest` → Ollama/vLLM (OpenAI API) |
| 🟢 P3 | Proactive Suggestions | New feature | Gemini Daily Brief / Alexa+ proactive alerts for traffic/prices/calendar. Needs scheduled tasks + user context reasoning | Suggest: `tokio-cron-scheduler` |
| 🟢 P3 | Multimodal (Voice + Screen + Video) | New feature | Gemini 2.5 / GPT-Realtime / Siri AI all support camera/screen input. chobits currently voice-only | Suggest: `webrtc-rs` (v0.17.x) |
| 🟢 P3 | Cross-device Continuity | New feature | Alexa+: seamless context switching Echo→phone→computer. Needs session state sync mechanism | — (custom: SQLite + WS delta sync) |
| 🟢 P3 | Speaker Diarization | `api/src/listener/` | Multi-speaker separation in multi-user scenarios. sherpa-onnx already supports (ECAPA-TDNN + AHC clustering) | Existing: `sherpa-onnx` / Suggest: `polyvoice` |
| 🟢 P3 | Voice Cloning | `api/src/tts/` | Reference project xinnan-tech supports Volcengine voice cloning; joey-zhou supports per-character voice cloning. MatchaTTS already supports reference audio — needs exposed config interface | Suggest: `sherpa-onnx` (speaker embedding) |
| 🟢 P3 | AEC Server-side Noise Reduction | `api/src/ws/default_listener.rs` → pipeline | joey-zhou already implements WebRTC AEC3 server-side echo cancellation (with noise suppression + high-pass filter + adaptive gain). chobits has only client-side AEC | Suggest: `aec3` — pure Rust WebRTC AEC3 |
| 🟢 P3 | WebRTC Real-time Audio/Video | New feature | Reference project dairoot/xiaozhi-webrtc implements WebRTC low-latency + Live2D + multimodal vision + MCP. chobits currently WS-only | Suggest: `webrtc-rs` (v0.17.x) |
| 🟢 P3 | Audio Normalization Integration | `api/src/util/compressor.rs` → pipeline | `adaptive_normalize()` implemented but not integrated into TTS output pipeline | — |
| 🟢 P3 | Live2D Avatar | Client feature | Reference project Android client (TOM88812) implements: multi-model switching + real-time animation + custom characters + mood mode. chobits Flutter client can reference this | Suggest: `live2d-cubism-core-sys` + `flutter_rust_bridge` |
| 🟢 P3 | Embodied AI / GPIO | New feature | Reference project py-xiaozhi implements: Raspberry Pi/Jetson/STM32 direct hardware control (motors/sensors/LEDs) + camera vision understanding. Vertical scenario, not core general-purpose voice assistant | Suggest: `rppal` (Raspberry Pi) / `gpio-cdev` |
| 🟢 P3 | UGC Character Marketplace | New feature | Reference project Character.AI has 10M+ user-created characters; Doubao agent platform supports no-code AI character creation. chobits could support user-defined AI personalities + voices + backstories | — |
| 🟢 P3 | Session Export/Delete | `api/src/record/` | Sessions can only be viewed — no export or deletion | — |
| 🟢 P3 | describe O(n) | `api/src/llm/model/qwen3/mod.rs` | Real-time full message history construction | — |
| 🟢 P3 | TTS Loop Cloning | `api/src/tts/` | `Arc<str>` vs `String` cloning storm | — |
| 🟢 P3 | Double Serialization | `api/src/record/recorder.rs` | Double JSON serialization on record path | — |
| 🟢 P3 | Matter/Thread Protocol | New feature | Smart home Hub standard protocol (Matter 1.3+Thread 1.4), enabling cross-platform device compatibility (Apple/Google/Amazon/Samsung). ESP32 already has Thread support, needs Matter SDK integration | Suggest: `rs-matter` — only production-grade Rust Matter implementation |
| 🟢 P3 | Guided Reasoning Conversation | New feature | Education AI (Zuoyebang/Youdao) guided mode: doesn't give answers directly, gradually guides user to think. Suitable for children/learning scenarios, needs LLM prompt engineering + conversation state management | — |

## Infrastructure

| Priority | Item | Location | Description | Open Source / Libraries |
|----------|------|----------|-------------|------------------------|
| 🔴 P0 | Signal Macro | `framework/src/signal.rs` | Uses non-existent `debug_error!` macro — fails to compile on non-unix | — |
| 🟡 P1 | email Constraint | `migration/src/m20241230_000001_init.rs` | Entity has `#[sea_orm(unique)]` but migration doesn't implement UNIQUE | Existing: `sea-orm` (migration fix) |
| 🟡 P1 | FK Constraints | `migration/src/m20241230_000001_init.rs` | Missing FKs: `round.session_id`, `round_data.round_id`, `frame.round_id` | Existing: `sea-orm` (migration fix) |
| 🟡 P1 | MCP Lock Ordering | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server lock order ABBA — potential deadlock | — |
| 🟡 P1 | Graceful Shutdown Order | `framework/src/signal.rs` + `apps/server` | Missing shutdown sequence across modules | — |
| 🟡 P1 | Panic Handling | `framework/src/panic.rs` | Uses `eprintln!` instead of `tracing::error!` — bypasses Sentry | Existing: `tracing` |
| 🟡 P1 | Runtime Race Condition | `framework/src/runtime.rs` | `OnceLock` initialization has race condition | — |
| 🟢 P3 | Timestamp Auto-fill | `entity/src/config.rs` | `Config` entity missing `ActiveModelBehavior` — timestamps not auto-filled | Existing: `sea-orm` |
| 🟢 P3 | MCP Error Handling | `api/src/mcp/` | Incomplete | Existing: `rmcp` |

## Frontend (apps/server-ui)

| Priority | Item | Location | Description | Open Source / Libraries |
|----------|------|----------|-------------|------------------------|
| 🟡 P1 | Dashboard Page | `routes/_pathlessLayout.admin/index.tsx` | Shell only — renders "Hello" with no stats/monitoring content | Existing: `@mantine/core` v9 + `@tanstack/react-query` |
| 🟡 P1 | User CRUD Admin UI | New page | No user list/create/delete/role management interface | Existing: `@mantine/core` v9 |
| 🟡 P1 | Multi-user/RBAC Management | New feature | Reference project busy-worker Java admin platform has token usage monitoring + conversation duration + device activity + data visualization + RBAC. chobits Dashboard should include these | Existing: `@mantine/core` v9 |
| 🟢 P3 | System Monitoring | New page | No server health/connection count/resource usage/error rate dashboard | Existing: `@mantine/core` v9 + `@tanstack/react-query` |
| 🟢 P3 | MCP Dashboard | New feature | Reference project xiaozhi-mcphub has React frontend: multi-endpoint management + tool sync + group access control + logs. chobits MCP management reference | Existing: `@mantine/core` v9 |

## Mobile (apps/app)

| Priority | Item | Location | Description | Open Source / Libraries |
|----------|------|----------|-------------|------------------------|
| 🟡 P1 | Flutter WS Integration | `apps/app/` | Flutter app exists as scaffold but has no WS + auth integration | Suggest: `web_socket_channel` |
| 🟡 P1 | App CI/CD | `.github/workflows/` | No iOS/Android build/sign/release pipeline | GitHub Actions |

## Testing

| Priority | Item | Location | Description | Open Source / Libraries |
|----------|------|----------|-------------|------------------------|
| 🔴 P0 | WS Auth Tests | `apps/server/api/tests/` | WS endpoint has zero auth — no corresponding tests | Existing: `axum` test utils + `reqwest` |
| 🔴 P0 | Wake Word Tests | `apps/server/api/tests/` | Wake word message handling has no tests — need protocol parsing + session wake flow coverage | Existing: `axum` test utils |
| 🟡 P1 | Auto Mode + AEC Tests | `apps/server/api/tests/session/` | Auto mode (barge_in=false) with/without AEC scenarios lack dedicated tests | Existing: `axum` test utils |
| 🟡 P1 | Button Talk Tests | `apps/server/api/tests/session/` | Manual mode (push-to-talk) lacks end-to-end tests | Existing: `axum` test utils |
| 🟡 P1 | Continued Conversation Tests | `apps/server/api/tests/session/` | Follow-up flow where mic stays open after reply has no tests | Existing: `axum` test utils |
| 🟡 P1 | Rate Limiting Tests | `apps/server/api/tests/` | Login rate limiting feature doesn't exist — no tests | Existing: `axum` test utils |
| 🟡 P1 | Emotion Recognition Tests | `apps/server/api/tests/` | analyze_emotion stub has no test coverage | — |
| 🟡 P1 | Speaker ID Tests | `apps/server/api/tests/` | Voiceprint register/identify/LLM injection flow has no tests | — |
| 🟡 P1 | Music Playback Tests | `apps/server/api/tests/` | Music playback feature has no tests | Existing: `axum` test utils |

## Pending / Exploration

| Item | Description | Open Source / Libraries |
|------|-------------|------------------------|
| Speaker ID approach | 3D-Speaker (used by xinnan-tech) vs sherpa-onnx speaker ID (already in tech stack) vs pyannote. sherpa-onnx is best: no new deps, supports ECAPA-TDNN/WeSpeaker/CAM++ models | Existing: `sherpa-onnx` |
| Plugin architecture | xinnan-tech has 13 built-in plugins + hot-reload. Does chobits need a similar mechanism, or is MCP sufficient for extensions? | Suggest: `wasmtime` — WASI sandbox + Component Model |
| Multi-provider strategy | Reference projects support 12+ ASR / 18+ TTS providers. Does chobits need multi-provider architecture, or stay lean? | Suggest: trait abstraction (`AsrProvider`/`TtsProvider`) |
| Music playback | Industry standard (Spotify/Apple Music integration). Need to determine approach: MCP tool calling external API / built-in audio playback / TTS extension | Suggest: `rodio` |
| ESP32 CI/CD | Separate project (xiaozhi-esp32), outside this repo's scope. chobits provides WS + OTA interfaces as backend only | — |
| Token persistent storage | Consider DB table for refresh tokens to support multi-device management | Existing: `sea-orm` + `redis-rs` |
| Smart Home integration | Alexa+ / Google Home core capability. Reference projects have 3 HA integration methods. Need to evaluate chobits positioning as hub | Existing: `rmcp` (HA MCP) |
| Agentic capabilities | Alexa+ "Experts" / Gemini Spark 24/7 agents can autonomously browse web/forms/orders. LLM + MCP tool chain has foundation — evaluate implementation depth | Existing: `rig-core` |
| Voice emotion TTS | Gemini 2.5 / Cartesia Sonic-3 support adjusting TTS tone based on emotion. Need to evaluate if MatchaTTS supports style/prosody control | Suggest: `voirs-emotion` |
| Local privacy processing | Industry trend: Echo/Fire devices moving toward local processing. chobits already has local Qwen3 — can expand to local ASR/TTS full chain | Existing: `sherpa-onnx` + `candle` |
| Vision capability | Reference project xinnan-tech supports VLLM (GLM-4V/Qwen-VL) photo recognition. Does chobits need vision capability? | Suggest: `reqwest` → Ollama/vLLM |
| MCP Market architecture | Reference project hacker365 Go version implements MCP tool "app store" (multi-market aggregation + hot-reload). Does chobits need MCP tool discovery/aggregation? | Existing: `rmcp` |
| Dynamic TTS voice switching | Reference project hacker365 Go version auto-switches TTS voice based on speaker ID. How should chobits link voiceprint recognition to TTS? | Existing: `sherpa-onnx` |
| Configuration wizard | Reference project hacker365 Go version has first-run wizard + full-chain latency tests. Should chobits add a deployment wizard to lower adoption barrier? | — |
| MQTT gateway architecture | Distributed deployment needs MQTT+UDP bridging + dynamic load balancing (reference xinnan-tech/xiaozhi-mqtt-gateway). Does chobits need an MQTT gateway layer? | Suggest: `rumqttc` |
| MCP tool aggregation strategy | Reference projects have pre-built tool libraries (DingTalk/QQ/system monitor/WebPilot/math). Should chobits include built-in MCP tool packs? | Existing: `rmcp` |
| End-to-end S2S architecture | OpenAI/Hume/Sesame use single model for audio in/out (not cascaded STT→LLM→TTS). Should chobits evolve beyond cascaded approach? | Suggest: `csm.rs` (AGPL-3.0) or `moshi` (Apache 2.0) |
| Semantic VAD approach | OpenAI's model-level turn-taking vs traditional VAD — how should chobits implement smarter interruption/turn-taking? | Suggest: `wavekat-vad` |
| Conversational prosody TTS | Sesame CSM open-source model (Apache 2.0) can add breathing/hesitation/laughter. Should chobits TTS integrate prosody? | Suggest: `csm.rs` (AGPL-3.0) |
| Privacy strategy | Always-listening devices need privacy architecture (Bee: no audio storage / Omi: local processing). How should chobits balance features with privacy? | Existing: `sherpa-onnx` (full-chain local) |
| SSM architecture TTS | Cartesia uses State Space Model instead of Transformer for <90ms TTS. Should chobits TTS consider SSM architecture? | — (no Rust implementation) |
| Piper/Kokoro evaluation | Open-source TTS models Piper (20M params/MIT) and Kokoro (82M/Apache 2.0) — suitable to replace or supplement current MatchaTTS? Piper for edge deployment, Kokoro for best quality/size ratio | Suggest: `sherpa-onnx` (Piper ONNX) |
| Step-Audio-TTS-3B integration | StepFun open-source Chinese TTS (Apache 2.0, emotion control, dialect support) — candidate for server TTS? Evaluate GPU requirements and latency | Suggest: `reqwest` → StepFun API |
| Ambient listening architecture | Medical AI passive listening mode (non-wake-word). Should chobits support this? How to ensure privacy? Reference Nabla (no raw audio storage) architecture | — |
| Matter protocol support | Smart home Hub standard protocol (Matter 1.3+Thread 1.4). ESP32 already has Thread support. Should chobits integrate full Matter SDK for cross-platform device compatibility? | Suggest: `rs-matter` |

---

## Appendix B: Open Source Voice Models

> Open-source TTS/voice models, sorted by quality/size/license. chobits can integrate directly.

| Model | Params | License | Latency | Voice Cloning | Best for chobits |
|-------|--------|---------|---------|---------------|-----------------|
| [Piper](https://github.com/rhasspy/piper) | ~20M | MIT | 55ms (10s audio) | No | ★★★★★ CPU run, 30+ languages, edge deployment first choice |
| [Kokoro v1.0](https://github.com/hexgrad/kokoro) | 82M | Apache 2.0 | CPU realtime | No (KokoClone extension supports) | ★★★★★ Best quality/size ratio, 54 voices |
| [Step-Audio-TTS-3B](https://github.com/stepfun-ai/Step-Audio-TTS-3B) | 3B | Apache 2.0 | Needs GPU | Yes | ★★★★ Best Chinese voice quality + emotion control + dialects |
| [Coqui XTTS v2](https://github.com/coqui-ai/TTS) | 467M | CPML (non-commercial) | <200ms | Yes (6s sample) | ★★★★ Best cloning, 17 languages, but license restricted |
| [F5-TTS](https://github.com/SWivid/F5-TTS) | 335M | CC-BY-NC | Needs GPU | Yes (zero-shot) | ★★★ Excellent cloning, Chinese-English mixed, but non-commercial |
| [Orpheus TTS](https://github.com/canopylabs-ai/orpheus-tts) | 3B | — | Needs GPU | No | ★★★ Llama backbone, style control |
| [Bark (Suno)](https://github.com/suno-ai/bark) | — | MIT | 0.8x (too slow) | No | ★★ Expressive (laughter/sighs/music), but poor real-time |

### Recommended Architecture

```
ESP32 (Edge)                   Server (chobits)
┌──────────┐               ┌─────────────────────┐
│ VAD      │               │ ASR: sherpa-onnx     │
│ Piper/   │◄── WebSocket ─│ LLM: Qwen3 candle    │
│ Kokoro   │               │ TTS: Kokoro/         │
│ (Local   │               │ Step-Audio-TTS-3B    │
│  TTS)    │               └─────────────────────┘
└──────────┘
```

- **Piper or Kokoro** for ESP32 edge TTS (lightweight, fast, CPU)
- **Kokoro** for server TTS (best quality/size ratio, Apache 2.0)
- **Step-Audio-TTS-3B** for server high-quality Chinese (emotion control, dialect support)

---

## Appendix: Reference Projects

> Key reference projects for chobits positioning and trade-off decisions, organized by category.

### Backend Services (Alternative Implementations)

| Project | Language | Stars | Value for chobits |
|---------|----------|-------|-------------------|
| [xinnan-tech/xiaozhi-esp32-server](https://github.com/xinnan-tech/xiaozhi-esp32-server) | Python | 10k+ | Full-featured reference: 12 ASR + 18 TTS + 13 plugins + PowerMem + RAGFlow + VLLM |
| [joey-zhou/xiaozhi-esp32-server-java](https://github.com/joey-zhou/xiaozhi-esp32-server-java) | Java | 1.3k | DDD architecture + WebRTC AEC3 + 3 memory modes + RBAC + A/B device coordination |
| [AnimeAIChat/xiaozhi-server-go](https://github.com/AnimeAIChat/xiaozhi-server-go) | Go | — | Production-grade: VLLM image safety + Quick Reply + character system + UPX compression |
| [hackers365/xiaozhi-esp32-server-golang](https://github.com/hackers365/xiaozhi-esp32-server-golang) | Go | — | MCP Market + OpenClaw + MCP Audio Server + config wizard + dynamic TTS |
| [78/xiaozhi](https://github.com/78/xiaozhi) | — | 772 | Original official version (deprecated) |
| [mm7h/XiaoZhi.Net](https://github.com/mm7h/XiaoZhi.Net) | C# | 35 | .NET 8 implementation + sherpa-onnx + plugin system |
| [daxpot/xiaozhi-cpp-server](https://github.com/daxpot/xiaozhi-cpp-server) | C++ | 32 | C++20 coroutine architecture + EdgeTTS + Doubao |
| [Hyrsoft/xiaozhi_linux_rs](https://github.com/Hyrsoft/xiaozhi_linux_rs) | Rust | 47 | **First Rust client**: ALSA + Opus + MCP dynamic loading |

### Client Implementations

| Project | Language | Stars | Value |
|---------|----------|-------|-------|
| [huangjunsen0406/py-xiaozhi](https://github.com/huangjunsen0406/py-xiaozhi) | Python | 3.4k | Cross-platform AI client: camera vision + GPIO + Live2D + MQTT |
| [TOM88812/xiaozhi-android-client](https://github.com/TOM88812/xiaozhi-android-client) | Flutter | — | Live2D + Mood Mode + HTML preview + thinking chain visualization |
| [shenjingnan/xiaozhi-client](https://github.com/shenjingnan/xiaozhi-client) | TS | 282 | MCP CLI bridge: aggregates multi MCP endpoints → Cursor/Cherry Studio |
| [TOM88812/xiaozhi-web-client](https://github.com/TOM88812/xiaozhi-web-client) | HTML | 184 | Browser voice chat: WebRTC + AudioWorklet + Opus |
| [SylarLi/xiaozhi-unity](https://github.com/SylarLi/xiaozhi-unity) | C# | 57 | Unity3D + VRM avatar + uLipSync + Xiaomi home control |
| [coloz/xiaozhi-library](https://github.com/coloz/xiaozhi-library) | Arduino | 10+ | Arduino library: 100+ boards + WS/MQTT dual protocol + LVGL + 20 languages |

### MCP Ecosystem Tools

| Project | Language | Value |
|---------|----------|-------|
| [xinnan-tech/mcp-endpoint-server](https://github.com/xinnan-tech/mcp-endpoint-server) | Python | Lightweight MCP registry, WebSocket protocol, Docker deployment |
| [huangjunsen0406/xiaozhi-mcphub](https://github.com/huangjunsen0406/xiaozhi-mcphub) | TS | Enterprise MCP management: multi-endpoint + vector routing + RBAC + React dashboard |
| [avxxoo/xiaozhi-mcp](https://github.com/avxxoo/xiaozhi-mcp) | Python | MCP tool aggregation: DingTalk/QQ/system monitor/WebPilot/math |
| [yuexianga/xiaozhi-mcp](https://github.com/yuexianga/xiaozhi-mcp) | Python | 18-tool MCP server: file/Telegram/system/Git/email/screenshots |
| [mcp2xiaozhi](https://pypi.org/project/mcp2xiaozhi/) | Python | Universal MCP bridge: stdio/SSE/HTTP → WS, PyPI package |
| [ZhongZiTongXue/xiaozhi-MCPTools](https://github.com/ZhongZiTongXue/xiaozhi-MCPTools) | VB6 | GUI MCP deployment: 25+ open API tools + music playback |
| [johz-chen/mcp-bridge](https://github.com/johz-chen/mcp-bridge) | Rust | Rust MCP bridge: WS/MQTT + process management + heartbeat |
| [dsw0000/xiaozhi-openclaw-plugin](https://github.com/dsw0000/xiaozhi-openclaw-plugin) | JS | OpenClaw bidirectional: messages/device control/agent tasks |

### Home Assistant Integration

| Project | Language | Value |
|---------|----------|-------|
| [RealDeco/xiaozhi-esphome](https://github.com/RealDeco/xiaozhi-esphome) | ESPHome | 767 stars, 15+ device support, HA voice satellite, no xiaozhi server needed |
| [AleksSem/xiaozhi-assistant](https://github.com/AleksSem/xiaozhi-assistant) | Python | Most complete HA integration: Conversation Agent + STT/TTS + MCP + OTA |
| [c1pher-cn/ha-mcp-for-xiaozhi](https://github.com/c1pher-cn/ha-mcp-for-xiaozhi) | Python | HA as MCP Server directly: WebSocket + multi-entity proxy |
| [mac8005/xiaozhi-mcp-ha](https://github.com/mac8005/xiaozhi-mcp-ha) | Python | HACS MCP proxy: SSE proxy + auto-reconnect |

### MQTT Gateway

| Project | Language | Value |
|---------|----------|-------|
| [xinnan-tech/xiaozhi-mqtt-gateway](https://github.com/xinnan-tech/xiaozhi-mqtt-gateway) | Python | MQTT+UDP → WS bridge: dynamic load balancing + HMAC auth + MCP command dispatch |

### Embedded Hardware Adaptation

| Project | Platform | Value |
|---------|----------|-------|
| [78/xiaozhi-sf32](https://github.com/78/xiaozhi-sf32) | SiFli SF32 | Bluetooth PAN + LCD + AEC + OTA |
| [100askTeam/xiaozhi-linux](https://github.com/100askTeam/xiaozhi-linux) | Embedded Linux | NXP/Allwinner/Canaan/Rockchip/STM32 multi-BSP |
| [QuecPython/solution-xiaozhiAI](https://github.com/QuecPython/solution-xiaozhiAI) | Quectel 4G | 4G cellular module + voice wake + WebSocket |
| [D-Robotics/xiaozhi-in-rdk](https://github.com/D-Robotics/xiaozhi-in-rdk) | Horizon RDK | Horizon RDK dev board + edge AI acceleration |

### Deployment Tools

| Project | Description |
|---------|-------------|
| [haotianshouwang/xiaozhi-server-installer-docker.sh](https://github.com/haotianshouwang/xiaozhi-server-installer-docker.sh) | One-click Docker deploy script + interactive config + 15+ API support |
| [jsntwdj/xiaozhi-esp32-server](https://hub.docker.com/r/jsntwdj/xiaozhi-esp32-server) | ARM64 Docker image (Raspberry Pi etc.) |
| [78/xiaozhi-assets-generator](https://github.com/78/xiaozhi-assets-generator) | Web asset generator: custom wake words/fonts/emojis/chat backgrounds |

### Management & Monitoring

| Project | Description |
|---------|-------------|
| [busy-worker/xiaozhi-esp32-server-java](https://github.com/busy-worker/xiaozhi-esp32-server-java) | Java admin platform: token usage + conversation duration + device activity + data visualization |
| [joey-zhou/xiaozhi-concurrent](https://github.com/joey-zhou/xiaozhi-concurrent) | WS concurrent load test tool: metrics dashboard + auto performance reports |

### Commercial Products Reference

> Key technologies and UX patterns from closed-source commercial products, for chobits trade-off reference.

#### Voice AI Platforms

| Product | Key Technologies | Value for chobits |
|---------|-----------------|-------------------|
| OpenAI Realtime | E2E S2S, semantic VAD, WebRTC, ~232ms latency | Semantic VAD is gold standard for turn-taking/interruption; WebRTC transport architecture reference |
| Cartesia Sonic 3.5 | SSM architecture TTS, <90ms latency, 3-second voice cloning | SSM is new TTS direction, faster than Transformer |
| ElevenLabs | Voice cloning (1-25 samples), Expressive Mode, 11.ai MCP voice assistant | Voice + MCP integration pattern; Agent coaching/evaluation |
| MiniMax Speech-2.8 | Insertion tags `(laughs)` `(sighs)`, 7 emotions + 0-100% intensity, per-sentence emotion control | Simple practical emotion expression, no SSML needed |
| Inworld AI | 15-second voice cloning, text-describe-generate voice, <130ms TTS | Lowest barrier voice personalization |
| Hume AI EVI | 600+ emotion tags, emotion-adaptive intonation, detects hesitation/sarcasm/relief | Emotion detection + adaptive intonation is key differentiator |
| Sesame CSM | Conversational prosody (breathing/hesitation/laughter), open-source Apache 2.0 | Usable open-source model, makes speech more human-like |

#### Voice AI Infrastructure

| Product | Key Technologies | Value for chobits |
|---------|-----------------|-------------------|
| LiveKit / Pipecat | Open-source SFU architecture, 50+ AI model integration, OpenAI ChatGPT backbone | Voice AI infrastructure standard, can use as transport layer |
| Retell AI | ~600ms E2E (no tuning), proprietary turn detection model, BYO-LLM | Turn detection is key differentiator |
| Vapi | BYO-stack (choose STT/LLM/TTS), A/B testing, 1000+ templates | Composable architecture pattern |

#### Smart Speakers / Phone Assistants

| Product | Key Technologies | Value for chobits |
|---------|-----------------|-------------------|
| Amazon Alexa+ | Autonomous task execution (Uber/OpenTable/Grubhub), model-agnostic routing, cross-device continuity | Agent chain execution pattern; chobits can implement via MCP |
| Google Gemini for Home | Natural language automation creation ("Ask Home"), AI camera understanding | "Describe automation" UX pattern |
| Xiaomi Super Xiaoai | Speaker ID for family members, AI call answering, dynamic bubble visual feedback | Multi-member voiceprint + family features |
| Xiaodu | Bluetooth Mesh local gateway, offline control, voice personality persistence | Local gateway architecture; voice personality as emotional bond |
| Tmall Genie | "1+3+N" architecture, spatial intelligence agent, edge scenario dispatch | Spatial intelligence agent: from command to context-aware |
| iFlytek Spark | One-sentence voice cloning, 74+ dialects, E2E voice translation | One-sentence cloning is killer feature |
| Doubao | Super mode (autonomous task decomposition), 100M+ DAU, Coze agent platform | Agent task decomposition + UGC character platform |

#### AI Wearables

| Product | Key Technologies | Value for chobits |
|---------|-----------------|-------------------|
| Meta Ray-Ban | 76% market share, audio-first, visual perception, 7+ language real-time translation | Audio-first + visual perception is success model |
| Omi (open-source) | $89, MIT license, 250+ community apps, local recording + cloud sync | Closest open-source wearable reference |
| Bee AI (Amazon) | $50, 7-day battery, no audio storage privacy strategy | Low price + long battery + privacy-first |
| Looki L1 | 30g pendant, 12-hour battery, AI ambient perception + life log | Smallest form factor + perception resonance philosophy |
| Limitless Pendant | Always-listening, speaker diarization, MCP server integration, 100+ languages | Ambient context capture + MCP integration pattern |

#### AI Companion Apps

| Product | Key Technologies | Value for chobits |
|---------|-----------------|-------------------|
| Character.AI | 10M+ user characters, Lorebook worldview, PipSqueak 2 model | UGC character marketplace + persistent persona mode |
| Replika 2.0 | Cross-month memory reconstruction, proactive reminders, AR experience, video calls | Proactive memory-driven suggestions is powerful UX |
| ChatGPT Voice | E2E audio model, 320ms latency, 22 neural TTS voices | Consumer voice AI benchmark |
| Microsoft Copilot | Work IQ persistent memory, Agent 365 governance, computer-use agent | Most mature enterprise memory system |

#### Key Industry Trends

| Trend | Representative Products | chobits Opportunity |
|-------|------------------------|---------------------|
| Semantic VAD | OpenAI (model-level turn-taking) | Smarter interruption than pure VAD |
| Conversational Prosody | Sesame CSM (breathing/hesitation/laughter) | More natural TTS |
| E2E S2S | OpenAI/Hume/Sesame | Ultimate goal, current cascaded more practical |
| Emotion-adaptive | Hume AI (detect → adaptive tone) | Key voice assistant differentiator |
| SSM Architecture TTS | Cartesia (State Space Model) | Faster TTS inference |
| Persistent Memory | Work IQ / Replika / Character.AI | From chat history to long-term memory |
| MCP Voice Integration | OpenAI + MCP, ElevenLabs 11.ai | Voice conversation calling tools |
| Voice Cloning Democratization | Cartesia 3s / Inworld 15s / MiniMax 10s | Lowest barrier voice personalization |
| Privacy-first | Bee (no audio storage), Omi (local processing) | Always-listening privacy strategy |

#### Vertical Industry Voice

> Voice AI solutions in automotive, healthcare, education, translation and other verticals, for chobits vertical scenario reference.

| Industry | Representative Products | Key Technologies | Value for chobits |
|----------|------------------------|------------------|-------------------|
| Automotive Voice | NIO NOMI, Xpeng Tianji, SoundHound | Edge offline processing, 14 emotion tones, RAG vehicle manual, Speech-to-Meaning | Edge offline is key; emotion tone customization; RAG reference |
| Healthcare Voice | Nuance DAX, Nabla, Abridge, Suki AI | Ambient clinical documentation, privacy-first (no data storage), HIPAA compliant, guided reasoning | Passive listening mode; privacy-first architecture; guided conversation |
| Education Voice | Duolingo Max, Zuoyebang P50, Youdao Ziyue | Step-by-step guided reasoning, personalized learning paths, gamification, real-time pronunciation correction | Guided conversation pattern; gamification drives engagement |
| Translation Devices | Pocketalk, Timekettle, Vasco | Dual-mic noise reduction, <1s latency, offline translation packs, dedicated hardware | Dual-mic + low latency key; offline capability |
| Smart Home Hub | Echo Hub, HomePod, SmartThings | Matter 1.3/Thread 1.4, offline voice (0.2-0.4s local), predictive automation | ESP32 as privacy-first local controller |

#### Chinese AI Startups (2025-2026)

> Voice AI technologies from China's "AI Six Tigers" and emerging companies, for chobits technology selection reference.

| Company | Key Technologies | Value for chobits |
|---------|-----------------|-------------------|
| [StepFun](https://github.com/stepfun-ai) | Step-Audio 2 (130B S2S), Step-Audio-TTS-3B, Apache 2.0 open-source, emotion control + dialects | **Most relevant**: best open-source voice AI, usable directly |
| [Zhipu AI](https://github.com/THUDM) | GLM-4V vision model, 85% revenue from local deployment, HK IPO | Local deployment business model reference; vision capability |
| [MiniMax](https://github.com/MiniMaxAI) | Hailuo AI, Xingyao, 1.75 trillion weekly token usage, Speech-2.8 | Consumer AI product operation reference; emotion TTS |
| [Moonshot Kimi](https://github.com/MoonshotAI) | Kimi K2.5 + Kimi Claw, valuation from $4.3B→$18B in 3 months | Ultra-long context + voice AI integration |
| [Baichuan](https://github.com/baichuan-inc) | Healthcare vertical LLM, 3B cash reserve | Deep vertical industry approach |

---

## Priority Legend

| Level | Meaning | Action |
|-------|---------|--------|
| 🔴 P0 | Must fix immediately | Compile errors, no auth, incomplete data |
| 🟡 P1 | Should fix | Race conditions, memory leaks, security risks, missing core features |
| ⚪ P2 | Feature gaps | Incomplete protocols, insufficient configurability |
| 🟢 P3 | Optimization | Performance, code quality, non-core features |

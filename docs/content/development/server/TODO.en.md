+++
title = "TODO"
weight = 204
[extra]
source_file_hash = "4184f69b5b34205820ca5cd8808f6e8d26b051d2"
translated_at = "2026-07-11T00:00:00Z"
+++

# TODO

> **⚠️ Note: This document is an exploratory feature inventory, referencing industry standards (Alexa+, Gemini, Siri AI) and reference projects (xiaozhi-esp32-server, xiaozhi-esp32-server-java, xiaozhi-server-go). Most items will not be implemented — they serve as reference for chobits' positioning and trade-offs. Actual development priorities follow the project Roadmap.**

To-do list organized by functional module. Before fixing, please read [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) to understand the development conventions.

## Security & Authentication

| Priority | Item | Location | Description | Status |
|----------|------|----------|-------------|--------|
| 🔴 P0 | WS Authentication | `api/src/ws/mod.rs` | WS handler has no auth layer, all WS connections unauthenticated | 🔴 P0 |
| 🔴 P0 | Invite Code System | New feature | No invite code generation/verification/management, new user registration uncontrolled | 🔴 P0 |
| 🟡 P1 | Rate Limiting | `api/src/auth.rs` | No login rate limiting / brute-force protection | 🟡 P1 |
| 🟡 P1 | Refresh Revocation | `api/src/auth.rs` | No revocation mechanism for refresh tokens, logout only clears client-side | 🟡 P1 |
| 🟡 P1 | Token Logging | `api/src/auth.rs` | Access token logged in plain text in tracing span | 🟡 P1 |
| 🟡 P1 | OTA Device Activation | `api/src/ota.rs` | `activate` endpoint is a stub, returns "success" without verifying device, device info not persisted to DB | 🟡 P1 |
| 🟡 P1 | MCP Authentication | `api/src/mcp/mod.rs` | `/mcp` endpoint auth is commented out | 🟡 P1 |

## Core Features

| Priority | Item | Location | Description | Status |
|----------|------|----------|-------------|--------|
| 🔴 P0 | stop_round Race Condition | `service/src/chobits/session/round.rs` | Missing synchronization between `llm_tts_handle` and `stop_round`, potential use-after-cancel | 🔴 P0 |
| 🔴 P0 | LLM Thread Safety | `api/src/llm/model/qwen3/mod.rs` | `thread::spawn` + `block_on`, missing `catch_unwind`, panic silently crashes | 🔴 P0 |
| 🔴 P0 | LLM Echo Thread | `api/src/llm/model/echo/mod.rs` | Same issue | 🔴 P0 |
| 🔴 P0 | Wake Word Support | `api/src/ws/` + protocol layer | ESP32 client already has ESP-SR offline wake, server needs to handle `wake_word` message type, current protocol lacks this field. Industry standard (Echo/Google/Xiaozhi all have it), response latency <200ms | 🔴 P0 |
| 🟡 P1 | Voiceprint Recognition | New feature | Reference project xinnan-tech implements this (3D-Speaker model), processed in parallel with ASR, identifies speaker identity for LLM personalization. sherpa-onnx already supports speaker identification (ECAPA-TDNN/WeSpeaker), no new dependency needed. hackers365 Go version uses Qdrant vector DB + dynamic TTS voice switching, more mature architecture. Needs: registration/management/recognition flow + DB storage of voiceprint vectors + LLM context injection | 🟡 P1 |
| 🟡 P1 | Opus Division by Zero | `api/src/ws/default_listener.rs` | Division by zero when channels=0 / sample_rate=0 | 🟡 P1 |
| 🟡 P1 | Clock Overflow | `service/src/chobits/session/mod.rs` | `Local::now()` is non-monotonic, subtraction can overflow | 🟡 P1 |
| 🟡 P1 | Device Management | New feature | No device registration/binding/listing, no device persistence after OTA activation. Reference projects have complete device lifecycle management (registration/status/config/OTA/batch operations) | 🟡 P1 |
| 🟡 P1 | Music Playback | MCP tool or standalone | Reference project xinnan-tech has `play_music.py` + `hass_play_music.py`; joey-zhou has `MusicPlayer` with LRC lyrics sync. Industry standard | 🟡 P1 |
| 🟡 P1 | Timer/Reminders/Alarms | MCP tool or standalone | Industry standard ("Alexa, set a timer"), currently no timer/reminder mechanism | 🟡 P1 |
| 🟡 P1 | Continued Conversation | `service/src/chobits/session/` | Microphone should briefly remain open after reply, allowing wake-word-free follow-up. Gemini / Alexa+ both support this | 🟡 P1 |
| 🟡 P1 | Emotion Recognition | `api/src/llm/model/` (analyze_emotion) | Current stub returns "happy". Industry approach: audio features (wav2vec2 SER) + text sentiment (GoEmotions) dual-channel fusion, used to adjust TTS tone and reply style | 🟡 P1 |
| 🟡 P1 | Personalized Memory / Long-term Preferences | New feature | Currently only chat history, no long-term preference storage. Reference project xinnan-tech has PowerMem (user profiling + Ebbinghaus forgetting curve + vector retrieval); joey-zhou has 3 memory modes (window/summary/long + graph retrieval) | 🟡 P1 |
| 🟡 P1 | RAG Knowledge Base | MCP or built-in module | Reference project xinnan-tech integrates RAGFlow; joey-zhou has EmbeddingModelFactory + graph retrieval. Current MCP framework can connect but has no built-in vector retrieval | 🟡 P1 |
| 🟡 P1 | Multi-language TTS | `api/src/tts/` | Currently single-language TTS voice. ESP32 client already supports 25+ language ASR, TTS side needs to match | 🟡 P1 |
| 🟡 P1 | Home Assistant Integration | MCP or standalone module | Reference project xinnan-tech has 3 HA integration methods (community plugin / HA as LLM tool / HA MCP Server). Smart home is a core voice assistant scenario | 🟡 P1 |
| 🟡 P1 | Inter-device Calls | MCP tool or standalone | Reference project xinnan-tech has `call_device.py`, ESP32 devices can call each other like phones, needs MQTT gateway + contacts management | 🟡 P1 |
| 🟡 P1 | Intent Recognition | New feature | Reference project xinnan-tech supports 3 modes: function_call (recommended), intent_llm (dedicated LLM), nointent. chobits currently has no independent intent layer | 🟡 P1 |
| 🟡 P1 | Audio Hot-path Cloning | `api/src/ws/default_listener.rs` | Frequent `data.to_vec()` cloning every 20ms | 🟡 P1 |
| 🟡 P1 | Quick Reply / Pre-acknowledgment | New feature | Play short phrases ("I'm here", "Coming") during LLM inference to reduce perceived latency. Reference project hackers365 Go version implements this, UX critical, simple to implement | 🟡 P1 |
| 🟡 P1 | Dynamic TTS Voice Switching | New feature | Automatically switch TTS voice based on voiceprint identification. Reference project hackers365 Go version implements this (sherpa-onnx voiceprint + per-speaker TTS voice), natural extension of voiceprint recognition | 🟡 P1 |
| 🟡 P1 | LLM History Blocking | `api/src/llm/model/qwen3/mod.rs` | DB persistence blocks the entire thread | 🟡 P1 |
| 🟡 P1 | Recorder Unbounded | `api/src/record/recorder.rs` | `Vec<RecordEntry>` has no size limit, unbounded memory growth under high concurrency | 🟡 P1 |
| ⚠️ P2 | Message Types | `api/src/ws/frame.rs` | Missing `system`, `alert`, `custom`, `wake_word` message types (compared to xiaozhi-esp32 spec) | ⚠️ P2 |
| ⚠️ P2 | Multi ASR/TTS Provider | `api/src/asr/` + `api/src/tts/` | Reference project xinnan-tech supports 12 ASR + 18+ TTS providers (including free EdgeTTS); joey-zhou has 7 STT + 8 TTS. chobits has only 1 ASR + 1 TTS | ⚠️ P2 |
| ⚠️ P2 | Plugin System | New feature | Reference project xinnan-tech has 13 built-in plugins + hot-reload. chobits has no plugin architecture, feature extensions require modifying core code | ⚠️ P2 |
| ⚠️ P2 | MCP Market / Tool Marketplace | New feature | Reference project hackers365 Go version implements MCP tool "app store": aggregates multiple third-party marketplaces (e.g. ModelScope), one-click import of remote MCP services + hot-reload. chobits currently has no MCP tool discovery/aggregation mechanism | ⚠️ P2 |
| ⚠️ P2 | MCP Debug Console | New feature | Reference project hackers365 Go version has Agent/Device-level MCP remote debugging: web console generates per-agent MCP endpoints, real-time call testing, supports per-agent tool filtering. chobits currently has no MCP debugging tools | ⚠️ P2 |
| ⚠️ P2 | Setup Wizard + Full-chain Testing | New feature | Reference project hackers365 Go version has first-run wizard (OTA/VAD/ASR/LLM/TTS step-by-step config) + per-component latency testing + visualization charts. chobits currently has no deployment wizard | ⚠️ P2 |
| ⚠️ P2 | MCP Tool Aggregation | New feature | Reference projects xiaozhi-mcp / yuexianga/xiaozhi-mcp provide pre-built tool libraries (DingTalk/QQ/system monitoring/WebPilot/math), ready to use. chobits has no built-in MCP tool package | ⚠️ P2 |
| ⚠️ P2 | MQTT Gateway | New feature | Reference project xinnan-tech/xiaozhi-mqtt-gateway implements MQTT+UDP → WS bridging: distributed deployment + dynamic load balancing + HMAC authentication + MCP command dispatch. chobits currently only supports WS protocol | ⚠️ P2 |
| ⚠️ P2 | VAD Sample Rate | `api/src/vad/` | Hardcoded 16kHz, non-16kHz input silently fails | ⚠️ P2 |
| ⚠️ P2 | ASR | `api/src/asr/` | SenseVoice (sherpa-onnx), no `Sync` trait, 16kHz mono only | ⚠️ P2 |
| 🟢 P3 | Vision Perception (VLLM) | New feature | Reference project xinnan-tech supports GLM-4V / Qwen-VL vision models for photo understanding. chobits currently voice-only | 🟢 P3 |
| 🟢 P3 | Proactive Suggestions | New feature | Gemini Daily Brief / Alexa+ proactive alerts for traffic/deals/calendar. Requires scheduled tasks + user context reasoning | 🟢 P3 |
| 🟢 P3 | Multimodal (Voice + Screen + Video) | New feature | Gemini 2.5 / GPT-Realtime / Siri AI all support camera/screen input. chobits currently voice-only | 🟢 P3 |
| 🟢 P3 | Cross-device Continuity | New feature | Alexa+: Echo→phone→PC seamless conversation context switching. Requires session state sync mechanism | 🟢 P3 |
| 🟢 P3 | Speaker Diarization | `api/src/listener/` | Speaker separation, distinguishing different users in multi-person scenarios. sherpa-onnx already supports this (ECAPA-TDNN + AHC clustering) | 🟢 P3 |
| 🟢 P3 | Voice Cloning | `api/src/tts/` | Reference project xinnan-tech supports Volcengine voice cloning; joey-zhou supports per-role voice cloning. MatchaTTS already supports reference audio, needs exposed config interface | 🟢 P3 |
| 🟢 P3 | Server-side AEC Denoising | `api/src/ws/default_listener.rs` | joey-zhou implements WebRTC AEC3 server-side echo cancellation (with noise suppression + high-pass filter + adaptive gain). chobits only has client-side AEC | 🟢 P3 |
| 🟢 P3 | WebRTC Real-time Audio/Video | New feature | Reference project dairoot/xiaozhi-webrtc implements WebRTC low-latency + Live2D + multimodal vision + MCP. chobits currently only WS protocol | 🟢 P3 |
| 🟢 P3 | Audio Normalization Integration | `api/src/util/compressor.rs` → pipeline | `adaptive_normalize()` implemented but not integrated into TTS output pipeline | 🟢 P3 |
| 🟢 P3 | Live2D Avatar | Client feature | Reference project Android client (TOM88812) implements: multi-model switching + real-time animation + custom characters + mood mode. chobits Flutter client can reference this | 🟢 P3 |
| 🟢 P3 | Embodied AI / GPIO | New feature | Reference project py-xiaozhi implements: Raspberry Pi/Jetson/STM32 direct hardware control (motors/sensors/LEDs), camera vision understanding. Vertical scenario, not core voice assistant | 🟢 P3 |
| 🟢 P3 | Session Export/Delete | `api/src/record/` | Sessions can only be viewed, not exported or deleted | 🟢 P3 |
| 🟢 P3 | describe O(n) | `api/src/llm/model/qwen3/mod.rs` | Rebuilds full message history on every request | 🟢 P3 |
| 🟢 P3 | TTS Clone Storm | `api/src/tts/` | `Arc<str>` vs `String` clone storm | 🟢 P3 |
| 🟢 P3 | Double Serialization | `api/src/record/recorder.rs` | Double JSON serialization in record path | 🟢 P3 |

## Infrastructure

| Priority | Item | Location | Description | Status |
|----------|------|----------|-------------|--------|
| 🟡 P1 | Email Constraint | `migration/src/m20241230_000001_init.rs` | Entity annotated with `#[sea_orm(unique)]`, migration does not implement UNIQUE | 🟡 P1 |
| 🟡 P1 | Foreign Key Constraints | `migration/src/m20241230_000001_init.rs` | Missing FK: `round.session_id`, `round_data.round_id`, `frame.round_id` | 🟡 P1 |
| 🟡 P1 | MCP Lock Ordering | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server lock order ABBA, potential deadlock | 🟡 P1 |
| 🟡 P1 | Graceful Shutdown Order | `framework/src/signal.rs` + `apps/server` | Missing shutdown ordering across modules | 🟡 P1 |
| 🟡 P1 | Panic Handling | `framework/src/panic.rs` | Uses `eprintln!` instead of `tracing::error!`, bypasses Sentry | 🟡 P1 |
| 🟡 P1 | Runtime Race Condition | `framework/src/runtime.rs` | Race condition in `OnceLock` initialization | 🟡 P1 |
| 🔴 P0 | Signal Macro | `framework/src/signal.rs` | Uses non-existent `debug_error!` macro, fails to compile on non-unix | 🔴 P0 |
| 🟢 P3 | Timestamp Auto-fill | `entity/src/config.rs` | `Config` entity missing `ActiveModelBehavior`, timestamps not auto-filled | 🟢 P3 |
| 🟢 P3 | MCP Error Handling | `api/src/mcp/` | Incomplete | 🟢 P3 |

## Frontend (apps/server-ui)

| Priority | Item | Location | Description | Status |
|----------|------|----------|-------------|--------|
| 🟡 P1 | Dashboard Page | `routes/_pathlessLayout.admin/index.tsx` | Shell only, renders "Hello", no stats/monitoring content | 🟡 P1 |
| 🟡 P1 | User CRUD Management UI | New page | No user list/create/delete/role management interface | 🟡 P1 |
| 🟡 P1 | Multi-user/RBAC Management | New feature | Reference project busy-worker Java management platform has Token usage monitoring + conversation duration + device activity + data visualization + RBAC. chobits Dashboard should include this | 🟡 P1 |
| 🟢 P3 | System Monitoring | New page | No server health/connection count/resource usage/error rate dashboard | 🟢 P3 |
| 🟢 P3 | MCP Dashboard | New feature | Reference project xiaozhi-mcphub has React frontend: multi-endpoint management + tool sync + group access control + logs. Reference for chobits MCP management | 🟢 P3 |

## Mobile (apps/app)

| Priority | Item | Location | Description | Status |
|----------|------|----------|-------------|--------|
| 🟡 P1 | Flutter WS Integration | `apps/app/` | Flutter app has scaffolding but no WS + auth integration | 🟡 P1 |
| 🟡 P1 | App CI/CD | `.github/workflows/` | No iOS/Android build/sign/publish pipeline | 🟡 P1 |

## Testing

| Priority | Item | Location | Description | Status |
|----------|------|----------|-------------|--------|
| 🔴 P0 | WS Auth Tests | `apps/server/api/tests/` | WS endpoint has zero auth, no corresponding tests | 🔴 P0 |
| 🔴 P0 | Wake Word Tests | `apps/server/api/tests/` | Wake word message handling has no tests, needs to verify protocol parsing and session wake flow | 🔴 P0 |
| 🟡 P1 | Auto Mode + AEC Tests | `apps/server/api/tests/session/` | Auto mode (barge_in=false) with/without AEC scenarios lacks dedicated tests | 🟡 P1 |
| 🟡 P1 | Button Talk Tests | `apps/server/api/tests/session/` | Manual mode (push-to-talk) lacks end-to-end tests | 🟡 P1 |
| 🟡 P1 | Continued Conversation Tests | `apps/server/api/tests/session/` | Follow-up flow with microphone kept open after reply has no tests | 🟡 P1 |
| 🟡 P1 | Rate Limiting Tests | `apps/server/api/tests/` | Login rate limiting feature does not exist, no tests | 🟡 P1 |
| 🟡 P1 | Emotion Recognition Tests | `apps/server/api/tests/` | analyze_emotion stub has no test coverage | 🟡 P1 |
| 🟡 P1 | Voiceprint Tests | `apps/server/api/tests/` | Voiceprint registration/recognition/LLM injection flow has no tests | 🟡 P1 |
| 🟡 P1 | Music Playback Tests | `apps/server/api/tests/` | Music playback feature has no tests | 🟡 P1 |

## To Be Confirmed / Exploratory

| Item | Description |
|------|-------------|
| Voiceprint Solution Choice | 3D-Speaker (used by xinnan-tech) vs sherpa-onnx speaker ID (already in tech stack) vs pyannote. sherpa-onnx is optimal: no new dependency, supports ECAPA-TDNN/WeSpeaker/CAM++ models |
| Plugin Architecture Design | xinnan-tech has 13 built-in plugins + hot-reload. Does chobits need a similar mechanism, or is MCP extension sufficient? |
| Multi-Provider Extension Strategy | Reference projects support 12+ ASR / 18+ TTS providers. Does chobits need a multi-provider architecture, or stay lean? |
| Music Playback | Industry standard (Spotify/Apple Music integration), need to confirm implementation: MCP tool calling external API / built-in audio playback / TTS extension |
| ESP32 CI/CD | Separate project (xiaozhi-esp32), outside this repo's scope, chobits provides WS + OTA interface as backend |
| Token Persistent Storage | Consider using DB table to store refresh tokens for multi-device management |
| Smart Home Integration | Alexa+ / Google Home core capability. Reference projects have 3 HA integration methods, need to evaluate chobits' positioning as hub |
| Agentic Capabilities | Alexa+ "Experts" / Gemini Spark 24/7 agents, can autonomously browse web/forms/orders. LLM + MCP tool calling has foundation, need to evaluate implementation depth |
| Emotional TTS | Gemini 2.5 / Cartesia Sonic-3 support adjusting TTS tone based on emotion. Need to evaluate if MatchaTTS supports style/prosody control |
| Local Privacy Processing | Industry trend: Echo/Fire devices migrating to local processing. chobits already has local Qwen3, can extend to full local ASR/TTS pipeline |
| Vision Capabilities | Reference project xinnan-tech supports VLLM (GLM-4V/Qwen-VL) for photo understanding. Does chobits need vision capabilities? |
| MCP Market Architecture Design | Reference project hackers365 Go version implements MCP tool "app store" (multi-market aggregation + hot-reload). Does chobits need MCP tool discovery/aggregation? |
| Dynamic TTS Voice Switching | Reference project hackers365 Go version auto-switches TTS voice based on voiceprint. How should chobits link voiceprint recognition to TTS? |
| Setup Wizard Design | Reference project hackers365 Go version has first-run wizard + full-chain latency testing. Does chobits need a deployment wizard to lower the barrier? |
| MQTT Gateway Architecture | Distributed deployment requires MQTT+UDP bridging + dynamic load balancing (ref: xinnan-tech/xiaozhi-mqtt-gateway). Does chobits need an MQTT gateway layer? |
| MCP Tool Aggregation Strategy | Reference projects have pre-built tool libraries (DingTalk/QQ/system monitoring/WebPilot/math, etc.). Does chobits need a built-in MCP tool package? |

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

---

## Priority Guide

| Level | Meaning | Action |
|-------|---------|--------|
| 🔴 P0 | Must fix immediately | Compile error, no auth, data inconsistency |
| 🟡 P1 | Should fix | Race conditions, memory leaks, security risks, core feature gaps |
| ⚠️ P2 | Missing feature | Incomplete protocol, insufficient configurability |
| 🟢 P3 | Optimization | Performance, code quality, non-core features |

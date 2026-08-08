+++
title = "Roadmap"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "c986a26a3f2338e85c7836c887a220bae7b4339b"
translated_at = "2026-08-08T12:00:00Z"
+++

## Overview

```txt │
                                            ┌──────────────────────────────────┐     │
                                            │Register│                         │     │
                                            │────────┘                         │     │
                   ┌─────────────────────────│  1.inner root                    │◄────│
                   │                         └──────────────────────────────────┘     │
                   │                                           │                      │
                   │                                           │                      │
                   ▼                                           ▼                      │
   ┌──────────────────────────────────┐      ┌──────────────┐───────────────────┐     │
   │Auth│                             │      │Activate&Bind │                   │     │
   │────┘                             │      └──────────────┘                   │     │
   │  1.OTA(v1,v2)                    │◄─────│  1.OTA(v1,v2)                    │◄────│
   │  2.Nostr                         │      │  2.Nostr                         │     │
   └──────────────────────────────────┘      └──────────────────────────────────┘     │
                   │              │                                                   │
                   │              └────────────────────────────┐                      │
                   │ (Bearer Token)                            │ (JWT)                │
                   ▼                                           ▼                      │
   ┌───────────────────┐──────────────┐       ╔══════════════════════════════════╗    │
   │Protocol Translator│              │       ║Restful │                         ║    │
   └───────────────────┘              │       ║────────┘                         ║────┘
   │  1.XiaoZhi                       │       ║                                  ║
   └──────────────────────────────────┘       ╚══════════════════════════════════╝
                   │
                   │
                   ▼
   ┌────────────┐─────────────────────┐
   │Input Filter│                     │
   └────────────┘                     │
   │  1.Recorder                      │───────────────────────────────────────────┐
   │  2.McpRouter                     │                                           │
   └──────────────────────────────────┘                                           │
                   │                                                              │
                   │ Frame                                                        │
                   ▼                                                              ▼
   ┌───────────────────────────────────────────────────┐       ┌──────────────────────────────────┐
   │ Session │                                         │       │ Mcp Session │                    │
   │─────────┘                                         │       │─────────────┘                    │
   │            ┌─────────────┐                        │       │                                  │
   │            │   Text      │                        │       │   ┌───────────────────────────┐  │
   │            │             ▼                        │       │   │Mcp Registry│              │  │
   │  ┌────────┐───────┐  ╔══════════════════════════╗ │       │   │────────────┘              │  │
   │  │Listener│       │  ║Round│                    ║ │   ╦══════►│  1.Device Mcp Client      │  │
   │  └────────┘       │  ║─────┘                    ║ │   ║   │   │  2.External Mcp Client    │  │
   │  │  1.VAD         │  ║  1.shadow/running round  ║ │   ║   │   │                           │  │
   │  │  2.ASR         │  ║  2.barge in              ║ │   ║   │   └───────────────────────────┘  │
   │  │                │  ║                          ║ │   ║   └──────────────────────────────────┘
   │  │                │  ╚══════════════════════════╝ │   ║       │
   │  │                │       │                       │   ║       │
   │  │                │       │ ask                   │   ║       │
   │  └────────────────┘       │                       │   ║       │
   │                           ▼                       │   ║       │
   │  ┌──────────────────────────────────────────────┐ │   ║       │
   │  │Ling Core│                            │       │ │   ║       │
   │  │─────────┘                            ▼       │ │   ║       │
   │  │  ┌────────┐─────┐       ┌────┐──────────┐    │ │   ║       │
   │  │  │Splitter│     │       │LLM │          │    │◄════╝       │
   │  │  └────────┘     │       └────┘          │    │ │           │
   │  │  │              │◄──────│               │    │ │           │
   │  │  └──────────────┘       └───────────────┘    │ │           │
   │  │    │                                         │ │           │
   │  │    ▼                                         │ │           │
   │  └──────────────────────────────────────────────┘ │           │
   │          │                                        │           │
   │          │ Text                                   │           │
   │          │                                        │           │
   │          ▼                                        │           │
   │  ┌───┐──────────┐                                 │           │
   │  │TTS│          │                                 │           │
   │  └───┘          │                                 │           │
   │  │              │                                 │           │
   │  └──────────────┘                                 │           │
   │      │                                            │           │
   │      │                                            │           │
   │      ▼                                            │           │
   └───────────────────────────────────────────────────┘           │
                   │                                               │
                   │ FrameResult                                   │
                   │◄──────────────────────────────────────────────┘
                   ▼
   ┌─────────────┐────────────────────┐
   │Output Filter│                    │
   └─────────────┘                    │
   │  1.Recorder                      │
   └──────────────────────────────────┘
                   │
                   │
                   ▼
   ┌───────────────────┐──────────────┐
   │Protocol Translator│              │
   └───────────────────┘              │
   │  1.XiaoZhi                       │
   └──────────────────────────────────┘
                   │
                   │
                   ▼
```

## Status

Column reference:

| Column | Description                 | Possible Values                         |
| ------ | --------------------------- | --------------------------------------- |
| Status | Feature completeness        | ✅ Complete / ⚠️ Defective (stub/incomplete) / ❌ Not implemented |
| Test   | Automated tests exist       | ✅ Yes / ❌ No / — N/A                 |
| Item   | Feature name                | —                                       |
| Desc   | Brief description           | —                                       |
| Link   | File path or Issue link     | (TBD)                                   |

### Register

| Status | Test | Item       | Description                              | Link |
| ------ | ---- | ---------- | ---------------------------------------- | ---- |
| ❌     | ❌   | Inner root | Migration only, no runtime register endpoint |  |

### Auth

#### OTA Authentication

| Status | Test | Item   | Description                                               | Link |
| ------ | ---- | ------ | --------------------------------------------------------- | ---- |
| ✅     | ✅   | OTA v1 | Basic device auth, returns ws_url+token (real JWT for activated, empty otherwise) |  |
| ❌     | ❌   | Firmware distribution | OTA returns version: "0.0.1" + url: null, no actual firmware download link |  |
| ❌     | ❌   | MQTT support | OTA returns mqtt: null, MQTT protocol not implemented |  |
| ❌     | ❌   | OTA v2 | Full auth + device info report + firmware distribution (returns static data) |  |

#### Nostr Authentication

| Status | Test | Item         | Description | Link |
| ------ | ---- | ------------ | ----------- | ---- |
| ❌     | ❌   | NIP-98 auth  | Not implemented |  |

#### WS Authentication

| Status | Test | Item               | Description                       | Link |
| ------ | ---- | ------------------ | --------------------------------- | ---- |
| ✅     | ✅   | Bearer Token verify | Authorization header + query param fallback, JWT decode verification |  |

### Activate & Bind

#### OTA Activation

| Status | Test | Item   | Description                               | Link |
| ------ | ---- | ------ | ----------------------------------------- | ---- |
| ✅     | ✅   | OTA v1 | Activation code verification + device info persisted |      |
| ❌     | ❌   | OTA v2 | Full activation flow (with challenge verification), not implemented |  |

#### Nostr Binding

| Status | Test | Item            | Description | Link |
| ------ | ---- | --------------- | ----------- | ---- |
| ❌     | ❌   | Bind / Role registration | Not implemented |  |

### Protocol Translator

#### XiaoZhi

| Status | Test | Item            | Description                                  | Link |
| ------ | ---- | --------------- | -------------------------------------------- | ---- |
| ✅     | ❌   | Input frame parsing | hello/listen/abort/mcp/voice fully parsed  |      |
| ✅     | ❌   | Output frame serialization | STT/LLM/TTS/Audio/Error fully serialized |  |
| ❌     | ❌   | Missing message types | system/alert/wake_word not implemented    |  |

### Input Filter

#### Recorder (Input)

| Status | Test | Item        | Description                | Link |
| ------ | ---- | ----------- | -------------------------- | ---- |
| ✅     | ✅   | Input persistence | Frame data persisted to database |  |

#### McpRouter

| Status | Test | Item          | Description                          | Link |
| ------ | ---- | ------------- | ------------------------------------ | ---- |
| ✅     | ✅   | MCP frame routing | Intercepts Frame::Mcp → McpSession |  |

### Session

#### Listener

| Status | Test | Item                    | Description                                      | Link |
| ------ | ---- | ----------------------- | ------------------------------------------------ | ---- |
| ✅     | ✅   | VAD (Silero)            | Earshot implementation, frame-level voice detection |  |
| ✅     | ✅   | ASR (sherpa-onnx)       | XAsr model, 16kHz mono                          |      |
| ✅     | ✅   | Voice Break detection   | Silence timeout triggers TurnComplete            |      |
| ✅     | ✅   | Continued Conversation  | auto/realtime mode, auto-listens after TTS       |      |
| ❌     | ❌   | Semantic VAD / Hierarchical turns | Layer1 Silero only, missing punctuation/semantic layers |  |

#### Round

| Status | Test | Item                   | Description                                  | Link |
| ------ | ---- | ---------------------- | -------------------------------------------- | ---- |
| ✅     | ✅   | Shadow / Running rounds | Pre-creates shadow round for lower switch latency |  |
| ✅     | ✅   | Barge-in + lockout      | Configurable lockout window (default 250ms)  |      |
| ✅     | ✅   | Epoch message filtering | Discards stale rounds, preserves TTS state notifications |  |
| ✅     | —    | stop_round race condition | Fixed, 5s timeout fallback                 |      |
| ✅     | ✅   | AudioPacer              | Rate-limits audio output by output_frame_duration |  |

#### LingCore

| Status | Test | Item                  | Description                                   | Link |
| ------ | ---- | --------------------- | --------------------------------------------- | ---- |
| ✅     | ✅   | Qwen3 LLM (candle)    | Streaming generation, thread-safety fixed     |      |
| ✅     | ✅   | Splitter (sentencex)  | Chinese/English sentence segmentation, 700ms timeout fallback |  |
| ✅     | ✅   | ToolCall loop         | LLM → MCP tool → result injection → LLM regeneration |  |
| ✅     | ✅   | History management    | Context truncation (max_prompt_len) and persistence |  |
| ⚠️     | ❌   | Emotion recognition   | Always returns "happy", no real analysis      |      |
| ❌     | ❌   | Personalized memory   | Long-term preferences/user profiles not implemented |  |

#### TTS

| Status | Test | Item                | Description                              | Link |
| ------ | ---- | ------------------- | ---------------------------------------- | ---- |
| ✅     | ✅   | MatchaTTS + Opus    | sherpa-onnx engine, Opus encoding output |      |
| ✅     | ✅   | Two-stage streaming | Token-level first packet fast + sentence-level steady |  |
| ✅     | ❌   | TTFA/RTF metrics    | First-packet delay / real-time factor monitoring |  |
| ✅     | ❌   | Fade-out            | Tail fade-out to eliminate clicks        |      |

### McpSession

#### McpRegistry

| Status | Test | Item              | Description                              | Link |
| ------ | ---- | ----------------- | ---------------------------------------- | ---- |
| ✅     | ✅   | Tool registration/aggregation | Multi-client tool merging, name-routed invocation |  |

#### Device Mcp Client

| Status | Test | Item         | Description                        | Link |
| ------ | ---- | ------------ | ---------------------------------- | ---- |
| ✅     | ✅   | rmcp transport | Device MCP protocol bridge, full handshake |  |

#### External Mcp Client

| Status | Test | Item           | Description                              | Link |
| ------ | ---- | -------------- | ---------------------------------------- | ---- |
| ✅     | ✅   | HTTP remote MCP | Connects to external MCP servers, tool list cached |  |

#### MCP Authentication

| Status | Test | Item           | Description                | Link |
| ------ | ---- | -------------- | -------------------------- | ---- |
| ✅     | ✅   | /mcp endpoint auth | Bearer JWT auth + rmcp default Host validation |  |

### Output Filter

#### Recorder (Output)

| Status | Test | Item          | Description                       | Link |
| ------ | ---- | ------------- | --------------------------------- | ---- |
| ✅     | ✅   | Output persistence | Frame/round data persisted to database |  |

### Restful

| Status | Test | Item                           | Description                                    | Link |
| ------ | ---- | ------------------------------ | ---------------------------------------------- | ---- |
| ✅     | ✅   | POST /api/auth/login           | Account password auth, returns access/refresh token |  |
| ✅     | ✅   | POST /api/auth/access_token    | Refresh token exchange                        |      |
| ✅     | ✅   | POST /api/auth/reset_password  | JWT auth, old password verification            |      |
| ✅     | ✅   | GET /api/auth/user             | Returns current user info                      |      |
| ✅     | ❌   | GET /api/stats/summary        | Overview stats (devices/sessions/messages/latency avg) |  |
| ✅     | ❌   | GET /api/stats/trends         | Trend data (daily device/session/message trends) |  |
| ✅     | ❌   | GET /api/stats/latency        | Latency percentile data (P50/P90/P99 etc.)     |      |
| ✅     | ✅   | Rate Limiting                  | GitHub-style buckets: auth/ota per-IP fixed window (20/15min, 30/min) + core per-user (1000/h) + X-RateLimit-* response headers + per-account login failure lockout (5/15min) |  |
| ✅     | ✅   | GET /api/security/rate_limit   | Authenticated introspection endpoint returning auth/ota/core quotas and live usage; does not consume quota |  |
| ❌     | ❌   | Invite Code system             | Registration invite code generation/verification not implemented |  |
| ✅     | ❌   | Device management CRUD         | List/activate (by code/by ID)/disable/enable/delete endpoints implemented (no detail) |  |

### Admin UI

| Status | Test | Item               | Description                                          | Link |
| ------ | ---- | ------------------ | ---------------------------------------------------- | ---- |
| ✅     | ❌   | Dashboard page     | 6 StatCards, TrendsChart, LatencyChart, RecentSessionsTable, LatencyTable |      |
| ✅     | ✅   | Session list/details | Search/date filter/pagination/SessionDetail component |  |
| ✅     | ✅   | Security event monitoring | Rate-limit/login events persisted + admin page query (type/IP filter + pagination) + 30-day auto cleanup |  |
| ❌     | ❌   | User CRUD UI       | User list/create/delete/role management not implemented |  |
| ❌     | ❌   | RBAC management    | Token usage/conversation duration/device activity/permissions not implemented |  |
| ❌     | ❌   | MCP dashboard      | Multi-endpoint management/tool sync/access control/logs not implemented |  |
| ❌     | ❌   | System monitoring  | Server health/connections/resource usage/error rates not implemented |  |

### Flutter App

| Status | Test | Item           | Description                                   | Link |
| ------ | ---- | -------------- | --------------------------------------------- | ---- |
| ❌     | ❌   | WS + Auth integration | Flutter scaffold exists, no WS/auth integration |  |
| ❌     | ❌   | CI/CD          | iOS/Android build/signing/publishing pipeline not implemented |  |

### Infrastructure

| Status | Test | Item                 | Description                                                         | Link |
| ------ | ---- | -------------------- | ------------------------------------------------------------------- | ---- |
| ✅     | ✅   | email UNIQUE constraint | user.email has UNIQUE constraint in migration (nullable, compatible with existing empty-email records) |      |
| ✅     | ✅   | Graceful shutdown order | Server CancellationToken drives axum/WS/Matrix graceful shutdown in order |      |
| ✅     | —    | Runtime race condition  | build() runs synchronously, set() happens-before any worker thread, no race |      |
| ✅     | ✅   | Timestamp auto-fill     | Config entity implements before_save, create/update timestamps auto-filled |      |


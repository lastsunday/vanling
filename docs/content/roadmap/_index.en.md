+++
title = "Roadmap"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "75b3cdbef309552ba9bc82a38054fbea3a7f7aed"
translated_at = "2026-10-10T17:09:06Z"
+++

## Overview

### Socket Session

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
  │  1.Recorder                      │
  │  2.McpRouter                     │───────────────────────────────────────────┐
  └──────────────────────────────────┘                                           │
                  │                                                              │
                  │ Frame                                                        │ Frame
                  ▼                                                              ▼
  ┌───────────────────────────────────────────────────┐       ┌──────────────────────────────────┐
  │ Session │                                         │       │ Mcp Session │                    │
  │─────────┘                                         │       │─────────────┘                    │
  │     ▲                            │                │       │                                  │
  │     │                            │ Pipeline Event │       │                                  │
  │     │ Round Event                │                │       │   ┌───────────────────────────┐  │
  │     │                            ▼                │       │   │Mcp Registry│              │  │
  │   ┌───────────────────────┐──────────────────┐    │       │   │────────────┘              │  │
  │   │ Round(shadow/running) │                  │    │   ┬──────►│  1.Device Mcp Client      │  │
  │   └───────────────────────┘                  │    │   │   │   │  2.External Mcp Client    │  │
  │   │                 ▲            │           │    │   │   │   │                           │  │
  │   │                 │            │ Pipeline  │    │   │   │   │                           │  │
  │   │                 │ Tap Event  │ Event     │    │   │   │   │                           │  │
  │   │                 │ (broadcast)▼           │    │   │   │   │                           │  │
  │   │               ┌──────────┐───────────┐   │    │   │   │   │                           │  │
  │   │               │ Pipeline │           │   │    │   │   │   └───────────────────────────┘  │
  │   │               └──────────┘           │   │    │   │   └──────────────────────────────────┘
  │   │               │ 1. VAD               │   │    │   │       │
  │   │               │ 2. ASR               │   │    │   │       │
  │   │               │ 3. Turn              │   │    │   │       │
  │   │               │ 4. Ling(LLM)    ◄─────────────────┘       │
  │   │               │ 5. TTS               │   │    │           │
  │   │               └──────────────────────┘   │    │           │
  │   │                              │           │    │           │
  │   │                              │ Pipeline  │    │           │
  │   │                              │ Event     │    │           │
  │   │                              ▼           │    │           │
  │   └──────────────────────────────────────────┘    │           │
  │                                  │                │           │
  │                                  │ Pipeline       │           │
  │                                  │ Event          │           │
  │                                  ▼                │           │
  └───────────────────────────────────────────────────┘           │
                  │                                               │
                  │ FrameResult                                   │
                  │                                               │
                  ▼                                               │
  ┌─────────────┐────────────────────┐                            │
  │Output Filter│                    │         FrameResult        │
  └─────────────┘                    │◄───────────────────────────┘
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

### Restful

```txt
                                                               ┌────┐──────────────────────────┐
                                                               │Docs│                          │
                                                               └────┘                          │
                                                               │  1.Scalar                     │
                                                               │                               │
                            │                                  └───────────────────────────────┘
                            │ Request
                            │
                            ▼
  ┌──────────────────────────────────────────────────┐         ┌────┐──────────────────────────┐
  │Layer│                                            │         │Task│                          │
  │─────┘                                            │         └────┘                          │
  │                                                  │         │  1.Security cleanup loop      │
  │           ┌────────┐─────────────────────┐       │         │                               │
  │           │CORS    │                     │       │         │                               │
  │           └────────┘                     │       │         └───────────────────────────────┘
  │           │                              │       │
  │           └──────────────────────────────┘       │
  │                         │                        │
  │                         │                        │
  │                         ▼                        │
  │           ┌────────┐─────────────────────┐       │
  │           │Tracing │                     │       │
  │           └────────┘                     │       │
  │           │                              │       │
  │           └──────────────────────────────┘       │
  │                         │                        │
  │                         │                        │
  │                         ▼                        │
  │           ┌───────────┐──────────────────┐       │
  │           │Body Limit │                  │       │
  │           └───────────┘                  │       │
  │           │                              │       │
  │           └──────────────────────────────┘       │
  │                         │                        │
  │                         │                        │
  │                         ▼                        │
  │           ┌────────┐─────────────────────┐       │
  │           │Timeout │                     │       │
  │           └────────┘                     │       │
  │           │                              │       │
  │           └──────────────────────────────┘       │
  │                         │                        │
  │                         │                        │
  │                         ▼                        │
  │           ┌────────┐─────────────────────┐       │
  │           │Security│                     │       │
  │           └────────┘                     │       │
  │           │  1.Access log                │       │
  │           │  2.Rate Limit                │       │
  │           │  3.Auth                      │       │
  │           └──────────────────────────────┘       │
  │                                                  │
  │                                                  │
  │                                                  │
  └──────────────────────────────────────────────────┘
                            │
                            │ Response
                            │
                            ▼

```

### IOT

```txt

     ┌─────────────────┐─────────────┐    ┌───────────────┐───────────────┐
     │ Input Controller│             │    │ Reporter      │               │
     └─────────────────┘             │    └───────────────┘               │
     │                               │    │                               │
     │  ┌──────────────────────┐     │    │                               │
     │  │Device Signal Input│  │     │    │                               │
     │  │───────────────────┘  │     │    │                               │
  │─►│  │  1.Device Signal     │     │    │                               │
  │  │  │  2.Intent(JSON)      │     │    └───────────────────────────────┘
  │  │  │                      │     │              │        ▲
  │  │  │                      │     │              │ Query  │ Query Response
  │  │  └──────────────────────┘     │              ▼        │
  │  │            │                  │    ┌───────────────┐───────────────┐
  │  │            ▼                  │    │ State Manager │               │
  │  │  ┌─────────────┐────────┐     │    └───────────────┘               │
  │  │  │Input Limiter│        │     │    │ 1. Render State               │
  │  │  └─────────────┘        │     │    │ 2. Demand State               │
  │  │  │                      │     │◄───│                               │
  │  │  │                      │     │    │                               │
  │  │  └──────────────────────┘     │    │                               │
  │  │                               │    └───────────────────────────────┘
  │  └───────────────────────────────┘               │             ▲   │
  │             ▲  │               │ Signal          │             │   │
  │             │  │               │                 │             │   │
  │             │  │               ▼                 ▼             │   │
  │             │  │              ┌───────────────┐──────┐         │   │
  │             │  │  Intent      │Intent Resolver│      │         │   │
  │             │  └────────────► └───────────────┘      │         │   │
  │             └─────────────────│                      │         │   │
  │                   Ack         │                      │         │   │
  │                               └──────────────────────┘         │   │
  │                                             │                  │   │
  │                                             │ Intent           │   │
  │                                             │                  │   │
  │                   ┌─────────────────────────│──────────────────│───│──┐
  │                   │ Loop Logic│             │                  │   │  │
  │                   │───────────┘             │                  │   │  │
  │                   │                         ▼                  │   │  │
  │                   │           ┌───────────┐───────────────┐    │   │  │
  │                   │           │ Intent Bus│               │    │   │  │
  │                   │           └───────────┘               │    │   │  │
  │                   │           │                           │    │   │  │
  │                   │           │                           │    │   │  │
  │                   │           └───────────────────────────┘    │   │  │
  │                   │                        │                   │   │  │
  │                   │                        │ Intent            │   │  │
  │                   │                        │                   │   │  │
  │                   │                        ▼                   │   │  │
  │                   │     ┌──────┐──────────────────────────┐    │   │  │
  │                   │     │ Logic│                          │    │   │  │
  │                   │     └──────┘                          │◄───┘   │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     │                                 │        │  │
  │                   │     └─────────────────────────────────┘        │  │
  │                   │                                                │  │
  │                   │             ┌──────────────────────────────────┤  │
  │                   │             │                               ┌──┘  │
  │                   │             ▼                               ▼     │
  │                   │   ┌─────────────┐────┐     ┌──────────┐────────┐  │
  │                   │   │ Configurator│    │     │ Render   │        │  │
  │                   │   └─────────────┘    │     └──────────┘        │  │
  │                   │   │                  │     │                   │  │
  │                   │   │                  │     │                   │  │
  │                   │   └──────────────────┘     └───────────────────┘  │
  │                   │                                                   │
  │                   └───────────────────────────────────────────────────┘
  │                                            │
  │                                            │
  │                                            │ Signal(Config,Render)
  │                                            │
  │                                            ▼
  │                   ┌───────┐───────────────────────────────────────────┐
  │                   │ Board │                                           │
  │                   └───────┘                                           │
  │                   │                                                   │
  │                   │       ┌──────────────┐────────────────────┐       │
  │                   │       │ Configuration│                    │       │
  │                   │       └──────────────┘                    │       │
  │                   │       │                                   │       │
  │                   │       │                                   │       │
  │                   │       └───────────────────────────────────┘       │
  │                   │                        │                          │
  │                   │                        │                          │
  │                   │                        ▼                          │
  │                   │       ┌───────────────────────────────────┐       │
  │                   │       │ Device│                           │       │
  └───────────────────│       │───────┘                           │       │
                      │       │                                   │       │
                      │       │                                   │       │
                      │       └───────────────────────────────────┘       │
                      │                                                   │
                      └───────────────────────────────────────────────────┘

```

## Status

Column reference:

| Column   | Description             | Possible Values                          |
| -------- | ----------------------- | ---------------------------------------- |
| Priority | Priority level          | 🔴 P0 / 🟠 P1 / 🟡 P2 / 🔵 P3            |
| Status   | Feature completeness    | ✅ Complete / ⚠️ Defective (stub/incomplete) / ❌ Not implemented |
| Test     | Automated tests exist  | ✅ Yes / ❌ No / — N/A                    |
| Item     | Feature name            | —                                        |
| Desc     | Brief description       | —                                        |
| Link     | File path or Issue link | (TBD)                                    |

Priority reference:

| Level | Meaning                                       | Examples                                        |
| ----- | --------------------------------------------- | ----------------------------------------------- |
| 🔴 P0 | Infrastructure / severe bug affecting runtime | Core-path gaps, auth & security, race defects   |
| 🟠 P1 | Required feature                              | Core product capabilities needed before release |
| 🟡 P2 | Enhancement feature                           | Polish, admin UI, optional protocol support     |
| 🔵 P3 | Optional optimization                         | Experience polish, power tuning, non-blocking   |

> Priorities are assigned by humans; agents only record them and must ask a human when unclear, never infer.

### Register

| Priority | Status | Test | Item           | Description                                          | Link |
| -------- | ------ | ---- | -------------- | -------------------------------------------------- | ---- |
| 🟡 P2    | ❌     | ❌   | Built-in root  | Created by migration only; no runtime registration endpoint |      |

### Auth

#### OTA Authentication

| Priority | Status | Test | Item          | Description                                                        | Link |
| -------- | ------ | ---- | ------------- | ------------------------------------------------------------------- | ---- |
| 🟠 P1    | ✅     | ✅   | OTA v1        | Basic device authentication, returns ws_url+token (real JWT when activated, empty otherwise) |      |
| 🟠 P1    | ❌     | ❌   | Firmware distribution | OTA returns version: "0.0.1" + url: null; no actual firmware download link |      |
| 🟡 P2    | ❌     | ❌   | MQTT support  | OTA returns mqtt: null; MQTT protocol not implemented               |      |
| 🟠 P1    | ❌     | ❌   | OTA v2        | Full authentication + device info reporting + firmware distribution (currently returns static data) |      |

#### Nostr Authentication

| Priority | Status | Test | Item              | Description   | Link |
| -------- | ------ | ---- | ----------------- | ------------- | ---- |
| 🟡 P2    | ❌     | ❌   | NIP-98 authentication | Not implemented |      |

#### WS Authentication

| Priority | Status | Test | Item                 | Description                                                    | Link |
| -------- | ------ | ---- | -------------------- | --------------------------------------------------------------- | ---- |
| 🔴 P0    | ✅     | ✅   | Bearer Token validation | Authorization header + query param fallback, JWT decode validation |      |

### Activate & Bind

#### OTA Activation

| Priority | Status | Test | Item   | Description                                 | Link |
| -------- | ------ | ---- | ------ | ------------------------------------------- | ---- |
| 🟠 P1    | ✅     | ✅   | OTA v1 | Activation code verification + device info persistence |      |
| 🟠 P1    | ❌     | ❌   | OTA v2 | Full activation flow (incl. challenge code verification), not implemented |      |

#### Nostr Binding

| Priority | Status | Test | Item                     | Description   | Link |
| -------- | ------ | ---- | ------------------------ | ------------- | ---- |
| 🟡 P2    | ❌     | ❌   | Binding/role registration | Not implemented |      |

### Protocol Translator

#### XiaoZhi

| Priority | Status | Test | Item                      | Description                              | Link |
| -------- | ------ | ---- | ------------------------- | ----------------------------------------- | ---- |
| 🔴 P0    | ✅     | ❌   | Input frame parsing       | hello/listen/abort/mcp/voice fully parsed |      |
| 🔴 P0    | ✅     | ❌   | Output frame serialization | STT/LLM/TTS/Audio/Error fully serialized |      |
| 🟠 P1    | ❌     | ❌   | Missing message types     | system/alert/wake_word not implemented    |      |

### Input Filter

#### Recorder (Input)

| Priority | Status | Test | Item               | Description                         | Link |
| -------- | ------ | ---- | ------------------ | ----------------------------------- | ---- |
| 🟡 P2    | ✅     | ✅   | Input persistence  | Frame data persisted to database    |      |

#### McpRouter

| Priority | Status | Test | Item             | Description                | Link |
| -------- | ------ | ---- | ---------------- | -------------------------- | ---- |
| 🟠 P1    | ✅     | ✅   | MCP frame routing | Intercept Frame::Mcp → McpSession |      |

### Session

#### Listener

| Priority | Status | Test | Item                      | Description                                      | Link |
| -------- | ------ | ---- | ------------------------- | ------------------------------------------------ | ---- |
| 🔴 P0    | ✅     | ✅   | VAD (Silero)              | Earshot implementation, frame-level voice detection |      |
| 🔴 P0    | ✅     | ✅   | ASR (sherpa-onnx)         | XAsr model, 16kHz mono                           |      |
| 🔴 P0    | ✅     | ✅   | Voice Break detection     | Silence timeout triggers TurnComplete            |      |
| 🟠 P1    | ✅     | ✅   | Continued Conversation    | auto/realtime modes, auto re-enter voice listening after TTS |      |
| 🟡 P2    | ❌     | ❌   | Semantic VAD / layered turn | Layer1 Silero only; missing punctuation/semantic layer |      |

#### Round

| Priority | Status | Test | Item                  | Description                                      | Link |
| -------- | ------ | ---- | --------------------- | ------------------------------------------------ | ---- |
| 🟠 P1    | ✅     | ✅   | Shadow/Running dual rounds | Pre-created shadow round reduces switch latency |      |
| 🟠 P1    | ✅     | ✅   | Barge-in + lockout    | Configurable lockout window (default 250ms)      |      |
| 🟠 P1    | ✅     | ✅   | Epoch message filtering | Stale round messages dropped; keeps TTS status notifications |      |
| 🔴 P0    | ✅     | —    | stop_round race       | Fixed; 5s timeout fallback                       |      |
| 🟠 P1    | ✅     | ✅   | AudioPacer            | Throttles audio output by output_frame_duration  |      |

#### LingCore

| Priority | Status | Test | Item                 | Description                                   | Link |
| -------- | ------ | ---- | -------------------- | --------------------------------------------- | ---- |
| 🔴 P0    | ✅     | ✅   | Qwen3 LLM (candle)   | Streaming generation; thread safety fixed     |      |
| 🟠 P1    | ✅     | ✅   | Splitter (sentencex) | Chinese/English sentence splitting; 700ms timeout fallback |      |
| 🟠 P1    | ✅     | ✅   | ToolCall loop        | LLM → MCP tool → result injection → LLM regeneration |      |
| 🟠 P1    | ✅     | ✅   | History management   | Context truncation (max_prompt_len) and persistence |      |
| 🟡 P2    | ⚠️     | ❌   | Emotion detection    | Always returns "happy"; no real analysis       |      |
| 🟡 P2    | ❌     | ❌   | Personalized memory  | Long-term preferences/user profile storage not implemented |      |

#### TTS

| Priority | Status | Test | Item             | Description                              | Link |
| -------- | ------ | ---- | ---------------- | ---------------------------------------- | ---- |
| 🔴 P0    | ✅     | ✅   | MatchaTTS + Opus | sherpa-onnx engine, Opus-encoded output  |      |
| 🟠 P1    | ✅     | ✅   | Two-stage streaming | token-level fast first packet + sentence-level steady state |      |
| 🟡 P2    | ✅     | ❌   | TTFA/RTF instrumentation | Time-to-first-audio/real-time-factor performance monitoring |      |
| 🔵 P3    | ✅     | ❌   | Fade-out         | Tail fade-out eliminates click           |      |

### McpSession

#### McpRegistry

| Priority | Status | Test | Item                      | Description                              | Link |
| -------- | ------ | ---- | ------------------------- | ---------------------------------------- | ---- |
| 🟠 P1    | ✅     | ✅   | Tool registration/aggregation | Multiple client tools merged, routed by name |      |

#### Device Mcp Client

| Priority | Status | Test | Item         | Description                        | Link |
| -------- | ------ | ---- | ------------ | ---------------------------------- | ---- |
| 🟠 P1    | ✅     | ✅   | rmcp transport layer | Device MCP protocol bridging, full handshake |      |

#### External Mcp Client

| Priority | Status | Test | Item           | Description                              | Link |
| -------- | ------ | ---- | -------------- | ---------------------------------------- | ---- |
| 🟡 P2    | ✅     | ✅   | HTTP remote MCP | Connect to external MCP servers, tool list cached |      |

#### MCP Authentication

| Priority | Status | Test | Item                 | Description                                | Link |
| -------- | ------ | ---- | -------------------- | ------------------------------------------ | ---- |
| 🔴 P0    | ✅     | ✅   | /mcp endpoint authentication | Bearer JWT auth + rmcp default Host validation |      |

### Output Filter

#### Recorder (Output)

| Priority | Status | Test | Item              | Description                       | Link |
| -------- | ------ | ---- | ----------------- | --------------------------------- | ---- |
| 🟡 P2    | ✅     | ✅   | Output persistence | Frame/round data persisted to database |      |

### Restful

| Priority | Status | Test | Item                            | Description                                                             | Link |
| -------- | ------ | ---- | ------------------------------- | ----------------------------------------------------------------------- | ---- |
| 🔴 P0    | ✅     | ✅   | POST /api/auth/login            | Account/password auth, returns access/refresh tokens                    |      |
| 🔴 P0    | ✅     | ✅   | POST /api/auth/access_token     | Refresh token exchange                                                  |      |
| 🟠 P1    | ✅     | ✅   | POST /api/auth/reset_password   | JWT auth, old-password verification                                     |      |
| 🟠 P1    | ✅     | ✅   | GET /api/auth/user              | Returns current user info                                               |      |
| 🟡 P2    | ✅     | ❌   | GET /api/stats/summary          | Overview stats (devices/sessions/message count/avg latency)             |      |
| 🟡 P2    | ✅     | ❌   | GET /api/stats/trends           | Trend data (daily device/session/message trends)                        |      |
| 🟡 P2    | ✅     | ❌   | GET /api/stats/latency          | Latency percentile data (P50/P90/P99 etc.)                              |      |
| 🔴 P0    | ✅     | ✅   | Rate Limiting                   | GitHub-style token bucket: auth/ota per-IP fixed window (20/15min, 30/min) + core per-user (1000/h) + X-RateLimit-* response headers + per-account login failure lockout (5/15 min) |      |
| 🔴 P0    | ✅     | ✅   | GET /api/security/rate_limit    | Auth introspection endpoint, returns auth/ota/core quotas & live usage; does not consume quota itself |      |
| 🟡 P2    | ❌     | ❌   | Invite Code system              | Registration invite code generation/validation not implemented          |      |
| 🟠 P1    | ✅     | ❌   | Device management CRUD          | List/activate (by code/by ID)/disable/enable/delete endpoints implemented (missing detail view) |      |

### Admin UI

| Priority | Status | Test | Item                 | Description                                                           | Link |
| -------- | ------ | ---- | -------------------- | --------------------------------------------------------------------- | ---- |
| 🟠 P1    | ✅     | ❌   | Dashboard page       | 6 StatCards, TrendsChart, LatencyChart, RecentSessionsTable, LatencyTable |      |
| 🟠 P1    | ✅     | ✅   | Session list/detail  | Search/date filter/pagination/SessionDetail component                 |      |
| 🟠 P1    | ✅     | ✅   | Security event monitoring | Rate-limit/login outcome events persisted + admin query (type/IP filter + pagination) + 30-day auto-cleanup |      |
| 🟡 P2    | ❌     | ❌   | User CRUD management UI | User list/create/delete/role management not implemented               |      |
| 🟡 P2    | ❌     | ❌   | RBAC management      | Token usage/conversation duration/device activity/permission roles not implemented |      |
| 🟡 P2    | ❌     | ❌   | MCP dashboard        | Multi-endpoint management/tool sync/access control/logs not implemented |      |
| 🟡 P2    | ❌     | ❌   | System monitoring    | Server health/connection count/resource usage/error rate not implemented |      |

### Flutter App

| Priority | Status | Test | Item               | Description                                                  | Link                                                 |
| -------- | ------ | ---- | ------------------ | ------------------------------------------------------------ | ---------------------------------------------------- |
| 🟠 P1    | ❌     | ❌   | Shared client protocol | device identity integration (isomorphic with IoT), WS voice + telemetry + MCP tool surface | [Spec](../../development/clients/shared-protocol.md) |
| 🟡 P2    | ❌     | ❌   | USB peripheral telemetry | Android first (OTG/USB-Host); iOS voice + screen only        |                                                      |
| 🟡 P2    | ❌     | ❌   | CI/CD              | iOS/Android build/sign/release pipeline not implemented      |                                                      |

### IoT

| Priority | Status | Test | Item                         | Description                                                            | Link                                                 |
| -------- | ------ | ---- | ---------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------- |
| 🔴 P0    | ✅     | ✅   | apps/iot skeleton            | core/chip/bsp/app four layers; esp32c6 no_std (riscv32imac, no espup/ESP-IDF needed) |                                                      |
| 🔴 P0    | ⚠️     | ✅   | core peripheral/capability modules | Board capability traits (HasLight/HasInput) composition tested; base (network/config/HTTP/OTA) pending P1 |                                                      |
| 🔴 P0    | ✅     | ✅   | LED app                      | WS2812 RGB LED (GPIO8) + RMT driver; declarative rendering + breathing animation |                                                      |
| 🔴 P0    | ✅     | ✅   | Button app                   | BootButton (GPIO9) debounce → Intent → intent bus → state dispatch → render diff |                                                      |
| 🔴 P0    | ✅     | —    | Build/flash tasks            | moon check/test/build/flash, artifacts .elf/.bin; espflash in devShell |                                                      |
| 🔴 P0    | ✅     | —    | CI integration               | moon ci builds firmware and uploads artifact                            |                                                      |
| 🔴 P0    | ⚠️     | —    | CD release                   | iot release workflow (tag `vanling-iot@x.y.z`) publishes; end-to-end not yet tested |                                                      |
| 🟠 P1    | ❌     | ❌   | On-device config             | softAP config webpage + peripheral capability enable/disable + config persistence |                                                      |
| 🟠 P1    | ❌     | ❌   | Open input (Other Input) + Ack | JSON/Server commands into intent bus + command Ack (see IOT diagram)   |                                                      |
| 🟠 P1    | ❌     | ❌   | Telemetry upload ① persistence | HTTP short transactions + device_key, persisted into server store      |                                                      |
| 🟠 P1    | ❌     | ❌   | Screen display               | Paged rendering peripherals (home/config/ota local pages; chat voice-driven) | [Spec](../../development/clients/shared-protocol.md) |
| 🟠 P1    | ❌     | ❌   | Server capability negotiation | hello capability set → dynamic pipeline assembly + hot reload           |                                                      |
| 🟠 P1    | ❌     | ❌   | Telemetry session read ②     | MCP home_state + device MCP live direct queries                         |                                                      |
| 🟠 P1    | ❌     | ❌   | Status query (Reporter)      | Query/Response read-only State Manager snapshot (see IOT diagram)       |                                                      |
| 🟠 P1    | ❌     | ❌   | Server auth tightening       | WS accepts device JWT only; telemetry attributed per device             |                                                      |
| 🟠 P1    | ❌     | ❌   | Server IoT API               | Telemetry ingestion + device management                                 |                                                      |
| 🟠 P1    | ⚠️     | ✅   | Business composition model   | bin board manifest move-injects capabilities; product tier = feature alias; criteria in features.md |                                                      |
| 🟠 P1    | ❌     | ❌   | Multi-chip port base         | Family split crates `iot-chip-<f>`/`iot-bsp-<f>` + main cfg family entry + toolchain/CI matrix |                                                      |
| 🟡 P2    | ❌     | ❌   | ESP32-S3 (Szpai) board support | xtensa-esp32s3 target + Szpai szpi-bsp + moon dual-board task + display-HasLight |                                                      |
| 🟠 P1    | ❌     | ❌   | First non-esp family migration | `-stm32` variant: app family alias + entry block + thumbv7em + firmware scoping |                                                      |
| 🟠 P1    | ❌     | ❌   | Voice assistant peripheral  | Opus codec + WS voice client + wake word                                |                                                      |
| 🔴 P0    | ✅     | ✅   | Hardware-free regression smoke | Host std harness (intent → state → render behavior assertions) + esp-emu C6 silicon boot smoke, mandatory in CI | [Emulation & smoke](../../development/iot/emulation.en.md) |
| 🟠 P1    | ❌     | ❌   | On-device validation/regression | espflash flashing + real-device smoke test (boot log/LED/button)        |                                                      |
| 🟠 P1    | ❌     | —    | Firmware web installer       | `docs/static/flasher/` static single page (GitHub Pages): upload bin → client-side parse (magic/ChipID/flash params/CRC/SHA256) → esptool-js WebSerial flash merged.bin to 0x0; real-chip comparison guards against wrong flashing; Chrome/Edge only, with GUI/esptool fallback |                                                      |
| 🟡 P2    | ❌     | ❌   | OTA firmware upgrade          | Version check/download verification/dual-partition rollback             |                                                      |
| 🟡 P2    | ❌     | ❌   | MQTT extended telemetry       | rumqttd broker + QoS/LWT online status + transport negotiation (prefer MQTT, fallback HTTP) |                                                      |
| 🟡 P2    | ❌     | ❌   | Reconnect/buffering           | Retransmission and message buffering                                    |                                                      |
| 🔵 P3    | ❌     | ❌   | SNTP time sync                | Trustworthy data timestamps                                             |                                                      |
| 🔵 P3    | ❌     | ❌   | Low-power/deep sleep          | Long standby power optimization, after model selection converges        |                                                      |
| 🔵 P3    | ❌     | ❌   | Watchdog/reset strategy       | Task watchdog + panic reset recovery strategy                           |                                                      |

### Infrastructure

| Priority | Status | Test | Item                          | Description                                                         | Link |
| -------- | ------ | ---- | ----------------------------- | ------------------------------------------------------------------- | ---- |
| 🟠 P1    | ✅     | ✅   | email UNIQUE constraint       | UNIQUE constraint added to user.email migration (nullable, adapts to existing empty email records) |      |
| 🔴 P0    | ✅     | ✅   | Graceful shutdown ordering    | Server CancellationToken drives axum/WS/Matrix orderly graceful shutdown |      |
| 🔴 P0    | ✅     | —    | Runtime race                  | build() runs synchronously; set() happens-before any worker thread; race-free |      |
| 🟠 P1    | ✅     | ✅   | Automatic timestamp fill      | before_save added to Config entities; create/update timestamps auto-filled |      |
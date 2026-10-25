+++
title = "Core Architecture"
weight = 200
[extra]
source_file_hash = "7838974fc9e1565ef13144535fe586737f515c31"
translated_at = "2026-09-09T00:00:00Z"
+++

# Core Architecture

## Session Overview

vanling server manages conversations using the **Session + Round** model:

- **Session**: The lifecycle of a single WebSocket connection. Manages connection, auth, state transitions.
- **Round**: Each turn of conversation (user speaks → server responds). A Session contains multiple Rounds.

The Session is defined in `service/src/session/mod.rs`, and is not tied to a specific transport protocol (WebSocket / Matrix, etc.) — it communicates via the `Frame` enum.

## Data Flow

```
Client → WebSocket
            ↓ (Message)
         ProtocolTranslator
            ↓ (Frame)
         InputFilters [McpRouterFilter, RecorderInputFilter]
            ↓ (Frame)
         Session (state machine)
            ↓ (OutputMessage)
         OutputFilters [RecorderOutputFilter]
            ↓ (OutputMessage)
         ProtocolTranslator
            ↓ (Message)
         WebSocket → Client
```

- **ProtocolTranslator**: `api/src/ws/protocol_translator.rs`, converts between WebSocket Messages and internal Frames.
- **InputFilters**: Pre-process inbound Frames, e.g., intercepting MCP messages for DeviceMcpSession routing.
- **OutputFilters**: Post-process outbound OutputMessages, e.g., recording.
- **Session**: Core state machine that processes Frames and produces OutputMessages.

## Session State Machine

The Session has three phases (`Phase`):

```
Idle → Listening ⇄ Speaking
             ↑       │
             └─ BargeIn ┘
```

### Idle

Initial state, waits for `Hello` from client.

- Receives `Frame::Hello` → replies with Hello response (`session_id`; `audio_params` decided by the `capabilities()` look-up (`downcast_ref::<AudioSpec>()`) at build time), transitions to **Listening**
- Automatically creates first Shadow Round

### Listening

Listening for input.

- `Frame::Voice { data }` → forwarded as `PipelineEvent::AudioFrame` to the chain head of the Shadow Round (VAD detects speech boundaries inside the chain)
- `Frame::Input { text, mode }` → text input (`mode=Wake` marks wake context), feeds `TurnText`; Shadow Round upgrades to Running, transitions to **Speaking**
- `Frame::ListenStart` → if a Running Round exists, BargeIn interrupts it; updates listen parameters (barge-in / voice-break detection)
- `Frame::ListenStop` → feeds `FinishTurn` to the chain; empty input (no valid speech) is classified by the Session into an `EmptyKind` and graded — criteria and prompt levels live in [Dialogue Flow](@/development/server/dialogue-flow.en.md)

### Speaking

Expression phase: on TurnComplete the Shadow Round upgrades to Running and auto-synthesizes TTS with streaming output, while a new Shadow Round is created for continued listening.

- Returns to **Listening** automatically when the expression completes

## Round Lifecycle

Rounds are implemented in `service/src/session/round.rs`, managing LLM inference and TTS synthesis for a single dialogue turn.

### Dual Round Model

A Session maintains two Rounds simultaneously:

```
Shadow Round        Running Round
    │                   │
    │ (preparing)       │ (executing)
    │                   │
    └── upgrade ──────→ │ (new replaces old)
```

- **Shadow Round**: Pre-created when a new request arrives, waiting to be upgraded to Running Round
- **Running Round**: The currently executing Round (LLM + TTS pipeline)
- **Upgrade**: When Shadow Round is ready, it upgrades to Running Round; the old Running Round is cancelled

This design allows seamless BargeIn handling — new requests start preparing immediately without blocking current processing.

### Round Internal Flow

A Round holds one node chain (`opus→vad→asr→turn→ling→tts`) and, as the **inner observer**, subscribes to the chain's broadcast to consume barge-in (with lockout), per-sentence `AudioOut` forwarding, the TTS state machine and timeouts uniformly. The Session is the **outer observer** and only makes lifecycle decisions (shadow→running upgrade / phase / interruption); it never polls the chain. Event names, the split of duties between the two observer layers, and the in-chain boundaries live in [Unified Pipeline](@/development/server/pipeline.en.md).

## LingCore

LingCore (`api/src/component/ling/mod.rs`) implements the `Ling` trait and is the LLM + MCP + history orchestration layer, producing per-sentence `TextChunk`s:

```
LingCore
  ├── model          (Arc<dyn Llm>: Qwen3 / Echo)
  ├── history        (message history / truncation)
  ├── mcp_registry   (MCP tool aggregation)
  └── splitter       (LLM output → sentence → TTS)
```

Pipeline:

1. LLM streaming response → if it returns a ToolCall → mcp tool call → result fed back to LLM
2. Text is split by `Sentence`, emitted per sentence as `TextChunk { text, emotion }` → TTS node

## Turn Termination

Turn termination is decided by two gates: **silence confirm** (`AsrNode`, counting consumed audio samples, 200 ms) and **transport stall** (Session, wall clock, 1200 ms without new audio). Both apply only when `streaming=true` (`auto`/`realtime`); push-to-talk (`manual`) honours only the device's `listen(stop)` and no timeout at all. Criteria, threshold provenance and "why this layer uses this clock" live in [VAD and Turn Detection](@/development/server/vad.en.md).

Interruption is the third path; see "Interruption (BargeIn)" below.

## Input and Output Filters

Inbound `Frame`s and outbound `OutputMessage`s each pass a filter chain between `ProtocolTranslator` and the `Session`; the chains do cross-cutting work only and contain no business judgement:

| Layer | trait | Built-in implementations |
| --- | --- | --- |
| Inbound | `InputFilter::process(&FilterCtx, Frame)` | `McpRouterFilter` (intercepts client MCP messages, routes them to `DeviceMcpSession`), `RecorderInputFilter` (records inbound frames) |
| Outbound | `OutputFilter::process(&FilterCtx, OutputMessage)` | `RecorderOutputFilter` (records outbound frames) |

`process` returns `FilterAction::Continue` (pass), `Consumed` (swallow) or `Break` (abort the pipeline). Adding a filter never requires touching the Session.

## SessionBuilder

```rust
SessionBuilder::new()
    .with_id(session_id)
    .with_node_templates(templates)   // Vec<Arc<dyn Node>>: raw prototypes, the caller assembles the chain
    .with_config(session_config)
    .build()  // → SessionContext { session, input_tx, output_rx }
```

`build()` looks up the downlink audio capability from the templates' `capabilities()`; without a TTS node the handshake does not declare `audio_params`. The capability protocol, template→per-round instance chain building and node lifecycle live in [Unified Pipeline](@/development/server/pipeline.en.md).

## Startup Sequence

```
main.rs
  → run()
    → Server::new(args)           // Load config, init logging
    → async_main(&server)
      → api::start(StartParams)
        → Jwt::init()             // JWT validation setup
        → Database connection + migration
        → TtsManager::init()      // OnceLock singleton
        → VadManager::init()
        → AsrManager::init()
        → LlmManager::init()
        → create_router()         // Register HTTP routes
        → axum::serve()           // Start HTTP server
        → (optional) Matrix Client
```

### Route Structure

| Path | Module | Description |
|------|--------|-------------|
| `/vanling/{version}` | `ws/` | WebSocket endpoint (Xiaozhi protocol) |
| `/mcp` | `mcp/` | MCP Streamable HTTP service |
| `/api/auth/*` | `auth/` | Login / Token refresh |
| `/api/ota*` | `ota/` | OTA protocol (device registration, activation verification) |
| `/api/devices/*` | `device/` | Device management (list/activate/disable/delete) |
| `/api/record/*` | `record/` | Session recording queries |
| `/docs` | — | OpenAPI Scalar UI |

### AI Manager Pattern

All AI modules use the **Manager + OnceLock** singleton pattern:

```rust
TtsManager (OnceLock)
  ├── init(config) → create_model() → stores in INSTANCE
  ├── default() → Arc<dyn Tts>
  └── global() → &'static TtsManager
```

Model variants are selected via config and instantiated at startup.

## Session Activity Timeout

Sessions implement no-activity timeout via idle timestamp tracking in the main loop:

- `idle_since: Option<Instant>` records when the session became idle; reset to `None` on activity
- `close_connection_no_activity_time` (default 30s): no-activity disconnect timeout
- `silence_voice_timeout` (default 1200ms): transport-stall detection (long time without audio while speaking → `FinishTurn` forces a wrap-up)

## Interruption (BargeIn)

Users can interrupt TTS playback:

1. Client sends `Abort` frame or new `ListenStart`
2. Session receives it, calls `stop_round(RoundStopReason::BargeIn)`
3. Current Running Round is cancelled
4. Epoch is incremented, stale OutputMessages are discarded
5. New Shadow Round upgrades to Running Round

## Logging and Observability

### Output Format

Logs use a structured output format:

```
2026-07-20T08:58:12.870289Z DEBUG [<SESSION> asr result] component="session" event="asr_result" session_id=... text=Yeah.
```

- The `[<COMPONENT> message]` bracket pair contains human-readable text
- After the brackets, `key=value` structured fields provide machine-parseable data
- Component names are uppercase: `SESSION`, `VAD`, `ROUND`, `LISTENER`, `ASR`, `MCP`, `WS`

### Structured Field Convention

| Field | Purpose | Example |
|-------|---------|---------|
| `component` | Component name | `session`, `vad`, `ws` |
| `event` | Event name | `asr_result`, `voice_received`, `round_upgraded` |
| `session_id` | Session ID | — |
| `reason` | Reason | `timeout`, `barge_in` |

### Output Targets

- **Console** (text/compact): `[<COMPONENT> msg]` format + `FmtSpan::NONE`
- **File** (JSON): Full structured logging + `FmtSpan::CLOSE`
- **Pretty** / **Json**: No bracket prefix

### Log Level Guidelines

| Level | Usage |
|-------|-------|
| `error!` | Unrecoverable errors, connection drops, ASR/TTS failures |
| `warn!` | Recoverable anomalies |
| `info!` | Key lifecycle events (session start/end, round upgrade) |
| `debug!` | Detailed events (ASR results, VAD state changes) |
| `trace!` | Raw frame data, debugging details |

`println!()` / `eprintln!()` are prohibited.

## Adding a Module

- **Rust business logic** (layers: see "Startup Flow" and "Route Structure" above): `migration/` table → `entity/` definition → `api/` handler → `service/` business logic → register routes via `create_routes(state)` → test
- **AI model** (pattern: see "AI Manager Pattern" above): implement the Trait → config enum variant → manifest → register in `model/mod.rs` → Manager match arm → test → update example config

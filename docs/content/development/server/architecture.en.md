+++
title = "Core Architecture"
weight = 200
[extra]
source_file_hash = "87da0d6865242832fa74d0204f273eb7309da048"
translated_at = "2026-08-31T00:00:00Z"
+++

# Core Architecture

> **Note**: This is an overview-layer document reflecting the author's mental model of the system. See the [documentation style guide](@/discussions/documentation-style.en.md) for context.

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
- `Frame::ListenStop` → feeds `FinishTurn` to the chain; empty input (no valid speech) is classified by the Session into an `EmptyKind` and graded: manual no-voice injects `Prompt{Manual, count}` to trigger a "didn't catch that" guide; auto spoke-but-empty is `AutoSpoke`, total silence is `Silence`, and continued listening after a reply is `Continuing` (silent, no prompt) — see the "Silence / No-Input Discrimination" subsection in `pipeline-redesign.en.md`

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

A Round owns a single node chain and, as the inner observer, subscribes to the broadcast to consume it uniformly:

```
Session │
        ↓ (RoundEvent: SpeechStarted / TurnComplete / EmptyTurn / SpokenEnd)
Round owns a NodeChain (opus→vad→asr→turn→ling→tts)
  ├── VAD/ASR produce TurnText → TurnNode closes the turn as TurnComplete → sends STT + notifies Session to upgrade
  ├── Ling text → TTS produces AudioOut (per-sentence forwarding: SentenceStart / Audio / SentenceEnd)
  └── Inner broadcast handles uniformly: TTS state machine / barge-in (with lockout) / timeout / tail Err
        ↓ (OutputMessage)
     Client
```

A Round outputs via `OutputMessage`; the Session only makes lifecycle decisions (shadow→running upgrade / phase / interruption) and no longer polls the chain.

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

## Turn-End Detection: Silence Confirm + Transport Stall

Turn-end detection lives in **AsrNode (audio-time silence confirm)** and **Session (wall-clock transport stall)**.
Both apply only when `streaming=true` (`auto`/`realtime`); **manual push-to-talk (`ListenMode{streaming:false}`) never answers early** — it pre-decodes the stream frame-by-frame while the button is held but emits nothing, and commits the recognition only when the device sends `listen(stop)` (which feeds `FinishTurn`). See [`dialogue-flow.en.md`](@/development/server/dialogue-flow.en.md).

### 1. Audio-Time Silence Confirm (AsrNode)

`AsrNode` (`service/src/pipeline/nodes/asr_node.rs`) measures the silence duration using **consumed audio sample count** instead of `Local::now()` wall clock:

- `spoken: bool` — whether VAD has ever detected speech
- `silence_samples: u64` — silent audio sample count accumulated after speech ends, zeroed on each speech frame
- `SILENCE_CONFIRM_MS` (200ms) — `streaming && spoken && !speech_active && silence_samples >= threshold * sample_rate / 1000` → `finish()` the current round immediately
- `streaming: bool` — toggled by `ListenMode{streaming}` (injected on `listen(start)`). When `false` (manual), the node **still feeds the stream frame-by-frame to pre-decode** (`accept_waveform` + `decode`) but suppresses `PartialTranscript` emission and silence confirm; `FinishTurn` runs the same `finish_stream()`, only draining tail frames for a near-instant result — decoding cost is spread across the hold instead of a cold whole-clip decode after stop

Three finish outcomes: valid input → `TurnText`; stream but empty text → `EmptyInput` (triggers a prompt); no stream → `Nothing`.
Manual reuses the same three outcomes; with no stream (VAD never detected speech) `finish_stream` returns `Nothing`, letting the Session's empty-input logic (`EmptyKind::Manual`) be the single driver of the prompt — avoiding a double-trigger with the in-chain `EmptyInput`.

### 2. Transport Stall Detection (Session)

When the client stops sending audio, `silence_samples` cannot grow, so a wall-clock fallback is needed: `check_transport_stall()` (`session/mod.rs`) feeds `FinishTurn` to the chain to force a wrap-up when `spoken` is still active and `last_audio_received.elapsed() >= silence_voice_timeout` (1200ms).
**Manual mode skips this check** (returns early when Listening and `is_voice_break_detect == false`) — the device's `stop` is the sole turn boundary, so the server never "jumps in" on silence timeout.

**Why not wall clock for silence confirm**: In tests and CI, audio can be injected instantaneously (no real-time pacing), decoupling wall clock from audio stream time, which caused premature turn finishing on slow machines under fast injection. Audio-sample counting makes the trigger depend only on decoded content, independent of consumption speed.

**Why wall clock for transport stall**: Audio-time counting can only tally samples that actually arrive. When the client stops sending audio entirely, there are no samples to count, so a wall-clock fallback is necessary to detect the stalled transport.

This matches the OpenAI Realtime (`silence_duration_ms` audio-level + `idle_timeout_ms` wall-clock-level) layered pattern.

## Input and Output Filters

### InputFilter trait

```rust
#[async_trait]
trait InputFilter: Send + Sync {
    async fn process(&self, ctx: &FilterCtx, frame: Frame) -> FilterAction<Frame>;
}
```

Returns `FilterAction::Continue(frame)` to continue, `FilterAction::Consumed` to intercept, or `FilterAction::Break` to abort the pipeline.

Built-in filters:
- **McpRouterFilter**: Intercepts client MCP messages, routes to DeviceMcpSession
- **RecorderInputFilter**: Records inbound frames

### OutputFilter trait

```rust
#[async_trait]
trait OutputFilter: Send + Sync {
    async fn process(&self, ctx: &FilterCtx, msg: OutputMessage) -> FilterAction<OutputMessage>;
}
```

Built-in filters:
- **RecorderOutputFilter**: Records outbound frames

## SessionBuilder

Sessions are constructed using the Builder pattern, injecting a **raw node prototype collection** (the api site assembles the chain dynamically per config, deciding the chain's stages/order):

```rust
SessionBuilder::new()
    .with_id(session_id)
    .with_node_templates(templates)   // Vec<Arc<dyn Node>>: opus→vad→asr→turn→ling→tts
    .with_config(session_config)
    .build()  // returns SessionContext
```

`build()` looks up the downlink audio capability via `capabilities()` (`downcast_ref::<AudioSpec>()`): without a TTS node, the handshake does not declare `audio_params` (no downlink voice capability, no pacer constructed). Removed old parameters: `with_listener` / `with_ling` / `with_tts` / `with_audio_config`.

`SessionContext` contains:
- `session`: The Session instance
- `input_tx`: Channel to send Frames to Session
- `output_rx`: Channel to receive OutputMessages from Session

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

+++
title = "Core Architecture"
weight = 200
[extra]
source_file_hash = "85d87fc42f1c1098fbed5f5855fee12cf2b0ac03"
translated_at = "2026-07-21T00:00:00Z"
+++

# Core Architecture

> **Note**: This is an overview-layer document reflecting the author's mental model of the system. See the [documentation style guide](@/discussions/documentation-style.md) for context.

## Session Overview

chobits server manages conversations using the **Session + Round** model:

- **Session**: The lifecycle of a single WebSocket connection. Manages connection, auth, state transitions.
- **Round**: Each turn of conversation (user speaks → server responds). A Session contains multiple Rounds.

The Session is defined in `service/src/chobits/session/mod.rs`, and is transport-agnostic — it communicates via the `Frame` enum.

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

The Session has four phases (`Phase`):

```
Idle → Ready → Listening → Speaking → Ready → ...
                ↑              │
                └── BargeIn ───┘
```

### Idle

Initial state, waits for `Hello` from client.

- Receives `Frame::Hello` → replies with Hello response (includes `session_id`, `audio_params`), transitions to **Ready**
- Automatically creates first Shadow Round

### Ready

Waiting for user input. Can accept:

- `Frame::ListenStart` → start listening, transitions to **Listening**. If a Running Round exists, BargeIn interrupts it
- `Frame::Input { text }` → text input, upgrades Shadow Round, transitions to **Speaking**
- `Frame::Voice { data }` → audio data, forwards to Listener (VAD processing)

### Listening

Receiving audio data, VAD detects speech boundaries.

- `Frame::Voice { data }` → passes to Listener (VAD determines if speaking)
- `Frame::ListenStop` → stop listening, retrieves ASR result from Listener, upgrades Shadow Round, transitions to **Speaking**
- Silence timeout → triggers the equivalent of ListenStop automatically

### Speaking

LLM inference + TTS synthesis + streaming audio output.

- Sends Command::Chat to Running Round, triggering LLM → TTS pipeline
- Returns to **Ready** automatically when complete

## Round Lifecycle

Rounds are implemented in `service/src/chobits/session/round.rs`, managing LLM inference and TTS synthesis for a single dialogue turn.

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

```
ChatParam → Round
  ├── Chii.ask() → LLM streaming output
  │     ├── LLMResult (text chunks)
  │     └── ToolCall (→ MCP → LLM)
  └── Tts.stream() → audio frames
        ├── TTSResult (state events)
        └── AudioResult (Opus encoded audio)
```

Round runs LLM and TTS in parallel, streaming output through OutputMessages.

## ChiiCore

ChiiCore (`api/src/chii/`) is the LLM + MCP orchestration layer.

```
ChiiCore
  ├── HistoryManager (message history management / truncation)
  ├── LlmClient (Qwen3 / Echo)
  ├── McpRegistry (MCP tool aggregation)
  └── TextSplitter (LLM output → sentence splitting → TTS)
```

Pipeline:

1. User text → HistoryManager builds ChatHistory
2. LlmClient.stream() → LLM streaming response
3. If LLM returns ToolCall → McpRegistry.call_tool() → result fed back to LLM
4. LLM text → TextSplitter → sentence chunks → TTS

## Listener

The Listener (`service/src/chobits/listener.rs` trait, implemented by `api/src/ws/default_listener.rs`) orchestrates VAD + ASR:

1. Receives audio data (`ListenInput::Audio`)
2. VAD detects speech activity (Earshot Silero VAD)
3. Silence timeout triggers ASR transcription (SenseVoice sherpa-onnx)
4. Returns `ListenResult::Text` or `ListenResult::Audio { text, prob }`

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

Sessions are constructed using the Builder pattern:

```rust
SessionBuilder::new()
    .with_id(session_id)
    .with_listener(DefaultListener::new(vad, asr))
    .with_chii(ChiiCoreBuilder::new(llm, mcp_registry).build())
    .with_tts(TtsManager::default())
    .with_config(session_config)
    .with_audio_config(audio_config)
    .build()  // returns SessionContext
```

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
| `/chobits/{version}` | `ws/` | WebSocket endpoint (Xiaozhi protocol) |
| `/mcp` | `mcp/` | MCP Streamable HTTP service |
| `/api/auth/*` | `auth/` | Login / Token refresh |
| `/api/ota*` | `ota/` | OTA firmware update |
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
- `silence_voice_timeout` (default 1200ms): VAD silence detection

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

+++
title = "Shared Client Protocol"
weight = 302
[extra]
source_file_hash = "5db2247ae798e289dbe5feda2cd4780145914c10"
translated_at = "2026-10-29T00:00:00Z"
+++

# Shared Client Protocol

The Flutter App and the IoT firmware are the **same class of client** of the server: both are the "base + peripheral capabilities" device abstraction, differing only in tech stack (Dart on phones vs Rust embedded). Both implement this single protocol specification, which is the authoritative description of that spec.

- Flutter App: phone device (microphone/speaker/screen + USB peripherals telemetry, Android first)
- IoT firmware: esp32c6 board-level device (on-board sensors/OLED/audio peripherals, combined via Cargo features)

> Server-side protocol and pipeline: see `development/server/` (`websocket-protocol.md`, `architecture.md`, `dialogue-flow.md`).

---

## 1. Client Model

```
client = base (mandatory) + peripheral capabilities (optional features, any combination)
base   = network / config / HTTP short-transaction telemetry + device_key / OTA
peripherals = voice (Opus + WS + wake word) / telemetry (sensor/USB) / display (OLED/LCD/screen) / MCP tool surface
```

- Identity: clients join as **device identity** (OTA/activation flow issues JWT, `token_type = device`), bound to the server `device` entity; telemetry belongs to the device dimension.
- Capability negotiation: `hello` carries the current capability set; the server assembles pipeline nodes accordingly (data-only devices do not get VAD/ASR/LLM/TTS).
- Transport: voice and commands over WS (long-lived, voice/hybrid only); telemetry over HTTP short-transaction (any client, coexists with voice).

## 2. Voice Session (WS)

Both stacks implement the protocol in `development/server/websocket-protocol.md`:

- `hello`: may carry `capabilities` (current peripheral set) and `audio_params` (Opus sample rate / frame duration)
- `listen[start|stop|detect|text]`, `abort`, binary Opus frames uplink/downlink
- Consume `stt` / `llm` (with `emotion`) / `tts` (`sentence_start` carries captions) / `error` text frames
- `mcp`: device MCP tool surface (see §5)

## 3. Session State & Feedback (Attention State Machine)

Session state (attention) is the single fact source driven by the protocol; output channels are its pluggable projections — optional and combinable.

**Scope**: the attention state machine exists only on clients compiled with the `voice` peripheral (voice-enabled). Voice-less devices have no session state machine, only device-local pages + the HTTP data path; this protocol is empty for them.

### Output Channels

| Channel | Description |
| --- | --- |
| `Display` | Renders via pages (below); `chat` page exists on voice-enabled devices only, other pages are device-local |
| `Led` | State color + `mic_live`; streaming audio must be clearly indicated (privacy baseline); idle contrasts with all states |
| `Audio` | Dual form: local earcon cue + TTS prompt; synced with visual cues; screen-less devices should indicate state changes at least with audio |
| `Vib` | Projection of the same state machine (incl. timeout/error alerts); suited to wearables/haptics |
| `Web` | Reuses the provisioning/telemetry HTTP surface: static page + REST state + WS/SSE realtime; serves as the "head" for headless devices |
| `None` | Headless null implementation (a valid channel) |

> `chat`-related states (listening/thinking/speaking) exist on voice-enabled clients only; other states/channels are optional for any device.
> Physical actuation (robot-level machine body language, e.g. QUBI) is a research area, out of scope for this protocol.

### Full Flow

Input channels (`Button`/`Touch`/`Mic`/`Web`; `Sensor` feeds telemetry only, `Motion` is future) → eventization (`listen_*`/`text`/`abort`/`close`) → attention state machine (the single fact source) → dispatched to output channels, closing the loop through output-channel echo (`mic_live`, `listening`). Interaction precedence: user input > TTS > alerts > notifications > media (AVS Interaction Model). `microphone off` is a persistent, independent privacy state above interaction states; the input event set adds no new protocol. Client implementations follow reducer folding (protocol frames ∪ local events); the chat page consumes the projection, not raw frames (page implementation: [`pages`](pages.en.md)).

### Display Rendering Form: Page-based Model

Display is handled **per page**: each page has its own content model and input source, with a single foreground page at any time. `status bar` (network/battery/clock) is a persistent layer outside the stack; `overlay` (notification/alert) is a transient layer. The protocol drives only the `chat` page; all other pages are device-local — cross-client sharing is limited to vocabulary, not rules.

| Page | Content model | Input source | Processing paradigm |
| --- | --- | --- | --- |
| `chat` | Session states + messages | Protocol frames (only, voice-enabled) | frame → state machine → projection |
| `home` | Telemetry snapshot | Local polling / MCP direct read | read → render |
| `config` | Network config / configuration forms | Local events / buttons | traditional UI |
| `ota` | Progress state | Local events | traditional UI |
| `settings` | Settings items | Local events | traditional UI |

### Chat Page (Normative)

`chat` exists on voice-enabled clients only and is the only page driven by server frames. Frames are **input events**, not playback frames — there is no "frame → pixel" direct mapping; the session state machine rebuilds semantics and the renderer draws a projection of the state. In implementation the page **consumes the folded projection, not frames one-by-one** (frame → reducer → projection, see [`pages`](pages.en.md)).

**State set** (single axis, aligned with the session state machine in `websocket-protocol.md`):

| State | Entered by | Payload |
| --- | --- | --- |
| `idle` | disconnect / timeout / session end | — |
| `connecting` | WS establishing, before `hello` | — |
| `listening` | `listen_start` / `detect` | `message { role: user, text, is_interim }` |
| `thinking` | LLM processing this turn | `message { role: assistant, text, is_interim }` + `emotion` |
| `speaking` | `tts.start` | `subtitle { text, speaking }` + `emotion` |

**Transition rules** (minimal set):

- `stt` partial → update the `user` message in place (`is_interim=true`), never append
- `llm` stream → update the `assistant` message in place (`is_interim=true`); `emotion` overridden per frame
- `tts.start` → `speaking`; `tts.sentence_start` → `subtitle {text, speaking=true}`
- `tts.stop` → back to `listening` (realtime) / `idle` (auto/manual)
- `abort` (wake_word_detected) → back to `listening` + clear current `message`/`subtitle` (recompute, don't chase frames)
- `close`/timeout → `idle` + clear
- Idempotency: repeated transition to the same state is a no-op (prevents flicker)
- `speaking` may layer a local media clock for linear caption/audio sync (a playback sub-module), without changing the event-driven main model

**Not in contract**: rendering behavior (scroll/focus/debounce/animation/screen-off) is device-local.

## 4. Telemetry Dual Paths

| Path | Trigger | Channel | Ownership |
| --- | --- | --- | --- |
| ① Persisted collection | Client-initiated, periodic | HTTP short-transaction + `device_key` | Server state store (DB), answerable offline |
| ② Session read | Server-initiated, inside voice session | MCP `home_state` local tool / device MCP live read | In-session ad-hoc answers |

- ①: Flutter (incl. USB peripheral collection) and IoT (on-board sensors) report in exactly the same way.
- ②: the `home_state` tool queries the state store (reads path ① data); device MCP `tools/call` reads sensors on demand.
- Telemetry is **not** carried on the voice Session pipeline, nor does it rely on the WS long-lived connection.

## 5. MCP Tool Surface

`self.*` tools are isomorphic; the LLM does not care whether a tool runs on a phone or a board:

- Flutter: notifications / location / USB peripheral control
- IoT: lights / sensors / OLED / audio

## 6. Common Behavioral Guarantees

- Reconnect: OTA → hello cycle; handle the 30s idle disconnect
- Lockout: no barge-in within 250ms after TTS start (`barge_in_lockout_ms`)
- Telemetry backoff: lower priority during voice-session burst traffic

## 7. Test Unification

Server-side WS end-to-end tests serve as the shared regression baseline for both client implementations; protocol changes require dual-stack synchronization.
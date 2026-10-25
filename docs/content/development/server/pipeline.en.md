+++
title = "Unified Pipeline"
weight = 202

[extra]
source_file_hash = "1a0855fe663dae3d28ea1e2265348b1da7483cce"
translated_at = "2026-10-25T00:00:00Z"
+++

# Unified Pipeline

> The whole chain for one turn: `opus → vad → asr → turn → ling → tts`. Nodes are pure stream transforms; the engine broadcasts at every node boundary, Round consumes the inner broadcast, and Session subscribes only to Round's lifecycle events.
> Why the chain looks like this and what it replaced: [pipeline evolution record](@/records/server/pipeline-evolution.en.md).

## Design constraints

- **One chain per Round**: `opus→vad→asr→turn→ling→tts` end to end; `TurnComplete` flows into `ling→tts` inside the chain.
- **All nodes are homogeneous**: a single `Node::stream(upstream, ctx)` transform; no capability sub-traits, no `InputChain`, no privileged ling/tts pairing.
- **Observer built into the abstraction layer**: `NodeContext` carries the observer registration point (Round injects `EventSink`) plus `session_id` (node log correlation / state attribution); **business nodes are completely unaware**, broadcasting is done by the engine.
- **Engines as references + pooling**: ASR is a shared engine (`create_stream()` is naturally concurrent); VAD is a serial state machine reused through an **object pool** (`clear()` before use).
- **The previous Round keeps running by default**: interrupted only when a new turn must occupy the output channel / cut off ongoing speech / grab the websocket (epoch prevents stale output).

## Roles & Ownership

| Role | Responsibility | Key |
|---|---|---|
| **Session (total orchestrator)** | connection / phase / Round lifecycle (shadow/upgrade/stop) / idle timeout; turns inbound audio into `PipelineEvent` and sends it to the active Round's chain head | does **lifecycle management** only — it drives neither recognition nor output and does not poll the chain; looks up `capabilities()` from templates at build time to drive the handshake declaration |
| **Round (single processor)** | **owns one chain**, auto-injects itself as observer, subscribes to the broadcast; centrally responds to barge-in / turn advance / per-sentence audio forwarding / timeout / errors | one utterance per turn; chain is internal implementation |
| **Ling (decision-core trait)** | receives recognized results → decides → produces expression intents; references LLM / DB / sensors / web | implementations may be LLM/agent · state machine · mixed · passthrough |
| **Node (engine adapter)** | Opus decode / VAD / ASR / turn / LLM / TTS / data acquisition | unified `Node::stream` stream transform, **business nodes unaware of the observer**, pluggable composition; `capabilities()` generalizes session-level capability declaration, plus `release_mode`/`on_acquire`/`on_release` lifecycle |

```
Session (total orchestrator) — manages Round lifecycle; forwards Frame::Voice → active Round chain head; registers as Round's outer observer (RoundEvent)
   Frame::Voice ──> PipelineEvent::AudioFrame ──> Round chain head
                        ↓
   Round (single processor) owns one chain (internal implementation):
     opus → vad → asr → turn ──(TurnComplete flows on in-chain)──> ling → tts
       engine auto-broadcasts at node boundaries (event tagged Before/After)
            │ inner: Round subscribes as the broadcast observer, unified consumption (barge-in detect/TurnComplete/AudioOut per-sentence forward/TTS state machine/tail Err)
            ▼
        Round → RoundEvent channel (SpeechStarted / TurnComplete / EmptyTurn / SpokenEnd)
            │ outer: Session registers as Round's observer → shadow→running upgrade / phase switch / epoch / ListenStop
            ▼
        OutputMessage ──> client
```

## Single Chain + Observer Protocol

### File: `service/src/pipeline/mod.rs`

```rust
pub enum PipelineEvent {
    // Perception
    AudioFrame(Vec<u8>),                   // raw Opus audio, from Frame::Voice
    PcmFrame(Vec<f32>, u32),               // decoded PCM samples + sample rate
    SpeechStarted,                         // VAD rising edge
    SpeechEnded,                           // VAD falling edge
    PartialTranscript(String),
    TurnText { text: String, prob: f32 },  // gateway: one recognition finished (ASR produces)
    TurnComplete { text: String, prob: f32 }, // turn boundary (turn node closes the turn; boundary detection lives in AsrNode + Session)
    Configure(AudioParam),                 // downlink audio params, applied through an in-stream event
    FinishTurn,                            // internal control: request ASR to finish now
    // Expression
    TextChunk { text: String, emotion: Option<String> },
    AudioOut { audio: Vec<Vec<u8>>, is_first: bool, is_last: bool },
}

pub struct NodeContext {
    pub cancel: CancellationToken,
    pub emit: EventSink,          // observer broadcast sender injected by Round
    pub session_id: String,       // session id injected by Round; used by nodes for log correlation / state attribution
}

/// Session-level capability value: any `Send + Sync + 'static` concrete type (wrapped as
/// `Box<NodeCapability>` where `NodeCapability = dyn Any + Send + Sync`) can be declared by a node
/// and looked up by Session via `downcast_ref::<T>()` by type.
pub type NodeCapability = dyn Any + Send + Sync;

pub enum ReleaseMode {
    /// Released as soon as the node's process completes (its stream is consumed to `None`).
    Immediate,
    /// Released at the end of the whole round; for stateful nodes spanning one recognition/expression.
    Deferred,
}

// Unified node protocol — business nodes are unaware pure transforms: read the variants
// they care about from the upstream stream, pass the rest through, may append new events.
// **Prototype**: a Node is both template and instance; it carries `new_instance` to "clone" a fresh
// instance using itself as the template. Session holds a template set and, per Round, has each
// template `new_instance` an independent instance (stateful nodes like Opus/VAD need fresh entities).
pub trait Node: Send + Sync {
    /// Clone: produce a new chain-runnable instance using `self` as the template (per-Round independent).
    fn new_instance(&self) -> Arc<dyn Node>;
    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream;
    /// Downlink config (template-level): Session forwards the transport `Configure` to the template once
    /// at hello; the template updates its own state and later rounds inherit it via `new_instance`.
    /// Default no-op = does not accept configuration.
    fn on_configure(&self, _event: &PipelineEvent) {}
    /// Generalized capability look up: the session-level capabilities this node declares (empty = none).
    /// Session retrieves by type via `downcast_ref`. Adding a capability only boxes a concrete type
    /// into `capabilities()`; it does not change the Node trait.
    fn capabilities(&self) -> Vec<Box<NodeCapability>> { Vec::new() }
    /// Release strategy (framework-enforced): default `Deferred`; Immediate nodes are released by the
    /// framework as soon as their stream is consumed to `None`.
    fn release_mode(&self) -> ReleaseMode { ReleaseMode::Deferred }
    /// Round lifecycle: driven by `NodeChain::begin` at round start (default no-op).
    fn on_acquire(&self) {}
    /// Round lifecycle: Immediate nodes released at the `with_observer` stream end, Deferred nodes at
    /// `NodeChain::finish` (round end).
    fn on_release(&self) {}
}
```

**Observer / broadcast (timing tag)**—attached to the broadcast payload, **not** a `NodeContext` field:

```rust
pub enum TapPoint { Before, After }        // before a node's input / after a node's output
pub struct Tapped { pub point: TapPoint, pub event: PipelineEvent }
```

- `EventSink`: a cloneable broadcast sender (`broadcast::Sender<Tapped>` or unbounded mpsc) injected by Round.
- **Broadcast is done by the engine**: in the `compose`/`NodeChain` driving layer, at every node boundary,
  both `Tapped{Before, ev}` and `Tapped{After, ev}` are sent to `emit`. Business nodes are **unaware** of
  the observer—they only write pure `stream` transforms and never call `ctx.emit`.

**Semantics**:

- **A single `Node` trait**: capabilities are declared through `capabilities()`; no capability sub-traits split the
  node kinds. reset is not public—VAD is per-Round (cleared from the pool before use); reconfigure is two-layered: Session forwards the in-stream
  `Configure` event to the **template** (`Node::on_configure`) at hello, and the template clones its
  configured state into each per-Round instance via `new_instance`.
- `NodeContext` carries `cancel` + `emit` (the observer entry) + `session_id` (injected by Round, used by nodes
  for log correlation / state attribution); no "timing" configuration—Before/After
  are labels on the broadcast payload, applied by the engine, not coupled to the context.
- Errors reuse `AppError` and propagate via `TryStreamExt` short-circuit; no `PipelineEvent::Error` variant.

**Use of the two timings** (complementary):

- **Before (input)**: Round senses **raw input events**—`SpeechStarted` (barge-in interrupt), audio arrival
  (reset idle timer). These are consumed/transformed downstream, so only a Before broadcast lets Round catch
  them in time.
- **After (output)**: Round senses **transformed results**—`TurnComplete` (advance speaking), `AudioOut`
  (per-sentence forwarding), `Err` (timeout / interrupt).

### Downlink Config (Template-level, `Configure` → `Node::on_configure`)

**Semantics**: `Configure(AudioParam)` is a runtime parameter pushed downlink from the transport layer (the
client's hello `audio_params`); it is currently consumed only by `OpusDecodeNode` (decoder sample rate). It is
**not sent per Round**—instead, Session forwards the event to the **template once** at `handle_connect`
(`templates.iter().for_each(|t| t.on_configure(&Configure(params)))`); `OpusDecodeNode` overrides
`on_configure` to update its template state, and `new_instance` **inherits** the configured sample rate from
`self`, so every per-Round instance is born with the right decoder/resampler source rate. The first round still
gets one in-stream `Configure` (because `on_idle` builds the round before `handle_connect`); **no persisted
field, no per-Round re-feed**.

**Sample-rate normalization (opus → 16k)**: `OpusDecodeNode` is responsible for `AudioFrame→PcmFrame` and
**resamples** the decoded PCM to **16000** before emitting `PcmFrame(_, 16000)` (always 16k). This is because
downstream VAD (Earshot) and ASR (sherpa X-ASR) are hardcoded to assume 16k input and there is no other
resampler in the chain; when the client declares a non-16k uplink rate (e.g. 24k) in hello, normalizing to 16k
here means VAD/ASR always receive the correct sample rate. Reuses `rubato` (already an api dependency; the
same `Fft::new` + chunked-processing pattern as `matcha`).

### Capability Look Up (Node → Session Uplink Declaration)

Orthogonal to the **downlink configuration** (`Configure(AudioParam)`, Session→Node pushes runtime
parameters), nodes also provide an **uplink capability declaration**, generalized so it is not exhausted
as per-capability Node methods: any `Send + Sync + 'static` concrete type, wrapped as the type alias
`NodeCapability = dyn Any + Send + Sync` (`Box<NodeCapability>`), can be declared by a node and looked up
by Session via `downcast_ref::<T>()`. The only current capability member is `AudioSpec` (downlink audio format):

```rust
#[derive(Debug, Clone)]
pub struct AudioSpec { pub sample_rate: u32, pub channel: u32, pub frame_duration_ms: u64 }

// Node trait default: capabilities() -> Vec<Box<NodeCapability>> { Vec::new() }
impl Node for TtsNode {
    fn capabilities(&self) -> Vec<Box<NodeCapability>> {
        vec![Box::new(self.tts.audio_spec())]   // any concrete type boxes directly
    }
}
```

- **`Tts::audio_spec() -> AudioSpec`** (service `component/tts.rs`): the engine self-reports its downlink output
  format. `TtsMatcha` returns its own `output_sample_rate/output_channel/output_frame_duration`; `TtsMute`
  (no audio, text-passthrough only) returns the default `(16000, 1, 60)`.
- **Session resolution**: computed once in `build()` by scanning `node_templates`, finding in each
  template's `capabilities()` the first `downcast_ref::<AudioSpec>()` hit via `find_map`, stored as an
  `Option<AudioSpec>` field.
- **Semantics (no fallback)**: no TTS node ⇒ `None`—the handshake reply sends `audio_params: None`
  (an explicit "no downlink voice capability"; the client stops expecting audio), and the pacer is not built
  (`AudioResult` only comes from TTS, so it can never fire).
- **Extensibility**: adding a capability (e.g. an uplink format or a per-session token budget) only requires
  boxing the concrete type into `capabilities()`; the Node trait is unchanged.
- **One config source**: api `config::audio::AudioConfig` (feeding `TtsManager`/`TtsMatcha`); the session side keeps
  no second copy of the audio config.

### Node lifecycle (framework-enforced resource release)

Stateful nodes (VAD / ASR) hold resources for the duration of one recognition round (the VAD state machine
comes from an injected pool; the ASR stream spans feed-frames → finish) and need **lifecycle hooks** driven
uniformly. The release strategy is declared by each node via `release_mode()` and enforced by the framework:

- **Immediate**: the `with_observer` wrapper detects when a leaf's stream is consumed to `None` (process
  complete) and, if that leaf's `release_mode() == Immediate`, immediately calls `leaf.on_release()`.
  Because `compose_chain` wraps each leaf exactly once with `with_observer`, the trigger fires **exactly
  once per leaf**; nested `compose` never double-fires.
- **Deferred**: the resource spans the whole round and is released uniformly by `NodeChain` at round end.

**Driving carrier—`NodeChain` holds the leaf list**: `compose_chain` folds the caller's ordered
`Vec<Arc<dyn Node>>` into a nested composite, swallowing the leaf references. To drive the lifecycle,
`NodeChain::new(head, leaves)` keeps a copy of the leaves:

```rust
// at chain construction in Session
let leaves: Vec<Arc<dyn Node>> = templates.iter().map(|t| t.new_instance()).collect();
let head = compose_chain(leaves.clone()).expect("chain");
let chain = NodeChain::new(head, leaves);
chain.begin();   // calls on_acquire() on every leaf
// at round end
chain.finish();  // calls on_release() only on Deferred leaves (Immediate already released at stream end)
```

| Node | `release_mode` | `on_acquire` | `on_release` |
|---|---|---|---|
| `OpusDecodeNode` | Immediate | — | reset decoder/resampler state |
| `VadNode` | Deferred | `pool.acquire()` stores instance | `take()` then `pool.release(vad)` |
| `AsrNode` | Deferred | reset internal stream/buffer/flags | finish stream if still active |
| `TurnNode` | Immediate | — | — |
| `LingNode` | Immediate | — | — |
| `TtsNode` | Immediate | — | — |

### Single-chain composition

`compose(a, b)` pipes `a.stream`'s output into `b.stream`'s input, returning a new `Node`
(pure pipe, no broadcast). The **unified chain-builder `compose_chain(Vec<Arc<dyn Node>>)`**
wraps each leaf once with `with_observer` (automatic broadcast) then folds with
`reduce(compose)` into a single chain—order and count come from the caller-supplied `Vec`:

```rust
pub fn compose_chain(nodes: Vec<Arc<dyn Node>>) -> Option<Arc<dyn Node>> {
    nodes.into_iter().map(with_observer).reduce(compose)
}
```

The resulting chain shape: `opus → vad → asr → turn → ling → tts`.
Assemblers provide only **bare prototypes** `Vec<Arc<dyn Node>>`; `with_observer` is fully
transparent to them, handled uniformly inside `compose_chain` (each leaf broadcasts exactly once).

## Chain Construction (prototypes + per-Round instances)

- **Three layers only**: the template set → per-Round `new_instance` → `compose_chain`. There is no
  `PipelineFactory` / `NodeFactory` / `NodeDeps` / `ChainBuilder` layer in between.
- **`Node` = prototype**: each node carries `new_instance(&self) -> Arc<dyn Node>` and clones an
  instance using itself as the template. The template holds the shared references it needs
  (`Arc<VadPool>` / `Arc<dyn Asr>` / `Arc<dyn Ling>` / `Arc<dyn Tts>`), and `new_instance` clones out
  a fresh per-Round entity. Nodes do **not** self-store a per-session `session_id`—the session id is
  injected by Round into `NodeContext.session_id`, and nodes only read it from `ctx` for log correlation.
- **Session** holds the bare prototype set `Vec<Arc<dyn Node>>` (`SessionBuilder::with_node_templates`);
  per Round: `templates.iter().map(|t| t.new_instance())` → `compose_chain(...)`.
- **Session looks up the session-level audio capability from templates at build time**: in each template's
  `capabilities()` via `downcast_ref::<AudioSpec>()` (see "Capability Look Up"), driving the handshake
  audio declaration and the pacer; with no TTS node no downlink audio is declared (`audio_params: None`).
- **Callers (api sites ws / matrix / tests)** assemble a bare-prototype `Vec<Arc<dyn Node>>` from config:
  conditional `push` / reordering decides which stages the chain has and in what order.
- One chain per Round = each `new_instance` acquires one new VAD instance from the pool.

## Node Ownership

| Node | File | Implements |
|---|---|---|
| `OpusDecodeNode` | `service/src/pipeline/nodes/opus_node.rs` | `AudioFrame→PcmFrame`; overrides `on_configure` to update template state + `new_instance` inherits; resamples decoded PCM to **16k** before emitting |
| `VadNode` | `service/src/pipeline/nodes/vad_node.rs` | `PcmFrame→{PcmFrame, SpeechStarted, SpeechEnded}`; instance from the VAD pool, released on `Drop` |
| `AsrNode` | `service/src/pipeline/nodes/asr_node.rs` | `→{PartialTranscript, TurnText}`; internal `create_stream()`, shared engine |
| `TurnNode` | `service/src/pipeline/nodes/turn_node.rs` | `TurnText→TurnComplete`; normalizes the ASR stream and direct text input (`TurnText`) paths into one explicit turn-close marker (boundary detection lives in AsrNode silence confirm and Session control events) |
| `LingNode` | `service/src/pipeline/nodes/ling_node.rs` | `TurnComplete\|TurnText→TextChunk` (unfold inner stream; direct text input also goes here) |
| `TtsNode` | `service/src/pipeline/nodes/tts_node.rs` | `TextChunk→{TextChunk, AudioOut}` (unified `Node::stream`); `capabilities()` reports `AudioSpec` (`Tts::audio_spec()`) |

All nodes are **pure transforms**: unaware of observer/broadcast; `TtsNode` balances emotion in **FIFO**
order, encoding errors pass through as `Err`.

## Engine Pooling

- **VAD object pool**: `VadPool { free: Mutex<Vec<Box<dyn Vad>>>, config }`. `acquire()` builds if empty,
  else takes and `clear()`s; `release()` returns it. Guarantees a VAD is held by at most one Round at a time
  (serial state machine; concurrent reuse corrupts `is_speech`).
- **ASR shared engine**: a single `Arc<dyn Asr>`; each Round calls `create_stream()` on demand; it ends
  after `finish()`, so pooling is not worthwhile.

## Round Unified Consumption (Two-Layer Observer)

**Round is a two-layer observer model: "inner wraps the pipeline broadcast + outer observable subject".**

- **Inner (Round ← pipeline broadcast)**: Round owns a single `NodeChain`; in `start()` it injects
  itself as the `EventSink` observer and subscribes to the `Tapped` broadcast to consume the chain
  uniformly. Branch on each `Tapped`'s `point` + `event`:
  - `Before SpeechStarted` → barge-in decision (with lockout; after the lockout window) → notify Session to decide
  - `After TurnComplete` → send STT + notify Session to upgrade
  - `After AudioOut` → per-sentence `SentenceStart`/`Audio`/`SentenceEnd` forwarding + TTS state machine + timeout
  - tail `Err` → `LlmNoUsableOutput` / `TtsEncode` / interrupt
  - others (`PcmFrame`/`PartialTranscript`) → forward / log
- **Outer (Session ← Round observable subject)**: Round itself implements the observer pattern, exposing a
  `RoundEvent` broadcast channel; **Session registers as its subscriber** (unique per Round). Round decides
  signals and **forwards output inline** (STT/LLM/TTS/Audio → `output_tx`) + maintains the TTS state machine;
  Session only does **lifecycle decisions** (shadow→running upgrade / phase switch / epoch / interrupt
  stop_round / ListenStop).

### File: `service/src/session/round.rs`

```rust
pub enum RoundEvent {
    SpeechStarted,                             // voice start (barge-in already past lockout)
    TurnComplete { text: String, prob: f32 },  // one recognition finished (upgrade shadow→running / Speaking / ListenStop)
    EmptyTurn,                                 // empty input finished (rotates a new shadow like TurnComplete, but no STT)
    SpokenEnd,                                 // TTS expression finished (this round done)
}

pub struct Round {
    chain: NodeChain,                                  // single chain
    round_event_tx: broadcast::Sender<RoundEvent>,     // outer observer channel (Round broadcasts)
    round_event_rx: broadcast::Receiver<RoundEvent>,   // Session registers/subscribes
    // ...
}
```

- `Round::event_receiver()` → returns `round_event_rx`, Session registers.
- Round's `start()` spawns an observer task: `select!` over { tail.next() (drive + Err) / tap_rx.recv()
  (control) / cancel }, consuming uniformly and forwarding per-sentence output.

## Turn Coordination

- The turn boundary is settled in **AsrNode's silence confirm** (`SILENCE_CONFIRM_MS=200`; AsrNode `finish()`es
  after VAD reports non-speech) and **Session control events** (`FinishTurn`: `ListenStop` / silence timeout /
  transport stall); `TurnNode` is only an explicit `TurnText→TurnComplete` close marker (D2).
- **Rule-based boundary (default)**: AsrNode ends a complete intent by the silence-confirm threshold
  (200ms) — after VAD reports non-speech, once `silence_samples >= SILENCE_CONFIRM_MS*sample_rate/1000`,
  it finishes; plus silence timeout, transport stall, `FinishTurn` control event, and prefix.
- **Handling the previous Round**: it keeps running by default; interrupted only on output/chatter/websocket
  contention—reusing `RoundStopReason::Upgrade/BargeIn` + epoch anti-stale output.

## Session Total Orchestration (Round Lifecycle Management)

- The phase state machine stays in Session: `Idle` / `Listening` / `Speaking`.
- `on_listening(Frame::Voice)` → forwards `AudioFrame` to the active Round's chain head (D1).
- **Session registers as the current Round's outer observer**: `select!` on the `RoundEvent` channel and
  makes **lifecycle decisions** on each signal:
  - `SpeechStarted` → barge-in: `next_round_epoch()` + `stop_round(BargeIn)` (the current shadow continues
    hearing the interrupt; its `TurnComplete` upgrades it — no `new_shadow_round`)
  - `TurnComplete{text, prob}` → `on_turn_complete`: `shadow→running` upgrade, phase Speaking, ListenStop,
    then immediately spawns the **next shadow round** (`new_shadow_round`) to keep listening during / between utterances
  - `SpokenEnd` → expression finished (shadow phase / idle accounting)
- **Drain queued `RoundEvent`s before handling an input frame**: lifecycle events travel on the broadcast,
  that round's output frames on the output channel, and the two queues are independent. Without draining
  first, a barge-in can be handled before `TurnComplete`, see a shadow that has not been promoted yet
  (`running_round == None`), give up on interrupting, and let the whole queued TTS batch play.
- **Pacing gates dequeueing, not sending**: `frame_duration_ms` only decides "when may the next frame be taken
  from the output queue", so at most one frame is in flight at any instant. Taking first and waiting afterwards
  prefetches the whole TTS batch into the forwarding loop, making every frame compete with input frames in
  `select!` and delaying barge-in by N frames.
- Session holds no `drain_hearing` / `handle_hearing_event` / `active_hearing`: the response logic lives in the
  Round observer.
- `with_node_templates(Vec<Arc<dyn Node>>)` injects the **bare prototype set** (api sites assemble it
  dynamically to decide which stages the chain has and in what order).
- The handshake (`handle_connect`) reply `audio_params` is decided by the `AudioSpec` looked up at build time;
  with no TTS node it sends `audio_params: None` (no downlink voice capability, no default fallback). The
  pacer timing takes `frame_duration_ms` (not built when there is no spec).
- epoch anti-crosstalk preserved; output forwarding counts as activity (resets `idle_since`).

### Empty-Input Decision (Hub Gatekeeper)

The classification table, counting and prompt grading live in [Dialogue Flow](@/development/server/dialogue-flow.en.md#empty-input-behavior); only the mechanism stays here. On entering `Listening` the Session derives an `EmptyKind` from "mode + previous turn" and, after observing `(After, EmptyInput)` / `(After, EmptyTurn)`, emits `Prompt{kind, count}` (re-injected at the chain head; nodes pass unrecognised events through), `Silence` (no expression injected) or `GiveUp` (back to `Idle`). `Ling` only composes a prompt from the Act and Echo only looks up a fixed sentence by kind/count — classification and rendering never cross.

## Tests

| Layer | Test | Assertion |
| --- | --- | --- |
| Broadcast | `service/src/pipeline/mod.rs` `with_observer_broadcasts_before_and_after_once_per_leaf` | Each leaf sees exactly one `Tapped{Before}` and one `Tapped{After}` per event |
| Broadcast | `nested_compose_does_not_duplicate_broadcast` | Nested `compose` does not broadcast the same event twice |
| Composition | `new_instance_reproduces_fresh_instance_and_compose_chain_broadcasts_once` | `new_instance` yields an independent instance while `compose_chain` still broadcasts once overall |
| Lifecycle | `lifecycle_immediate_releases_at_stream_end_deferred_at_finish` | Immediate releases at stream end, Deferred on `finish()`, exactly once each |
| Nodes | `api/tests/pipeline_nodes_test.rs` (16 + 1 `#[ignore]`) | Resampling to 16k, VAD onset and prefix cap, ASR empty input, TTS encoding and `AudioSpec` capability, ling cancellation |
| End to end | `api/tests/session/` (`chat.rs`, `empty_input_round.rs`) | Handshake `audio_params`, listen/speak rotation, barge-in, no-input prompt tiers |

```shell
cargo test -p service --lib pipeline
cargo test -p api --test pipeline_nodes_test
cargo test -p api --test session
```

## Related Files

| Path | Purpose |
| --- | --- |
| `service/src/pipeline/mod.rs` | `PipelineEvent`, `NodeContext{cancel, emit, session_id}`, `EventSink`/`Tapped`/`TapPoint`, the `Node` trait (prototype + `on_configure` + `capabilities()` + lifecycle), `compose`/`compose_chain` and the boundary broadcast |
| `service/src/pipeline/nodes/` | The six nodes: `opus` / `vad` / `asr` / `turn` / `ling` / `tts`, all pure `stream` transforms plus `new_instance` cloning |
| `service/src/session/mod.rs` | Round lifecycle (shadow/upgrade/stop/phase/idle), `with_node_templates` taking bare prototypes, `AudioSpec` look up at build time, `handle_connect` forwarding `Configure` to the templates once |
| `service/src/session/round.rs` | Holds the single chain; subscribes to `Tapped` for inner consumption; broadcasts `RoundEvent` (`SpeechStarted`/`TurnComplete`/`EmptyTurn`/`SpokenEnd`) outward |
| `service/src/component/tts.rs` | The `Tts` trait, including `audio_spec() -> AudioSpec` (`AudioSpec` is defined in pipeline) |
| `api/src/component/tts/model/` | `TtsMatcha` reports `audio_spec()` from its own fields; `TtsMute` returns 16000·1·60 |
| `api/src/component/tts/opus_encoder.rs` | `StreamingOpusEncoder` |
| `api/src/ws/mod.rs`, `api/src/matrix/client.rs` | Assemble bare prototypes `Vec<Arc<dyn Node>>` from config and hand them to `with_node_templates` |
| `api/tests/pipeline_nodes_test.rs` | Node-level contracts: resampling, VAD onset and prefix, ASR empty input, TTS encoding and capability declaration |
| `api/tests/session/` | End to end: handshake audio declaration, listen/speak rotation, barge-in, no-input prompt tiers |

## Hard constraints

- **VAD pool discipline**: VAD is a serial state machine; acquire/release pair strictly, and one instance is held by at most one Round at a time.
- **Who decides the turn boundary**: the boundary comes from AsrNode's silence confirmation plus Session control events (`FinishTurn` / silence timeout / transport stall); `TurnNode` only marks `TurnComplete`.
- **Broadcast order**: Before/After labels are affixed by the engine at node boundaries only; nodes never call `ctx.emit` themselves, otherwise observers see out-of-order or duplicate signals.
- **epoch and termination**: epoch prevents stale output and does not change with the chain layout.
- **emotion**: TTS balances emotion through a FIFO, never a text-key map.

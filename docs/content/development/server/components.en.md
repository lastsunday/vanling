+++
title = "Components and Module Layout"
weight = 201

[extra]
source_file_hash = "f293dfd9bb6a1d1d1d13be0e2535b1b7dc509b89"
translated_at = "2026-10-25T00:00:00Z"
+++

# Components and Module Layout

> The server's layers and module ownership: the core skeleton, the dual-layer `component/`, and `ling` as a special node.
> Why the layers look like this: [pipeline evolution record](@/records/server/pipeline-evolution.en.md).

## Core Skeleton

The data-flow / containment hierarchy, top to bottom:

```
socket / other → filter → session → round → pipeline → ling
```

- **socket**: transport entry (WebSocket / Matrix). `api/src/ws/`, `api/src/matrix/`
- **filter**: in/outbound filters (MCP routing, recording). `api/src/ws/filter/`
- **session**: connection lifecycle, phase state machine, Round lifecycle management
  (shadow/upgrade/stop). `service/src/session/`
- **round**: each dialogue turn (user speaks → server responds), **owns a single NodeChain**.
  `service/src/session/round.rs`
- **pipeline**: the outer node chain (`opus→vad→asr→turn→ling→tts`); the `Node`/`NodeChain`/
  `PipelineEvent` framework. `service/src/pipeline/`
- **ling**: the decision core (special node); receives recognized results → decides → produces
  expression intents. `service/src/ling/`; implementation in `api/src/component/ling/`

## service/src top-level modules

```
service/src/
  lib.rs        # pub mod component; pub mod frame; pub mod ling; pub mod message; pub mod pipeline; pub mod session; pub mod types;
  component/    # engine contracts: vad.rs  asr.rs  tts.rs  llm/{mod,token_converter}.rs  mcp/{mod,registry}.rs
  ling/         # Ling decision engine (special node): Ling trait (ask → OutputBlock stream)
  pipeline/     # Node framework: mod.rs + nodes/
  session/      # session / round / history; TurnEvent lives in round.rs
  message/      # transport protocol: hello/audio/close/llm/mcp/stt/tts + Message/Type/Transport/AudioFormat
  frame.rs      # Frame / FrameResult / OutputMessage (transport presentation layer)
  types.rs      # shared types: EmptyKind / Sentence / Input / OutputBlock / ContentBlock
```

Key points:

- **`component/`** = engine contracts (trait + types), not bound to a specific model; referenced by
  pipeline nodes and ling.
- **`ling/`** = only the `Ling` trait (decision engine). As it must judge/handle everything, it is a
  **special node** and must explicitly define every possible node (see "Ling special node" below).
- **`types.rs`** = cross-layer shared types (`EmptyKind`/`Sentence`/`Input`/`OutputBlock`/
  `ContentBlock`), referenced by pipeline, session, component, and api.
- **`message/` + `frame.rs`** = the transport protocol/presentation layer, mapping to the outermost
  "socket" of the skeleton.
- `TurnEvent` lives in `session/round.rs` (a turn event belongs to the Round lifecycle).

## Dual-Layer component

**Both layers — service and api — gather the engines**:

| Layer | Content | Use | Key path |
|---|---|---|---|
| **service `component/`** | engine **contracts** (trait + protocol types) | referenced by pipeline nodes and ling; not bound to a specific model | `service/src/component/{vad,asr,tts,llm,mcp}` |
| **api `component/`** | engine **implementations** (Manager + model) | choose model by config at startup, implement the contracts | `api/src/component/{vad,asr,llm,tts,mcp}` |

- Scope is **only the five engines**: vad / asr / llm / tts / mcp.
- Strictly one-way dependency: **api depends on service; service does not depend on api**.
- The `Ling` implementation `LingCore` lives in `api/src/component/ling/`, at the same layer as the
  other engine implementations (symmetric).
- Engines can be **instantiated standalone** (the composition root decides where to attach them) and
  reuse downlink info such as `AudioSpec` from the `component/` capability look up.

## Ling special node

`Ling` (`service/src/ling/`) is the **decision core**; it works as a node (`LingNode`) on the pipeline
chain, but unlike ordinary pure-transform nodes:

- It must **judge/handle everything** (LLM stream, MCP tools, history, sentence splitting), so it must
  explicitly define every node that can appear inside it.
- The implementation side is `LingCore` in `api/src/component/ling/` (LLM + MCP + history + splitter
  orchestration), producing per-sentence expression intents.

## Related Docs

- [Core Architecture](@/development/server/architecture.en.md): Session/Round/pipeline semantics
- [Unified Pipeline](@/development/server/pipeline.en.md): single-chain + observer protocol, capability look up, node lifecycle

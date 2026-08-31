+++
title = "Components and Module Layout"
weight = 206

[extra]
translated_at = "2026-08-31T00:00:00Z"
source_file_hash = "08f55bb7504325e0e1d3395cf1f41fc85b5f5277"
+++

# Components and Module Layout

> **This document describes the target architecture**: the core skeleton hierarchy, the dual-layer
> `component/` organization, the `ling` special-node semantics, and the phase-2 sub-pipeline plan.
> Use this as ground truth when opening a new session to continue.

## Target Core Skeleton

The data-flow / containment hierarchy, top to bottom:

```
socket / other → filter → session → round → pipeline → ling → sub-pipeline
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
- **sub-pipeline**: ling's internal decision sub-pipeline (**phase 2**, not yet implemented).
  See "sub-pipeline (phase 2)" below.

## service/src Fully Flat

The service crate **drops the `ling/` umbrella namespace** and flattens to top-level modules:

```
service/src/
  lib.rs        # pub mod component; pub mod ling; pub mod message; pub mod pipeline; pub mod session;
  component/    # engine contracts: vad.rs  asr.rs  tts.rs  llm/{mod,token_converter}.rs  mcp/{mod,registry}.rs
  ling/         # Ling decision engine (special node): Ling trait (ask → OutputBlock stream)
  pipeline/     # Node framework: mod.rs (the former pipeline.rs) + nodes/
  session/      # session / round / history; TurnEvent merged into round.rs
  message/      # transport protocol: hello/audio/close/llm/mcp/stt/tts + Message/Type/Transport/AudioFormat
  frame.rs      # Frame / FrameResult / OutputMessage (transport presentation layer)
  types.rs      # shared types: EmptyKind / Sentence / Input / OutputBlock / ContentBlock
```

Key points:

- **`component/`** = engine contracts (trait + types). **Runs standalone**: attached to the outer
  pipeline and to ling's sub-pipeline (phase 2).
- **`ling/`** = only the `Ling` trait (decision engine). As it must judge/handle everything, it is a
  **special node** and must explicitly define every possible node (see "Ling special node" below).
- **`types.rs`** = the shared types split out of the former `ling/core.rs` (`EmptyKind`/`Sentence`/
  `Input`/`OutputBlock`/`ContentBlock`), referenced cross-layer by pipeline, session, component, and api.
- **`message/` + `frame.rs`** = the transport protocol/presentation layer, mapping to the outermost
  "socket" of the skeleton.
- `TurnEvent` has been merged into `session/round.rs` (a turn event belongs to the Round lifecycle).

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
- When data is missing it can declare `NeedsInfo` (Stage E data loop), orchestrated by Session to loop
  back and decide again.

## sub-pipeline (phase 2)

**This phase only reorganizes containers**; the sub-pipeline is **not implemented** (node-ized in phase 2).

Current state: ~200 lines of imperative `while has_next_step` loop inside `LingCore` (LLM stream + MCP
tool + history + splitter). Phase 2 pulls this out into ling's internal **decision sub-pipeline**,
node-ized for parallelism/orchestration:

```
web_node / agent_node / data_node / re_decision_node ...
```

Then each data/tool node likewise reuses the `service/src/component/` contracts to form a pluggable
sub-pipeline.

## Convention fixes

- **`pipeline.rs` → `pipeline/mod.rs`**: unify the `X/mod.rs` convention.
- **`StreamingOpusEncoder` split into its own file**: moved out of `api/src/component/tts/mod.rs` into
  `api/src/component/tts/opus_encoder.rs`.
- **`NodeCapability` as a type alias** (Solution A): `pub type NodeCapability = dyn Any + Send + Sync;`,
  `capabilities() -> Vec<Box<NodeCapability>>`, consumers use `c.downcast_ref::<T>()` directly. The
  custom-trait + `as_any()` approach is dropped (the blanket impl loses `TypeId` through `dyn` dynamic
  dispatch, making `downcast_ref` return `None`).

## Related Docs

- [Core Architecture](@/development/server/architecture.md): Session/Round/pipeline semantics
- [Pipeline Redesign](@/development/server/pipeline-redesign.md): single-chain + observer protocol, capability look up, node lifecycle

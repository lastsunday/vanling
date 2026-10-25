+++
title = "Pipeline Evolution Record"
weight = 403
sort_by = "weight"

[extra]
source_file_hash = "c8e06b0e6e78deaa84a3e076de29d0743a2d8b2e"
translated_at = "2026-10-25T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Pipeline Evolution Record

What the single-chain + observer form replaced, and why it was chosen at the time. For what the pipeline is now, see [Unified Pipeline](@/development/server/pipeline.en.md).

## Structural problems it solved

Unify engines such as VAD / ASR / LLM / TTS from "hard-coded orchestration" into a "pluggable node chain",
flowing a unified `PipelineEvent` through the whole chain. The core claim: **streams are inherently
one-directional** (`audio → ASR → text → LLM → text → TTS → audio`), just a series of nodes chained end to
end. Therefore **every node should be a homogeneous stream transform**, and the chain is their nested fold—
"the previous node's output = the next node's input".

This blueprint resolves the structural problems left over from the current implementation:

1. **Parallel Vecs + magic indices**: the old `InputChain{nodes, resettable, reconfigurable}` were three
   parallel Vecs aligned by fixed indices (`nodes[2]`=ASR, `resettable[1]`=ASR). Reordering silently
   misaligns them—invisible to the compiler.
2. **ling/tts privilege**: Ling and TTS were treated as a privileged pair that always appeared together
   (`with_ling_node` next to `with_tts_node`), tearing apart the homogeneous essence that "they are just a
   few ordinary nodes in the chain".
3. **Hearing/speaking two chains**: `build_hearing` (`opus→vad→asr→turn`) + `build_speaking` (`ling→tts`)
   were treated as two separate chains—Session polled the hearing tail while Round spun a long task to
   consume the speaking tail: **two lifecycles, two consumer stacks**.
4. **Factory layering redundancy**: `PipelineFactory` trait + `NodeFactory` struct were two layers added only
   to keep "service from importing api concrete types"; the real need is just "build a chain per Round".
5. **Misplaced responsibility**: Session did both total orchestration and drove the recognition/output
   chains, overlapping with Round's per-turn processing responsibility.

## The form it replaced

| Dimension | Before | After |
|---|---|---|
| Chain structure | hearing `build_hearing` + speaking `build_speaking` two segments | **single chain** `opus→vad→asr→turn→ling→tts` |
| Chain abstraction | `PipelineFactory` trait + `NodeFactory` struct | **`Node` self-carried prototype (`new_instance`) + `compose_chain` unified builder** + `with_node_templates` injecting the bare prototype set |
| Observer | none (Session polls the hearing tail; Round long task consumes the speaking tail) | **two-layer**: inner Round ← pipeline broadcast (`Tapped`); outer Session ← `RoundEvent` (Round observable subject) |
| Broadcast timing | none | `Tapped{Before, After}` both timings, labeled by engine, business nodes unaware |
| Session driver source | polls the hearing tail (`drain_hearing`/`handle_hearing_event`/`active_hearing`) | **registers/subscribes to `RoundEvent`**, `select!`s only on RoundEvent for lifecycle decisions |
| Session responsibility | drives recognition (poll) + drives output | **Round lifecycle only** (shadow/upgrade/stop/phase/idle); no longer polls the chain |
| Round | long task consuming the speaking chain | **two-layer observer**: inner unified consumption (barge-in/advance/forward/timeout/errors); outer exposes `RoundEvent` for Session to subscribe |

## Blueprint parts that were never implemented

Two pieces of the same blueprint never landed; there is no implementation in the code:

| Idea | Current state |
| --- | --- |
| Stage E data loop: Ling declares missing data as `NeedsInfo{query, via}`, data nodes (MCP/DB/sensors) fetch it and loop back to Ling | `PipelineEvent` has no `NeedsInfo` and there is no `DataNode`; `LingCore` holds MCP and history itself |
| ling-internal sub-pipeline: split the ~200-line imperative `while has_next_step` loop in `LingCore` into `web_node` / `agent_node` / `data_node` / `re_decision_node` | Still an imperative loop |

Neither is tracked in the [roadmap](@/roadmap/_index.en.md): pushing either needs a human to assign priority first, otherwise it stays "not done".

## Layout and capability trade-offs

| Decision | Basis |
| --- | --- |
| `pipeline.rs` → `pipeline/mod.rs` | Follow the `X/mod.rs` convention |
| `StreamingOpusEncoder` in its own `api/src/component/tts/opus_encoder.rs` | Keep the encoder apart from the TTS engine implementation; `mod.rs` only assembles engines |
| `NodeCapability` as the type alias `dyn Any + Send + Sync` rather than a custom trait + `as_any()` | A blanket impl loses `TypeId` through `dyn` dynamic dispatch, so `downcast_ref` returns `None` |

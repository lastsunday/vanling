+++
title = "Server"
weight = 100
sort_by = "weight"
[extra]
source_file_hash = "18719742fcf486fa075d906fa750a8ac179ef14b"
translated_at = "2026-10-26T00:00:00Z"
+++

# Server

The vanling server is built with Rust + axum, implementing the Xiaozhi smart speaker protocol with real-time voice conversation capabilities.

Reading order: core architecture first (Session and Round lifetimes), then components and the pipeline (how one turn's data flows), then VAD/turn detection, dialogue flow and protocol details; deployment is for reference as needed.

| Page | Description |
| --- | --- |
| [Core Architecture](@/development/server/architecture.en.md) | Overall architecture, Session state machine, Round lifecycle, filters and construction entry points |
| [Components and Module Layout](@/development/server/components.en.md) | Core skeleton hierarchy, dual-layer components, the ling special node |
| [Unified Pipeline](@/development/server/pipeline.en.md) | Node contract, chain assembly and release, event-stream conventions |
| [VAD and Turn Detection](@/development/server/vad.en.md) | Speech onset/offset criteria, the two silence gates, turn-termination paths, parameter reference |
| [Dialogue Flow](@/development/server/dialogue-flow.en.md) | Handshake and communication sequence diagrams, listen modes, empty-input behaviour |
| [WebSocket Protocol](@/development/server/websocket-protocol.en.md) | Protocol field definitions and binary format |
| [Models and Deployment](@/development/server/models-and-deployment.en.md) | AI model specs, config system, asset installation, CUDA deployment |

There is exactly one backlog: [Work Items](@/roadmap/worksheet.en.md). Positioning and technology references are in [Positioning and Trade-off Reference](@/discussions/positioning.en.md); for why the old TODO was split up, see the [server records](@/records/server/todo-triage.en.md).

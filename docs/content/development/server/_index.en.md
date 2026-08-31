+++
title = "Server"
weight = 100
sort_by = "weight"
[extra]
source_file_hash = "ab983376536c2dde6c39bda3c0578c576f6391eb"
translated_at = "2026-08-31T00:00:00Z"
+++

# Server

vanling server is built with Rust + axum, implementing the Xiaozhi smart speaker protocol with real-time voice conversation capabilities.

This section contains the following documents:

| Page | Weight | Description |
|------|--------|-------------|
| [Core Architecture](@/development/server/architecture.en.md) | 200 | Overall architecture, Session state machine, Round lifecycle |
| [Dialogue Flow](@/development/server/dialogue-flow.en.md) | 201 | Complete sequence diagrams for handshake and communication phases |
| [WebSocket Protocol](@/development/server/websocket-protocol.en.md) | 202 | Protocol field definitions and binary format |
| [Models and Deployment](@/development/server/models-and-deployment.en.md) | 203 | AI model specs, config system, Downloader, CUDA deployment |
| [TODO](@/development/server/TODO.en.md) | 204 | Task list and known issues |
| [Pipeline Redesign](@/development/server/pipeline-redesign.en.md) | 205 | Unified pipeline refactor plan (PipelineNode replaces Ling/Tts/Listener) |
| [Components and Module Layout](@/development/server/components.en.md) | 206 | Core skeleton hierarchy, dual-layer component, ling special node, sub-pipeline plan |

+++
title = "Server"
weight = 100
sort_by = "weight"
[extra]
source_file_hash = "517538420f067e6e88d9a76963757cde88b5241d"
translated_at = "2026-07-09T00:00:00Z"
+++

# Server

chobits server is built with Rust + axum, implementing the Xiaozhi smart speaker protocol with real-time voice conversation capabilities.

This section contains the following documents:

| Page | Weight | Description |
|------|--------|-------------|
| [Core Architecture](@/development/server/architecture.en.md) | 200 | Overall architecture, Session state machine, Round lifecycle |
| [Dialogue Flow](@/development/server/dialogue-flow.en.md) | 201 | Complete sequence diagrams for handshake and communication phases |
| [WebSocket Protocol](@/development/server/websocket-protocol.en.md) | 202 | Protocol field definitions and binary format |
| [Models and Deployment](@/development/server/models-and-deployment.en.md) | 203 | AI model specs, config system, Downloader, CUDA deployment |
| [TODO](@/development/server/TODO.en.md) | 204 | Task list and known issues |

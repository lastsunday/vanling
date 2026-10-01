+++
title = "Development Documentation"
weight = 30
sort_by = "weight"
[extra]
source_file_hash = "d694825c805acf02f5d66f5564a58f7f0efc5cc6"
translated_at = "2026-10-01T07:37:04Z"
+++

# Development Documentation

Development guides for the Vanling server and related projects.

## [Server](@/development/server/architecture.en.md)

Server architecture design, business data flow, protocol reference, model specifications, and deployment.

- [Core Architecture](@/development/server/architecture.en.md) — Session state machine, concurrency model, factory pattern
- [Dialogue Flow](@/development/server/dialogue-flow.en.md) — Handshake, communication, Listen Mode, MCP flow
- [WebSocket Protocol](@/development/server/websocket-protocol.en.md) — Protocol field reference
- [Models and Deployment](@/development/server/models-and-deployment.en.md) — Model specifications, CUDA installation, reference specifications
- [TODO](@/development/server/TODO.en.md) — Completed/in-progress checklist (work entry point)
- [Positioning and Trade-off Reference](@/development/server/research.en.md) — Feature exploration, technology selection, reference projects

## [Client](@/development/clients/app.en.md)

Client application development documentation.

- [App (Flutter)](@/development/clients/app.en.md)
- [Admin Panel (React)](@/development/clients/server-ui.en.md)
- [ESP32](@/development/clients/esp32.en.md)

## [Debugging](@/development/debugging/vad-listener.en.md)

Debugging and diagnostics documentation.

- [VAD and Listener](@/development/debugging/vad-listener.en.md)
- [Audio Debugging](@/development/debugging/audio-debug.en.md)
- [ASR Debugging](@/development/debugging/asr-debug.en.md)

## [IoT Firmware](@/development/iot/features.en.md)

Development documentation for the Vanling ESP32 firmware (`apps/iot`).

- [Cargo Feature Criteria and Cohesion](@/development/iot/features.en.md) — hard/soft feature definitions, introduction criteria, cohesion rules
- [Hardware-free Emulation and Regression Smoke](@/development/iot/emulation.en.md) — host harness + esp-emu, decision matrix

How the implementation became what it is, and what was rejected, is in the [development records](@/records/_index.en.md) (AI-produced, not reviewed by a human).

## [Model Download](@/development/downloader.en.md)

Usage instructions for the model download tool.

## Related Projects

Documentation for ecosystem-related projects.

- [xiaozhi-esp32](@/development/related-project/xiaozhi-esp32.en.md)
- [xiaozhi-esp32-server](@/development/related-project/xiaozhi-esp32-server.en.md)
- [xiaozhi-esp32-server-java](@/development/related-project/xiaozhi-esp32-server-java.en.md)
- [xiaozhi-android-client](@/development/related-project/xiaozhi-android-client.en.md)

## Multilingual Documentation

After editing a Chinese page under `docs/`, translate it to `.en.md` and run `git hash-object <source.md>` to update the `source_file_hash` and `translated_at` in its front matter.

+++
title = "Clients"
weight = 200
sort_by = "weight"
[extra]
source_file_hash = "6edc39c9071d722ebae2a2963d5c80b6f0f0396d"
translated_at = "2026-10-25T00:00:00Z"
+++

# Clients

The client implementations that talk to the server. Semantics shared by every client live in one place; everything else is per platform.

- [Shared Client Protocol](@/development/clients/shared-protocol.en.md) — the client model, voice session, attention state machine, dual telemetry path, MCP tool surface
- [Page Implementation](@/development/clients/pages.en.md) — four-layer architecture, page descriptors, the display stack, three-way input routing, capability gates
- [Flutter App](@/development/clients/app.en.md) — current capabilities, environment, release and coordinate conventions
- [Admin UI](@/development/clients/server-ui.en.md) — React + Mantine, the path for adding a page

Firmware lives under [IoT Firmware](@/development/iot/_index.en.md).

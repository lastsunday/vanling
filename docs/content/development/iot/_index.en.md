+++
title = "IoT Firmware"
weight = 250
sort_by = "weight"
[extra]
source_file_hash = "690e4f0434a8612fb0707f44c8567a5958bf5f9d"
translated_at = "2026-10-25T00:00:00Z"
+++

# IoT Firmware

Layer structure, composition, and Cargo feature criteria for the Vanling ESP32 firmware (`apps/iot`).

Reading order: layers first (why they are cut this way), then the feature criteria (what may come in), then how to run it and what each path means.

- [Layering and Composition](@/development/iot/architecture.en.md) — layers, three-axis orthogonal composition, the needs model and its transport, render plugability, add-board/chip-family flows
- [Cargo Feature Criteria and Cohesion](@/development/iot/features.en.md) — hard/soft feature definitions, four introduction criteria, six cohesion rules
- [Firmware Installation](@/development/iot/flashing.en.md) — browser one-click install, esptool/GUI, batch flashing and debugging
- [Hardware-free Emulation and Regression Smoke](@/development/iot/emulation.en.md) — host harness + esp-emu, decision matrix, console UART constraint
- [Audio Metering Semantics](@/development/iot/audio.en.md) — the capture contract, screen readouts and scale, the log map's chord error, a derived release rate
- [Motion Semantic Framework](@/development/iot/motion.en.md) — the data/semantic dual plane, three-family arbitration, capability declaration, the QMI8658A register binding
- [Playback Semantics](@/development/iot/playback.en.md) — the playback contract, the phase of a one-shot, where the five MIDI sounds come from, the 20 ms DMA arm

Common commands and bench operations live in the [Cheat Sheet](@/roadmap/cheatsheet.en.md).

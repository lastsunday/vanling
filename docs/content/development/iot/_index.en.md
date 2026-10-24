+++
title = "IoT Firmware"
weight = 250
sort_by = "weight"
[extra]
source_file_hash = "11fb44df6fcb7508ad8b1973dd2d9af73f4e2bc7"
translated_at = "2026-10-24T02:34:00Z"
+++

# IoT Firmware

Layer structure, composition, and Cargo feature criteria for the Vanling ESP32 firmware (`apps/iot`).

- [Cheat Sheet](@/development/iot/cheatsheet.en.md) — common commands, the two boards' ports and resets, build-time variables, symptom triage, what the checks do not prove
- [Cargo Feature Criteria and Cohesion](@/development/iot/features.en.md) — hard/soft feature definitions, four introduction criteria, six cohesion rules, and the decision record for this removal
- [Layering and Composition](@/development/iot/architecture.en.md) — layers, three-axis orthogonal composition, render plugability, add-board/chip-family flows
- [Motion Semantic Framework](@/development/iot/motion.en.md) — the data/semantic dual plane, three-family arbitration, capability declaration, the QMI8658A register binding, adding a source
- [Audio Metering Semantics](@/development/iot/audio.en.md) — the capture contract, screen readouts and scale, the log map's chord error, a derived release rate
- [Playback Semantics](@/development/iot/playback.en.md) — the playback contract, the phase of a one-shot, where the five MIDI sounds come from, the 20 ms DMA arm, the ES8311 register rows
- [Firmware Installation](@/development/iot/flashing.en.md) — browser one-click install, esptool/GUI, batch flashing and debugging
- [Hardware-free Emulation and Regression Smoke](@/development/iot/emulation.en.md) — host harness + esp-emu, decision matrix, console UART constraint
+++
title = "IoT Firmware"
weight = 250
sort_by = "weight"
[extra]
source_file_hash = "521e1cc59dde8be16b1a9035a25af5745aee299b"
translated_at = "2026-09-26T00:00:00Z"
+++

# IoT Firmware

Layer structure, composition, and Cargo feature criteria for the Vanling ESP32 firmware (`apps/iot`).

- [Cargo Feature Criteria and Cohesion](@/development/iot/features.en.md) — hard/soft feature definitions, four introduction criteria, six cohesion rules, and the decision record for this removal
- [Layering and Composition](@/development/iot/architecture.en.md) — layers, three-axis orthogonal composition, render plugability, add-board/chip-family flows
- [Motion Semantic Framework](@/development/iot/motion.en.md) — the data/semantic dual plane, three-family arbitration, capability declaration, the QMI8658A register binding, adding a source
- [Firmware Installation](@/development/iot/flashing.en.md) — browser one-click install, esptool/GUI, batch flashing and debugging
- [Hardware-free Emulation and Regression Smoke](@/development/iot/emulation.en.md) — host harness + esp-emu, decision matrix, console UART constraint
+++
title = "IoT Implementation Records"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "a81aebb6aa8f2c78e19893df993aba62db74e6c9b15e544605c5773786161fe9"
translated_at = "2026-09-30T17:36:07Z"
+++

<!-- doc-audience: ai -->

# IoT Implementation Records

Judgement records from the implementation of Vanling's own ESP32 firmware (`apps/iot`). **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

The system description lives in the [development docs / IoT firmware](@/development/iot/_index.en.md).

| Section                                                            | Contents                                                              |
| ------------------------------------------------------------------ | --------------------------------------------------------------------- |
| [Playback Implementation Record](@/records/iot/playback.en.md)     | The `Cp0Disabled` investigation, rejected approaches, feed cadence ownership, the ISR split |
| [Hardware Constraint Record](@/records/iot/hardware-constraints.en.md) | Microphone front-end write order and calibration, motion detection, stack size |

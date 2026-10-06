+++
title = "IoT Implementation Records"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "af2a549e1351587350546c872e691cf4820c7180"
translated_at = "2026-10-01T07:37:04Z"
+++

<!-- doc-audience: ai -->

# IoT Implementation Records

Judgement records from the implementation of Vanling's own ESP32 firmware (`apps/iot`). **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

The system description lives in the [development docs / IoT firmware](@/development/iot/_index.en.md).

| Section                                                            | Contents                                                              |
| ------------------------------------------------------------------ | --------------------------------------------------------------------- |
| [Playback Implementation Record](@/records/iot/playback.en.md)     | The `Cp0Disabled` investigation, rejected approaches, feed cadence ownership, the ISR split |
| [Hardware Constraint Record](@/records/iot/hardware-constraints.en.md) | Microphone front-end write order and calibration, motion detection, stack size |
| [Camera Implementation Record](@/records/iot/camera.en.md) | GC2145 field-of-view arithmetic, measured bins and scalar, why PSRAM is unusable, panel wiring |

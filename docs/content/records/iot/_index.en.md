+++
title = "IoT Implementation Records"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "49ccc612f3e81648e95ff72ba3adf86599a90217"
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

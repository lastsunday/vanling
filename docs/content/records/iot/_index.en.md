+++
title = "IoT Implementation Records"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "7260ca8dc54a69bb401d5001c918e43c65e4ca54"
translated_at = "2026-10-06T05:35:23Z"
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
| [Audio Implementation Record](@/records/iot/audio.en.md) | Silence as "no change", measured backlog depths, dBA against the raw columns, quantisation-noise floor |
| [Motion Implementation Record](@/records/iot/motion.en.md) | Rejected knock-detection approaches, where the thresholds came from, No-Motion as a level |

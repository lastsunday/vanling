+++
title = "Development Records"
weight = 50
sort_by = "weight"
[extra]
source_file_hash = "db5e6e344f4837eff8acda0a04624589260f253723dafddcda1a5a43262f89b1"
translated_at = "2026-09-30T17:36:07Z"
+++

<!-- doc-audience: ai -->

# Development Records

**Every judgement in this section was produced by AI and has not been reviewed by a human.** Nothing here claims authority: it records *why an implementation looks the way it does*, so a later reader can judge whether it still holds — it is not an endorsement of the current code.

## What belongs here

- **The basis of a judgement** — why A over B, and what was rejected
- **Measured data** — with the test name or the reproduction, so a reader can check it
- **The path of an investigation** — including the wrong turns, especially the ones that looked right and were not

## What does not

- Anything the code or the tests answer directly
- Unverified observations. **An assertion without test or measurement backing does not enter this section.** Where a note is genuinely worth keeping, it is written as "reported" and marked unreproduced

## Relation to the development docs

The [development docs](@/development/_index.en.md) describe what the system **is now**; this section records **how it became so**, and which conclusions were overturned. Measured numbers cited by the development docs originate here, so that the same figure is not carried in several places where it can drift apart.

## Sections

| Section                                                                | Contents                                                       |
| ---------------------------------------------------------------------- | -------------------------------------------------------------- |
| [Playback Implementation Record](@/records/iot/playback.en.md)          | The `Cp0Disabled` investigation, rejected approaches, feed cadence ownership, the ISR/cooperative split |
| [Hardware Constraint Record](@/records/iot/hardware-constraints.en.md) | Microphone front-end write order and calibration, motion detection, where the stack size came from |

+++
title = "IoT Implementation Records"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "e0e7efc9f8d12103f431980422a57a6b181bcdd4"
translated_at = "2026-10-11T05:41:46Z"
+++

<!-- doc-audience: ai -->

# IoT Implementation Records

Implementation judgement recorded for Vanling's own ESP32 firmware (`apps/iot`). **AI-produced, not human-reviewed** — see the [parent note](@/records/_index.en.md).

The system description lives in [development docs / IoT firmware](@/development/iot/_index.en.md).

## Findings, quick

**This table is where "what is the conclusion, and on how many numbers" is answered. Most questions stop here; the full text is for derivation, rejected alternatives and reproduction.**

### Camera (GC2145 + ST7789)

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| How the field of view is computed | **output width ÷ array width, independent of any register value.** ratio 5 divides evenly, so 320×240 is the full view | 320/1600 = 1/5 |
| bins and frame rate | Two faces of one switch. This repository writes `0x00` | `0x00` 4.5 fps / 691 KB/s; `0x01` 10 fps / 1558 KB/s |
| ratio steps | ratio 5 is the only step that reads the whole array; 1 yields no frame, 3/4 are undersampled | ratio 3 = 3444 KB/s; ratio 1 = 0 frames |
| Scalar mode `0xfd` | Doubles frame rate but gives a **narrower** view; this driver does not write it (leaves the power-on `0x00`) but reads it back | 5 → 10 fps |
| Ring descriptor chain | **Withdrawn; the linear chain stays.** The peripheral stops before completing one lap | ring 28 KB/s vs linear 645 KB/s |
| The linear chain drops bytes | The ring must be a whole frame, at the cost of rebuilds falling to 4 per second | one dropped byte shifts every later line (moving banding) |
| PSRAM ring | **Always writes garbage.** esp-hal cannot express `internal SRAM only`, so it stays in the configuration the vendor explicitly refuses | 150 KB ring |
| Frame-rate ceiling | Comes from data volume, **not** AEC exposure. Faster needs a smaller window or a different part | 5.2 fps; exposure 486 lines vs ~1200 read out |
| Pixel clock | Fixed at 24 MHz. At 20 MHz the PLL will not lock and the output is **structurally correct, entirely noise**, with every register reading back normally | — |
| Entering/leaving the Camera page | The chain is rebuilt each time and blocks once built. Render side 8% → 29–36% | 11 repaints/window at boot; 25–29 when cycling |
| The blank window on entry | ~192 ms, not shortenable; a `CAMERA WAIT` transition frame mitigates it | 5.2 fps ⇒ ~192 ms |
| Panel read-back | **The panel has no MISO**, so every register reads `0x00` and verification is visual only | — |
| Frame-buffer owner | `FrameOwner` with three variants makes the fourth combination unrepresentable (two flags once deadlocked permanently) | 153,600 B |

### Audio (ES7210 + ZTS6216)

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| May the capture plane be throttled | **No.** Only `sample()` drains the ring, so batching to 30 s fills it and halts I2S | ring full at 60 ms |
| How `Off` is implemented | The chain must be parked from the consumer side, not merely left unpoll'ed | chain stops 1 ms after the demand lands |
| Silence detection | Must read as "no change", or the whole diff chain spins | — |
| A plane's marginal cost | **Rises with what else is awake; it is not a constant** | 12.52% (alone) → 15.73% (something else awake) |
| What throttling achieves | Does not save the device total; it moves executor from one place to another | 94.8% → 94.0% |
| Scaling the poll side | Scaled by the cadence in force, matching measurement at both ends | +0.6% / −4.1% (40% high before scaling) |
| Quantisation noise | Feeds a **65 dBA phantom floor**; the dBA table needs converting against the raw table | 65 dBA |
| SPL calibration | `0 dBFS = 102 dB SPL` is this microphone's figure (397.8 mV/Pa ⇒ full scale 2.51 Pa) | 101.98 dB |

### Motion (QMI8658A)

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| Tap engine | **Unusable** (reported, not reproduced); tap detection moved core-side and `QMI8658_CAPABILITIES` does not claim `TAP` | — |
| Tap threshold | Uses the **raw magnitude**, not a de-gravitated estimate | 8–22 mG at rest; 150 mG quiet / 250 mG peak |
| The aLPF filter | **Measured irrelevant, switched off.** A tap is a broadband transient and the filter only clips its peak | static noise and tap readings identical either way |
| No-Motion | A **level, not an event**; only the 0→1 edge is published, or a still device reports `Still` every frame | — |
| Shake criterion | Compares vector residual and counts **zero crossings**, not time above a threshold | 2.0 g; W3C's 2.5 g / 60 Hz single-axis amplitude does not apply |

### Input and touch

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| Page cycle | Correct on hardware. **`[DISPLAY]`'s 2 s window cannot show page order** — reading it that way once suggested "pages are jumping around" | 46 transitions, 0 off-cycle, 0 repeated |
| The four polled sources | Only capture may be switched off; the interaction path is off-limits by design | steady `button 0 / touch 1 / motion 3 / capture 16–20 ms` |
| The interaction floor | A theoretical lower bound unrelated to demand, not tuning headroom | 10.87% |

### Hardware constraints

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| Power management | **No PM yet, so no clock-source lock is the right call** (two locks with no holders is dead code). The order still matters | — |
| Light-sleep | Peripherals are clock-gated and raise no interrupt, so drivers relying on peripheral interrupts must hold `NO_LIGHT_SLEEP` | DVP and I2S chains both fall here |
| Shared MCLK | The two units run in their own clock domains; **the transmit unit drives the pin and capture is forced subordinate**, and the direction is not interchangeable | `with_signal_loopback` is the only misleading name here |
| Main task stack | Changed when the camera entered the product; read the linked ELF symbols | `_stack_start_cpu0` = 83,060 B |
| esp-hal stack-overflow watchpoint | Fires spuriously and cannot be used as an overflow criterion | — |

## Probes

**Each probe is a standalone bin that wires one layer of the framework straight to real hardware. Consult this before touching a driver — the question "which probe answers this" should end here.**

A probe is not another form of the product: the product firmware **cannot produce** the inputs a probe can. All five pages ask all four planes for `Continuous`, so on a device you can never reach any cell of the (configuration × cadence) matrix but the default one.

| Probe | What it answers | What it reports |
| --- | --- | --- |
| `demand-probe` | What each cell of the motion plane's configuration × cadence costs | Each cell settled before measuring: bus transactions/second, executor occupancy as a wall-clock share, and the number of polls measured so the first two can check each other |
| `camera-probe` | Whether the camera's frame reaches the panel unmodified, or something substituted for it | Frame bytes and checksum; "the picture looks wrong" is not a measurement |
| `audio-probe` | The transmit path's steady state with the runtime to itself | Stalls and gaps across the two alternated sounds |
| `multicore-s3` | Whether starting the second core survives the main stack sitting beneath it | Whether the stack top — where the thread-mode executor's `ThreadFlag` lives — is overwritten by the startup prologue |

```sh
moon run iot:probe-demand-s3 && moon run iot:flash-probe-demand-s3
```

`probe-*-s3` only builds; `flash-probe-*-s3` is what flashes. The port is in the command in `moon.yml`.

**All four bins are compiled on every `check-s3`, and never linked.** A probe nothing compiles rots in place: each one calls a bring-up or a chip API of its own, so a change to `Board`, `Speaker` or the chip layer breaks its build and nobody notices until the day it is needed on a bench.

**The boundary between probe and product can move silently**: both sides still compile, so drift is not a compile error. When a change touches a framework path a probe calls (`poll_plan_of`, `MotionConfig::for_mode`, a driver entry point), flash the probe and treat its output as evidence rather than waiting for the build to go green.

## Full records

Read on for derivations, the full list of rejected alternatives, and reproduction steps.

| Chapter | Contents |
| --- | --- |
| [Playback record](@/records/iot/playback.en.md) | `Cp0Disabled` investigation, rejected approaches, feed-tick ownership, ISR split, sound recipes |
| [Hardware constraints record](@/records/iot/hardware-constraints.en.md) | Shared MCLK, clock-source lock vs PM ordering, microphone front-end write order and calibration, where the stack size comes from |
| [Camera record](@/records/iot/camera.en.md) | GC2145 field-of-view arithmetic, bins and scalar measurements, why PSRAM is unusable, panel wiring, repeated page cycling rebuilding and stalling the chain |
| [Audio record](@/records/iot/audio.en.md) | Silence detection, measured marginal executor cost, only one place the capture chain can stop, calibration walk gated by demand, backlog depth, dBA against the raw table, quantisation noise floor |
| [Motion record](@/records/iot/motion.en.md) | Rejected tap approaches, where the thresholds come from, No-Motion is a level not an event |
| [Input and touch record](@/records/iot/input.en.md) | Page cycle measured on hardware, 2 s windows cannot show page order, only one of four polled sources may be switched off |

> The full derivation of throttling, with measurements and method, is in the [architecture doc](@/development/iot/architecture.en.md).
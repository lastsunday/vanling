+++
title = "IoT Implementation Records"
weight = 10
sort_by = "weight"
[extra]
source_file_hash = "323765c3b7a73ee4a8bb15d91a098e98eef6a94f"
translated_at = "2026-10-18T00:00:00Z"
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

### Echo cancellation

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| **Is there a public recording to validate against** | **No.** All four tried: two Espressif recordings, Microsoft AEC-Challenge (unusable **by design** — consumer stacks may already run AEC/NS on the send path), and ITU-T G.168 (Annex D does carry explicit impulse responses, but a transcription of it cannot be verified) | correlation 0.137 / 0.743 / ≤0.137; Wiener bound −4.97…**−0.42** dB (best of 40 GUIDs still negative) |
| Is ERLE this dataset's metric | **No.** The paper says "we can not use ERLE and PESQ" | ERLE vs subjective quality PCC **0.31** |
| ITU-T G.168's standard paths | **Right idea, unverifiable transcription.** Eight `m_i(k)` impulse-response tables, needing no audio files at all | extraction loses the first column; reading order ambiguous; **both readings violate the standard's own "\|G\| ≤ 0 dB" invariant by 1–11 dB** |
| What the standard itself says | G.168 Test 12 note: real acoustic test vectors are "**for further study**" | the standards body confirms they do not exist |
| The reference became | **The offline optimum on the same path**, solved there and then. Computed, not transcribed | gap to optimum **0.1–1.9 dB** across four paths |
| **The 15 dB bar** | **It is about the path, not the algorithm.** With the tail longer than the filter the offline optimum itself is 14.4 dB | no 128-tap linear filter can reach 15 |
| A generated path needs a direct reflection | Otherwise it has no onset, the delay search has nothing to lock onto, and it reports a noise peak | without one: a 12-tap path reported 15, a 4-tap one 22, ERLE **0.4 dB**; with one every delay hits exactly, 61 dB |
| The freeze stops continued damage, not the first half second | The coherence meter has a ~512-sample time constant and the filter has already run through them | freeze covers 98%; energy past the path grows **3.7e6×**; no growth after the freeze. **Unfixed** |
| **CPU: measure the clock first** | **The whole board runs at 80 MHz, not 240.** `CpuClock`'s default arm *is* `_80MHz`; `Config::default()` never sets the maximum | `esp_hal::clock::cpu_clock()` = 80,000,000 Hz |
| The measurement used the wrong clock | The first version's "219 cycles/MAC" was converted at 240 MHz and is wrong. **A reference loop caught it** — a trivial integer loop should be faster, yet ran 193× slower | reference FIR **12** cycles/MAC @80 MHz |
| **Block adaptation: it passes** | **Coefficients updated once every four samples.** Steady-state ERLE bit-identical; convergence slower | **L=16 9,680 µs ≤ 10 ms**; L=128 70,945 → **34,911 µs** |
| Frequency-domain MDF (best practice) | Wrote the 512-point fixed-point FFT, measured it before discussing the architecture, **then deleted it** (no caller on the product path). **The transform alone is 1.8× over budget**; partitioning and filtering not yet counted | 3,046 µs each = **182% of budget**; RAM 3,064 B → 6 KB (no RAM saved) |
| **The second core: identical digits** | `FROM_CPU_INTR1` is a second interrupt source on the *same* core. The product runs on core 0; core 1 is idle | twenty-odd measurements **all identical to the digit** → the bottleneck is shared |
| **IRAM: not that either** | The butterfly moved into `.rwtext`, confirmed at 0x4037a4c4 and not flash | 3,046 → **3,235 µs, 6% slower.** Fetch is not the bottleneck |
| Four loops, IPC 0.1–0.33 | Fetch ruled out, second core ruled out, **so it is the memory system itself.** The cleanest code (FFT butterfly) has the lowest IPC | butterfly **0.09**; reference FIR 0.33 |
| **The delay search can break the 30 ms bound alone** | One search is about **32 ms**, once every 256 ms, **3.0 ms per tick** amortised (31% of L=16). **Thinning the grid is ruled out** | Against a broadband reference **the correlation peak is one sample wide** — at a one-sample delay even adjacent lags 0 and 4 score nothing (146 against 1000) |
| **240 MHz, and the console survived** | esp-hal's "changing the PLL breaks USB Serial/JTAG" was `[reported]`. **Measured: 240 MHz runs and the console is fine** | L=16 **3,248 µs**; L=128 **11,639 µs**; **worst tick 21.6 ms, inside the 30 ms bound** |
| **Not one cycle count moved** | The clock is only a divisor: cycles/butterfly 112 → **112**, reference FIR 12 → **12** | every time figure ÷3 at 240 MHz |
| **The data cache is out of reach** | ESPHome/ESP-SR's AEC wants `DATA_CACHE_64KB` + `LINE_64B` and **refuses to start without it**. esp-hal has no API, esp-metadata no eFuse field | `[reported]` whether this board has one, unread |
| The IPC puzzle is not the cache | Ordered by writes, not working set: read-only 12 / 31.6, **read+write 105.8 / 119**. The smallest working set is not the fastest | write-back cache, rewriting the same location every tap |
| **But it exposed a much larger lever** | The board has been at a third of its rated clock, and **the existing 17–25 ms worst tick is paid at that same 80 MHz** | @240 MHz: AEC L=64 → **13.4 ms (fits)**; worst tick → 5.7–8.3 ms. esp-hal notes that changing PLL affects USB Serial/JTAG, which is this repo's console |
| The coefficient update dominates | **`gain` is 64-bit**, so `gain · x` costs `muluh`+`mull`+`srai`+`add`. The clamp branches are almost never taken and are not the cost | L=128 update **48.9 ms** vs convolution 16.2 ms |
| **A 7.6 ms fixed floor** | Delay search 3.0 + meters 1.8 + unattributed 2.7, **independent of tap count** | Payable at zero taps; even a free update lands L=16 at ~8 ms |
| **Does the conclusion change** | **No.** What binds is wall clock, independent of the CPU clock (systimer runs off the crystal). All three changes together bought 9.6% | L=128 **71.0 ms** = 355% of a tick; L=16 **15.5 ms** against a ≤10 ms bar |
| **The FPU is off on this target** | A same-shaped f32 loop ran over 120 s against 140 ms for its i32 twin, with libgcc's whole soft-float complement linked | Float runs in software emulation; "switch to float" is not a shortcut |
| Cost | **3,040 B RAM (on the board)** | `state_bytes::<128>()` = `size_of::<Canceller<128>>()`; the old function counted only the filter and under-reported by 3.4× |

### Hardware constraints

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| Power management | **No PM yet, so no clock-source lock is the right call** (two locks with no holders is dead code). The order still matters | — |
| Light-sleep | Peripherals are clock-gated and raise no interrupt, so drivers relying on peripheral interrupts must hold `NO_LIGHT_SLEEP` | DVP and I2S chains both fall here |
| Shared MCLK | The two units run in their own clock domains; **the transmit unit drives the pin and capture is forced subordinate**, and the direction is not interchangeable | `with_signal_loopback` is the only misleading name here |
| Main task stack | Changed when the camera entered the product; read the linked ELF symbols | `_stack_start_cpu0` = 83,060 B |
| esp-hal stack-overflow watchpoint | Fires spuriously and cannot be used as an overflow criterion | — |
| **A 64 KB data cache leaves the reclaimed region empty** | esp-hal calls the space above 0x3FCF0000 "leaving to the heap", but takes the data cache off its top. At the default, `#[ram(reclaimed)]` has nowhere to go | The existing ELF in this tree has **zero symbols** above 0x3FCF0000; halving the cache buys **32 KiB** back |

### Network (Wi-Fi)

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| Is there a TCP/IP stack in this generation | **No lwIP, no esp-netif** (404 on crates.io, no directory in the repo, zero hits in the changelog). What replaces it is `esp-radio` (L2) plus `embassy-net` (smoltcp) | `esp-radio` ships an `embassy_net_driver::Driver` implementation, so getting an address is wiring, not writing |
| What "connected" means | **An address, not an association.** The reference firmware's `STA_CONNECTED` handler is empty; the bit is set from `GOT_IP` | Associated ≠ connected |
| The three stages have three budgets | The scan is the radio's, association is the AP's reply and retries, DHCP is the network's lease — one "connecting" label hides which is slow | The probe times them apart |
| The trap in the driver's connect defaults | A fast scan takes the first match, so **a strong same-name AP that fails authentication once is silently replaced by a weak one** | Measured on the reference firmware: −73 dBm in range, connected to −79 dBm |
| **The DRAM budget is the hardest constraint on this board** | `dram_seg` can give **12,344 B** more (all that is left under the 65,536 B stack assertion) — not enough for the driver's buffers | Main stack 77,880 B; the Wi-Fi heap is 8 KiB + 32 KiB reclaimed |
| **PHY calibration restore is `todo!()` on the S3** | `PROVIDE(misc_nvs_restore = __esp_radio_misc_nvs_restore)` binds the blob's undefined reference to a `#[cfg(xtensa)]` `todo!()`, and this is an Xtensa target | Workaround: **no `phy_init` partition** (at the cost of calibrating at every boot) |
| Partition table | `espflash` defaults to a 1 MB factory partition and the firmware was already 630 KB before the Wi-Fi blobs | factory **6 MB**; a 16 MB part |

## Probes

**Each probe is a standalone bin that wires one layer of the framework straight to real hardware. Consult this before touching a driver — the question "which probe answers this" should end here.**

A probe is not another form of the product: the product firmware **cannot produce** the inputs a probe can. All five pages ask all four planes for `Continuous`, so on a device you can never reach any cell of the (configuration × cadence) matrix but the default one.

| Probe | What it answers | What it reports |
| --- | --- | --- |
| `demand-probe` | What each cell of the motion plane's configuration × cadence costs | Each cell settled before measuring: bus transactions/second, executor occupancy as a wall-clock share, and the number of polls measured so the first two can check each other |
| `camera-probe` | Whether the camera's frame reaches the panel unmodified, or something substituted for it | Frame bytes and checksum; "the picture looks wrong" is not a measurement |
| `audio-probe` | The transmit path's steady state with the runtime to itself | Stalls and gaps across the two alternated sounds |
| `multicore-s3` | Whether starting the second core survives the main stack sitting beneath it | Whether the stack top — where the thread-mode executor's `ThreadFlag` lives — is overwritten by the startup prologue |
| `aec-probe` | What one audio tick of the linear canceller costs, at L = 16/32/64/128 | First/mean/worst tick, share of 20 ms, and the delay search, filter and meters timed separately |
| `clock-probe` | What a runtime CPU clock switch costs, what stays true afterwards, and what core 1 makes of it | Ratio of one work unit across the three presets (**not `cpu_clock()` — it reads configuration, not hardware**), switch wall clock, `Instant` across a switch, both cores measuring the same work |
| `wifi-probe` | Whether the radio and the IP stack come up at all, and whether this board's DRAM budget holds them | Scan / association / DHCP **timed apart**, how many access points the scan found, the driver's heap every 2 s, what calibration costs at boot |

```sh
moon run iot:probe-demand-s3 && moon run iot:flash-probe-demand-s3
```

`probe-*-s3` only builds; `flash-probe-*-s3` is what flashes. The port is in the command in `moon.yml`. `wifi-probe` exists **for both chips** (`-s3` / `-c6`): one source, and the C6 runs on a **plain reset** rather than `-a watchdog-reset` (why: `@/development/iot/flashing.md`).

`wifi-probe` has one extra step: its credentials are a **build-time input**, so the build command carries two variables.

```sh
IOT_WIFI_SSID=home IOT_WIFI_PSK=secret moon run iot:probe-wifi-s3
IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-probe-wifi-s3
# the C6 probe: same source, plain DTR reset instead of `-a watchdog-reset`
IOT_WIFI_SSID=home IOT_WIFI_PSK=secret moon run iot:probe-wifi-c6
IOT_PORT=/dev/cu.usbmodem14401 moon run iot:flash-probe-wifi-c6
```

Without them it still runs and says that this build was not told to join anything — which answers "does the scan work on its own" without pretending to be a connection test. Both names are in `FORWARDED_ENV` in `scripts/iot-xtensa.sh`, and the host test `build_env.rs` fails the build if one is read but not forwarded.

**All five bins are compiled on every `check-s3`, and never linked.** A probe nothing compiles rots in place: each one calls a bring-up or a chip API of its own, so a change to `Board`, `Speaker` or the chip layer breaks its build and nobody notices until the day it is needed on a bench.

**The boundary between probe and product can move silently**: both sides still compile, so drift is not a compile error. When a change touches a framework path a probe calls (`poll_plan_of`, `MotionConfig::for_mode`, a driver entry point), flash the probe and treat its output as evidence rather than waiting for the build to go green.

## Check tasks

`moon` caches on a content hash. With no `inputs` its default takes in **every file under the project**; declaring `inputs` **replaces** that default rather than adding to it. So a task that compiles through a workspace-root script (`check-s3` / `check-features` / the `probe-*-s3` ones) has to name that script, or editing the dispatcher leaves the cache valid. See the [Check tasks and caching record](@/records/iot/tasks.en.md).

| Finding | Conclusion | Numbers |
| --- | --- | --- |
| What is in the cache key | Task text + project `env:` + **one sha1 per input file**; the default input is the whole project | `check-host`'s default key holds 121 entries |
| The script is outside it | A task compiling through `scripts/iot-xtensa.sh` must name it, or editing the script does not invalidate | Append one comment: key `0973f854` → `577401ca`; restore → cache hit |
| `check-features` used to run every time | It was `cache: false`. Now an unchanged tree replays in 0.09 s, a source change still pays full price | 2m20s570ms (eight S3 subsets at ~15 s each) |
| The C6 product check | Moved to the host's stable toolchain: the target has a prebuilt `core`/`alloc`, and that is the toolchain producing its firmware | 1m12s → 1.65 s |
| Merging 8 containers into 1 | **Rejected**: starting a container measures 0.45 s, so the ceiling is 3.5 s — not worth a batch protocol | 3 × 0.45 s (5.4 s was a cold start) |
| The guard that stays on the dispatcher | `check-features`' C6 subset is kept deliberately: it is CI's only witness of the S3-only `ESP_HAL_CONFIG_DATA_CACHE_SIZE` gating | — |

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
| [Echo cancellation record](@/records/iot/aec.en.md) | Why Espressif's vectors cannot measure AEC, why the step must not be scaled by the residual, coherence as the freeze criterion, the delay search's confidence and rate limit, steady-state against whole-file ERLE |
| [Runtime CPU clock record](@/records/iot/clock.en.md) | esp-hal's frozen clock frequencies, the off-path register route measured at 1 : 2.00 : 2.95, a switch costing 1–50 us, no difference on the second core or outside the CPU, the table following the chip, one existing defect in the way |
| [Wi-Fi record](@/records/iot/wifi.en.md) | No lwIP in this generation, only esp-radio plus embassy-net; "connected" means an address; three connection defaults copied from the reference firmware; how the DRAM budget was made to fit; the PHY calibration `todo!()` and the partition table's workaround |
| [Check tasks and caching record](@/records/iot/tasks.en.md) | What moon's cache key is made of and the script blind spot, `check-features` gaining a cache, the C6 check moving to the host, the rejected container merge |

> The full derivation of throttling, with measurements and method, is in the [architecture doc](@/development/iot/architecture.en.md).
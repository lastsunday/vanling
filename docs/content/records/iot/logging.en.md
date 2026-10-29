+++
title = "Logging Implementation Record"
weight = 82
sort_by = "weight"
[extra]
source_file_hash = "b230265e5988042ea90beab8cf28047fd3af3968"
translated_at = "2026-10-29T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Logging Implementation Record

Trade-offs and measurements for the product firmware's serial log. **AI-produced, not human-reviewed** — see the [parent note](@/records/_index.en.md).

Every judgement is tagged: **`[test]`** has a unit test, **`[measured]`** has a hardware measurement with a reproduction, **`[claimed]`** is unreproduced and recorded as such.

---

## The product defaults to `info`: a quiet steady state is normal

`[measured]` The same S3, the same default product build (no `IOT_BOOT_PAGE` / `IOT_DEMAND` / credentials), one serial capture before and one after:

| | before | after (`info`) | after (`IOT_LOG_LEVEL=debug`) |
| --- | --- | --- | --- |
| duration | 43.8 s | 80 s | 40 s |
| lines | 169 | 35 | 137 |
| after boot | one frame-cost line per 2 s, one stack-peak line per 500 ms, one feed line per 2 s, one link-phase line per 2 s | **last line at 6.2 s; nothing for the following 73.8 s** | 102 DEBUG lines (frame cost, feeds, link, scans) |

Of those 169 lines, 81 were `[DISPLAY]`, 25 `[WIFI]` and 22 `[PLAY]` — three variations of **the same sentence repeated per window**, answering questions (is the panel expensive, did a feed slip, which step is the link on) whose answer does not change in a steady state.

Reproduction: `espflash monitor --non-interactive --port <s3> --chip esp32s3 -a watchdog-reset`, cutting one section before and one after touching any input. **Silence is not a hang**: the boot lines `[IOT] boot complete` and `[HW] N devices: …` are what say so (it was `[POST]` before the rename), and the very first line `[IOT] log level …` says how quiet this image was built to be.

## Every `[HW]` line comes from the hardware

`[test]` `SelfCheck { name, id, check }` in `hardware.rs`: a driver states its own name, its own bus address, and answers one self-check on the spot. The board only hands the objects over; it writes neither the names nor the outcome — the old `report.part("motion", State::Present, Provenance::Asserted)` had all three written by the board, so swapping a driver or miswiring a part changed nothing in the log.

```
[HW] panel             present      (declared)
[HW] button            present      (declared)
[HW] touch     0x38    present      (read)
[HW] motion    0x6A    present      (read)
[HW] mic       0x41    absent       (read)
[HW] speaker   0x18    degraded: not an ES8311 (read)
[HW] camera    0x3C    present      (read)
[HW] 7 devices: 5 present, 1 degraded, 1 absent; 5 read, 2 declared
```

- The second column is `Option<Address>`: the SPI panel, the RMT LED and the GPIO button have no address, so the column stays blank.
- `(read)` means one real read from the part (WHO_AM_I, chip id, the GC2145's ID, the ES7210's config register, the digitizer's GESTURE_ID); `(declared)` means the part has no readback path and only the board can declare it (panel, LED, button).
- **A line exists only for a slot the board actually wires**: seven on the S3, two on the C6. The C6 has no panel, digitizer, IMU, microphone, speaker or camera, so those six get no line at all — that would be a restatement of `Cargo.toml`, not an observation. A slot that is wired but did not come up (`Option` is `None`) still keeps its line as `absent (read)`, because that is an answer the driver got from the bus.
- The summary is counted from the lines themselves, so `N` disagreeing with the split can only mean a count was missed; a reader does not have to add them up.

## One knob for the level, and one place that holds it

`[test]` The parse of `configured_level` (`chip-esp`); `build_env.rs` holds `IOT_LOG_LEVEL` to being listed in `FORWARDED_ENV` in `scripts/iot-xtensa.sh`.

`IOT_LOG_LEVEL=error|warn|info|debug|trace`, `info` when unset, and **a name it does not recognise falls back to `info` rather than going silent**: a typo should cost the telemetry it was meant to raise, not every line the device prints. The level is read with `option_env!` in `logging.rs`, and `enabled()` reads `log::max_level()` rather than keeping a copy — two states are two answers that can disagree.

**Per-tag filtering was not implemented** (`IOT_DISPLAY=off` and the like): the tags are `[DISPLAY]` / `[PLAY]` / `[WIFI]`, one global knob is enough, and a second dimension adds a second "why is this line missing" to investigate. The tags keep that door open: filtering by `metadata.target()` inside `enabled()` is where it would go.

`init_logging` is implemented once in `chip-esp` and shared by both parts: neither the logger nor the level is a chip fact. It prints `[IOT] log level INFO (IOT_LOG_LEVEL)` at boot — **the only evidence of how quiet an image is**, so check it before concluding that nothing happened.

## Events are `info`, telemetry is `debug`, a crossed bound is `warn`

| Line | Level | Rule |
| --- | --- | --- |
| Boot inventory (`[HW]`, `[CREDS]`, `[HEAP]`, `[CLOCK]`, `[MOTION]`, `[DEMAND]`) | `info` | Exactly once per boot; the answer to "what is this board today" |
| State transitions (page, link phase and address, light, setup mode's fronts) | `info`, **only on change** | The same phase repeated is not an event |
| User actions (tap, hold, panel requests) | `info` | Someone pressed something; that has to leave a trace |
| Frame cost and stack peak, feed cadence, link heartbeat, scan lists, descriptor and register dumps | `debug` | Repeats per window; the steady-state answer does not change |
| One executor hold ≥ 30 ms, one frame write > 20 ms or > 25% duty, a late feed or under 95% of nominal, a degraded part, a DMA restart | `warn` | Only a crossed bound is an event |

When one sentence is both telemetry and a fault, `log::log!(level, …)` writes it once and gives it both (`paint.rs` frame cost, `input.rs` executor hold, `playback.rs` feed cadence). **`warn` lines stay in an `info` build on purpose**: the only output a product has is the panel, so a warn on the serial port is a reading that should not have happened, and its presence is the reason to read.

`[measured]` The boot-time `[INPUT] held the executor 1970 ms of 5003 ms … worst single hold 600 ms; worst source: motion 598 ms` is real cost: the motion driver waits for the QMI8658's first sample, and afterwards the worst single hold inside every 5 s window is under 5 ms, so that line drops to `debug`. **It keeps its `warn` shape because the first window really did cross the bound**, not because it is noise.

## A change comparison has to sit behind both entry points, or it reports the previous touch

`[measured]` Page changes used to be compared and printed inside the `Intent::Operation` arm only, while **the splash's end arrives as an `Intent::Business`** (`SplashOver`). The boot-time `Splash → Ambient` therefore never printed, `last_page` stayed at `Splash`, and the **first touch** reported the page that had been drawn long before: one run printed `[00:00:22.745] page is now Ambient` while the page had been Ambient since `00:00:02.7`. A reader would conclude someone turned a page at 22.7 s.

The comparison now sits outside the `match`, shared by both entry points: the same firmware prints `page is now Ambient` at `00:00:02.710` and the splash's end at `00:00:02.711`, and later touches repeat nothing. **Quieting is not enough on its own; the change detector has to cover every writer** — otherwise a quiet log keeps one correctly-worded event with the wrong timestamp, which is harder to spot than noise.

## Filtering happens before formatting

`log::debug!` **does not format its arguments** when the level is off; the cost is one atomic read and a branch, so demoted periodic lines barely touch the executor. That leaves one implementation rule: **arguments to a demoted line must be cheap** (`&'static str`, integers, struct fields already in hand) — do not `format!` or read I²C for a line that may not print. `[CAM] decim regs` sits on that boundary: it reads the `geometry` already in hand rather than re-reading registers.

`[measured]` **The lines that were removed do not save executor time**: before, `[INPUT] held the executor 1426 ms of 5002 ms over 1001 ticks`; after, the same window reads `1417 ms of 5000 ms` — within 0.2%. A UART write over USB CDC never entered that account. What this change bought is **the reader's time**, not CPU; saving CPU needs the level filter to skip formatting, which only matters for calls that carry something large by value.

## Tags follow "who is speaking"

`[MIC]` → `[AUDIO]`, `[SPEAKER]` → `[PLAY]`, `[TOUCH]` → `[INPUT]`, `[RADIO]` → `[WIFI]`, `[PSRAM]` → `[HEAP]`, `[BACKLIGHT]` → `[DISPLAY]`; the rest unchanged.

The rule is **one tag per module**, not one per peripheral: the microphone and the speaker are two ends of the same capture/playback path, touch and the button are two ends of one input path, PSRAM is the heap, the backlight is part of the panel, and the radio serves only Wi-Fi. Before the change `[MIC]` and `[AUDIO]` coexisted, as did `[SPEAKER]` and `[PLAY]`, so a reader had to know which tag was the driver and which the application.

## Known gaps

- **No post-mortem ring buffer**: the log only goes out over USB CDC, so whether the last lines before a panic or reset ever arrive depends on the host still reading. `esp-backtrace` prints its own backtrace on a different channel. Seeing the state before a reset means catching the serial port; nothing in this repository uses an `rtc_cntl` region to persist it.
- **`HPF_CORNER_WALK` is still `true` in the product build**, while the [audio record](@/records/iot/audio.en.md) says production builds turn it off. It walks 8 corner codes at 3 s each at boot; at `info` it leaves one line (`[AUDIO] HPF corner walk starting …`), but **it changes audio behaviour** (the working corner only comes back after the walk), so this pass left it alone: turning it off is an audio decision, not a logging one.
- The panel has no read-back, so `[HW] panel present (declared)` means exactly that: the log can show the driver accepted a configuration, not that the pixels are right. Every line carries `(read)` or `(declared)`, and the summary counts both, so a reader does not have to tally them.

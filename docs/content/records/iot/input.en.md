+++
title = "Input and Touch Implementation Record"
weight = 60
sort_by = "weight"
[extra]
source_file_hash = "d2e0c99dabdb32d0da4bef1d8cbd4fd1cd4f172d"
translated_at = "2026-11-01T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Input and Touch Implementation Record

The four polled input paths in `apps/iot` — button / touch / motion / capture — plus gesture recognition and the page cycle. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is labelled by its evidence: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

To reproduce: from `apps/iot/`, flash the default firmware and read `[INPUT] page is now` (one line per page change) and `[INPUT] held the executor …` over serial. The touch path **has to be exercised by a hand on the glass** — the FT6336's touch registers cannot be faked from the host — so all the evidence in this section is a person plus the hardware together.

---

## The page cycle is right on hardware, and a 2-second window cannot show the order

`[measured]` To reproduce: flash the default firmware, ask someone to tap through `Ambient → Attitude → Audio → Speaker → Camera` and back round, repeat for a few minutes, and read `[INPUT] page is now`.

The verdict over one run of 46 transitions:

- **0** consecutive repeated pages
- **0** transitions deviating from `DisplayPage::CYCLE`
- all five pages appeared
- one physical tap = one transition, which is what the zero repeated pages proves

`[test]` `every_page_the_board_can_back_is_shown_on_the_way_round` (`core/src/state/mod.rs`) has pinned the order table on the host for a while, so what this measurement adds is not "is the order right" but that **the driver, the recognizer, the intent, the state and the renderer really do advance one step at a time on hardware.**

`[measured]` One **wrong reading** is worth recording, because it changed how I read logs: `[DISPLAY]`'s cost report is a **2-second window** and only names the page at the end of it. `Ambient → Attitude → Audio` can all happen inside one window, and from "the window ended on Audio" I concluded the pages were jumping. **The person was right and the 2-second window was not** — the same suspicious observation was settled by eye.

Hence one line per page change: `[INPUT] page is now <page>`. Two details were learned the hard way:

- **On the change, not on the operation.** The capture path publishes an `Intent::Operation` on every cadence tick, so a line per operation is a line per 20ms.
- **The page name, not the whole event.** An `AudioSample` carries a 200-column envelope, so printing it is 1.6 KB a line and tens of MB of noise per capture.

That line also makes "did one gesture become several operations" falsifiable — the repeated-page count is the test.

---

## Only one of the four polled paths can be switched off

`[measured]` `[INPUT] held the executor … worst single hold … worst source: …` reports the whole tick, its longest single hold, and each path's worst time. The stable shape is `button 0 ms, touch 1 ms, motion 3 ms, capture 16–20 ms`. Capture is the largest because folding a sample runs a sixth-order IIR.

`[test]` The interaction path is off-limits by design (`poll_plan_of` returns `active: true` outright for `DemandKind::Interaction`). Measured floor: **10.87%** on a build with every device `Off`, over 8 windows of 5 s within 0.1 point — all of it button and touch polling.

`[measured]` That floor survives switching everything off, because it has nothing to do with demand: **a device given nothing still has to answer the button.** So it is a theoretical minimum rather than tuning headroom.

`[measured]` Whatever this side gives up under throttling, the animation clock takes: batching only motion to 30 s, the input side drops from 49.5% to 35.5% (equivalent to the old `IOT_BUDGET=thrift`, see the [audio record](@/records/iot/audio.en.md)) while `[DISPLAY]` goes from 45.3% to 58.5%, for a device total of 94.8% → 94.0%. So "the input side got cheaper" is not "the device got cheaper" — their sum is the device's occupancy.

`[measured]` And the `[DEMAND]` poll side now keeps up with it: scaled to the cadence actually installed it reads 497,300 against a measured 494,600 (+0.6%) on the default build and 340,104 against 354,800 (−4.1%) under thrift. Before the scaling it read 497,300 for thrift — 40% high.

`[measured]` Throttling's effect on the worst single hold: 22–23 ms in the default build (capture due every tick) and 17–19 ms under thrift, dropping to 1 ms in the windows where neither batched source was due. `MAX_SINGLE_HOLD_MS = 30` is not breached in either.

`[test]` The capture path may not be batched (`Es7210Rx::accepts_slower_poll()` answers `false`), because the ring is emptied only by `sample()` — see the [audio record](@/records/iot/audio.en.md).

`[measured]` `capture 0 ms` is not "cheaper", it is **a ring with nothing in it** — see the failure chain in the [audio record](@/records/iot/audio.en.md). Reading that line, `0 ms` and `1 ms` mean opposite things, and the first one is a fault.

---

## I2C goes from 100 kHz to 400 kHz, and the input side's hold halves (2026-11-01)

`[measured]` This bus (PCA9557 button, FT6336 touch, QMI8658 motion, ES7210 and ES8311 codecs) ran on esp-hal's default **100 kHz**, and every poll is a blocking `write_read` straight off the cooperative executor. Raising it to fast-mode **400 kHz** (`I2C_FREQ_HZ` and `i2c_config()` in `lckfb_szpi_esp32s3/mod.rs`):

| window | 100 kHz | 400 kHz |
| --- | --- | --- |
| `[INPUT] held the executor`, 5 s steady state | 1,293–1,302 ms (26%) | **531–574 ms (11%)** |
| same, for the 5 s window holding the boot association | 1,871 ms | 1,147 ms |
| worst single hold, steady state | 4 ms | 1–3 ms |
| `[DISPLAY]` side, same windows | 8–14% | 4–11% |

`[measured]` All of it comes out of polling: the frame-write window reads the same on both (`worst write 15–17 ms`, i.e. 153,600 B / 80 MHz). The 11% steady state is already below the "every device `Off` floor" of 10.87% quoted above — that floor was measured at 100 kHz.

`[measured]` Acceptance on the board (`/dev/cu.usbmodem1442301`): `[HW] 7 devices: 7 present, 0 degraded, 0 absent`; `[AUDIO] ES7210 serial: 16-bit word, I2S`, `[PLAY] Tap`, `[CAM] chain verified: 40 descriptors` and `[WIFI] Connected, address …` all unchanged; 70 s with no `[INPUT]`/`[DISPLAY]` line over bound (at 100 kHz every window printed one).

`[measured]` **The 597 ms is not something this change fixes**: the same 597–601 ms single hold shows up at both clocks, lands on whichever source was polling (once `touch 597 ms`, once `motion 599 ms`), appears once in the boot window and never again. So it is not touch-driver cost — the whole core stops for that long once during association/DHCP, and the poll just happens to be the code that was running.

---

## The touch coordinates need one frame change, and `IDENTITY` was wrong all along

`[reported]` This module's digitizer reports the **panel controller's own 240×320 portrait window**, while the panel is addressed as 320×240 landscape (`MADCTL = MV|MX`): the two differ by a **transpose plus a mirror**, and the correct net mapping is

```
raw(rx 0..240, ry 0..320)  →  fb(x, y) = (ry, 239 − rx)
```

Before that, the FT6336 was handed `TouchMap::IDENTITY`, so a single touch read as `(rx, ry) = (239 − fy, fx)`: the **column came from the finger's vertical position and the row from its horizontal one, with the column mirrored**. Tapping `7` (`pqrs`, box centre `(96,133)`) hits `(106,96)` — the `2` (`abc`) key; a list band came from `fx` (`(fx−47)/61`, the vertical position ignored entirely); and a physical up/down swipe on the light pages classified as `Left`/`Right` — changing the colour rather than the brightness.

Two independent sources:

- The vendor's touch configuration for this panel (wiki chapters 9/11; the `08-lcd_lvgl` and `14-handheld` examples carry the same one, as does a third-party LVGL port blog): `.x_max = BSP_LCD_V_RES(240)`, `.y_max = BSP_LCD_H_RES(320)`, `swap_xy = 1`, `mirror_x = 1`, `mirror_y = 0`, paired with the display configuration `swap_xy(true)` + `mirror(true, false)` — that is `MV|MX`, this repository's `MADCTL_LANDSCAPE = 0x60`.
- Upstream `esp_lcd_touch.c` transforms in the order **mirror first (using `x_max`/`y_max`, so on the raw axes) and swap last**, while `esp_lcd_touch_ft5x06.c` hands the registers up untouched; the two compose to exactly `(ry, 240 − rx)`.

So this end's value is `bsp-esp::components::ft6336::LANDSCAPE_MAP`: `TouchMap { swap_xy: true, mirror_x: false, mirror_y: true }`. Its `mirror_y` is the **post-swap** `y` (`TouchMap::map` swaps first and mirrors after), which is the same act as the vendor's `mirror_x`; this repository uses `239 − rx` rather than the vendor's `240 − rx`, because the latter puts the top row outside the framebuffer.

`[test]` Two of them, in `bsp-esp/src/components/ft6336.rs`, run by `iot:test-bsp` (which is why `ft6336` was added to that task's feature list — it was the one bsp driver with no host coverage at all):

- `a_finger_lands_where_the_panel_shows_it`: writes "a finger at (96,133)" as the raw coordinates the inverse map implies and feeds them to `read_sample`, asserting that what comes out is still (96,133). The input side is derived from `LANDSCAPE_MAP`, so putting the constant back to `IDENTITY` turns this one red.
- `the_panels_own_frame_is_transposed_and_mirrored`: the four corners, `(0,0)→(0,239)`, `(239,0)→(0,0)`, `(0,319)→(319,239)`, `(239,319)→(319,0)`. A transpose is invisible in the middle of either axis, so only the corners pin it.

The mistake survived this long because **every touch behaviour verified on hardware until now was direction-agnostic** (the page cycle: any tap counted as one transition), and each list row is a full-width band — a horizontal error is only obvious in the keyboard's **columns**.

**Not verified yet**, because software cannot synthesise touch and only the FT6336 produces panel coordinates: tapping the `abc` key must type `abc`, tapping `7` must type `7`, and a physical up/down swipe on the light pages must change the brightness.

`[measured]` This build is already on the S3 (bench image, `IOT_PORT=/dev/cu.usbmodem1422301`): `App/part. size: 880,112/6,291,456 bytes`, `[IOT] boot page is Network` at boot, no panic, and `[DISPLAY] Network 1 repaints in 5901 ms, held the executor 18 ms (0%)` — **so the board is running this build**, and the item above owes a finger, not a flash.

---

## Catch-up lands in one division; a zero cadence no longer spins (`[test]`, 2026-10-28)

`PollEntry::advance_past` moves "the next slot" past `now` after every poll (`app/src/input.rs:212`, inside the input task's loop). It used to step `+= cadence` cell by cell: a source that had been off for an hour (`set_active` parks the next slot at 0) walked 180k of them, on the main task's stack. Now one division lands on the same cell — `a_source_that_resumes_after_an_hour_lands_on_the_next_slot_once` holds it to "the first slot, not several cadences late".

`cadence_ms == 0` is not a value any board wires (`set_cadence_ms` refuses anything below the board's floor, see `a_budget_can_never_shorten_a_cadence_the_board_wired`), but the constructor takes any value and the old walk looped forever on it. It now lands at `now + 1`: this tick does nothing, the next one decides. `a_zero_cadence_moves_the_schedule_instead_of_dividing_by_zero` holds it to that.

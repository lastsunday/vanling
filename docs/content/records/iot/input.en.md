+++
title = "Input and Touch Implementation Record"
weight = 60
sort_by = "weight"
[extra]
source_file_hash = "8f56a82581811e4db3339ce3153bb19e58c78945"
translated_at = "2026-10-18T00:00:00Z"
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

`[test]` The interaction path is off-limits by design (`poll_plan_of` returns `active: true` outright for `DemandField::Interaction`). Measured floor: **10.87%** on a build with every plane `Off`, over 8 windows of 5 s within 0.1 point — all of it button and touch polling.

`[measured]` That floor survives switching everything off, because it has nothing to do with demand: **a device given nothing still has to answer the button.** So it is a theoretical minimum rather than tuning headroom.

`[measured]` Whatever this side gives up under throttling, the animation clock takes: batching only motion to 30 s, the input side drops from 49.5% to 35.5% (equivalent to the old `IOT_BUDGET=thrift`, see the [audio record](@/records/iot/audio.en.md)) while `[DISPLAY]` goes from 45.3% to 58.5%, for a device total of 94.8% → 94.0%. So "the input side got cheaper" is not "the device got cheaper" — their sum is the device's occupancy.

`[measured]` And the `[DEMAND]` poll side now keeps up with it: scaled to the cadence actually installed it reads 497,300 against a measured 494,600 (+0.6%) on the default build and 340,104 against 354,800 (−4.1%) under thrift. Before the scaling it read 497,300 for thrift — 40% high.

`[measured]` Throttling's effect on the worst single hold: 22–23 ms in the default build (capture due every tick) and 17–19 ms under thrift, dropping to 1 ms in the windows where neither batched source was due. `MAX_SINGLE_HOLD_MS = 30` is not breached in either.

`[test]` The capture path may not be batched (`Es7210Rx::accepts_slower_poll()` answers `false`), because the ring is emptied only by `sample()` — see the [audio record](@/records/iot/audio.en.md).

`[measured]` `capture 0 ms` is not "cheaper", it is **a ring with nothing in it** — see the failure chain in the [audio record](@/records/iot/audio.en.md). Reading that line, `0 ms` and `1 ms` mean opposite things, and the first one is a fault.

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

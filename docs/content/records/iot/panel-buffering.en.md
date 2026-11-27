+++
title = "Panel double buffering: measured, rejected"
description = "A PSRAM staging copy raised the panel from 24 to 28 fps but doubled the coordinator core to 84% and produced a 674 ms frame in the Wi-Fi association window; the only memory that could hold two panel frames is internal DRAM, with 16.7 KB free. The real cause was the lamp's breath repainting the panel; it is now a runtime `GlassBreath`."
weight = 25
sort_by = "weight"
[extra]
source_file_hash = "8a06037cdcfd740d7649d10edda772e5afe77f6f"
translated_at = "2026-11-27T02:00:00Z"
+++

<!-- doc-audience: ai -->

# Panel double buffering: measured, rejected

Whether the panel path in `apps/iot` can overlap painting with the SPI transfer by double buffering. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Evidence follows this domain's convention: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

## Verdict

`[measured]` **Not doing it.** The panel stays single-buffered: the model is the board's 153,600 B DRAM arena (shared with the camera ring), and a painted frame is posted and awaited directly. Copying each frame into a PSRAM staging area buys +16% frame rate at twice the coordinator core, plus one **674 ms** frame inside the Wi-Fi association window — a visible freeze that recurs on every reconnect.

`[measured]` The trial also found the real waste: the repaint was triggered not by the page but by the **lamp's breath**. Once the breath is decoupled from the lamp, a steady page's core drops from 40% to 0–1% (see "Breath repaints" below), with no double buffering.

## The two paths

- **Single buffer (current)**: the paint writes the DRAM model (19–20 ms on the Menu page); after posting, the coordinator parks until the SPI transfer finishes (15–16 ms). Parking is idle time, and other tasks keep running.
- **Staging double buffer (this trial)**: the model stays in DRAM and a 153,600 B staging block is allocated from the PSRAM heap; each frame copies its damaged pixels into the staging block, posts that, and the next paint overlaps the previous wire. Camera frames are the exception — the ring *is* the panel frame, so they are still posted and awaited directly.

## Measurements (same tree, same page, one window per 2 s)

Menu demo page (no `IOT_BOOT_PAGE` at build time), three steady-state `[DISPLAY]` windows:

| Metric | Single buffer | Staging double buffer |
| --- | --- | --- |
| repaints / 2 s | 47–49 | 56–58 |
| frame rate | ≈24 fps | ≈28 fps |
| `held the executor` | 40% (16.8 ms/frame) | 82–84% (29.6 ms/frame) |
| `worst render` | 19–20 ms | 31–33 ms |
| `worst wait` | 19–22 ms | 0 ms |
| `worst write` | 15–16 ms | 16–17 ms |
| `longest chunk` | 1 ms | 2 ms |
| worst single frame in the association window | 25 ms | **674 ms** |
| `_stack_end` / main stack | `0x3fcd5598` / 82,296 B | identical |

> The 25 ms comes from the first window of the same build with `IOT_LOG_LEVEL=debug` (an `info` build does not print a first window that stays inside budget); every other number is from an `info` build.

- The copy itself `[measured]`: the `worst render` difference is 12–13 ms — 153,600 B written into PSRAM (a write miss reads the line first, about 12 MB/s effective).
- The 674 ms frame `[measured]`: it landed in the same window as the Wi-Fi association; the single-buffer build peaked at 25 ms in that window. The stall was not attributed layer by layer; the most likely cause is PSRAM and flash sharing the MSPI, the same class of 600 ms stall already recorded in `camera.en.md`.
- Net effect: frame interval 41 ms → 35 ms (−6 ms). The 20 ms saved was **idle**; the 13 ms spent is **held** — hence +16% frames for +40 percentage points of core, leaving 18% for timers and the Wi-Fi worker.
- `worst write` and the chunk time rose slightly: DMA reads from PSRAM are slower than from DRAM.

## Why it does not fit in fast memory

`[measured]` Two full frames need 307,200 B. Internal DRAM on this board has no slack: `dram_seg` headroom is 0, and the main stack has **16,760 B** between its current 82,296 B and this task's 65,536 B floor. PSRAM does fit (a 512 KiB heap; two frames use 59%), but per the numbers above its write bandwidth and MSPI contention are exactly the problem.

Striped double buffering (LVGL's partial-buffer semantics) is the only theoretical alternative: 16 KB is about 34 rows, the painter would have to draw band by band with rewritten clipping and layout, and the ceiling is the same — overlapping the 19 ms paint with the 16 ms wire. Not implemented.

## To raise the panel frame rate, cut the paint first

`[measured]` A single-buffered frame is paint 19–20 ms + wire 15–16 ms. Double buffering can only touch the second half, and cannot touch it without PSRAM. The first half has no such limit: the Menu demo's paint is 19 ms (`fill` is only 1–2 ms), and cutting that is the only path that needs no extra memory.

## Partial repaint: the framework derives the damage

`[measured]` A page no longer names its repaint rectangles. After layout the framework fingerprints every node twice: `structure` (node count plus each node's kind, rectangle and clip) and `content` (a field-by-field hash of `Style`, the text bytes, and a custom slot's `state`). The previous frame keeps only `FramePrint { structure, content: [u32; 64], len }` (about 262 B, on the stack); a different structure is a whole frame, otherwise only the nodes whose content moved are repainted inside their own rectangles (intersected with their clips). `Ui::custom_state(style, slot, state)` lets a page answer "is this the same picture?" with one `u32`; `Ui::custom` says the page does not describe its pixels, so the slot is repainted whenever the snapshot moves.

`[measured]` A partial frame first lays the ground back down inside the damage (page text is drawn by turning the ground over, so painting the same block twice would toggle it), then redraws every node that intersects the damage in tree order, so a card or scrim drawn later covers it again. Each dialog column declares `Style::cover()` (it paints its whole rectangle), which lets the framework skip the nodes it hides; without it the page text behind the card is re-typeset every frame and the dialog wheel drops to ≈35 fps, and with it the wheel is back at 50 fps. The `Picker` scrim is still the four bands around the card, so a body repaint touches no blend.

`[measured]` Same S3, `320×240`, debug build, one window per 2 s (a temporary bench drove the wheels; removed before landing):

| Metric | Whole-frame baseline | Dialog wheel (one column + footer) | Inline hour wheel (wheel + reading line) |
| --- | --- | --- | --- |
| repaints / 2 s | 38 | 91–99 | 100–101 |
| frame rate | ≈19 fps | ≈45–50 fps | ≈50 fps |
| `held the executor` | 66% | 46% | 26–29% |
| `worst render` | 36 ms | 10–15 ms | 5–9 ms |
| `worst write` | 10 ms | 4–5 ms | 3–4 ms |

50 fps is the `STEP_MS = 20` ceiling, not the renderer's ability; the whole-frame column is the same bench's baseline. There is no gain beyond frame rate: the pixels are identical to a whole-frame repaint point for point (a test compares a partial frame with the same frame painted whole).

## Breath repaints: the cause was not the buffer

> The three paragraphs below record the implementation as of mid-November 2026; the entry is now `DeviceState.glow` plus `LightingView.breath` (`GlowScope` and `GlowPattern`), and the backlight is independent and dark at 0. See `development/iot/architecture.md`.

`[measured]` What repainted the glass every step was not the page but the lamp: `ColorOutput::step` → `Screen::set_rgb` → `fill` + `page_dirty`, so one lamp breath redrew the whole panel (19 ms of paint plus 15 ms on the wire). Pages with their own `ground()` — Camera, Network — stayed still, which is exactly what a background following the lamp would look like.

`[measured]` With the lamp and the glass decoupled, a steady page (Menu, `IOT_LOG_LEVEL=debug`) reports 2–4 repaints per window, `held the executor` 0–1%, `worst render` 17–21 ms and `worst write` 15–16 ms; the same page before the change reported 47–49 repaints per 2 s and 40%.

A glass breath is now an explicit capability, `GlassBreath { period_ms, hue, saturation, min_value, max_value }` (`shared/model/light.rs`). `DeviceState.glass_breath` is the global (alarm) entry, `PageSnapshot.glow` the page-private one, and the projection composes them into `FrameSnapshot.glass_breath` (global wins). The default is `None`: entering a page or changing the lamp colour lays one still ground, and `step()` returns `Idle`. Pages with their own `ground()` (Camera, Network) still fill the glass themselves, so an alarm breath does not cover them.

The demo page `lighting` (`PageId::Lighting`, formerly `breath`) offers the `CALM` (3 s warm white) and `ALERT` (800 ms red) presets, `SLOWER`/`FASTER` (±500 ms, 500–5000 ms), `DARKER`/`BRIGHTER` (both envelope ends move ±32, floored at `min ≥ 0` and a peak `max ≥ 32`; a vertical swipe does the same) and `EVERY PAGE` (writes the current parameters into the global, a second tap clears them). Booting it with `IOT_BOOT_PAGE=lighting` measured 18–26 repaints per 2 s (≈12 fps, bounded by the `REPAINT_STEP=12` colour threshold, not by a cadence cap), `held` 9–14%, `worst render` 11–13 ms and `worst write` 15–16 ms.

`[measured]` The backlight path at the time: `Off` 0, still 100, a glass breath riding the phase between `BACKLIGHT_FLOOR_PCT..=100`, `OnChange` clamped to 12%. It now takes only `Backlight`, no longer follows the breath, and 0 is dark. The lamp (the C6's WS2812) still breathes as before; the glass no longer follows it. Making the backlight follow the lamp's brightness instead was not done: the default boot breath's `max_brightness=80` would dim every screen to about 72%, more risk than the change is worth.

## Reproduction

Both builds use `--no-default-features --features lckfb-szpi-esp32s3,console-auto` and the same S3 board (`lckfb-szpi-esp32s3`):

```sh
IOT_PORT=/dev/cu.usbmodem1442301 moon run iot:flash-s3
espflash monitor --port /dev/cu.usbmodem1442301 --non-interactive
```

Read the line printed every 2 s: `[DISPLAY] … repaints in … held the executor …%, worst render … ms (fill …), worst wait … ms, worst write … ms (longest chunk … ms)`. The first boot line, `[DISPLAY] backlight dark until the first frame`, confirms the panel path is running. The acceptance gate set before the trial was: `worst write` ≤17 ms, no visual corruption, no 600 ms-class stall in the Wi-Fi association window. The third one failed, so the design was rejected.

For a retry, the staging build touched `virtual_components/screen/{mod,paint,panel}.rs` (PSRAM staging block plus `copy_from`) and added `FrameDamage::copy_from` in `core/drivers/surface.rs`.

The two breath measurements come from the same S3 with `IOT_LOG_LEVEL=debug`, so steady-state windows print too. Build-time values do not enter moon's cache key, so run the script directly:

```sh
cd apps/iot
IOT_LOG_LEVEL=debug bash ../../scripts/iot-xtensa.sh build --release \
  -p iot-app --bin vanling --target xtensa-esp32s3-none-elf \
  -Z build-std=core,alloc \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto
# demo page: the same build plus IOT_BOOT_PAGE=lighting (formerly breath)
```

With `IOT_LOG_LEVEL` left at `info`, a window inside budget prints nothing (`over_budget` only looks at 25% and 20 ms), so "no line" on a steady page is itself the passing signal.

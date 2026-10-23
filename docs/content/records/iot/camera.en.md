+++
title = "Camera Implementation Record"
weight = 30
sort_by = "weight"
[extra]
source_file_hash = "dbf577be03324a4b213942aa9366b8ddc2f359a7"
translated_at = "2026-10-23T03:49:18Z"
+++

<!-- doc-audience: ai -->

# Camera Implementation Record

Judgement records from the GC2145 capture path and the ST7789 panel wiring in `apps/iot`, established on real hardware. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is labelled by its evidence: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

Reproduction is the same throughout: flash `camera-probe` from `apps/iot/` and read the `[PROBE]` and `[CAM]` lines off the serial port. Every 2 s the probe prints `sensor exposure`, `blanking h/v` (measured page-0 `0x05`–`0x08`) and the window's KB/s — every frame-rate claim below rests on those three lines.

## The sensor on this board is a GC2145; the LCKFB wiki's GC0308 is wrong

`[measured]` This is the first hardware fact in this record, and the first time the whole capture chain has run on a real board.

The vendor documentation ([LCKFB Shizhan ESP32-S3 camera chapter](https://wiki.lckfb.com/zh-hans/szpi-esp32s3/beginner/camera.html)) says the board ships a **GC0308**; the repository drives a **GC2145**. The two are not just differently named:

| | GC2145 (what the code drives) | GC0308 (what the wiki claims) |
|---|---|---|
| Chip ID register | `0xf0`/`0xf1` on page 2 | **`0x00` on page 0**, value `0x9b` |
| Array | 1600×1200 | 640×480 |
| Page-select mechanism | different | different |

So "the code is wrong" and "the wiki is wrong" would not look alike: on a real GC0308 the repository's `read_id()`, which selects page 2, would read back **garbage** rather than `0x0308` — and `lckfb_szpi_esp32s3/camera.rs` **logs that ID without checking it**, which is the silent-failure class this repository keeps running into.

Measured (`moon run iot:probe-camera-s3`, flashed to the S3):

```
[00:00:00.850] [CAM] sensor id 0x2145
[00:00:00.851] [CAM] sensor reports 320x240 out, read 1616x1208 at 0,0 ratio 5:5, format 0x06
[00:00:02.892] [PROBE] sensor exposure 486 lines
[00:00:02.896] [PROBE] painted 10, 9 new in 2000 ms = 691 KB/s of new sensor data
```

**The ID is `0x2145`, with a 1616×1208 read window at 5:5** — GC2145's shape, not GC0308's 640×480. And 691 KB/s of new sensor data means it is genuinely delivering frames rather than answering register reads. **The code is right and the wiki is wrong.**

This also says something structural: **a disagreement between a driver that has never run on hardware and a vendor document can only be settled by measurement** — and here the code won. It equally says that `read_id()` logging without checking is a real gap; see the same class of problem once more in `aec.en.md`.

---

## Field of view is arithmetic, not a register

`[measured]` **Field of view = output pixel width ÷ array width, and no register changes it.** A 320-pixel-wide output can cover at most a fifth of a 1600-wide array. Changing the ratio, the bins, the scalar or the margins does not move that number. It is the identity "as many pixels out as you want to see".

`[measured]` Ratio 5 applies to the whole 1600×1200 array, and `1600÷5` and `1200÷5` are both whole, so `320×240` **is** the decimated image and `out_win` crops nothing off it. That is the full field of view.

`[measured]` Which refutes "a bigger output buys a wider view": `800×600` at ratio 2 also divides exactly and is also full width, it just carries 6.25× the pixels to throw away. It was implemented (streaming capture with a 5:2 box downscale); the view did not widen and it cost three new faults. There is therefore no 800-wide path in the driver.

`[test]` The divisibility is a compile-time assertion (`ARRAY_WIDTH / EXTRACT_RATIO == POWER_ON_OUT_WIDTH`), so a ratio that stops dividing does not compile.

---

## The bins decide the view, at the cost of frame rate

`[measured]` `0x9b`–`0xa2` (`Sub_row_N1`…`Sub_col_N4`) name **which rows and columns inside each decimation bin are kept**. Zero averages the whole bin; `0x01`/`0x23` keeps one fixed offset per bin. Measured, the difference is large:

| bins | field of view | frame rate | DMA |
| --- | --- | --- | --- |
| `0x01`/`0x23` | about 1/5, **visibly magnified** | 10 fps | 1558 KB/s |
| `0x00` | **the whole array** | 4.5 fps | 691 KB/s |

**On this sensor full field of view and frame rate are two ends of one switch.** `esp32-camera`'s own table comment says it: `A smaller ratio brings a larger view, but it reduces the frame rate`. This repository writes `0x00`.

`[reported]` Why averaging a 5×5 bin costs twice what taking one point does is unexplained — there is no material on the sensor's DSP.

---

## Where the read window's margins come from

`[measured]` `0x09`-`0x10` are the read window's origin and size, and the part **counts its margins into its own line timing**, so the size written in code has to include them. The values are **+8 vertically, +16 horizontally**, from `esp32-camera`'s `set_framesize`:

```c
write_reg(sensor->slv_addr, 0x0d, H8(win_h + 8));
write_reg(sensor->slv_addr, 0x0f, L8(win_w + 16));
```

`[measured]` `+16` was used at one point, on the grounds that the datasheet gives these registers' reset values as 1216 rather than 1208 (i.e. `1200 + 16`). **That reset value describes the part sitting in its own default window**, not the state after code set it to 320x240; the vendor driver computes the margins from the window it just chose. Reverting to `+8` made the view correct.

`[measured]` 16 horizontally against 8 vertically is not a typo — those are the two values the vendor driver writes for QVGA.

---

## Measured throughput and detail per ratio

`[measured]` Once the decimation ratio in `0x99` is written, its effect can only be inferred from how many bytes the DMA receives: if the ratio takes effect the byte rate falls roughly in proportion, and if it does not, nothing moves. Measured one ratio at a time:

| ratio | read window | DMA | distinct values per row |
| --- | --- | --- | --- |
| 5 | 1600x1200 | 1558 KB/s | 186 |
| 3 | 976x736 | 3444 KB/s | - |
| 2 | 656x496 | 3474 KB/s | - |
| 1 | 336x256 | 4641 KB/s | 0 (no frames) |

`[measured]` **ratio 1 produces no frames at all**; ratios 3 and 4 produce frames but each row carries only about half the distinct values ratio 5 does — the frame is right, the pixels are undersampled. Ratio 5 is the only one that reads the full array.

`[reported]` Why ratio 1 yields no frames while 3 and 4 undersample is not explicable from the registers.

---

## Scalar mode: double the frame rate, narrower view

`[measured]` `0xfd` (`Scalar mode`, bit 0 row scalar, bit 1 column scalar) **doubles the frame rate from 5 to 10 fps with every other register unchanged.** It replaces "skip a row at a time and average the bin" with one resampling step.

`[measured]` **But the picture comes back narrower than the decimation path it replaced, not wider.** `0x95`–`0x98` name a rectangle of the read window however the part gets there, and a 320-wide rectangle over a 1616-wide read window is a fifth of the array either way.

`[measured]` Every reference driver pairs it with `out_win` as though it made the output size independent of the read window (sunxi's `sensor_svga_regs`, rockchip's `sensor_preview_data`, Linux mainline's `gc2145_mode_640_480_regs` all write `0xfd = 0x01`, while the one full-resolution mode, `1600x1200`, writes `0x00`).

**So this driver never writes `0xfd`**, leaving it at the power-on table's `0x00` — but it **reads it back** (`WindowGeometry::scalar`) so a caller can confirm the part really is running with it off.

---

## `0x9a`: the two values differ in nothing measurable

`[measured]` `0x06` (the datasheet reset value, which Linux mainline and the ST 640×480 mode both write) and `0x0e` (`esp32-camera`'s power-on value, which its `set_framesize` never touches) agree on field of view, byte rate (`691`/`768`/`839` KB/s in a 2 s window for both) and frame rate. The only difference is bit 3, Y interpolation.

This driver takes the datasheet's `0x06`.

---

## The unit of `0x95`–`0x98` is still unconfirmed

`[reported]` The datasheet calls these four registers `Out window height/width` and **does not say whether they are sensor pixels or output pixels**, and three references give opposite answers: Zephyr and ArduCAM write the **output** size, while the sensor's own power-on table and the only Linux mode anybody has verified working (`640x480`) are consistent with the **read** size.

`[measured]` One experiment to tell them apart was run and **could not**: writing `320×240` and writing `1600×1200` to `out_win` gave identical DMA byte rates — the readback changed, the rate did not. So the parameter was removed from the API, leaving only the "write the output size" behaviour, with a note that the unit is unconfirmed.

`[reported]` The theoretical way to tell would be `EofMode::VsyncSignal`, taking the boundary from the part rather than from a byte count. It raises one EOF per frame while a descriptor here is a fortieth of a frame, so it would ask the first descriptor to hold a whole frame and overrun it.

---

## A circular descriptor chain does not run on this chip

`[measured]` Making the descriptor chain a **circle** — the last descriptor pointing back at the first — removes re-arming entirely, and with it the bytes lost in each re-arm gap. Measured: **28 KB/s, against 645 KB/s for a line.** The peripheral does not run a lap; the ring fills once and afterwards only the stall path re-arming makes progress. **Reverted; the line stays.**

`[measured]` The line's gap really does lose bytes: with the same sensor configuration a 61 KB ring gives the DMA 645 KB/s and a ring back to one frame (153 KB) gives 1558 KB/s. **One lost byte offsets every row after it, and the picture arrives as horizontal bands — "venetian blinds" — that move.** So the ring must be a whole frame; the current 320×240 frame is 153,600 B and `camera-probe` reports 10–11 re-arms per 2 s (about 5.2 fps). The band used 76,800 B per frame, measured 9.5–10 fps and 19–20 re-arms per 2 s; that scheme has been reverted.

---

## Internal DRAM is not a memory shortage; the DMA cannot reach PSRAM

`[measured]` The ring must be in internal DRAM. The ESP32-S3 has 8 MB of PSRAM, ample for this 150 KB arena, but:

- Espressif's own `ll_cam.c` gates the receive descriptor and data burst bits under `//internal SRAM only`, leaving both clear for PSRAM;
- esp-hal **cannot express that split**: `set_descr_burst_mode` is called with a literal `true` on every receive, and `BurstConfig::is_burst_enabled` is unconditionally true on the PSRAM path.

A PSRAM ring is therefore always left in the configuration Espressif declines to use, and the part streams scrambled data into it. The ESP32-S3 camera issue tracker describes the same field: `the artifacts you see are exactly missing bytes in the frame`. **No setting reaches the official state.**

`[measured]` The descriptor list may **not** point into PSRAM either.

`[measured]` In internal DRAM the alignment constraint is the 32-byte cache line (`DMA_ALIGNMENT_BYTES`), not the external burst block.

---

## The ring's 150 KB used to hang off the feature composition; it no longer does

`[measured]` Those 150 KB are now **the panel's frame buffer itself**. `FRAME_ARENA` (formerly `FRAME_RING`) is unconditionally in `.bss`, `camera` is in the board feature, and `--gc-sections` no longer has any opportunity to drop it — so the 150 KB went from "whatever a link-time GC decides" to a link-time fact. `cargo check` does not link and `check-s3` cannot see it, so only `moon run iot:image` / `build-s3` can falsify this section.

`[measured]` What was saved is not DRAM but the per-frame 150 KB memcpy (measured frame rate 5.5 fps at 320×240, about 4.5 ms each): the panel's former `Vec` became this static arena, and the two are the same size, so `.bss` grows by 14,336 B net. In DRAM terms "one buffer" and "both buffers" are equivalent; they differ in CPU time and in ownership coupling.

---

## The camera fills the whole panel, with the readout inverted over the picture

`[test]` (compile-time assertions) The Camera page's picture is the panel's whole frame: `dvp::FRAME_WIDTH/FRAME_HEIGHT` are the panel's 320×240, and `dvp::RING_BYTES` equals `FRAME_ARENA`'s 153,600 B. The board asserts width, height and ring byte count all match the panel, so the picture fills the glass; the page keeps no black header, and there is no longer a "readout rows must not intersect camera rows" constraint. `FRAME_ARENA` is still the panel's frame buffer itself; the camera ring costs no extra memory.

`[test]` (compile-time assertions) The sensor is configured with `set_window(320, 240, 5)`: a 1600×1200 read window at 5:5, the `RD` readback is 1616×1208 including the margins, and the output is 320×240. The descriptors slice one frame into 40 chunks of 3,840 B; each chunk is a whole multiple of the 32-byte DMA alignment.

`[measured]` Full-screen frame rate: `camera-probe`'s `[PROBE] painted` reports 10–11 new frames every 2 s at `691`–`844 KB/s`, i.e. **5.0–5.5 fps**; `[PROBE] capture` reports 10–11 re-arms in the same window with 40 descriptors; exposure is still 486 lines. The product booted straight into Camera (`IOT_BOOT_PAGE=camera`) measures `11 repaints in 2119 ms` (about 5.2 fps) and `stack peak 43224 B of 66360 B`. Reproduce: `IOT_PORT=… moon run iot:flash-probe-camera-s3` and read `[PROBE] painted … KB/s`; for the product read `[DISPLAY] Camera … repaints`.

`[measured]` The picture has been confirmed by eye: a full-screen 320×240 sensor picture with the readout inverted over it (`CAMERA`/`WIN`/`OUT`/`RD` and `FRM`/`RM`/`MOD`/`FP`), and no black header. The FPS badge is back in the bottom-right corner where every other page puts it (`FPS_Y = 232`), not on row 4.

`[test]` The ownership rule is unchanged: while the Camera page is up the panel refuses CPU fills (the `Camera` variant of `FrameOwner`, covered by five unit tests). The camera path stamps the readout into the same frame after `advance` reports a frame and before the next re-arm, then ships the whole frame; the DMA is idle then, so the CPU and the camera never write those bytes at once. The fingerprint is folded over the whole frame before any glyph goes down, so the readout itself does not move it on every repaint.

---

## The band: implemented, measured, reverted

`[measured]` The camera was once confined to the panel's bottom 120 rows (320×120, 76,800 B, 20 descriptors), with the top 120 rows left for the black ground and readout. `set_window(320, 120, 5)` read 1600×600, vertically centred; it was not a crop of the old picture but a wider strip of the array.

`[measured]` Band frame rate: `camera-probe`'s `[PROBE] capture` grew by 19–20 frames every 2 s (19 → 39 → 59 → 78), i.e. **9.5–10 fps**; `[PROBE] painted` reported 18–20 new frames in the same window at `691`–`768 KB/s`. The product booted straight into the Camera page agreed after settling (20 repaints / 2020–2040 ms, about 9.9 fps; the first window was 17 / 2077 ms while starting). The byte rate was almost the same as the full screen (`691`–`844 KB/s`); the frame rate doubled only because a frame shrank from 153,600 B to 76,800 B. The entry gap also fell from about 192 ms to about 100–105 ms (derived from the measured rate, not timed on its own).

`[reported]` The band's picture quality was not confirmed by eye before the reversion: only the frame rate and geometry were measured, and "the picture is fine" is not promoted to a measurement.

`[measured]` It was reverted because the product wants the full screen: the band was an 8:3 strip, and it pushed the FPS badge to row 4. Restoring the full screen brings back the complete 4:3 picture with the readout over it and the FPS badge in the bottom-right corner. The band is not the current configuration; the numbers above are only the "frame size decides frame rate" comparison.

---

## The descriptor TX list once ate the main stack, so the full screen would not boot

`[measured]` After the full-screen change grew each frame from 76,800 B to 153,600 B, the product hung at speaker bring-up. The cause was neither DRAM nor DMA but the second argument of `dma_descriptors_chunk_size!`: it is the TX total size, and passing `RING_BYTES` statically allocated an unused TX descriptor list (484 B). The camera only receives, so that static ate the main-stack region for nothing; after the full-screen growth the region fell from 66,360 B to 65,848 B, and speaker initialisation ran out of main stack, hanging boot.

`[measured]` With the three-argument `dma_descriptors_chunk_size!(RING_BYTES, 0, CHUNK_BYTES)` the TX list is 0, the main-stack region returns to 66,360 B, and the product boots to Camera; the log reads `stack peak 43224 B of 66360 B`. This, like `RING_BYTES`, is a link-time fact: `check-s3` only type-checks and does not link, so it cannot see it; only `build-s3` / `iot:image` can falsify it.

`[test]` `CHUNK_BYTES` stays 3,840: 7,680 trips esp-hal's `chunk size must be <= 4095` (the descriptor size field is 12 bits), so a frame can only be 40 chunks; static cost cannot be cut by using fewer descriptors.

---

## The pixel clock is not free; the frame rate is bounded by data volume, not by exposure

`[measured]` `PIXEL_CLOCK_HZ` is fixed at 24 MHz and is not a free parameter: at 20 MHz the sensor's PLL does not lock, and thereafter it **streams frames that are perfectly framed and entirely noise** — every register reads back correctly and the eye sees speckle. No register readback reports this.

`[measured]` **This section previously recorded that the frame period is capped by the page-1 AEC exposure ceiling, and that rockchip's frame-rate steps (`0x27`–`0x2e`, `0x04e2`, for 8/12/14/20 fps) are written nowhere in this repository — both were wrong.** Flashing `camera-probe` measures `sensor exposure 486 lines` against a full readout of ~1200 lines: the exposure sits **below** the window and locks nothing, so that group is not an exposure-driven frame-rate knob.

`[measured]` The second error is in the registers themselves: rockchip's frame-rate steps (`0x27`–`0x2e`) **are** in the power-on table, on page 1, written **twice** — `0x27` `0x03` then `0x01`, `0x28`–`0x2c` `0x96` then `0xe6`, `0x25` `0x01` then `0x00`, `0x26` `0x32` then `0xa2` (`0x2d`/`0x2e` identical both times). So "writes none of them" does not hold. The `0x05`–`0x08` = `0x3090`/`0x2070` (12432 / 8304 lines) quoted above **read the wrong page**: on page 1 those four addresses are part of the exposure ladder, while the driver reads exposure from page 0's `0x03`/`0x04` (`REG_EXPOSURE_HIGH`/`LOW`), which the table writes once, `0x04`/`0x62` = 1122 lines.

`[measured]` The frame rate is set by data volume: the current 320×240 full panel measures `691`–`844 KB/s`, i.e. 5.0–5.2 fps, and 153,600 B per frame is exactly that number; the reverted 76,800 B band measured `691`–`768 KB/s` and 9.5–10 fps — **the byte rate did not change; the frame rate doubled only because each frame was half the size.** The band's rate is not the current product value.

`[measured]` The datasheet (CSP DataSheet V1.0, §7.1.1, on the Pine64 mirror) gives a computable formula: `Ft = VB + Vt + 8`, where `VB` is page 0's `0x07`/`0x08` (vertical blanking) and `Vt` is `win_height`. Both were measured — `blanking v=46`, `win_height=1208` ⇒ `Ft = 1262` rows; at `PIXEL_CLOCK_HZ` = 24 MHz over 1600 pixels per row that is an 84 ms frame, **11.9 fps**.

`[measured]` That is 2.29× the measured 5.2 fps: **each row actually takes 2.29× what 24 MHz over 1600 pixels implies** (back-solving an equivalent ~10.5 MHz line clock). The blanking reads exactly as the power-on table wrote it, so the gap is **not in the registers** — either the datasheet's timing formula does not hold for this revision, or `0xf7`/`0xf8` (PLL mode, written `0x1d`/`0x84` where the datasheet's reset values are `0x05`/`0x81`) leave the real line clock out of step with `PIXEL_CLOCK_HZ`. **This step is not located.** The table writes `0xf7`/`0xf8` as `0x83` mid-sequence and then `0x84` again, and those two differing writes are where to look first.

**So the conclusion is rewritten**: going faster means a smaller readout window (the same switch as field of view — see "bins decide the field of view, and pay for it in frame rate") or a different sensor. **There is no orthogonal register knob left untried** — the claim in the first half of this section is falsified by the measurements above. The datasheet names vertical blanking `0x07`/`0x08` as the frame-rate control, yet the blanking reads exactly as the table wrote it while the rate stays 2.29× under the formula, so changing that register **would not** buy speed until the 2.29× is understood.

---

## The frame buffer's owner is one value, not two flags

`[test]` The panel and the camera share one frame buffer (153,600 B), so exactly one of them may be writing it. This was once recorded as two flags — "is the camera running" plus "does the CPU have a colour waiting" — which has four combinations, one of which cannot exist in this domain: the camera running *and* no colour waiting.

`[test]` That combination was unreachable because the way out of it was itself guarded. `consume()` repaints only when there is a colour waiting, and `paint()` is where "the camera is running, so release it" lived; entering the Camera page cleared the colour to "none", so **leaving the page meant the only call that could release the camera was blocked by its own guard**. The flag stayed true, every fill afterwards was refused, and the panel kept the last camera frame with no column of ticks able to walk out of it.

`[test]` The fix is `FrameOwner` (`iot-core::drivers::camera`): `NeedsPaint` / `Painted { fill, color }` / `Camera`, one variant per legal state, so the fourth combination is unrepresentable. Five unit tests cover the transitions, of which `a_released_camera_lets_the_next_fill_through` walks exactly the sequence that could not be left; `host-smoke`'s `HostLight` uses the same type and asserts that a fill must reach the surface after leaving the Camera page.

`[test]` Alongside it, "who owns it" went back to being recorded once: `LightRenderer` carried both `camera_page: bool` and `page: Option<DisplayPage>` for the same fact, so the former is gone and `step()` computes it. The panel's `owner` field is **not** gated on the camera feature — the colour half is also how a panel with no camera decides whether a fill is worth shipping, and gating it leaves half of that decision outside the value that holds it.

---

## Entering and leaving the Camera page rebuilds the DMA chain, and the fresh one stalls

`[measured]` To reproduce: flash the default firmware, tap through the cycle repeatedly for several minutes, and read `[CAM] no descriptor completed`, `[DISPLAY] Camera … repaints … held the executor` and `[INPUT] held … worst single hold`.

Booting straight into the Camera page (`IOT_BOOT_PAGE=camera`) and tapping in and out of it are **two different costs**:

| | Booted straight in | Tapping in and out (183 page switches) |
|---|---|---|
| Repaints / window | 11 / 2119 ms (5.2 fps) | **25-29 / 2100 ms** |
| Render-side occupancy | 188 ms = **8%** | **626-754 ms = 29-36%** |
| Chain re-arms | 0 | **140** |

"Booted straight in" is the current full screen (`11 / 2119 ms`, about 5.2 fps); "tapping in and out" at 25–29 / 2100 ms was measured in the 320×240 full-panel era, and the repaint occupancy and re-arm count were not re-measured. The reverted band booted straight in at 20 repaints / 2020–2040 ms (about 9.9 fps).

The fps figure **should not be taken at face value**: during 140 rebuilds each one may hand out a frame that gets counted as new, so "more repaints" may be re-arm churn rather than genuinely more pictures out. The render side going 8% -> 29-36% is not in doubt.

`[measured]` The mechanism follows from the code: leaving the Camera page makes the demand's camera `Off`, so `Screen::consume` calls `pause()` and hands the buffer back; coming back, `paint_camera` finds the buffer is not the camera's and calls `resume()` — and `resume` **rebuilds the whole chain** (`camera.receive(ring)`) rather than resuming it. This run entered the Camera page 36 times for 140 re-arms, stalling several times right after each entry: **enter the camera page and the chain stalls as soon as it is built.**

`[measured]` One single-hold breach came with it: `worst single hold 31 ms` at 00:00:06 against `MAX_SINGLE_HOLD_MS = 30`, with the Camera page entered at 00:00:04.68 — **entering the camera page can breach the single-hold bound**, and it is probably the same thing as the re-arm churn.

`[reported]` The cause of "stalls as soon as it is built" is not located. Candidates are `check_stall` judging the ring stalled before the rebuild has filled it (`STALL_MS` is 420 ms), or the rebuild's own window losing the first `take_latest`. Not confirmed, recorded as a lead.

---

## Entering the Camera page has a ~192 ms window with nothing on it

`[measured]` Under a single buffer the first frame cannot be shown until the DMA has filled the whole frame, so there is a fixed gap between the triple-tap and the picture appearing. The current 320×240 full screen measures `11 repaints / 2.12 s` (about 5.2 fps), so the gap is about **192 ms**; the reverted 76,800 B band measured 9.5–10 fps, so its gap was about 100–105 ms (derived from the measured rate, not timed on its own), from the same cause as the frame-rate bound above.

`[measured]` What is done about it is a transition frame: entering the page paints `CAMERA WAIT / ----` (the `starting` arm of `stamp_camera`) rather than leaving the previous page's colour up. The previous behaviour left the previous page frozen on screen, which reads as a hang; the transition frame reads as starting up. **This is mitigation, not removal** — the gap's length is set by how long the sensor takes to produce a frame, not by the code.

`[measured]` The transition frame also skips the fingerprint fold: at that moment the buffer holds a fill rather than a picture, so `frame_fingerprint` would report a fingerprint for a frame that does not exist.

`[measured]` **The gap can no longer be shortened through the frame-rate steps**: the page-1 step was the intended lever, and measurement falsified its premise (see "The pixel clock is not free, and the frame rate is bounded by it"). The frame rate is this sensor's physical limit at the current readout window. **The band once halved the gap from about 192 ms to about 100 ms (half the frame), but that scheme is reverted; the current full-screen Camera page is still a ~5.2 fps preview, not a state waiting to be optimised.**

---

## The sensor can be read back, the panel cannot: two verification routes

`[measured]` The GC2145 is on I²C, and `read_window_geometry()` reads the registers the part **actually holds** rather than the bytes that were written. So a write that did not land shows up on the sensor side immediately, instead of surfacing later as a picture of the wrong content.

**Rejected verification**: reading only `0x95`–`0x98`. That reports the size that was **asked for** and says nothing about the read window or the extract ratio — which is exactly how a stretched picture passes as correctly sized.

The panel is the other way round: **no MISO**, so every register read comes back `0x00` (see the next section). `MADCTL` therefore cannot be verified by reading it back, and window geometry has to be confirmed by eye against a known pattern.

That boundary decides which call sites can carry a "read-back, not write-echo" note and which cannot. `gc2145.rs`, `lckfb_szpi_esp32s3/camera.rs` and `camera-probe.rs` were all saying the same thing, so it is now stated once on `read_window_geometry`.

---

## The panel has no MISO, so register readback is all zeroes

`[measured]` The panel's SPI on this board is SCK and MOSI only, **no MISO**. Any register read returns `0x00`, which is what a floating input looks like. Reading back `COLMOD` to verify `MADCTL` was tried and returned zero where `0x55` had just been written, so that route was ruled out. Window geometry can only be confirmed by eye against a known pattern.

`[measured]` The panel is 320×240, `MADCTL` is `MADCTL_LANDSCAPE` (`0x60`), and a 320×240 buffer goes out unmodified with nothing transposed. The controller's own size is the transpose of the panel's (240×320), and the two must not be confused.

`[measured]` **Getting it wrong truncates rather than rotates**, because the address window transposes with `MADCTL`: a `CASET` of 0..319 then overruns a 240-wide RAM and the frame comes back cut. The orientation tables prescribing `(0x60, 320, 240, 0, 0)` are for 240×320 panels.

---

## GC2145 has no official wide-angle support

`[reported]` The Espressif FAQ's wide-angle answer recommends BF3005 and OV2640, and **GC2145 is not among them**. Low-resolution cropping on this sensor was raised in espressif/esp32-camera issue #845 — in exactly the wide-angle-lens-at-low-resolution case — with no official response.

Set against "field of view is arithmetic" above: **if changing the sensor configuration does not widen the view, what is left is the lens and the sensor itself.**
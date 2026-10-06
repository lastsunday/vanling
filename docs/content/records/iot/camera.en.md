+++
title = "Camera Implementation Record"
weight = 30
sort_by = "weight"
[extra]
source_file_hash = "1f98281e68e4b2bf34def38b0b2cf274868eb0c6"
translated_at = "2026-10-06T05:35:23Z"
+++

<!-- doc-audience: ai -->

# Camera Implementation Record

Judgement records from the GC2145 capture path and the ST7789 panel wiring in `apps/iot`, established on real hardware. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is labelled by its evidence: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

Reproduction is the same throughout: flash `camera-probe` from `apps/iot/` and read the `[PROBE]` and `[CAM]` lines off the serial port. Every 2 s the probe prints `sensor exposure`, `blanking h/v` (measured page-0 `0x05`–`0x08`) and the window's KB/s — every frame-rate claim below rests on those three lines.

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

`[measured]` The line's gap really does lose bytes: with the same sensor configuration a 61 KB ring gives the DMA 645 KB/s and a ring back to one frame (153 KB) gives 1558 KB/s. **One lost byte offsets every row after it, and the picture arrives as horizontal bands — "venetian blinds" — that move.** So the ring must be a whole frame, at the cost of rebuilding four times a second instead of ten.

---

## Internal DRAM is not a memory shortage; the DMA cannot reach PSRAM

`[measured]` The ring must be in internal DRAM. The ESP32-S3 has 8 MB of PSRAM, ample for 150 KB, but:

- Espressif's own `ll_cam.c` gates the receive descriptor and data burst bits under `//internal SRAM only`, leaving both clear for PSRAM;
- esp-hal **cannot express that split**: `set_descr_burst_mode` is called with a literal `true` on every receive, and `BurstConfig::is_burst_enabled` is unconditionally true on the PSRAM path.

A PSRAM ring is therefore always left in the configuration Espressif declines to use, and the part streams scrambled data into it. The ESP32-S3 camera issue tracker describes the same field: `the artifacts you see are exactly missing bytes in the frame`. **No setting reaches the official state.**

`[measured]` The descriptor list may **not** point into PSRAM either.

`[measured]` In internal DRAM the alignment constraint is the 32-byte cache line (`DMA_ALIGNMENT_BYTES`), not the external burst block.

---

## The ring's 150 KB used to hang off the feature composition; it no longer does

> `[measured]` **This section previously recorded that the ring would region-overflow the moment it was reachable in `vanling`. That was wrong.** The mistake was treating `dram_seg` as a hard ceiling for the main stack — it is not: `crates/app/linker/esp32s3-main-stack.x` raises `_stack_start` into `dram2_seg`, so the stack spans both regions contiguously, and the original reasoning only counted the tail of `dram_seg`. The same premise produced another wrong number (`hardware-constraints.md` recorded `_stack_start_cpu0 = 0x3fced710` / 115,140 B, omitting `ROM_BOOT_STACKS = 0x4000`); both are corrected in place. **Lesson**: the "space left" in `dram_seg` is not the space available for static data — read the linker script first.

`[measured]` Those 150 KB are now **the panel's frame buffer itself**. `FRAME_ARENA` (formerly `FRAME_RING`) is unconditionally in `.bss`, `camera` is in the board feature, and `--gc-sections` no longer has any opportunity to drop it — so the 150 KB went from "whatever a link-time GC decides" to a link-time fact. `cargo check` does not link and `check-s3` cannot see it, so only `moon run iot:image` / `build-s3` can falsify this section.

`[measured]` What was saved is not DRAM but the per-frame 150 KB memcpy (measured frame rate 5.5 fps, about 4.5 ms each): the panel's former `Vec` became this static arena, and the two are the same size, so `.bss` grows by 14,336 B net. In DRAM terms "one buffer" and "both buffers" are equivalent; they differ in CPU time and in ownership coupling.

---

## The pixel clock is not free; the frame rate is bounded by data volume, not by exposure

`[measured]` `PIXEL_CLOCK_HZ` is fixed at 24 MHz and is not a free parameter: at 20 MHz the sensor's PLL does not lock, and thereafter it **streams frames that are perfectly framed and entirely noise** — every register reads back correctly and the eye sees speckle. No register readback reports this.

`[measured]` **This section previously recorded that the frame period is capped by the page-1 AEC exposure ceiling, and that rockchip's frame-rate steps (`0x27`–`0x2e`, `0x04e2`, for 8/12/14/20 fps) are written nowhere in this repository — both were wrong.** Flashing `camera-probe` measures `sensor exposure 486 lines` against a full readout of ~1200 lines: the exposure sits **below** the window and locks nothing, so that group is not an exposure-driven frame-rate knob.

`[measured]` The second error is in the registers themselves: rockchip's frame-rate steps (`0x27`–`0x2e`) **are** in the power-on table, on page 1, written **twice** — `0x27` `0x03` then `0x01`, `0x28`–`0x2c` `0x96` then `0xe6`, `0x25` `0x01` then `0x00`, `0x26` `0x32` then `0xa2` (`0x2d`/`0x2e` identical both times). So "writes none of them" does not hold. The `0x05`–`0x08` = `0x3090`/`0x2070` (12432 / 8304 lines) quoted above **read the wrong page**: on page 1 those four addresses are part of the exposure ladder, while the driver reads exposure from page 0's `0x03`/`0x04` (`REG_EXPOSURE_HIGH`/`LOW`), which the table writes once, `0x04`/`0x62` = 1122 lines.

`[measured]` The frame rate is set by data volume: measured `691`–`844 KB/s`, i.e. 5.0–5.2 fps, and 153,600 B per frame is exactly that number.

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

## Entering the Camera page has a ~192 ms window with nothing on it

`[measured]` Under a single buffer the first frame cannot be shown until the DMA has filled all 153,600 B, so there is a fixed gap between the triple-tap and the picture appearing. The measured rate is `11 repaints / 2.12 s` (about 5.2 fps), so the gap is about 192 ms, from the same cause as the frame-rate bound above.

`[measured]` What is done about it is a transition frame: entering the page paints `CAMERA WAIT / ----` (the `starting` arm of `stamp_camera`) rather than leaving the previous page's colour up. The previous behaviour left the previous page frozen on screen, which reads as a hang; the transition frame reads as starting up. **This is mitigation, not removal** — the gap's length is set by how long the sensor takes to produce a frame, not by the code.

`[measured]` The transition frame also skips the fingerprint fold: at that moment the buffer holds a fill rather than a picture, so `frame_fingerprint` would report a fingerprint for a frame that does not exist.

`[measured]` **This gap could not be shortened this round.** The page-1 frame-rate step was the intended lever, and measurement falsified its premise (see "The pixel clock is not free, and the frame rate is bounded by it"): the exposure reads 486 lines against a 1200-line readout window, and the frame rate is this sensor's physical limit at the current readout window. Going faster means a smaller window (the same switch as field of view) or a different sensor. **So the Camera page is a 5.2 fps preview as it stands, not a state waiting to be optimised.**

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
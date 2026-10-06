+++
title = "Camera Implementation Record"
weight = 30
sort_by = "weight"
[extra]
source_file_hash = "25cf7c75c971cccb7f6ed943eddee8afce58c597"
translated_at = "2026-10-06T02:57:00Z"
+++

<!-- doc-audience: ai -->

# Camera Implementation Record

Judgement records from the GC2145 capture path and the ST7789 panel wiring in `apps/iot`, established on real hardware. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is labelled by its evidence: **`[test]`** a unit test covers it, **`[measured]`** there is a hardware measurement with a reproduction, **`[reported]`** not reproduced, recorded only as a lead.

Reproduction is the same throughout: flash `camera-probe` from `apps/iot/` and read the `[PROBE]` and `[CAM]` lines off the serial port.

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

## The ring's 150 KB hangs off the feature composition, not off the link

`[measured]` `FRAME_RING` is a 153,600 B `static mut` landing in `.bss` → `RWDATA` = `dram_seg` (esp-hal 1.2.2; `dram_seg` is 341,760 B in `ld/esp32s3/memory.x`). The product image's `_stack_end` sits at `0x3FCD1554`, leaving **41,388 B** — so any build in which this code is in `vanling`'s reachable set overflows the region at link time.

`[measured]` It does **not** overflow today, because it is unreachable in `vanling`: with `camera` in the board feature, a `vanling` built that way has zero symbols for `FRAME_RING`, `new_camera_only` and `Gc2145` — release `--gc-sections` drops the whole camera path. Doing the arithmetic on `.bss` while ignoring reachability concludes "the link must fail", which is wrong.

`[measured]` The real problem is that **the product's DRAM budget then rests on a link-time GC**. The day the product actually uses the camera, the board feature's aggregation quietly brings those 150 KB in, and `cargo check` does not link — `check-s3` cannot see it, only `moon run iot:image` / `build-s3` fail.

So `camera` is **not** in the board feature: `camera-probe` asks for it through `required-features`, and the board's camera wiring lives in `lckfb_szpi_esp32s3/camera.rs` so `#[cfg]` appears once, at the `mod` (features.md rules 1 and 2). When the product does grow a camera path, DRAM has to come from somewhere else, and the link will say so at the moment it runs.

---

## The pixel clock is not free, and the frame rate is bounded by it

`[measured]` `PIXEL_CLOCK_HZ` is fixed at 24 MHz and is not a free parameter: at 20 MHz the sensor's PLL does not lock, and thereafter it **streams frames that are perfectly framed and entirely noise** — every register reads back correctly and the eye sees speckle. No register readback reports this.

`[reported]` The frame period is capped by the page-1 AEC exposure ceiling. The power-on table writes `0x05`–`0x08` = `0x3090`/`0x2070` (12432 / 8304 lines) against a full readout of ~1200 lines. rockchip's table has dedicated frame-rate steps (`0x27`–`0x2e`, `0x04e2`, for 8/12/14/20 fps) and **this repository writes none of them**. That is the one untried knob left, and it is orthogonal to field of view.

---

## The panel has no MISO, so register readback is all zeroes

`[measured]` The panel's SPI on this board is SCK and MOSI only, **no MISO**. Any register read returns `0x00`, which is what a floating input looks like. Reading back `COLMOD` to verify `MADCTL` was tried and returned zero where `0x55` had just been written, so that route was ruled out. Window geometry can only be confirmed by eye against a known pattern.

`[measured]` The panel is 320×240, `MADCTL` is `MADCTL_LANDSCAPE` (`0x60`), and a 320×240 buffer goes out unmodified with nothing transposed. The controller's own size is the transpose of the panel's (240×320), and the two must not be confused.

`[measured]` **Getting it wrong truncates rather than rotates**, because the address window transposes with `MADCTL`: a `CASET` of 0..319 then overruns a 240-wide RAM and the frame comes back cut. The orientation tables prescribing `(0x60, 320, 240, 0, 0)` are for 240×320 panels.

---

## GC2145 has no official wide-angle support

`[reported]` The Espressif FAQ's wide-angle answer recommends BF3005 and OV2640, and **GC2145 is not among them**. Low-resolution cropping on this sensor was raised in espressif/esp32-camera issue #845 — in exactly the wide-angle-lens-at-low-resolution case — with no official response.

Set against "field of view is arithmetic" above: **if changing the sensor configuration does not widen the view, what is left is the lens and the sensor itself.**
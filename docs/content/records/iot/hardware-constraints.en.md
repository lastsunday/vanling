+++
title = "Hardware Constraint Record"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "a4c24e0c5fa0f064020a8787043288e7f9518564"
translated_at = "2026-10-21T14:10:00Z"
+++

<!-- doc-audience: ai -->

# Hardware Constraint Record

Constraints established against real hardware in `apps/iot`. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is marked with its basis: **`[test]`** a unit test backs it, **`[measured]`** hardware measurement with a way to reproduce, **`[reported]`** not reproduced, kept only for the record.

---

## No power management yet, so no clock-source locks either — but the order matters

`[reported]` (not implemented, recorded as a lead) ESP-IDF's `esp_pm` expresses a component's power requirement with three locks: `ESP_PM_CPU_FREQ_MAX`, `ESP_PM_APB_FREQ_MAX` and `ESP_PM_NO_LIGHT_SLEEP`. Locks are recursive and must be released as many times as they were acquired. Its documentation gives two facts this board will eventually hit:

- **In Light-sleep, peripherals are clock gated and no interrupt is generated.** So any driver relying on a peripheral interrupt has to hold `NO_LIGHT_SLEEP`. This board's DVP capture chain and both I2S chains move their data by DMA and fall under that rule.
- Peripheral clock sources such as `REF_TICK`, `XTAL` and `RC_FAST` (i.e. RTC_8M) **are not influenced by APB frequency**, so downclocking is not a full stop and some clocks survive an APB change.

The firmware has **no PM configuration at all** — no light-sleep, no `esp_pm_configure`, no `WFI`. The feed already runs on its own `InterruptExecutor` (`SpeakerRunner::OnInterrupt` in `lib.rs`), so the executor-contention route is settled structurally and needs no lock.

So **not implementing locks is the right call**: adding two with no holder is dead code, and a deadlock is harder to diagnose than dead code.

But the order does matter. Before light-sleep there has to be a reference-counted lock set, held at minimum by:

| Holder | Why |
|---|---|
| Playback feed | its existence is the clock source; stopping it stops the codec's master clock |
| The capture chain | a completed DMA releases the ring, and a gated DMA means the ring is never drained |
| The DVP capture chain | same, and its chain-repair poll lives render-side |

`InputSource::set_plane_collecting` is already this shape (a plane that is off cannot learn it from the next poll, so someone has to say it explicitly), so the wiring point is right and only the locks themselves are missing.

Zephyr's split confirms it: its device runtime PM explicitly **does not manage device dependencies** — "devices can manually get or put their dependencies" — so dependencies are the consumer's job, not something automatic.

---

## One shared MCLK

`[measured]` Capture and playback hang off the same MCLK / BCLK / LRCK. Were they declared from separate clocks, they could sit a few hertz apart with **nothing reporting it**: the panel would meter the capture correctly while the speaker ran slightly sharp — which sounds like a hardware fault and is not.

`[measured]` Each fault is quiet, and each reads as the other: a speaker fed a misaligned clock crackles, while a microphone sampled off-clock returns constant full scale that looks like a loud room.

`[measured]` **Which unit drives that pin pair was tried both ways, and both ways are broken**:

| Who drives the pins | What the transmit side hears | What the capture side sees |
| --- | --- | --- |
| The capture unit | continuous crackle (the transmit unit shifts out on a divider the ES8311 is not being clocked by) | normal |
| The transmit unit | normal | a constant full-scale reading and a dead flat waveform (sampling the ES7210 off a divider that is not the one clocking the part) |

The two units run in separate clock domains and neither can own the clock alone, so **the transmit unit drives the pins and the capture is held in slave mode**. The direction is not interchangeable.

The corresponding constraint in code (`shared_tdm_config`): `with_signal_loopback` slaves the receive unit to follow, and **its name is the one misleading thing about it** — it carries no samples across; it shares the clock.

`audio::tdm_config` and `audio_out::shared_tdm_config` are **a deliberate second copy**, because the two features are independent (a speaker with no microphone and a microphone with no speaker are both real boards) and neither half may own the shared answer. The capture's copy must leave sharing off — with no transmit unit there is nothing to be master.

---

## DC-blocking write order in the microphone front end

`[test]` Each ES7210 input pair's DC-blocking filter occupies two registers differing only in bit 5 (`0x0A ^ 0x2A == 0x20`), so the corner is the only field the two share. A MockI2c write-order trace test holds that order in place.

The registers are named for the bit they carry, not for a stage order: no public document supports a stage naming — the datasheet is "Everest Semiconductor Confidential" and defers the bit definitions to an undistributed guide, and the two driver families disagree even on the names (`esp-bsp` calls `0x22` HPF2 where `esp-audio-dev` calls it HPF1).

`[measured]` Swapping that pair **is not cosmetic**: it lifts the quiet-room floor. So the order follows the reference driver rather than an undocumented stage order.

---

## 0 dBFS = 102 dB SPL

`[reported]` Anchored on 2026-09-28 against speech at 30 cm: a 66 dB SPL floor, peaks to 80. Not reproduced in this pass.

The derivation (in full in the code comment): the ES7210's full scale is AVDD/3.3 Vrms, so under `ANALOG_POWER_RUN` it is 1.0 Vrms rather than the 2 Vrms "headroom" would suggest; the ZTS6216's −38 dBV/Pa is 12.59 mV/Pa, which through `GAIN_30DB` is 397.8 mV/Pa at the converter — so full scale is 2.51 Pa, or 101.98 dB against the 20 µPa reference pressure.

**To re-anchor** (when the capsule or a gain step changes): set `CAL_SPL_LOG` and speak at the same distance.

> These numbers live in the code comment because they decide the readout; changing a value here requires changing it there too.

Core-side consumption of this offset (and why `spl()` takes it as a parameter) is in the [audio implementation record](@/records/iot/audio.en.md).

---

## The QMI8658A tap engine is unusable

`[reported]` The part's tap engine latched a stuck tap bit and a frozen `TAP_NUM` at its enable transient and never resolved a real blow, so only No-Motion is armed and knock detection moved to the core-side recognizer (`recognizer.rs`, with `[test]` 7 cases covering knocks, turns and the first-knock count). Not reproduced in this pass.

The full detection rules, where the thresholds came from, and the rejected approaches are in the [motion implementation record](@/records/iot/motion.en.md).

---

## Main task stack size

> `[measured]` **All three numbers below changed when the camera joined the product; these are the current ones** (`vanling`, read from the ELF symbols after `moon run iot:build-s3`):
> ```
> _stack_end        = 0x3fcd529c   (was 0x3fcd154c)
> _stack_start_cpu0 = 0x3fce9710   →  83 060 B = 81.1 KB   (was 98 756 B)
> ```
> The panel's frame buffer went from a heap `Vec` to a board-level static arena shared with the camera ring (see `camera.md`), so `HEAP_MEM` dropped from 204,800 to 65,536. Net effect: `.bss` **grows** by 14,336 B and the stack shrinks by about 15 KB. `FRAME_ARENA` measured at `0x3fcaf900`, `% 64 == 0`.

`[measured]` `esp-hal`'s own `stack.x` places the main task's stack top at the end of `dram_seg`, leaving `dram2_seg` unclaimed. `crates/app/linker/esp32s3-main-stack.x` connects it, and the stack becomes contiguous across both regions.

`[measured]` The real geometry read from the ELF symbols:

```
_stack_end        = 0x3fcd154c
_stack_start_cpu0 = 0x3fce9710   →  98 756 B = 96.4 KB
```

> `[measured]` **These two values were measured before the camera joined the product** — see the head of this section. `_stack_start_cpu0` was also previously recorded as `0x3fced710` / 115 140 B, omitting the ROM's two 16 KB boot stacks. `esp32s3-main-stack.x` places the stack top at the top of `dram2_seg` **minus** `ROM_BOOT_STACKS = 0x4000` (`0x3fced710 - 0x4000 = 0x3fce9710`); the ROM's PRO/APP boot stacks must not be reached by the main stack. The same omission led `camera.md` to the wrong conclusion that the ring would necessarily region-overflow in `vanling`; that section now carries the correction.

**Why it is needed**: drawing a render puts `Diagnostics` snapshots and two `[u16; ENVELOPE_COLUMNS]` column arrays on the stack by value, and the Audio page once crashed on this stack (the exception's `A1` sat below `_stack_end`).

`[measured]` Current use is 36 453 B (43.9%), leaving about 46.6 KB spare (measured on the Ambient page, deepest value in 47 s of post-flash logs). The firmware ships its own high-water measurement (`[DISPLAY] stack peak`), sampled once per repaint window and taken as the deepest value — a measurement, not an estimate. The Camera page's reading used to be `0 B` throughout: that path passes no sampling point, so it was unmeasured rather than shallow, and now samples like the others.

**An open question**: this peak is nearly constant from boot, and switching pages does not raise it — yet the historical crash point was the Audio page's full-frame stamp. Which path the peak actually belongs to is **not yet identified**. If it turns out to be rendering, rendering is heavier than the code comment describes, and it is worth chasing.

---

## esp-hal's stack-overflow watchpoint fires falsely

`[measured]` esp-hal / esp-rtos arm hardware STORE watchpoints as stack-overflow guards, and the armed window overlaps legitimate stores, raising a level-6 Debug exception that kills the boot (observed at the render `on_appearance` on esp32s3).

Hence `.cargo/config.toml` turns hardware detection off and the software canary on:

```toml
ESP_RTOS_CONFIG_HW_TASK_OVERFLOW_DETECTION = "false"
ESP_HAL_CONFIG_STACK_GUARD_MONITORING = "false"
ESP_RTOS_CONFIG_SW_TASK_OVERFLOW_DETECTION = "true"
```

The software canary checks the sentinel below the stack floor on every context switch and panics naming the offending task — better than silently corrupting memory.

---

## The panel's glyph table: Spleen 5×8, one face (`[test]`, 2026-10-22)

The table lives in `apps/iot/crates/bsp-esp/src/font.rs`: 95 printable ASCII glyphs, five columns by
eight rows, column-major, bit 0 the top row. The pitch is still 9×13 (`FONT_W 5 + OVERLAY_GAP 4`,
`FONT_H 8 + OVERLAY_ROW_GAP 5`).

- **The first swap was for the counters**: Adafruit `glcdfont` leaves 1 px openings in `a/e/g/p/r/t/y`
  at 5×7, and network names and passphrases are read slowly through them. m5x7 (Daniel Linssen, CC0)
  draws 2 px openings in the same five columns — but its `m/M/w/W` are seven columns wide, its `0`
  is its `O`, and `\` is missing; those five had to keep the Adafruit shapes, so any word carrying
  `m` or `w` mixed two faces and read worse on the panel than the table it replaced (feedback,
  2026-10-22).
- **It is Spleen 5×8 now** (Frederic Cambus, BSD-2-Clause): the 5×8 cut carries printable ASCII
  only, and every glyph is natively five columns by eight rows — `m/M/w/W`, `0` and `\` are all from
  the same face, with no borrowed shapes and no cropped descender row.
- **One row more, nothing else**: `FONT_H` 7→8 and `OVERLAY_ROW_GAP` 6→5 keep `ROW_PITCH` at 13.
  The FPS badge at `232 + 8 = 240` fills the footer exactly, and the Audio page's `WAVE_TOP 42` sits
  exactly under the second row — both are compile-time equality assertions, so a cell change stops
  there first.
- **Regression**: `iot:test-bsp` holds "every printable glyph has ink", "look-alike pairs are not one
  bitmap" (`l/1`, `I/l`, `0/O`, `c/e`, `u/v`, `g/q`, `p/q`, `y/v`, `m/w`, `M/W`) and "the descenders
  reach the bottom row" (`g/p/q/y/j`). The `0/O` pair was in fact identical after the first swap, and
  the second test is what reported it.

The table is flash: `[u8; 95 × 5] = 475 B`, the same size as the old one; the static wall is 65,720 B
before and after (floor 65,536 B, 184 B spare).

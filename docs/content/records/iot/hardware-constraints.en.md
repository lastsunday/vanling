+++
title = "Hardware Constraint Record"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "9639a958cf42528c9dc6b49274a0229c484d04cc1423499b74f6716af3acd824"
translated_at = "2026-09-30T17:36:07Z"
+++

<!-- doc-audience: ai -->

# Hardware Constraint Record

Constraints established against real hardware in `apps/iot`. **AI-produced, not reviewed by a human** — see the [section above](@/records/_index.en.md).

Each judgement is marked with its basis: **`[test]`** a unit test backs it, **`[measured]`** hardware measurement with a way to reproduce, **`[reported]`** not reproduced, kept only for the record.

---

## One shared MCLK

`[measured]` Capture and playback hang off the same MCLK / BCLK / LRCK. Were they declared from separate clocks, they could sit a few hertz apart with **nothing reporting it**: the panel would meter the capture correctly while the speaker ran slightly sharp — which sounds like a hardware fault and is not.

`[measured]` Each fault is quiet, and each reads as the other: a speaker fed a misaligned clock crackles, while a microphone sampled off-clock returns constant full scale that looks like a loud room.

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

---

## The QMI8658A tap engine is unusable

`[reported]` The part's tap engine latched a stuck tap bit and a frozen `TAP_NUM` at its enable transient and never resolved a real blow, so knock detection moved to the core-side recognizer (`recognizer.rs`, with `[test]` 7 cases covering knocks, turns and the first-knock count). Not reproduced in this pass.

Only No-Motion is armed.

---

## Main task stack size

`[measured]` `esp-hal`'s own `stack.x` places the main task's stack top at the end of `dram_seg`, leaving `dram2_seg` unclaimed. `crates/app/linker/esp32s3-main-stack.x` connects it, and the stack becomes contiguous across both regions.

`[measured]` The real geometry read from the ELF symbols:

```
_stack_end        = 0x3fcd154c
_stack_start_cpu0 = 0x3fced710   →  115 140 B = 112.4 KB
```

**Why it is needed**: drawing a render puts `Diagnostics` snapshots and two `[u16; ENVELOPE_COLUMNS]` column arrays on the stack by value, and the Audio page once crashed on this stack (the exception's `A1` sat below `_stack_end`).

`[measured]` Current use is 32 617 B (28.3%), leaving 82.5 KB spare. The firmware ships its own high-water measurement (`[DISPLAY] stack peak`), sampled once per repaint window and taken as the deepest value — a measurement, not an estimate.

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

+++
title = "Changing the CPU clock at runtime"
weight = 72
sort_by = "weight"
[extra]
source_file_hash = "804fa49c9b41dfe6ebfe8d341f38a7b2c09504e0"
translated_at = "2026-10-12T05:29:49Z"
+++
title: "Changing the CPU clock at runtime: yes, but off the documented path"
description: "esp-hal's docs state the frequencies are frozen; bypassing it and writing the registers really does move this board between 80/160/240, measured 1 : 2.00 : 2.95. A switch costs 1–50 us, the tick is unaffected, and both cores agree."
source_file_hash = "f67e4bf01bbf131acb735b085cd44a0310452f51"
translated_at = "2026-10-12T03:47:32Z"
---

<!-- doc-audience: ai -->

# Changing the CPU clock at runtime: yes, but off the documented path

The full record of `clock-probe`. **Every figure here was measured on the board**; what was not measured is marked as such.

## esp-hal does not offer this, and says so

esp-hal 1.2.2's clock documentation has a section called **Frozen Clock Frequencies**:

> Once the clock configuration is applied, the clock frequencies become `frozen`
> and cannot be changed.

That is a design decision, not an oversight. The module's entire runtime surface is: `cpu_clock()` (read), `xtal_clock()` (read), `RtcClock`, `PhyClockGuard` (**prevents** a clock from being disabled), `ModemClockController` (for modem peripherals), `init_radio_clocks`. **Nothing changes the CPU frequency.**

The old route is gone too — the `Clocks` struct and `Clocks::get()` were removed in favour of lockless free functions.

**The only supported-API route means moving to `esp-idf-hal`**, which wraps ESP-IDF's `esp_pm_configure` / `esp_pm_lock_*`. That is the `std` + FreeRTOS ecosystem, incompatible with this repository's `no_std` + embassy + bare metal. The cost is a rewrite, not an increment.

## What the off-path route can do

`set_cpu_clock` writes three public peripherals and calls one ROM function, all copied from the reachable subset of esp-hal's own `configure_cpu_clk_impl`:

| Register | Content |
|---|---|
| `SYSTEM.cpu_per_conf.cpuperiod_sel` | **2 bits**, the only field that actually changes |
| `SYSTEM.cpu_per_conf.pll_freq_sel` | constant `1` (PLL_480 always) |
| `SYSTEM.sysclk_conf.soc_clk_sel` | constant `1` (PLL source) |
| `LPWR.date.ldo_slave` | `0x7 >> (mhz / 80)` → `0x3` at 80 MHz, `0x1` at 160, `0x0` at 240 |
| `ets_update_cpu_frequency_rom(mhz)` | ROM tick calibration |

**No PLL reconfiguration, no flash cache flush, no lock wait.** All three presets use `pll_clk: PllClkConfig::_480` and differ only in the divider selection — so the old comment in `esp32s3.rs` saying the 160/240 presets "move the PLL off the 480 MHz the chip boots on" and thereby "break USB Serial/JTAG" **misread the source.** The measured 240 MHz runs with an intact console refute it independently.

**What is skipped**: esp-hal also writes the digital and RTC bias through `regi2c` on a frequency change, with PVT calibration (`pvt_supported()` / `dig_dbias_v1()`). Those are `pub(crate)` or private `fn` and unreachable from outside.

**Why skipping is still safe**: esp-hal writes the PVT-trimmed bias on the way *up* to 240 MHz and drops it on the way down to save power. The probe **only ever goes down** — over-biased at a lower frequency is the conservative direction.

**"Climbing from a slow boot" was the worry, and it was measured rather than assumed** — see below.

## Measured: the frequency really moves

One work unit (`#[inline(never)]` + `black_box`, 256×64 integer operations):

| Preset | Elapsed | Relative to the first |
|---|---:|---:|
| `_80MHz` | **1731 µs** | 1.00× |
| `_160MHz` | **864 µs** | 2.00× |
| `_240MHz` | **587 µs** | 2.95× |

**1 : 2.00 : 2.95 ≈ 80 : 160 : 240.**

Note: `esp_hal::clock::cpu_clock()` still reports 240 MHz at all three — it reads the configuration tree, not the hardware. So this probe takes the ratio of work-unit timings as its evidence, not that call.

## Measured: the boot frequency does not change the result

The starting frequency looked like it might matter: esp-hal keeps two bias values (about dbias 28 for 240, 27 for below it), so climbing to 240 from an 80 MHz boot is *under-driven*. That is cheap to check — the failure mode is a reset, not damage.

**The probe boots at 80 MHz (which is what the product's `chip_init()` does) and the whole run matches a 240 MHz boot digit for digit:**

| Quantity | 240 MHz boot | 80 MHz boot |
|---|---:|---:|
| Work unit @80 | 1731 µs | **1730 µs** |
| Work unit @160 | 864 µs | **864 µs** |
| Work unit @240 | 587 µs | **582 µs** |
| Switch to 80 (mean/worst) | 50 / 58 µs | **48 / 56 µs** |
| Switch to 160 | 2 / 3 µs | **2 / 3 µs** |
| Switch to 240 | 1 / 2 µs | **1 / 2 µs** |
| 128 consecutive switches | 1–2 µs each | **1–2 µs each** |

**No under-drive, no reset, the ratio still 1 : 2.00 : 2.97.**

**The boundary**: the probe runs about 0.24 s. That establishes "it does not fail", not "it is optimal", and not long-run reliability — the repository already carries that caveat for 240 MHz.

## Measured: what a switch costs

| To | Mean | Worst |
|---|---:|---:|
| `_80MHz` | **50 µs** | 58 µs |
| `_160MHz` | **2 µs** | 3 µs |
| `_240MHz` | **1 µs** | 2 µs |

The first is much dearer because it touches the register page for the first time. 128 consecutive switches hold at 1–2 µs each with **no accumulated drift**.

Against the literature's "a whole-chip DVFS decision can take 6 ms, so keep decision points ≥20 ms apart" — **those figures are voltage scaling on large chips.** This board is two register writes, three orders of magnitude cheaper, and they do not transfer.

## Measured: nothing outside the CPU moves

Across a switch `Instant` advances **43–47 µs**, and a work unit measured immediately afterwards is correct (1715 / 867 / 586 µs). **The system timer is indifferent to the CPU frequency** — esp-hal's source says it is "only clocked by XTAL divided by 2 or 2.5".

Clock domain ownership:

| Peripheral | Clock domain | After a CPU-only switch |
|---|---|---|
| embassy tick (SYSTIMER) | XTAL/2 | unaffected |
| I2S / I2C / SPI / TimerGroup | APB | APB unmoved → unaffected |
| USB Serial/JTAG | separate | unaffected (240 MHz measured) |

**Nothing on the S3 hangs off the CPU clock.**

## Measured: nothing observable on the second core

`ets_update_cpu_frequency_rom` documents itself as updating "on the current core only", so core 1's ROM timing is stale after a core 0 switch. Running the same work on both cores:

**core 0 585 µs / core 1 566 µs, a ratio of 1.03×.**

585 µs sits on the 240 MHz single-core figure (582–587 µs), so both cores really are at 240 MHz and **the stale calibration produces no observable difference on this board**.

That is one workload on one board. Code that depends on `ets_delay_us` being exact cannot assume both cores are right on the strength of it.

## Not measured, and deliberately not done

- **Power.** The probe never lowers the bias, so it cannot report a real saving. That is a deliberate trade, not an oversight.
- **APB frequency.** Explicitly out of scope — it would move I2S/I2C/SPI/TimerGroup and USB Serial/JTAG, and the failure mode is a bricked board.
- **Light sleep.** esp-hal 1.2.2 has no such API, so there is no middle setting between "downclock" and "sleep".
- **Continued validity across esp-hal upgrades.** This probe depends on `configure_cpu_clk_impl`'s current shape; if esp-hal changes its policy or seals a register, the probe changes with it.
- **Long-run reliability.** The whole run is about 0.24 s. "Climbing to 240 from a slow boot does not fail" is measured; "it will not fail later" is not.

## Four ways the probe got itself wrong first

**① LTO deleted the work loop outright.** The first run reported `21 / 20 / 21 µs` at all three presets — that was the cost of two clock reads and no arithmetic. This repository's `architecture.md` already records "LTO optimizes the probe's timing brackets away, so `process` carries `#[inline(never)]`"; the same trap twice. Fixed with `#[inline(never)]` and `core::hint::black_box`.

**② The second-core read was ordered backwards.** The first version read core 1's value inside `measure()` while the handshake happened after it, and got `u64::MAX`. Now: core 0 finishes all switching → signals → reads its own value → waits for the peer's → prints the ratio.

**③ The log went inside the timed function.** To make a brownout diagnosable, `switch()` grew a `log::info!` — and `cost()` times a region that contains it. At 80 MHz a switch then read as **2634 µs**, fifty times the real 48 µs. **`switch()`'s own signature had no way to admit that.** The log moved to the callers.

**④ Both cores measured at once and slowed each other down.** Signalling before reading core 0's local value put core 1's executor startup on the same memory — **both readings landed in the 160 MHz range (861 / 830 µs) while the chip was at 240.** The ratio survives that; the absolute figures do not. Reading locally first gives **585 / 566 µs (1.03×)**, with 585 sitting exactly on the 240 MHz single-core work unit.

③④ share a feature worth recording: **both produced numbers that looked entirely normal**, with nothing failing. What caught them was the ratio being wrong and the absolute values landing in the wrong band — not any assertion.

## Related records

- [The AEC's CPU budget](@/records/iot/aec.md) — 240 MHz is forced from the AEC side: at 80 MHz the delay search alone busts the 30 ms bound
- [Clock and probe methodology](@/records/iot/_index.md) — where "measure the clock before talking about anything else" came from
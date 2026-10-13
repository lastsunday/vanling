+++
title = "Changing the CPU clock at runtime"
weight = 72
sort_by = "weight"
[extra]
source_file_hash = "5250dfdb400d960a21f9fa95d92b13f2dce3f791"
translated_at = "2026-10-13T04:12:17Z"
+++
title: "Changing the CPU clock at runtime: yes, but off the documented path"
description: "esp-hal's docs state the frequencies are frozen; bypassing it and writing the registers really does move this board between 80/160/240, measured 1 : 2.00 : 2.95. A switch costs 1–50 us, the tick is unaffected, and both cores agree."
source_file_hash = "5250dfdb400d960a21f9fa95d92b13f2dce3f791"
translated_at = "2026-10-13T04:12:17Z"
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

## Wired into the product: the whole chain, run on hardware

`ClockGovernor::apply` sits beside `demand::propose` in `render_loop`, resolves once per frame, and writes the register only when the answer changes. The decision is a type in `iot-core`; the mapping and the register write are the chip's.

`IOT_DEMAND` gained a `speed=` term, so the whole chain can be walked by changing one build-time variable:

```sh
IOT_DEMAND="display=continuous,audio=continuous,speed=slow"
```

**Measured, on the product firmware rather than a probe:**

```
[CLOCK] a page asked for Slow, but PlaneKey { field: Motion, instance: 0 } is running and needs Fast
[CLOCK] running at _80MHz
```

**An explicit request is not arbitrated away, and the plane it undercuts is named** — exactly the shape the design asks for.

**What it costs, from the same run:**

| | @240 MHz | @80 MHz |
|---|---:|---:|
| Repaints per 2 s | 49–50 | **19–27** |
| Executor held | 48% | **24%** |
| Worst render | 4 ms | **10 ms** |

The same work at a third of the clock finishes a third to a half of the repaints. **Downclocking does not manufacture idle; it spreads the same work over more wall clock** — and every frame of this product has a deadline.

## The table makes a choice for the first time

With the device table refilled from measurement, `Auto` is no longer a constant:

| Planes running | Arbitration |
|---|---|
| motion + audio + display (the default boot shape) | **240 MHz** |
| **motion alone** | **80 MHz** |

All three paths were checked on the product firmware: `Auto` fully loaded gives 240, `Auto` with only motion gives 80, and an explicit `speed=slow` forces 80 while naming the plane it undercuts.

**Where each row comes from, and why one of them is worth saying separately:**

| device | worst @240 | x3 | row |
|---|---:|---:|---|
| Interaction (touch poll) | 3 ms | 9 ms | Slow |
| Motion | 3 ms | 9 ms | **Slow** |
| Audio (AEC tick) | 13.3 ms | 40 ms | Fast |
| Camera (capture poll) | 8 ms | 24 ms | Fast |
| Display (render) | 4 ms | 12 ms | Fast |

**Motion is 3 ms at 240 and 3 ms at 80** — its cost is almost entirely I2C wait and does not scale with the clock, which makes it the only row downclocking does not touch.

**Display is charged the 4 ms render, not the 16 ms panel write** — the write is DMA and does not stretch with a slower clock, and charging it would make the table pessimistic about the one figure that was never a CPU cost.

**The per-source worst cases already existed**: `input.rs` reports `worst source: button 0 ms, touch 1-3 ms, motion 3 ms, capture 5-8 ms` every window. These figures are not a new probe; they are the product's own report being read.

## The question the table cannot answer: the whole loop's worst case

Per-source worst cases do not add up to one loop's worst case. Two numbers the product already reports show that they do stack:

| @240 MHz | |
|---|---:|
| Slowest single source (capture poll) | 5–8 ms |
| **Worst single hold** | **10–13 ms** |

More than one source comes due in a loop, so **rows that each qualify on their own need not qualify together.** `speed_for` answers "how fast does this plane need", not "how long does one loop take" — two questions that do not meet.

**The whole loop is the binding constraint**, because the 30 ms bound is stated over a single hold:

| @80 MHz, every plane on | |
|---|---:|
| Slowest single source | 20–21 ms |
| **Worst single hold** | **25–26 ms** |
| Bound | 30 ms |

**83% of it.** Any additional work busts it.

**One criterion corrected in passing**: 13 ms @240 to 26 ms @80 is **2.0×**, not 3×. Part of it does not scale with the clock (I2C wait). So the table's "x3" is a conservative over-estimate — the real ratio is smaller, which if anything favours running slow.

**Not measured**: which sources actually coincide in one loop and by how much. `input.rs` reports the loop's worst without per-source timestamps, so this layer is still open.

## The table follows the chip, and does not live in core

Every row is a **worst hold in wall time, measured on that part**. The same deadline is a longer hold on a slower core and a shorter one on a faster, so **the numbers cannot live in a layer that cannot see the chip.**

`speed_for` is therefore a trait, implemented per chip:

| Layer | Content |
|---|---|
| `iot-core::demand` | `Speed` / `SpeedRequest` / `SpeedTable` / `ClockSink` / `resolve_speed` / `speed_shortfall` / `ClockGovernor` — mechanism, no numbers |
| `iot-chip-esp` | one `Table` per chip, `Speed → CpuClock`, and the `Part` that implements `ClockSink` |
| `iot-app` | two log lines in `render_loop`. **No longer knows `CpuClock` exists** |

**The table comes off the sink rather than being passed beside it** — `ClockSink::Table` is an associated type. The two are one measurement: a row says how long a plane holds **on that part**, so pairing one part's rows with another's `set_speed` computes a deadline for a chip that is not running. As an associated type, that pairing is a compile error rather than a runtime surprise.

**The governor lives in core because that is a shape this repository already has** — `RenderController` sits in `core::render`: equally stateful (it holds `last`), equally `&mut self`, acts only on a change, reaches its effect through something injected (a closure there, a `ClockSink` here), and returns whether anything moved (`bool` there, `Option<Applied>` here). **core allocates a `Box` too**, so "core only depends on libm" was never the reason to keep a stateful controller out of it — that is a fact about today, not a constraint.

**`&mut self` rather than a `static` buys three things back**:

- **The tests split.** As a global, "what it was last time" is shared across calls, so four independent assertions had to be packed into one sequential walk — the test said so itself: "split into tests, they would share it and pass or fail according to the order the runner picked". As `&mut self` they are four separate tests.
- **Two `run`s no longer tread on each other.** A static is process-wide, so two devices in one process each answer for the other.
- **`iot-core` does not need `log`.** `apply` returns a structured `Applied { asked, running, undercuts }`, leaving *what to report* to the layer that owns the output channel — the same split as `RenderController` not logging and `DiagnosticsSink` living in core while the reporting does not.

Switching every frame would cost microseconds against a 20 ms frame (0.25%), so **the expensive thing is the serial line, not the switch**: a USB-serial write measured 2634 µs at 80 MHz against 48 µs for the switch it would announce. The dedup is there to save the log, not the CPU.

**The two parts differ, so their answers differ:**

| | `CpuClock` presets | `Fast` means |
|---|---|---|
| S3 | 80 / 160 / **240** | 240 MHz |
| C6 | 80 / **160** | 160 MHz (the top this part has) |

**Not one row on the C6 has been measured**, and that is deliberate: **an unmeasured worst case on a chip is not a small one.** So it is all `Fast` — the top this part can reach — and whoever measures a plane here fills its row and cites it.

**Handing the C6 the S3's three numbers was wrong.** They are wall-clock holds measured on the S3 and mean nothing on the C6.

**`set_speed` returns the speed actually reached, not the one asked for**, so a downgrade is a return value rather than a failure: `Fast → Fast` on the S3; `Fast → Normal` on the C6 (160 is its top). `iot-chip_esp::Part` compares and reports "asked Fast, this part reached Normal". **No panic, no silence, no lie** — better than the `unimplemented!()` this started as, and better than moving nothing at all.

**The `set_cpu_clock` out-of-contract caveat does not hold on the C6**: esp-hal offers no runtime clock change there, so the C6's `set_speed` accepts the request and the clock does not move. That is stated rather than hidden — **a build relying on it should be visible.**

**None of this needs the board to test**: `ClockGovernor::apply` takes its sink by injection (the same shape as `Board` and `SpeakerRunner`), so a host build can pass a recording sink and assert that what comes back is what the part reached. The C6's downgrade is pinned by `Some(Normal)` instead of `Some(Fast)` on the host.

## An existing defect that was in the way

The release build of `vanling` **did not compile**, and `check-s3` could not see it (`cargo check` runs no codegen) while `build-s3` and `image-s3` are both marked `runInCI: false`.

rustc's Xtensa backend aborted with `Cannot select: i32 = XtensaISD::PCREL_WRAPPER TargetConstantPool`, in `Board::new`, on `DMA_CH2::INFO` — an associated constant holding interrupt handler pointers.

**Ruled out**: not introduced here (a `git stash` reproduces it); not LTO (it fails without it too); not inlining depth generally (the same `#[inline(never)]` on `Board::new` does nothing).

**What works is putting it on `new_dvp`** — keeping the DVP construction out of `Board::new`'s codegen keeps the constant out of it. The cost is one optimisation opportunity on a one-shot bring-up, unmeasured.

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
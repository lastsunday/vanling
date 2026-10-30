+++
title = "Hardware Constraint Record"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "7a61d153e9ee3ba2363f5f22ec3fd90432beb1b9"
translated_at = "2026-10-30T00:00:00Z"
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

The firmware has **no PM configuration at all** — no light-sleep, no `esp_pm_configure`, no `WFI`. The feed already runs on its own `InterruptExecutor` (`Feed::Interrupt` in `lib.rs`), so the executor-contention route is settled structurally and needs no lock.

So **not implementing locks is the right call**: adding two with no holder is dead code, and a deadlock is harder to diagnose than dead code.

But the order does matter. Before light-sleep there has to be a reference-counted lock set, held at minimum by:

| Holder | Why |
|---|---|
| Playback feed | its existence is the clock source; stopping it stops the codec's master clock |
| The capture chain | a completed DMA releases the ring, and a gated DMA means the ring is never drained |
| The DVP capture chain | same, and its chain-repair poll lives render-side |

`InputSource::set_demand_collecting` is already this shape (a device that is off cannot learn it from the next poll, so someone has to say it explicitly), so the wiring point is right and only the locks themselves are missing.

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

`[measured]` On 2026-10-28 the Ambient page reads **42,392 B of 65,976 B** (64.3%), leaving 23,584 B (info image boot log, `[DISPLAY] stack peak 42392 B of 65976 B`, the first window at 2.7 s); the earlier 36,453 B (43.9%) predates the render/state rounds. The firmware ships its own high-water measurement (`[DISPLAY] stack peak`), sampled once per repaint window and taken as the deepest value — a measurement, not an estimate. The Camera page's reading used to be `0 B` throughout: that path passes no sampling point, so it was unmeasured rather than shallow, and now samples like the others.

**An open question**: this peak is nearly constant from boot, and switching pages does not raise it — yet the historical crash point was the Audio page's full-frame stamp. Which path the peak actually belongs to is **not yet identified**. If it turns out to be rendering, rendering is heavier than the code comment describes, and it is worth chasing.

### After the per-page split: the wall did not move, the peak is 42,392 B (`[measured]`, 2026-10-28)

`[measured]` The same ruler (`nm apps/iot/target/xtensa-esp32s3-none-elf/release/vanling`), read after `state/` was split per page and the sound catalogue moved into `app`:

| Quantity | Reading |
| --- | --- |
| `_stack_start_cpu0 − _stack_end_cpu0` | `0x3fce9710 − 0x3fcd9558` = **65,976 B** (floor 65,536, margin **440 B**) |
| `__embassy_main::POOL` | **42,776 B** (`0xa718`) |
| `moon run iot:image` | S3 `1,059,904 / 6,291,456 B` (16.85%), C6 `842,400 / 1,048,576 B` (80.34%) |

These addresses are byte-identical to the readings before the split, so the two rounds add **0** to the S3 static footprint: what moved is where code lives, not the layout. The images are S3 `1,059,888 → 1,059,904 B` (+16 B); C6's last recorded figure was `826,976 B`, but rebuilt at HEAD with the same command it is **826,656 → 842,400 B (+15,744 B)**.

**That 15.7 KB sits entirely on the C6 side, and the source is measured**: `app/src/sounds.rs`'s `play` now parses before handing a source to the speaker, whereas at HEAD the parse lived in the S3 driver — C6's `UnwiredSpeaker::play(Sound)` did nothing, so LTO dropped it. The symbols show it: HEAD's C6 image has **zero** occurrences of `from_midi`, the `Score::into_notes` sort or `Tone::fill`; this round has seven. `control_loop::poll` therefore grew by 5,466 B, and `iot_core::drivers::playback` symbols total 10,608 B this round. C6's `take_playback()` returns `None`, so the path never runs: it costs slot, not cycles. C6 has 206,176 B left, so this is recorded rather than urgent — whether to put the parse back behind compile time (a soft feature, or keeping `play` only in builds with a speaker) remains an open decision.

That build set no `IOT_WIFI_*`, and credential strings shift `_stack_end`, so this wall is comparable only with builds that also carry no credentials.

`[measured]` **Peak: `42,392 B of 65,976 B`** (64.3%, leaving 23,584 B). The line appears in the product info image's boot log (`moon run iot:flash-s3`, the first window 2.7 s after reset, Ambient page), and `main stack` matches `nm` — so `[DISPLAY] stack peak` does not need a debug image: the high-water prints at info for every 1,024 B it gains, and only a repeated reading drops to debug. The figure covers the splash and the page it lands on.

`[measured]` **Cross-page worst case: 42,392 B of 65,976 B** (the Speaker page reads 42,376 B, the only one lower). Each of the six pages was rebuilt with `IOT_BOOT_PAGE=<page>`, flashed and monitored once; same board, same session, every one of them got `192.168.1.136`:

| Page | `App/part` B | `main stack` B | Peak B |
| --- | --- | --- | --- |
| ambient | 1,059,904 | 65,976 | 42,392 |
| attitude | 1,059,952 | 65,976 | 42,392 |
| audio | 1,059,984 | 65,976 | 42,392 |
| speaker | 1,060,192 | 65,976 | 42,376 |
| camera | 1,059,920 | 65,976 | 42,392 |
| network | 1,059,936 | 65,976 | 42,392 |

`main stack` is identical across all six: the boot page only picks an entry, it does not move static layout. The Speaker page's peak is 16 B shallower than the rest — that is that page's draw chain being shallower, not measurement granularity: the line triggers once per 1,024 B gained, but the value printed is the exact high-water. Budgeting still takes the ceiling, 42,392 B. **The 440 B is the distance from the wall to the floor; 42,392 B is the water line already reached.**

### After the B/C split the wall is 160 B wider (`[measured]`, 2026-10-29)

`[measured]` Same ruler, read after `core/src/page/` became one module per page, `state/tests.rs` moved into the modules it tests, `ui/src/pages/live.rs` split into three pages, and the frame cadence became `app/src/frame.rs` (measured once the Docker engine came back; the build carried no `IOT_WIFI_*`):

| quantity | reading | previous (end of A) | delta |
| --- | --- | --- | --- |
| `_stack_start_cpu0 − _stack_end_cpu0` | `0x3fce9710 − 0x3fcd94b8` = **66,136 B** | 65,976 B | **+160 B** |
| margin (minus 65,536) | **600 B** | 440 B | +160 B |
| `__embassy_main::POOL` | **42,784 B** (`0xa720`) | 42,776 B | +8 B |
| `moon run iot:image` S3 | `1,059,216 / 6,291,456 B` (16.84%) | 1,059,904 | **−688 B** |
| `moon run iot:image` C6 | `843,728 / 1,048,576 B` (80.46%) | 842,400 | **+1,328 B** |

`[measured]` POOL grew 8 B while the wall widened 160 B, so other statics shrank by 168 B: B/C only move code, and LTO laid the result out again across the new module boundaries rather than any one thing being deliberately made smaller. The C6's +1,328 B is not attributed symbol by symbol — the C6 still has 204,848 B left, so only the number is recorded. `check-s3`, `build-s3`, `iot:image` and `check-features` (six host subsets plus nine bare-metal/C6 subsets) all pass this round.

`[measured]` The first attempt went unmeasured because Docker Desktop's engine was down on this machine: `moon run iot:check-s3` stopped at `image vanling/xtensa:esp32s3_1.98.1.0 is missing` and `docker info` reported `Cannot connect to the Docker daemon at unix:///Users/yu/.docker/run/docker.sock`. After the engine came back (Server 28.5.1) the same tree produced the table above.

### Two large board fields boxed: the wall is 35,616 B wider (`[measured]`, 2026-10-30)

`[measured]` `Board` travels by value into `main`'s future, and that future lives wholly inside the `__embassy_main::POOL` static pool, so **every inline field is paid again as a static**. This round boxes the two devices the `lckfb_szpi_esp32s3` board carried by value (the field `light` is renamed to `screen` at the same time; the C6's `light` is a real WS2812 and stays):

```rust
screen: Option<Box<Screen>>,                    // was light: Option<Screen>
audio: Option<Box<Es7210Rx<…, REFERENCE_SAMPLES>>>,   // was Option<Es7210Rx<…>>
```

`size_of` (probed under `check-s3` with `let _: [u8; 0] = [0u8; size_of::<T>()]`; the probe is not left in the tree): `Screen` **2,712 B** (`Diagnostics` is 2,568 B of it), `Option<Es7210Rx<…>>` **9,168 B** (the AEC's `AdaptiveFilter` dominates), `Board` **240 B** with both boxed (**12,112 B** with both inline, derived from the two readings above).

| quantity | both boxed (this round) | screen only | neither | delta (neither → both) |
| --- | --- | --- | --- | --- |
| `_stack_start_cpu0 − _stack_end_cpu0` | `0x3fce9710 − 0x3fcd09b8` = **101,720 B** | 74,232 B | 66,104 B | **+35,616 B** |
| margin (minus 65,536) | **36,184 B** | 8,696 B | 568 B | +35,616 B |
| `__embassy_main::POOL` | **7,192 B** (`0x1c18`) | 34,696 B (`0x8788`) | 42,808 B (`0xa738`) | **−35,616 B** |

All three readings come from the same tree and the same ruler (`nm -S apps/iot/target/xtensa-esp32s3-none-elf/release/vanling`), the only differences being those two boxings and the field rename. Each POOL drop is about three times the field's size (screen **−8,112 B** ≈ 3 × 2,712; microphone front end **−27,504 B** = 3 × 9,168), i.e. each value sits three times in the future's layout (board construction, the `take_*` temporary, and the landing in `WiredSurface`/`PollEntry`); the symbols are not attributed one by one.

**The heap is unchanged**: both `Box`es are freed in `take_surfaces` / `take_audio` and the values move straight into the `Vec<WiredSurface<Screen>>` and the `PollEntry` `Box` that already existed, so the S3's 96 KB PSRAM heap peaks at the same size — those bytes simply land on the heap at boot and are moved again as they were before.

`check-s3`, `check-c6`, `check-host`, `build-s3` and `cargo clippy -- -D warnings` on the S3 target all pass this round.

---

## `IOT_BOOT_PAGE` must be resolved at build time (`[measured]`, 2026-10-29)

`[measured]` The symptom: any build that **set** `IOT_BOOT_PAGE` died at the join — the log stopped after `[WIFI] this device saved a network; joining …`, with `StoreProhibited` (sometimes `LoadProhibited`), the PC inside esp-radio's `os_adapter::free` / `event_post` and `EXCVADDR` at `0x3`. The same source built without the variable, flashed to the same board, connected and got an address every time; HEAD (`a13e96f5`) with `IOT_BOOT_PAGE=ambient` also connected (peak 41,880 B of 65,912 B).

`[measured]` The fault is not on the main stack: the exception's `THREADPTR = 0x3fcd4908` is a radio task's TCB and `A1 = 0x3fcd4570` sits 20,472 B below the main stack's `_stack_end = 0x3fcd9568` — that is the radio task's own stack. What got corrupted is the radio blob's static data, the run starting at `sta_con_timer` (`0x3fcd8974`) that sits directly below the main stack floor. `ESP_HAL_CONFIG_STACK_GUARD_MONITORING = "false"` in `.cargo/config.toml` means an overrun produces no warning at all, only "somebody else's code crashed" (next section).

`[measured]` The difference is pinned to one place: `boot_page()` used to resolve `option_env!` at run time — `trim().to_ascii_lowercase()` into a `String`, then a table lookup. Same source: without the variable the join uses 23,409 B and the peak is 42,392 B; with `IOT_BOOT_PAGE=ambient` it is 27,745 B / 46,728 B, **+4,336 B**, and then it always crashes. The build with the variable has a main-task poll frame of `entry a1, 0x6710` (26,384 B); HEAD's is `entry a1, 0x5540` (21,824 B) — the latter also carries this round's other changes, so the absolute frame difference cannot all be charged to the boot page; what lines up item for item is the pair of same-source readings above.

`[not separated]` Which region those 4,336 B overwrote was not separated: it could be the main stack running past the floor straight into the blob's `.bss`, or simply the larger frame changing the internal-heap layout so a radio task's stack landed on the blob's statics. Both explain the readings and neither changes the fix.

`[measured]` The fix: the boot page becomes a **compile-time** constant — `const BOOT_PAGE: DisplayPage = boot_page_from_env(option_env!("IOT_BOOT_PAGE"))`, with a `const fn` trimming whitespace and comparing case-insensitively; an unrecognised name is kept separately in `const UNKNOWN_BOOT_PAGE: Option<&str>` and `boot_page()` logs the `[IOT] IOT_BOOT_PAGE=… is not a page` line from it (it is `None` for a valid name, so the whole `if let` folds away). After the fix an `IOT_BOOT_PAGE=attitude` build matches the product build item for item (`main stack 65,976 B`, peak `42,392 B`, 23,409 B at the join) and associates and gets `192.168.1.136` normally. The host test `only_a_name_no_page_answers_to_is_reported` pins "only an unknown name warns, and a valid one stays quiet in every spelling".

**Constraint**: the boot page stays compile-time. A bench image and the product image share one stack budget, and that is the premise — the six pages above have identical `main stack` because the boot page takes no part in run-time layout.

---

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

The table lives in `apps/iot/crates/ui/src/font.rs`: 95 printable ASCII glyphs, five columns by
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
- **Regression**: `iot:test` holds "every printable glyph has ink", "look-alike pairs are not one
  bitmap" (`l/1`, `I/l`, `0/O`, `c/e`, `u/v`, `g/q`, `p/q`, `y/v`, `m/w`, `M/W`) and "the descenders
  reach the bottom row" (`g/p/q/y/j`). The `0/O` pair was in fact identical after the first swap, and
  the second test is what reported it.

The table is flash: `[u8; 95 × 5] = 475 B`, the same size as the old one; the static wall is 65,720 B
before and after (floor 65,536 B, 184 B spare).

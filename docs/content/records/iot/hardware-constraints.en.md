+++
title = "Hardware Constraint Record"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "c5cd9c428f1436e61ac9036dbe6bdca3204e4e77"
translated_at = "2026-11-05T00:00:00Z"
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

**That 15.7 KB sits entirely on the C6 side, and the source is measured**: `app/src/playback/sounds.rs`'s `play` now parses before handing a source to the speaker, whereas at HEAD the parse lived in the S3 driver — C6's `UnwiredSpeaker::play(Sound)` did nothing, so LTO dropped it. The symbols show it: HEAD's C6 image has **zero** occurrences of `from_midi`, the `Score::into_notes` sort or `Tone::fill`; this round has seven. `control_loop::poll` therefore grew by 5,466 B, and `iot_core::drivers::playback` symbols total 10,608 B this round. C6's `take_playback()` returns `None`, so the path never runs: it costs slot, not cycles. C6 has 206,176 B left, so this is recorded rather than urgent — whether to put the parse back behind compile time (a soft feature, or keeping `play` only in builds with a speaker) remains an open decision.

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

### Panel SPI copy buffers 8 KiB → 128 B, widening the stack wall by 8,088 B (`[measured]`, 2026-11-01)

`[measured]` The panel bus's two 4 KiB `with_buffers` never take part in a frame transfer. `SpiDma::write`'s `DmaOperationKind::for_write` calls an **internal-DRAM slice** `InPlace` (`memory_range!("DRAM")` is `0x3FC88000..0x3FD00000`; `FRAME_ARENA` sits at `0x3fc9b420`), so the 153,600-byte frame and the on-stack commands and arguments all go straight through; only a **constant slice rustc promoted into `.rodata`** (`&[0x01]` and friends) lands in flash, out of the DMA's reach, and takes the copy buffers.

`[measured]` Dropping `with_buffers` outright **does not boot**: `board init failed, resetting: Spi(DmaError(BufferTooSmall))` — the software reset in `panel_spi` writes exactly such a constant. The buffers therefore stay, sized by the longest command argument: 64 B each, 128 B together.

| quantity | before | after | delta |
| --- | --- | --- | --- |
| `.bss` | 231,064 B | 222,976 B | −8,088 B |
| `.stack` (the `dram_seg` half) | 43,880 B | 52,000 B | +8,120 B |
| `main stack` (boot log) | 101,240 B | **109,328 B** | +8,088 B |
| `[HEAP]` | 96 KiB | **512 KiB** | the heap is PSRAM, not internal DRAM |

`dram_seg` is still 341,760 B with no slack: the linker folds whatever is saved straight into the main stack, so the gain is a **wider wall**, not bytes that become available.

`[measured]` The frame transfer did not slow down: with the 64 B buffers, eight of the nine repaints in one window landed at 15–19 ms (153,600 B / 80 MHz = 15.36 ms). The ninth read 610 ms, and in that same window `[INPUT]` reported `worst single hold 597 ms … worst source: touch 597 ms` — the touch poll was holding the executor, not the buffers (the frame lives in `.bss` and never goes near them).

`[measured]` The same round cut the radio driver's `dram_seg` share from 16 KiB to 8 KiB (`INTERNAL_BYTES` in `radio/esp32s3.rs`): boot prints `[WIFI] driver heap: 8192 B in dram_seg + 32768 B above it`, so the total goes 49,152 → **40,960 B** and the main stack 109,328 → **117,520 B** (+8,192, matching the share taken away byte for byte). Against the driver's already-measured 33,332 B peak that leaves about **7.6 KB** (32.2% → 23.1%); the product associates, gets an address and holds it at that size with no `OutOfMemory`. The S3 and C6 driver heaps are no longer the same size (the C6 keeps a single 48 KiB block).

`[measured]` The heap went 96 KiB → 512 KiB: boot prints `[HEAP] 524288 B at 0x3c100000 in PSRAM, of a 8388608-byte window`. Before, boot left about 1.6 KB free, so the next `Box` was an allocation failure; the PSRAM window is 8 MiB, so 512 KiB costs no internal DRAM.

### The panel frame goes out in 32-row chunks with a yield: longest contiguous hold 15 ms → 1–2 ms (`[measured]`, 2026-11-01)

`[measured]` One frame is 153,600 B / 80 MHz = **15.36 ms**, and a single `SpiBus::write` held core 0's cooperative executor for that whole stretch — the feed, the touch/motion poll, the capture poll and the render all share it. The fix is not a different driver but a transfer that lets go of the executor in the middle.

**Async DMA does not work here**: `SpiDma<Async>::write_async` puts its `[DmaDescriptor; LINK_DESCRIPTOR_COUNT]` (`MAX_DMA_SIZE.div_ceil(CHUNK_SIZE) + 3`) inside the coroutine frame, and this repo always `Box::pin`s futures onto the PSRAM heap, so the first real write panics:

```
panicked at ... unwrap of DescriptorSet::new(descriptors) failed: UnsupportedMemoryRegion
```

The descriptors have to live in internal DRAM, which a heap-boxed future cannot offer. So this board **cannot** use esp-hal's async DMA together with a boxed future.

What is used instead: the driver stays `SpiDma<'static, Blocking>`, `write_frame` becomes `async`, and the frame ships in **32 controller rows = 15,360 B** chunks with `embassy_futures::yield_now().await` between them; the descriptors stay in the blocking call's own stack frame (internal DRAM). `FRAME_CHUNK_BYTES ≤ 32,736` (esp-hal's private `MAX_DMA_SIZE`) is pinned at compile time by `const _: () = assert!(...)`, because overrunning it fails at run time with `MaxDmaTransferSizeExceeded` and shows up as a frame that never reaches the glass.

**Safe on the wire**: `panel_spi` wires no CS pin (only `.with_sck().with_mosi()`), so chunking inserts SCK idle gaps, DC stays high throughout, and the byte stream is identical to one transfer; rendered output and page behaviour are unchanged.

Only one interface surface grew: `PageSurface::flush` / `Renderer::flush` return `Option<Pin<Box<dyn Future>>>` and `Registry::flush()` awaits it once after `tick`. Painting stays synchronous (RAM only); the transfer happens in the flush. `camera-probe` awaits the same `write_frame`.

Accounting: `hold` is render + total wire time (the CPU load did not change, it is just no longer one unbroken stretch), and the new `worst chunk` is the longest single transfer — that is the **real longest contiguous hold**. This section's readings use the old accounting; the next section moves the transfer to the second core, after which `hold` is render only and `worst wait` reports the parked time.

| reading (Attitude page, `IOT_BOOT_PAGE=Attitude IOT_LOG_LEVEL=debug`) | before | after |
| --- | --- | --- |
| first window | `6 repaints in 2046 ms, held 110 ms (5%), worst render 3 ms, worst write 15 ms` | `6 repaints in 2724 ms, held 110 ms (4%), worst render 2 ms, worst write 17 ms (longest chunk 2 ms)` |
| steady window | `10 repaints in 2097 ms, held 193 ms (9%), worst render 4 ms, worst write 16 ms` | `12 repaints in 2202 ms, held 229 ms (10%), worst render 3 ms, worst write 16 ms (longest chunk 2 ms)` |

`worst write` is still 15–17 ms (the wire time does not shrink because it is chunked); what changed is that it is no longer one stretch: `longest chunk` is 1–2 ms. The occasional 6xx ms in the boot window is Wi-Fi association landing inside a single chunk transfer; it predates this change (the same window reports `[INPUT] worst single hold 597 ms … worst source: touch 597 ms`) and has nothing to do with chunking.

`[measured]` Resources are unchanged: boot prints `[DISPLAY] … main stack 117520 B`, the same as HEAD (`nm` reads `_stack_start_cpu0 − _stack_end` = `0x3fce9710 − 0x3fcccc00`), with `.data` 18,904 B, `.data.wifi` 496 B, `.bss` 214,784 B and a 1,055,264 B app. Dropping the async driver also dropped the +88 B of `.data` it had added.

### Audio repaint step doubled + panel transfer moved to the second core: core 0 holds 40% → 3% (`[measured]`, 2026-11-01)

`[measured]` The section above cut a frame's **contiguous** hold from 15 ms to 1–2 ms, but the 16 ms of wire time was still core 0's bill: `SpiDma<Blocking>`'s busy wait was merely sliced, and the whole stretch still belonged to the cooperative executor. The Audio page makes that visible — `[DISPLAY] Audio 44 repaints in 2059 ms, held the executor 819 ms (40%)`, while `[INPUT]` takes 39% in the steady window of the same build (`1970 ms of 5003 ms`, below) — meaning core 0's bulk was always the capture fold, not the panel. Both were changed, cheapest first.

**One: the Audio page's repaint step goes 40 → 80 ms.** The threshold moves off `render::DIAG_REPAINT_STEP_MS` onto each page's render policy (`ui::render::policy::repaint_step_ms(page)`); only Audio is twice the default, every other page stays at 40. The reason is that this page's drift arrives faster than anyone reads it: a capture poll closes two envelope columns every 20 ms, so the 40 ms step already redrew the 200-column sweep every ~45 ms; at twice the step a frame advances eight columns — still a sweep, at half the cost. `the_audio_page_waits_twice_the_default_step` pins it.

| reading (`IOT_BOOT_PAGE=Audio IOT_LOG_LEVEL=debug`, ~2 s window) | before | step only | then second core |
| --- | --- | --- | --- |
| repaints | 44–45 | 27–30 | 26–30 |
| `held the executor` | 819 ms (40%) | 505–561 ms (24–27%) | **65–76 ms (3%)** |
| `worst render` | 3 ms | 2–3 ms | 2–3 ms |
| `worst wait` | — | — | 17 ms |
| `worst write` (`longest chunk`) | 16 ms (1–2 ms) | 16 ms (1–2 ms) | 15–16 ms (1 ms) |

**Two: the panel's transfer moves to the second APP core.** Controller and bus become `PanelBus` and `Screen` keeps only a channel; the coordinator posts the finished frame as a `Ticket` (raw pointer plus length, `unsafe impl Send`) into `REQUESTS: Channel<_, Ticket, 1>` and then parks in `SHIPPED.wait()` for the answer, while `panel_service` runs `write_frame` on the other core and signals a `Result<Wire, Error>`. Safety is the handshake itself: the frame is the `'static` `static mut` arena, `Panel::write` sits in `wait()` for the ticket's whole life without touching it, and only one request is ever in flight.

`[measured]` The trigger had to move: `esp_rtos::start_second_core` hard-codes `FROM_CPU_INTR1`, which the product's `Priority2` speaker feed executor already held, so the feed moves to `FROM_CPU_INTR2` (`InterruptExecutor<2>`) and the cross-core wakeup keeps INTR1. The second core's stack is a 16 KiB static (`PANEL_CORE_STACK` in `chip-esp`) — `.stack` fills whatever is left of the region, so this is a slice off the main-stack wall, not an addition.

`[measured]` The accounting changes with it: `hold` is now render only (the coordinator's own CPU) and the parked time is its own `worst wait`. So "40% → 3%" is **a transfer of ownership of the hold**, not a shorter wire time: those 16 ms are still spent, just on the other core; `worst write` is still 15–16 ms and `longest chunk` 1 ms — but from this section on, `longest chunk` measures **core 1's hold** and no longer enters core 0's account; it is a jitter figure for the panel service's own lane and may not be added to the `[INPUT]`/`[DISPLAY]` holds.

| resource (`size -A` / `nm`, same tree) | before | after | delta |
| --- | --- | --- | --- |
| `.text` | 850,713 B | 857,729 B | +7,016 B |
| `.bss` | 214,784 B | 231,544 B | +16,760 B (16 KiB stack + channel/signal) |
| `.data` | 18,904 B | 18,928 B | +24 B |
| main stack (boot log / `_stack_start_cpu0 − _stack_end`) | 117,520 B (`0x3fcccc00`) | **100,728 B** (`0x3fcd0d98`) | −16,792 B |

`[measured]` Nothing regressed: the Attitude page reads `[DISPLAY] Attitude 5–9 repaints in 2041–2560 ms, held the executor 16–29 ms (0–1%), worst render 3 ms, worst wait 16 ms, worst write 15 ms (longest chunk 1 ms)`; both pages report `stack peak 29848 B of 100728 B`, with no panic and no `[DISPLAY]` warning. Core 0's largest remaining holder is the capture fold (`[INPUT] held the executor 1970 ms of 5003 ms`, ~39%), untouched here.

`[measured]` Checks: `check-s3`, `check-c6`, `check-host`, `check-features` (every subset), `build-s3` (only the pre-existing RWX linker warning), `cargo clippy -- -D warnings` for the S3 target, `lint`, `lint-host`, `test` and `test-bsp` all pass this round.

### The driver half moves to core 1 whole: core 0 keeps only render's 4% (`[measured]`, 2026-11-01)

The section above left core 0's remaining 39% — the capture fold — recorded as "untouched here". This section touches it: `input_task` and `control_loop`, together with the drivers they hold, become one `Io` bundle that an `IoPlacement` routes — `Elsewhere(&IoQueue)` hands it to `run_io_from` on the second core, `Inline(spawner)` keeps it on the coordinator's own executor. A single-core C6, and an S3 with no panel, take `Inline`: same tasks, same code path, only the place differs.

| reading (`IOT_BOOT_PAGE=Audio IOT_LOG_LEVEL=debug`, ~2 s window, steady state) | before (panel on core 1) | after (driver half also on core 1) |
| --- | --- | --- |
| `[DISPLAY] Audio` repaints | 26–30 | 26–29 |
| `held the executor` | 65–76 ms (3%) | 83–95 ms (4%) |
| `worst render` | 2–3 ms | 3–4 ms |
| `worst wait` | 17 ms | 26–47 ms |
| `worst write` (`longest chunk`) | 15–16 ms (1 ms) | 15–16 ms (1 ms) |
| `[INPUT] held the executor` | 1970 ms of 5003 ms (39%), charged to core 0 | 1677–2026 ms of 5001–5007 ms (33–40%), charged to core 1 |
| `[INPUT] worst single hold` | — | 9–19 ms (bound 30 ms), capture 7–16 ms |
| `[PLAY] cadence` | — | 400 feeds in 2000 ms (100% of nominal), 0 gaps over 20 ms, worst gap 5 ms, worst work 428–707 µs |
| `stack peak` | 29848 B of 100728 B | 24008 B of 91824 B |

`[measured]` The capture fold's bill moved house almost verbatim: the same `[INPUT] held the executor … of 5001 ms` reading, same numerator and denominator, now lands on core 1 instead of core 0; core 0's executor keeps only render's 4%. The price is single and nameable — the Audio page's `worst wait` goes from 17 ms to 26–47 ms: the panel service shares core 1's one cooperative executor with the capture poll (7 ms at worst, 16 ms in the boot window) and the speaker feed, so a full 15 ms transfer plus that stretch lands around 30 ms, with the occasional 47 ms. The repaint count is unchanged (26–29): the extra wait does not drop frames.

`[measured]` Playback, motion and camera did not get worse: `[PLAY]` runs at the full 400 feeds/2 s with 0 gaps; the Attitude page reads `[DISPLAY] Attitude 5–9 repaints in 2022–2779 ms, held the executor 15–30 ms (0–1%), worst render 3–4 ms, worst wait 17–19 ms, worst write 15 ms (longest chunk 1 ms)` with `[INPUT] held the executor 632–637 ms of 5003–5005 ms` in the same window (12–13%: that page does not capture) and a 1856 B core-1 stack; the Camera page's steady state reads `[DISPLAY] Camera 11 repaints in 2118–2120 ms, held the executor 3–4 ms (0%), worst render 0 ms, worst wait 17 ms, worst write 15 ms (longest chunk 1 ms)`, the same order as before. Every boot onto the Camera page reports one `[CAM] no descriptor completed in 420 ms, stalled at 0 of 40, peripheral running (re-arm 1/2)` in its first window (same window: `worst wait 639 ms`) and never again; that path rides core 0's render loop (`advance(frame)` writes the panel frame during paint) and is untouched here.

`[measured]` Resources: `.text` 857,729 → 857,321 B (−408), `.bss` 231,544 → 240,448 B (+8,904: second core's stack 16 → 24 KiB, the rest the `IoQueue` and `run_io_from`'s task slot), `.data` 18,928 B unchanged, App 1,062,400 B; main stack 100,728 → 91,824 B.

`[measured]` Those 24 KiB now have a reading: one `[STACK] core 1 peak {high} B of {total} B` line every 5 s (info only once the peak moves a kilobyte, debug otherwise), one round per page:

| page (`IOT_BOOT_PAGE=…`) | what stays on | core 1 stack peak |
| --- | --- | --- |
| Audio | capture + panel + playback | **4556 → 4780 B** of 24,576 B (19%) |
| Speaker | panel + playback | 2336 B |
| Attitude | panel + motion polling | 1856 B |
| Camera | panel | 1920 B |

`[measured]` The deepest path is the audio capture poll, and 19,796 B of the 24 KiB are left for the driver tasks still to come. The method is to fill the stack with 0xA5 (a word pattern: a lone byte turns up in ordinary data) and then find the lowest word that is still intact; the watermark only grows. The scan starts 512 B above the floor because esp-hal parks its own entry closure in the bottom few bytes before the second core starts, and without that skip the reading is always a full stack (the first attempt read 24,512 B). Re-painting from inside core 1 was tried and the board would not come up, so it was reverted: writing across cores into the stack you are standing on does not work.

`[measured]` Checks: `check-s3`, `check-c6`, `check-host`, `check-features`, `lint`, `lint-host`, `test` (726 + 10 + 29 + 42), `test-bsp` (101), `cargo clippy -- -D warnings` for the S3 target and `build-s3` (only the pre-existing RWX linker warning) all pass; a C6 board flashed with it comes up on the Ambient page, lights the WS2812, no panic. The `Send` on the three trait objects (`InputSource`/`EventAggregator`/`FrameSource`) and on `CaptureSink` is a compile-time constraint: something that cannot cross a core cannot go into the bundle, and no per-site inspection is needed to say so.

### The network worker moves from `join` to a task: the stack region loses 64 B (`[measured]`, 2026-11-02)

`main` no longer `join(run, net::serve)`: the network is an `#[embassy_executor::task]` (started by `run` in that cut), the same lifecycle as the driver half. Its static storage moves out of `main`'s pool into a pool of its own, so the total differs by one task header. Measured before and after on the same tree with the same command (`bash ../../scripts/iot-xtensa.sh build --release -p iot-app --bin vanling --target xtensa-esp32s3-none-elf -Z build-std=core,alloc --no-default-features --features lckfb-szpi-esp32s3,console-auto`):

| Reading (`nm -S`, S3 `vanling`) | Before | After |
| --- | --- | --- |
| `__embassy_main::POOL` | 2,528 B (`0x9e0`) | 1,432 B (`0x598`) |
| `network::task::POOL` | — | 1,168 B (`0x490`) |
| `_stack_start_cpu0 − _stack_end_cpu0` | 96,432 B | 96,368 B |

`[measured]` The two pools add up from 2,528 to 2,600 B (+72 B) and the stack region loses only 64 B. `network::task`'s future is just 1,168 B because the large pieces inside `link` (the credential store, the scan history, `browsing`'s future) are already on the heap. `_stack_end_cpu0` sits at `0x3fcd1ea0`, still far above the 65,536 B floor. `check-s3` only type-checks; this delta is visible only to `nm` on a linked image.

The boot log from that image confirms the task actually starts (in that cut `run` started the worker, hence the order): `[IOT] starting the net worker` appears after `[IOT] demand …` and before `[DISPLAY] stack peak`, immediately followed by `[WIFI] network stack up, DHCP requested`. `[DISPLAY] backlight dark until the first frame; main stack 96368 B` and `[DISPLAY] stack peak 14917 B of 96368 B` agree with the region size from `nm`, so moving the pool did not change the stack region.

### The network worker is spawned at the composition point: pool −16 B, stack region +32 B (`[measured]`, 2026-11-02)

`WorkerStart { name, start }` is gone: only the entry point knows the network task's driver types, so `main` spawns it directly before handing control to `run`; `run` holds no worker list and no longer owns "which workers start this boot". Same tree, same command:

| Reading (`nm -S`, S3 `vanling`) | Previous | This cut |
| --- | --- | --- |
| `__embassy_main::POOL` | 1,432 B (`0x598`) | 1,416 B (`0x588`) |
| `network::task::POOL` | 1,168 B (`0x490`) | 1,152 B (`0x480`) |
| `_stack_start_cpu0 − _stack_end_cpu0` | 96,368 B | 96,400 B |

`[measured]` The pools lose 32 B and the stack region gains 32 B — mirror images, so the net is zero; no per-symbol attribution. `_stack_end_cpu0` = `0x3fcd1e80`. App 1,062,128 B (S3) / 845,392 B (C6, 80.62% of its partition).

`[measured]` The boot log pins both the new start order and the splash verdict: the S3 prints `[IOT] starting the net worker` and then `[IOT] demand …` (because `spawn` happens before `run`), and `[DISPLAY] … main stack 96400 B` matches the `nm` reading. Eight milliseconds after `[INPUT] the splash is over; setup mode was not asked for` comes `[WIFI] this device saved a network; joining …`, so the worker really was waiting on `NetworkCommand::SplashOver` and not on some other signal. The C6 shows the same `[IOT] starting the net worker` → `[WIFI] network stack up`, then `[WIFI] no page surface and no saved network; waiting offline` as soon as the splash ends, with the WS2812 walking the offline burst back to the page's own light. Neither board panicked.

`[not re-measured]` The later Wi-Fi rewrite moved the start back into `run`: the entry point only hands over `Wifi::Worker(NetworkWorker)`, a closure taking a `Spawner`, and `run` runs it before `run_loop`. The two pool readings above belong to the "entry point spawns directly" cut; the rewrite ran only `check-s3` / `check-c6` / `check-host` (type-check only), so there is no new `nm` reading.

### The IRAM mirror is 47,396 B and none of it is this repo's (`[measured]`, 2026-11-01)

`[measured]` On the S3, IRAM and DRAM are two views of the same SRAM, so every piece of code placed in IRAM also occupies a same-sized mirror inside `dram_seg` — that is what `.rwdata_dummy` is: **47,396 B**, the sum of three sections (`objdump -h`):

| section | bytes | what it is |
| --- | --- | --- |
| `.vectors` | 1,024 | the interrupt vector table |
| `.rwtext` | 14,392 | esp-hal / esp-rtos / esp-storage / xtensa-runtime code placed in IRAM |
| `.rwtext.wifi` | 31,980 | the radio blob's IRAM copy (an ESP-IDF requirement for the blob) |

`[measured]` Grouping `.rwtext` by symbol (`nm -S` over `0x40378400..0x4037bc38`):

| owner | bytes | why it has to be in IRAM |
| --- | --- | --- |
| esp-hal | 4,843 | PSRAM init and mapping (cannot fetch from flash while it rewrites the cache/SPI0), GPIO and I2C ISRs, cache setup |
| esp-rtos | 3,547 | the scheduler: `trigger_task_switch`, `RunQueue`, `timer_tick_handler`, all in interrupt context |
| xtensa runtime, exception/interrupt entry | 1,789 | `Reset`, `save_context`/`restore_context`, `__level_{1..7}_interrupt`, `__INTERRUPTS` |
| esp-radio / phy glue | 1,048 | `semphr_take/give`, `interrupt_dispatch`, `tx_done_cb`, `__esp_phy_*`, `__pender` |
| esp-storage | 765 | `spiflash_read/write/erase_*`: the cache is off while flash is being written |
| linker symbol table and alignment | 2,400 | the `sym_*` address table (984 B) plus the gaps between 127 symbols |

`[measured]` **This repo contributes 0 of those 14,392 B**: `rg '#\[ram'` finds exactly one use, `RECLAIMED_MEM` in `radio/esp32s3.rs`, and that is data rather than code. So "move the functions that do not need IRAM back to flash" has no target here — the only movable bytes are esp-hal's own choices (about 1.7 KB of PSRAM init, 410 B of `gpio::wakeup::record_wakeup`), and taking them means patching a dependency, which is not worth it.

`[measured]` **D-cache 32 → 16 KB is not an option**: the S3 has only 32 KB and 64 KB. Setting `ESP_HAL_CONFIG_DATA_CACHE_SIZE` to `16KB` is rejected by esp-hal's build script:

```
Validation error for crate esp-hal, option data-cache-size: Expected one of ["32KB", "64KB"], found '16KB'
```

`[measured]` **The second core is not free**: `Stack<N>` is a DRAM static (the probes use 8 KiB / 16 KiB), so it takes the same amount straight out of the main-stack wall; and `esp_rtos::start_second_core` takes `FROM_CPU_INTR1` by signature, which the product's `Priority2` speaker feed executor used to hold (`start_feed_executor(feed_intr)`). Using the second core means moving the feed to `FROM_CPU_INTR2` first and paying for the cross-core plumbing — an architecture change, not a tuning knob; the panel transfer's price and payoff are in "Audio repaint step doubled + panel transfer moved to the second core" above (`.text` +7,016 B, `.bss` +16,760 B, main stack −16,792 B).

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

The table lives in `apps/iot/crates/core/src/ui/font.rs`: 95 printable ASCII glyphs, five columns by
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

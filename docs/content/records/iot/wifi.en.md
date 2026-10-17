+++
title = "Wi-Fi Implementation Record"
weight = 45
sort_by = "weight"
[extra]
source_file_hash = "dfd4a736f6867881145553a289276930db362c96"
translated_at = "2026-10-17T12:56:45Z"
+++

<!-- doc-audience: ai -->

# Wi-Fi Implementation Record

The Wi-Fi link in `apps/iot`, from "which networks are out there" to "the device has
an address". **AI output, not reviewed by a human** — see the
[parent note](@/records/_index.md).

Each judgement carries its basis: **`[test]`** a unit test pins it, **`[measured]`**
a hardware measurement, **`[as-reported]`** not reproduced, left as a trace.

## State

**The probe has run end to end on both parts** (scan, associate, DHCP). The product page
lands in two slices: **the read-only one is in** (`DisplayPage::Network`: phase, the access
point in use, the address, and the scan list), and **the second slice is in as well** (the
4 × 4 keyboard, the reveal key, a list that slides a row at a time, hysteresis and caching for
the list, the selected row reversed, open networks, failure classes, the page-gated survey with
its backoff, and **the action strip** — a `LEAVE` button when there is a link and a `SCAN`
button when there is not, the first asking once before it acts and the second not asking at all
— see "The product page"), **the credential is kept on the part** (a `creds` partition and our
own slot format; read back at boot and written the moment an association succeeds — see "Where a
credential lives"), **a stored network carries a `*` in the list, with a long press on its row
to forget it**, and **a tap on a saved row joins it with the copy of the passphrase the device
keeps** (see "Tapping a saved row joins it with the copy the device keeps"). The defects the
four later rounds reported, and the two structural corrections, are still in this record.

That path has **an answer that needs no finger**: an `IOT_PANEL_BENCH` build types the
passphrase at its own drawn coordinates at boot, then walks the action strip and the
confirmation at their own drawn coordinates too — the strip, the row it was just taken off, the
leave, the forget, the row again, cancel, look around (readings in "Typing along the panel's own
path" and the later runs). What it proves is the state machine and the geometry, while **the
touch's feel and the black ground are still a human's to confirm** (only the FT6336 produces
panel coordinates on this board; touch cannot be synthesised in software). The state layer also
gained a **reachable-state sweep** (35 moves, 8,800 states, four invariants — see "Tapping back
into the row a link was left from, and one dropped list correction"). The loose ends are at the
[bottom](#Still_to_do_([not_done])).

## What the bench said

`[measured, 2026-10-15]` One probe source, built for each part and flashed to each: the
S3 booted with `-a watchdog-reset`, the C6 on a plain reset (why the difference, and the
commands, are in `@/development/iot/flashing.md`). Both joined the network this bench sits next
to (written `<ssid>` below; the name and the passphrase go into a build command line and nowhere
else).

| | S3 (LCKFB SZPI) | C6 (DevKitC-1) |
| --- | --- | --- |
| six scan rounds | 0.65–1.0 s each, 1–6 APs | 0.73–0.80 s each, 1–4 APs |
| **association** | **2184 / 2203 / 2189 ms** (three boots) | **3246 / 3261 ms** (two boots) |
| **DHCP** | **546 / 22 / 44 ms** | **31 / 548 ms** |
| link up | 9428–9789 ms after boot | 10325–10995 ms after boot |
| address | `192.168.1.136` | `192.168.1.158` |
| driver's C heap | **32,204 B free of 65,536 B**, unchanged for 40 s | **14,124 B free of 49,152 B** |
| `phase Connected` | steady | steady |

> `[measured, updated 2026-10-16]` The S3's **65,536 B in that table is that day's budget.**
> From today both parts are at **49,152 B** (the S3's `dram_seg` half went from 32 KiB to
> 16 KiB — see the driver's-heap section below for why). Re-running the S3 probe with
> credentials, through the same scan + association + DHCP:
>
> - driver init plus six scan rounds pass at 48 KiB, with no `OutOfMemory`;
> - association takes **2,228 ms**, then **42 ms** to the address `192.168.1.136` (link up
>   9,337 ms after boot);
> - once connected the driver heap is **15,820 B free of 49,152 B** (32.2%), the same reading
>   38 times over 85 s — the table's 32,204 B on the S3 was a 64 KiB-budget reading;
> - the product image walks the same path: `[WIFI] this build joins the network named …`,
>   `associated`, `phase Connected`, no panic.

Both boards see the same access points (`<ssid>` strongest at −45…−50 dBm, then `ap1`,
`ap5`, `ap6`, `ap7`, `aa:00:00:00:00:01`), so what differs between the two columns
is the part, not the room.

esp-radio's documented heap need for a station is **47–57 KiB** with the default
`ControllerConfig` — the high end is the 6 dynamic RX buffers this repository sets — and
32 KiB makes `esp_wifi_init` fail with `OutOfMemory`. Measured, the driver takes 33,332 B
on the S3 and 35,028 B on the C6: **the C6 needs 1,696 B more**, so one source needs two
budgets. See [the section on that](#One_source,_two_parts).

Access points around the bench (union of 12 rounds and 6 more, strongest per row). **Names and
BSSIDs are placeholders** (`ap1`…, `aa:00:…`); channel, signal and security are measured:

| Name / BSSID | Channel | Strongest | Security |
| --- | --- | --- | --- |
| **`<ssid>`** | 11 | **−50 dBm** | secured |
| `ap2` | 11 | −80 dBm | secured |
| `ap1` | 6 | −81 dBm | secured |
| ap3 | 6 | −82 dBm | secured |
| `aa:00:00:00:00:01` | 1 | −78 dBm | secured |
| `aa:00:00:00:00:02` | 8 | −84 dBm | secured |
| ap4 | 1 | −87 dBm | secured |

A name that is not printable ASCII shows as its BSSID by design: the panel's glyph table
has no other option.

At link time (`[measured]`, from link symbols; the floor is
`ESP_HAL_CONFIG_ENSURE_MAIN_STACK_MINIMUM = 65536` from `.cargo/config.toml`, which
esp-hal's `ld/sections/stack.x` turns into a link-time `ASSERT`):

| | main stack | against the 65,536 B assertion |
| --- | --- | --- |
| S3 product, no Wi-Fi | 77,880 B | +12,344 B |
| S3 product, Wi-Fi, app heap still in `dram_seg` | 63,788 B | **−1,748 B, does not link** |
| S3 product, Wi-Fi, app heap in PSRAM | **125,788 B** | +60,252 B |
| S3 product, Wi-Fi, 48 KiB driver heap (2026-10-16) | **72,056 B** | +6,520 B |
| S3 product + Network page (2026-10-16, evening) | **70,008 B** | +4,472 B |
| S3 product + T9 keyboard and list (2026-10-16, late) | **68,792 B** | +3,256 B |
| S3 `wifi-probe` | 102,872 B | +37,336 B |
| C6 `wifi-probe` | 247,400 B | +181,864 B |
| C6 product `vanling` | 348,124 B | +282,588 B |

The C6's margin is that large for two structural reasons: it has 400 KiB of internal DRAM
(of the S3's 512 KiB, 150 KiB is the camera's frame arena) and this board mounts no panel
and no camera.

The S3 product image is now **852,432 B against a 6 MB partition (13.55%)** and its probe
664,768 B (10.57%) — the product links the driver's IRAM segments now too (only the probe
used to), so the difference between them is back to the product's own code. The C6 does not
need the 6 MB: its product is **650,560 B against 1 MB (62.04%)** — three times its 223,584 B
(21.32%) once the driver's code is linked, so that 1 MB slot is 38% free and the tightest of
the two parts. Wi-Fi's own **statics** are **14,092 B** (measured on the S3; this is not the
IRAM code's mirror in `dram_seg`): `esp_phy::PHY_STATE` 2,008 B, the blob's `s_wifi_nvs`
1,440 B, `esp_rtos`'s scheduler 1,416 B, `g_ic`/`g_pm` 760/584 B, `phy_param` 740 B, the rest
under 512 B each.

`[measured]` **This part describes the state before the product wired Wi-Fi up.** Back then
only the probe called `init_radio_heap()`: the product's `vanling` had no `INTERNAL_MEM` /
`RECLAIMED_MEM`, the linker dropped the regions as dead code, and the driver's statics were
still there — 17 symbols on the S3. Once the product calls `take_wifi()`, the bill below
appears, and the answer to "why did the S3's budget end up at 48 KiB" lives in it too.

`[measured]` **Once Wi-Fi is wired up both regions return, and the bill is 40 KB larger than
the earlier estimate.** With the HEAD `vanling` ELF as the baseline (`_data_start =
0x3fc8d0b0`, `_stack_start_cpu0 = 0x3fce9710`, main stack 125,788 B — the row in the table
above), wiring `init_radio_heap()` in leaves `ld`'s data cursor at `0x3fcdc6f8`: the statics
grew by **72,518 B**, in three parts:

| source | bytes | what it is |
| --- | --- | --- |
| `INTERNAL_MEM` | 32,768 | the driver's block in `dram_seg`, see the driver's-heap section below |
| the blob's IRAM code | 31,980 | `.rwtext.wifi`; the same SRAM answers two addresses, so esp-hal's `.rwdata_dummy` reserves its size out of `dram_seg` **as well** |
| the app's own statics | about 7.7 KB | `network.rs` and what else Wi-Fi newly references (not broken down symbol by symbol) |

**`RECLAIMED_MEM` is not in this bill**: it lands at `0x3fcf0000`, just past the end of
`dram_seg`, so it takes no main stack. The earlier version counted the reclaimed region and
missed the IRAM mirror — the two are coincidentally both about 32 KiB, so the total looked
right while the split was wrong. The main stack is therefore 53,272 B, **12,264 B short** of
the 65,536 B assertion, and the link fails.

The earliest arithmetic here assumed only `dram_seg` returned: 125,788 − 32,768 = 93,020 B,
which looked like it passed. Both parts of that were wrong — the 31,980 B mirror of the IRAM
code was not in the model, nor were `network.rs`'s statics. **The linker cursor is the
authority**, and the failure reproduces in one line: set `INTERNAL_BYTES` back to 32 KiB,
rebuild, and `ld` says the same thing — `stack.x:13 cannot move location counter backwards
(from 3fcdc6f8 to 3fcdb700)`, the identical cursor. That line has to land at the end of
`RWDATA` while the statics are already 2,400 B past it, so the main stack's slot is only
53,272 B — 12,264 B under the floor.

`[measured]` **The capture plane (direction ②) gives back 11,464 B.** Every slot of a
`Channel<T, N>` is allocated at `size_of::<T>()`, so the width of `INTENT_BUS` is set by its
widest `InputEvent` variant — the capture window `Audio(AudioSample)` at 1,760 B priced all
eight slots. With the window on its own single-slot plane:

| | before | after |
| --- | --- | --- |
| `INTENT_BUS` | **14,184 B** (HEAD ELF symbol, `nm -S`) | **896 B** (`size_of::<Intent>()` 104 B × 8 + 64 B of channel state) |
| capture window | on `INTENT_BUS` | `CapturePlane` **1,824 B** |

The shortfall therefore goes from about 23.7 KB to **12,264 B**.

## This generation has no lwIP — only "driver + embassy-net"

`[as-reported, checked two independent ways]` The current esp-rs generation
(esp-hal 1.2 / esp-rtos 0.4) has **no TCP/IP stack crate**: `esp-netif` returns 404 on
crates.io (as do `esp-lwip`, `esp-network`, `esp-tcpip`), the `esp-hal` repository has no
such directory, `CHANGELOG.md` contains zero occurrences of `netif` or `lwip`, and every
network example goes through **embassy-net**.

| Layer | crate | What it does |
| --- | --- | --- |
| Radio | `esp-radio` | Wi-Fi driver (L2); `esp-wifi-sys-esp32s3` carries the prebuilt blobs |
| Protocol | `embassy-net` | smoltcp: DHCPv4 / IPv4 / TCP / UDP / DNS |

`esp-radio`'s `wifi` feature **ships an `embassy_net_driver::Driver` implementation**
(the renamed `embassy-net-driver-02` package), so getting an address is wiring, not
writing a protocol.

**Pinned to `=1.0.0-beta.1`**, the only release whose `esp-hal ~1.2.2` /
`esp-rtos 0.4.0` / `esp-config ~0.8.0` / `esp-sync ~0.3.0` requirements all match this
tree. beta.2's S3 source is **byte-identical** to beta.1's — its only change is an S31
build fix — so a version bump buys nothing today.

## Memory: three decisions (the S3's budget)

This section is the S3's three decisions; the C6's counterparts are in [the section
below](#One_source,_two_parts).

### The driver's heap: 32 KiB reclaimed + 16 KiB `dram_seg`

`[measured]` `esp-radio` is a closed blob that allocates through `malloc`, so the symbols
have to exist in the final image. `esp-alloc` provides them; this module is the memory
behind them.

- **The reclaimed region** (above 0x3FCF0000) is DRAM the second-stage bootloader used
  for its flash cache, which esp-hal's linker leaves to the heap. Its size is 64 KiB minus
  the data cache, so **it is empty at the default 64 KiB cache** (the existing ELF in this
  tree has no symbol above 0x3FCF0000). `scripts/iot-xtensa.sh` halves the cache to buy
  32 KiB back.
- **The `dram_seg` half** (32 KiB at first, cut to **16 KiB** on 2026-10-16) is affordable
  only because the app's own heap moved to PSRAM (next): that took the main stack from
  63,788 B to 125,788 B with 39,809 B in use. **This cut is a different bill**: the blob's
  IRAM code reserves a mirror of its size (31,980 B) in `dram_seg`, and once the product's
  own statics are added the statics run past the end of `RWDATA`, leaving the main stack's slot
  at 53,272 B — **12,264 B** under the assertion — so the link fails — see the driver's-heap
  section under "One source, two parts".

Both are `MemoryCapability::Internal`: the driver's pbuf pools go to the MAC's DMA, and
that path cannot reach PSRAM.

`ESP_HAL_CONFIG_DATA_CACHE_SIZE` **cannot** live in `.cargo/config.toml`: esp-config
treats an option that is not active for the chip being built as an unknown one and fails
outright, and this one is active on the S2 and S3 only, so an `[env]` entry breaks the C6
build. It lives in `iot-xtensa.sh` instead, **keyed off `--target`** (exported
unconditionally it killed `moon run iot:check-c6` inside esp-config).

**The cost**: the panel and camera DMA now run with half the data cache. Whole frames are
written by DMA and the render path's working set is internal DRAM, so the measured paths
do not read it — but that is an expectation, not a measurement, and `[INPUT]`'s 17–25 ms
worst tick plus the AEC figures need re-measuring.

### The app's heap: PSRAM, because there is no third place (S3)

`[measured]` For the product to carry Wi-Fi, its 64 KiB Rust heap has to leave
`dram_seg`, and two places were left: the camera's 150 KiB arena (a page that already
ships, **needs a human's agreement**) or the board's 8 MB octal PSRAM. `chip_init_at`
maps the PSRAM on the way past (`Psram::new(PSRAM::steal(), PsramConfig::default())` —
that peripheral is one field of `Peripherals`, so taking it out is a partial move and the
function could not hand the whole struct back; `steal()` yields the same handle), and the
app's `main` points the heap at the window.

**The mapping has to be proved afterwards**: a pattern stepping by seven, written and read
back over 4 KiB at each end. esp-hal only promises that it mapped a window, not that this
build's SPI timing is right — and wrong PSRAM timing does not panic, it corrupts something
ten minutes later. So the three outcomes are reported apart: no window / mapped but the
pattern did not read back / ready, and the product resets on the second. Measured on the
bench: `Ready(len: 8388608)`.

`PsramConfig::default()` **names no core clock**: the S3's SPI0/SPI1 hang off APB and
[`set_cpu_clock`] moves the CPU while leaving APB alone, so the CPU frequency is not what
the SPI block is clocked from — pinning one would divide by a clock that is not the source.

### `wifi_force_pd`: doing the `init()` esp-radio never calls (S3)

`[measured]` esp-radio 1.0.0-beta.1 carries a `pub(crate) fn init()` that un-gates the
modem clocks, powers the WiFi domain up, pulses its reset and takes the RTC wake lock —
and **nothing in that crate calls it**. This part sits at the ROM's
`APB_CTRL.wifi_clk_en = 0xfffce030` with bit 17 of `dig_pwc` (`wifi_force_pd`) set.
`chip-esp::ungate_radio()` now makes those writes (the S3 arms of upstream's `init_clocks`
and `enable_wifi_power_domain`), the probe calls it before `take_wifi()`, and the registers
move: `0xfffce030 → 0xffffdfdf`, `0x20000 → 0`.

**It does not steady the scan** (0/0/1 before, 0/1/1/1/1/1 and 1/1/0/1/0/1 after), so the
radio's power and clock state is not what makes the scan wobble. The writes stay because no
driver should run on a radio whose power state nobody set — not because they fixed
anything.

## One source, two parts

`[measured]` The probe and the product are **the same source**, built per part and flashed
to each. Every difference lives below `iot-chip-esp`, and a caller sees one entry point:

```rust
iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);   // where the heap comes from: not asked
iot_chip_esp::ungate_radio();                // powering the radio: not asked
let station = board.take_wifi();             // which part's radio: not asked
```

`take_wifi()` has one implementation per board module — a board knows what it is by
construction — and the driver's blob is the board feature's choice:
`lckfb-szpi-esp32s3` carries `esp-radio/esp32s3`, `esp32c6-devkitc-1` carries
`esp-radio/esp32c6`. `esp-alloc`'s chip feature moved the same way, from the `radio`
component feature onto the chip features (`esp-alloc?/<chip>`), because only a chip feature
knows which part is being compiled; `scripts/iot-feature-check.sh` now builds that
combination on each part.

### The general heap: PSRAM on the S3, a static on the C6

`[measured]` The C6 **has no PSRAM** — esp-hal's `psram` module is behind `soc_has_psram`
and the C6 is not in it — so "the app's heap goes to PSRAM" holds on the S3 alone. The C6
gets a 64 KiB `dram_seg` static in the chip layer instead, which is the same block of memory
in the same section that the product's entry point used to declare for itself: it moved
because where memory comes from is a fact about the part, not something an entry point
should know. The two arms differ only in where the window is, and the source appears in the
log and nowhere else.

`[measured]` An entry point now says `init_heap` once, and that one call does three things
that used to sit with the caller: finding the window, checking that `bytes` fits what this
part can offer, and — when there is no window — resetting with the reason
("this module has no PSRAM to map" / "PSRAM mapped and did not read back"). The second
reason carries the mapped size, and `init_psram` is the only place that can still see it, so
that log line lives there.

### The driver's heap: two blocks on the S3, one on the C6

`[measured]` **Both parts now total 49,152 B (48 KiB), split differently**: the C6 has one
`dram_seg` block of 48 KiB, the S3 has 32 KiB reclaimed plus **16 KiB** of `dram_seg`. One
line of the board log is those two blocks: `[WIFI] driver heap: 16384 B in dram_seg + 32768 B
above it (dcache-reclaimed), both internal`.

`[measured]` **The S3's `dram_seg` half went from 32 KiB to 16 KiB (2026-10-16)** for the bill
above: the blob's IRAM code reserves a mirror of its size in `dram_seg` (31,980 B), and with
the product's own statics the statics run past the end of `RWDATA`, leaving the main stack's
slot at 53,272 B — **12,264 B** under the floor — so the link **failed**, and putting the
constant back is a one-line repro. What was cut is the `dram_seg` half, not the reclaimed one:
the latter lands at `0x3fcf0000`, outside `dram_seg`, and never took main stack.

Measured after the cut (link symbols for the first row, the board log for the second):

| | value |
| --- | --- |
| main stack, link time | **68,600 B** (70,008 B before the list and keyboard landed; 68,792 B on the T9 version), **+3,064 B** above the floor |
| main stack peak, on the board | **37,281 B** of 68,600 B — **31,319 B** left (54%) |
| driver heap | 49,152 B (16,384 `dram_seg` + 32,768 reclaimed), **15,820 B** free after association + DHCP (measured) |

The page's 2,048 B is read off an `nm -S` difference rather than estimated, and it is two
things: the new `SCAN` static (`Channel<CriticalSectionRawMutex, ApList, 1>`) at **368 B**,
and `POOL` (the executor pool holding `run`'s future) at **+1,616 B** — the latter because
`DeviceState` gained `Option<ApList>` and the future's layout widened with it. The peak is
**37,281 B** this time (32,353 B on the read-only version), and it is set during the boot stretch —
between the first scan and the first drawing of a list — and never raised afterwards (the timestamps
and the corroboration are under "What the board said"), so those 4,928 B hold both the page's
drawing path and that scan itself, and nothing here has separated the two.

Four constraints decide whether 48 KiB is two blocks or one:

- **`#[ram(unstable(dcache_reclaimed))]` cannot compile on the C6.** The procmacro feature is
  opened by `esp-hal/esp32s3` alone (it is one line of the `esp32s3 = [...]` group in
  esp-hal's `Cargo.toml`; esp32s2 does not open it either), so the reclaimed block is the
  S3's alone rather than something that was forgotten on the C6.
- **The reclaimed block takes no main stack, but it is not free**: it is DRAM the second-stage
  bootloader used for its flash cache, which esp-hal's linker leaves to the heap, and the price
  is **halving the data cache** (`scripts/iot-xtensa.sh`) — what is traded away is cache hits.
- **The C6 has no third place.** It has 400 KiB of internal DRAM; with 64 KiB for the app's
  heap and 48 KiB for the driver's, the main stack **measures** 247,400 B — 181,864 B above
  the floor — and the rest is the driver's statics.
- **Both parts are equally tight now**: the C6's driver measures 35,028 B used (28.7% headroom
  of 48 KiB) and the S3's 33,332 B (**32.2%** headroom of 48 KiB; the usage is the one measured
  on 10-15 under the 64 KiB budget, while the budget itself was measured on the board today).
  **Raising `DYNAMIC_RX_BUFFERS` is a bill both parts pay** — the high end of the documented
  47–57 KiB range is exactly the 6 dynamic buffers this build sets. It is on the list at the
  bottom.

**Getting that 32 KiB back means sending the blob's IRAM code to flash, and this repository
cannot do it.** In esp-hal's script the rule would have to sit between `.rotext_dummy`'s
reservation and `rwtext.x`'s ownership rule, and those two live in the **same generated file**
(`ld/esp32s3/esp32s3.x`), which is not ours to edit. The attempt that was tried (an earlier
script claiming those sections before `linkall.x`) links but produces a broken image: the
reservation always lands before `.text`, so code sent to flash can only land at the IROM
origin, `0x42000020` — flash offset `0x20` — on top of `.rodata`'s first page; in the ELF the
segment's `paddr` is its vaddr, so two segments end up writing the same flash. The right route
is a hook esp-hal can expose (upstream), not this repository assembling its own script.

### `ungate_radio` is an empty function on the C6, and that was checked

`[measured, checked call by call against esp-radio 1.0.0-beta.1]` The S3's version copies
register writes from `clocks_ll/esp32s3.rs` and `common_adapter.rs`. **On the C6 all three
of esp-radio's calls are empty**:

| esp-radio's call | on the C6 |
| --- | --- |
| `init_clocks()` | empty, commented `// done in esp-hal` |
| `reset_wifi_mac()` | empty |
| `enable_wifi_power_domain()` | the whole body is `#[cfg(not(any(soc_has_pmu, esp32c2)))]`, and the C6 has `soc_has_pmu` and no `APB_CTRL` |
| the clock bits that are left | the driver writes them itself: `os_adapter/mod.rs` calls `enable_wifi(true)` as it starts, into `MODEM_SYSCON.clk_conf1` and `MODEM_LPCON.clk_conf` |

So the C6 arm is an empty body and this table — not "not written yet", but written-and-cfg'd-
out. The S3's arm stays because it really does move registers there
(`0xfffce030 → 0xffffdfdf`, `dig_pwc 0x20000 → 0`), and because the radio scans without it
anyway ([next section](#Scanning:_one_round_is_not_a_measurement,_and_neither_is_two)).

## Scanning: one round is not a measurement, and neither is two

`[measured]` One image, three boots, and the first scan returned 0 / 0 / 1 access points.
Six rounds in a row, two boots:

```
boot A:  0, 1, 1, 2, 2, 1
boot B:  1, 1, 1, 2, 0, 0
```

**The only conclusion that survives: one scan is not a measurement.** The count moves
between 0 and 2 within a single boot. The strongest AP nearby sits at a steady −50 dBm, so
this is not a signal-margin problem. **It was briefly written as "the first scan is always
empty" and boot B contradicted it outright** — one AP on the first round, nothing on
rounds 5 and 6.

**Two factors, separated:**

| | per-channel dwell | one round | over 12 rounds |
| --- | --- | --- | --- |
| driver default | 10–20 ms | 135–160 ms | 1–2 APs |
| `CHANNEL_DWELL` 50–120 ms | 50–120 ms | 0.65–1.0 s | 1–6 APs |
| same, **camera not started** | 50–120 ms | 0.73–0.88 s | **none empty** |
| same, **camera streaming** | 50–120 ms | 0.65–1.0 s | **1 round empty** |

**The dwell decides how complete the list is** (1–2 → 1–6), at the cost of a user's wait
going from 0.15 s to 0.8 s. **The camera decides whether a whole round comes back empty**,
with no mechanism yet: it is a continuously running DVP DMA on the same core as the WiFi
driver task, so bus or interrupt contention is the obvious suspect. That does not block
delivery — a page that keeps scanning while the list is empty is fine, which is what the
probe does (`SCAN_ROUNDS = 6`, 500 ms apart).

### The rhythm of a round

The product **asks the radio only while the scan list is really on the panel**
(`DisplayPage::Network` — see "When the page scans"). While it is scanning, the rhythm lives in
`iot-core::drivers::network` (`SURVEY_PAUSE_MS = 5_000`, `SURVEY_EMPTY_PAUSE_MS = 1_000`,
`SURVEY_EMPTY_RETRIES = 3`, and the idle backoff's `SURVEY_IDLE_PAUSE_MS = 15_000` /
`SURVEY_IDLE_MAX_PAUSE_MS = 30_000` / `SURVEY_IDLE_ROUNDS = 3`, decided by
`survey_pause_after(empty_rounds, idle_rounds)`) because, like `MAX_LISTED_APS`, it is a policy
about how the radio is asked rather than an implementation detail of the loop — and a path only
the device can run has to be pinned by a test.

Three rules: **a round that found something rests the whole 5 s**; **an empty round is retried
after 1 s, but three empties in a row are the end of that** (the fourth takes the 5 s rest); and
**three rounds nobody acted on push the rest out to 15 s, then three more to 30 s** (`idle_rounds`
is cleared by any tap, any slide of the list, or the page appearing). The second exists because a
place with nothing in range — a device in its factory state, an access point that is off —
answers every round the same way, and asking every second is the radio scanning for most of
every second. The third because a list nobody is looking at, rescanned every 5.8 s, spends the
radio and most of the main stack drawing nothing new.

`[test]` Three tests pin it (`a_survey_that_found_something_rests_the_whole_pause`,
`an_empty_place_stops_being_retried_every_second`,
`a_room_nobody_acts_on_is_read_less_and_less_often`). The first two were written because the
board caught the mistake first: with the condition written as
`*empty_rounds <= SURVEY_EMPTY_RETRIES`, a **non-empty** round (count zero) also fell into the
fast-retry arm, and the log immediately showed a round every 1.8 s instead of 5.8 s. Reflashed
after the fix, the interval is back at 5.8 s and the empty round's retry landed 1.76 s later.

### Why the list does not jump

`[measured]` In 281 lines of the product image's log, one and the same list changed its row order
between neighbouring rounds: rows a round came in at 0 rows 17 times, 1 row 53, 2 rows 39, 3 rows
31 and 4–6 rows 4 times, while channel 11 read σ≈3.1 dB (−81..−51), channel 1 2.4 and channel 6
2.1. And `rank_aps` **already orders by signal, strongest first** — so the jumping was not a wrong
order, it was **reordering on one reading, every round**.

The policy therefore lives in core (`SurveyHistory` in `core/src/drivers/network.rs`), alongside
`<n>-<m>/8` as a statement about how the radio is read rather than an implementation detail of the
loop:

| Rule | Value | Why |
| --- | --- | --- |
| Hysteresis | a row has to beat another by **4 dB** to move (`LIST_HYSTERESIS_DB`) | `bars_for` is 10 dB a bar, and a swap inside half a bar is invisible on the page |
| Missed rounds | a row a round did not see survives **2 rounds** (`LIST_MISSED_ROUNDS`) | a single scan is not a measurement (above); two in a row missing is when it has really gone |
| Publish only on a drawn change | `observe()` hands the plane a list only when it returns `Some(list)`, otherwise the log reads `[WIFI] the list is unchanged, so the page is not redrawn` | beyond the page's own repaint threshold (`DIAG_REPAINT_STEP_MS = 40` plus changed content), one more saving: identical readings must not redraw |
| A full list drops its weakest | at `MAX_LISTED_APS = 8`, only a new row 4 dB above the weakest gets in | one new row pushing one out is the same event as a row's order changing |
| A new network starts over | `setup` calls `history.clear()` before every association | the previous list has nothing to do with this link |

`[test]` Five tests pin it (no move inside a band / a move past 4 dB, a row surviving a missed
round, `None` when the page would be identical, a full list dropping its weakest, a change counted
after `clear`).

### When the page scans

`PagePlane` (`PAGE` in `app/src/render.rs`) is the **only shared reading** of the page: the render
loop publishes `manager.state().page` once a pass, and the network task waits on
`PAGE.wait_for_list()` for "the list is on the panel", waking on `select3` between "the scan
interval is up", "the page has come **back** to the list" and "a staged credential is ready".

Shared state rather than one more message: a page is state (idempotent, with only the latest
value meaning anything), and a channel would need a one-deep queue, change detection on the
writer side and a retry for a refused send — three mechanisms that exist only to work around a
state being sent as a message. It has the same shape as `EffectiveDemand`.

The gate is in the **task** rather than in the scan itself because the scan is a loop with no
"page" input; the layer that can see both the page and the radio is the one that connects them.
Coming back to the page, `wait_for_list()` reports that it waited, so the backoff counters are
cleared and the page is scanned again at once.

`[measured]` **"Back on the list" has to be an edge, not a level** — this is the second time the
board overturned this one spot. The first version put `wait_for_list()` in the third arm of the
scan-interval select, and the list was **already up** at that point: a level check satisfies its
first poll, so the entire 5 s rest was **skipped**. The reading before the fix, over 120 s:
**152 rounds**, adjacent rounds **790 ms** apart (the `survey found` timestamps) — the backoff never
once took effect, which is to say the radio scanned continuously while nothing on the panel moved.

The fix is `wait_for_return()` on `PagePlane`: read the page now, then wait for it to **change** to
the list's, so "left and came back" stops being the same thing as "never left". It is an **edge**;
`wait_for_list()` is a **level**, and both are in use: the edge is the rest's circuit breaker, the
level is how the loop asks "should I be scanning at all". `[test]` One test pins the difference
(`a_rest_on_the_list_is_not_cut_short_by_the_list_being_up`: on one plane, the level is `Ready` while
the list is up and the edge is `Pending`; only away-then-back is `Ready`).

## "Connected" means associated **and** addressed, and somebody has to say so

`[as-reported, from the reference firmware]` In xiaozhi-esp32's `WifiStation` the handler
for `WIFI_EVENT_STA_CONNECTED` is **empty**; the connected bit is set from
`IP_EVENT_STA_GOT_IP`. This repository uses the same rule: a link with no address is one
the product cannot do anything with.

`[measured]` Only the credentialed run exposed it: associated, addressed, and forty
seconds of reports all saying `phase Connecting` — **nothing in the tree ever assigned
`Connected`**. The address belongs to embassy-net's stack and the `Station` trait had only
`status()`, so whoever holds the address had no way to say so. A product page built on
this interface would say "connecting" forever while passing traffic.

The rule therefore lives in core as a testable value rather than a comment:
`LinkStatus::with_address(Option<Ipv4>)` (an address makes it `Connected`, losing the
address goes back to `Connecting`, a `Failed` is not resurrected — three host tests pin
it) plus `Station::note_address`. The call site is **once per report window**, not only the
first: only that loop can see a lease lapse. After the change: `phase Connected`, steady
for forty seconds.

## Three connection settings copied from the reference

`[as-reported, from xiaozhi-esp32]` This board is an ESP32-S3-WROOM-1-N16R8, the same
module that firmware targets (note: `wifi_station.cc` is **not** at that path in the current
checkout — locate it again before citing it):

| Setting | Why the reference firmware set it that way | Value here |
| --- | --- | --- |
| `scan_method` | The default fast scan takes the first match, so **a strong AP that fails authentication once is silently replaced by a weak one** — measured there as connecting to −79 dBm while a −73 dBm AP of the same name was in range | `AllChannels` |
| `failure_retry_cnt` | Same cause: retry the strongest before falling back | 3 (default 1) |
| `listen_interval` | The AP may skip us for that many beacon intervals; power bought with latency | 10 beacons (default 3) |

`[checked against the ESP-IDF Wi-Fi driver guide]` Two more things that bear directly on this
page:

- **A scan cannot run while a connection is in progress**: `esp_wifi_scan_start()` fails
  immediately (the connection has the higher priority), and the guide's advice is to wait and
  retry. This repository's shape satisfies that by construction: scanning and connecting are the
  same task, and a credential that arrives leaves the survey loop for `hold`/`join` (`setup`
  and `hold` are two functions) — so no round is ever cut off by an association. It is also why
  no such failure appears in any log.
- **How complete a round is, is decided by the dwell time**: the default min/max dwell is 120 ms,
  and the guide says outright that these two values can be tuned to improve scanning — which
  agrees with the measurements here (the driver's 10–20 ms saw 1–2 APs while `CHANNEL_DWELL` at
  50–120 ms saw 1–6), see "one round is not a measurement" above.

On the authentication side there is exactly one **product decision**: a passphrase means
`Wpa2Personal`, no passphrase means `Open`. The second half used to be written wrongly — an empty
passphrase was handed to `Wpa2Personal` too, and a WPA2 config with no password is a request the
access point **refuses** rather than one it ignores, so "tap an `OPEN` row → hand the credential
over with no keyboard" could not work at the bsp layer at all (the panel and the state machine
were right). Reading the driver's own enumeration is what found it, and one line is the fix.
`[not measured]` There is no open network on the bench: this arm has the state-machine tests and
that reading of the code behind it, and **no board reading**.

**WPA3-only and WPA(1)-only networks are out of scope**: `esp-radio 1.0.0-beta.1`'s
`AuthenticationMethodConfig` holds only `Open` / `Wep` / `Wpa` / `Wpa2Personal` /
`WpaWpa2Personal`, with no WPA3 variant, so a WPA3-only AP cannot be joined; a WPA2/WPA3 mixed
network works as usual (it negotiates down to WPA2).

## The partition table

`[measured]` `espflash` writes a 1 MB factory partition for the esp32s3 by default and this
firmware was already 630 KB before the Wi-Fi blobs.
`apps/iot/partitions/lckfb-szpi-esp32s3.csv` gives factory **6 MB** and carries a 4 KiB
`phy_init`.

`phy_init` was **deliberately left out at first**: esp-radio's Xtensa build implements the
driver's calibration-restore hook as `todo!()` (`ld/esp32s3_provides.x` binds
`misc_nvs_restore` to it, and the blob references it undefined), and the driver reaches for
that path once it can see a calibration partition. `[measured]` **it does not reach it** —
with the partition present `esp_wifi_init` does not panic and the scan works. The partition
stays: it is where the driver writes its calibration on the first boot. The C6's table
carries a `phy_init` for the same reason.

The tables are named after the **board**: `scripts/iot-image.sh` has always looked for
`partitions/<board>.csv`, and the layout is a board fact anyway. A 6 MB slot does not fit
inside espflash's assumed 4 MB, so every S3 flash and image task passes `--flash-size 16mb`.
**The C6's table does not move**: 1 MB holds its largest image, the probe, at 597,264 B. It
is still a file — the image task and the flash task then have exactly one place that knows
the layout.

## Where a credential lives (the `creds` partition)

`esp-storage` 0.10 plus a slot format of our own, and not NVS: NVS is another blob with its own
key-value semantics, where what has to be stored here is **a fixed-length list** — a format we hold
ourselves is one we can read a line at a time.

**The partition.** Both boards' tables gain `creds,data,undefined,,0x2000,`, immediately **after**
`factory`, and the three partitions that were already there do not move. `[measured]` checked
offline (`espflash partition-table --to-binary <csv> -o pt.bin`, then `--to-csv pt.bin`): `creds`
lands at `0x610000` on the S3 and `0x110000` on the C6, and `factory` is still at `0x10000`. The two
4 KiB slots (8 KiB) are what **writing the slots in turn** needs: a save writes only the slot the
last one did not, and a read takes the record with the larger `sequence` — power lost halfway
through a write leaves the other slot's older copy, never half a record. The subtype has to be
spelled as a **name** (`undefined` = `DATA_UNDEFINED`, 0x06): in hex, `esp-idf-part` panics on an
`unwrap` (settled without flashing, see still-to-do 5's ②).

**The format.** `core/src/drivers/credentials.rs`. A record is a 12-byte header (magic `VLNC`, a
version, a count, a body length) plus `1 + 32 + 1 + 64 + 2` bytes per network plus a 4-byte CRC,
`RECORD_BYTES = 1016`, with two `const _: () = assert!(...)` pinning that it fits one sector; nine
host tests pin the slot rules, a save interrupted by power loss, and how a bad CRC reads.

**When it is read and written** (both in `Board::new` and in `app/src/network.rs`):

| Moment | Where | Why there |
| --- | --- | --- |
| Reading the table and opening the store | `Board::new`, **before the radio** | reading the table wants a 3 KiB buffer, and that frame is still available before the other parts are up; a table that will not read, or one with no `creds`, logs one line and hands over `None` — **no partition means nothing is kept**, and the device still joins networks |
| Which credential to try | `link`: the build input first, then the **first** entry of the stored list (the one saved most recently), and only then `setup` | a stored credential that is refused is a `GiveBack` rather than a `Retry`: the panel is one tap away, which is this device's other answer; retrying with backoff is what a build that named its only network does |
| Writing | `hold`: **the moment an association succeeds**, once per credential, and **only for a passphrase from the build or from the panel** (`Provenance::Named`) | what is being written is the passphrase, and its answer is in hand right there; waiting for DHCP would put "the link gets an address" between a working passphrase and the next boot's copy of it. The `Refusal::Retry` path does not write twice — `kept` is the latch. **The copy the store itself handed over is not written** (`Provenance::Stored`: the boot read's first entry, and the tapped saved row): that copy *is* the entry on the part, so writing it back only burns a sector, and the boot entry is already the first one; the price is that joining a saved row does **not** move it to the front (so `remember` now only promotes on a passphrase typed into the panel again) |
| Erasing | `Store::forget`: the moment a **stored** row is long-pressed and the confirmation answers "forget", the whole record is rewritten | what is erased is a passphrase, and that is not reversible — joining again means typing it from the start, which is why only this one asks; leaving can be redone, so it costs little |

**The write itself.** The S3 uses `FlashStorage::multicore_auto_park()`, the C6 constructs it
directly (one core). This application **never starts the second core**, so today's default strategy
(refuse while the other core runs) would write just as well; auto-park is chosen so that a future
that does start it does not have to come back and remember this.

**The main stack's bill** (this was the fourth problem the wiring ran into, `[measured]`). A
`Store` value is ~1000 B (`SavedNetworks` 981 plus the sequence and the slot), and
`serve`/`link`/`named_by_build`/`setup` are `async fn`s polled inside one task, so **every frame
that carries one by value is that many bytes of static data in the main stack's own region**
(`__embassy_main::POOL`). Handing it down by value took POOL from 42,144 B to 53,176 B, put the
static data 3,928 B past the end of `RWDATA`, and made `ld` report `stack.x:13 cannot move location
counter backwards` with 53,624 B left for a stack whose floor is 65,536 B. **Boxing it once, in the
board layer** (`Option<Box<CredentialStore>>`, a kilobyte on the PSRAM heap) leaves POOL only 976 B
larger: `[DISPLAY] main stack 65,944 B` (bench image) and `66,168 B` (product image) against the A
phase's 67,704 / 68,600 B — the whole feature costs ~1.8 KB, leaving 408 B. **That headroom is the
wall the next piece of static data runs into**, and what it hits is a compile-time assertion rather
than a runtime stack overflow. **The toolbar-and-mask round that followed** took a little more:
`[measured]` link-time headroom is now 65,880 B (+344) for the product image and 65,560 B (+24) for
the bench — the bench's extra 320 B is its own `log` `Arguments` tables landing in `.data` (which is
why `render_loop`'s `PanelBench` is boxed: 28 B off that future is what got the bench linking again).

**The page-layering round** (`page::Part`/`page::walk` plus `stamp::fill_surface`) measured again:
**+408 B** for the product image that was given a network at build time and **+88 B** for the bench
(`nm … | grep _stack_` inside `docker run … espressif/idf-rust:esp32s3_1.95.0.0`; headroom =
`_stack_start − _stack_end − 65536`); the product image given no network but
`IOT_BOOT_PAGE=network` — the one left on the board at the end of this round — is **+376 B**, and
the 32 B between the two is in the strings compiled in. The round adds **no new `static`**: the two
images' named static data is the same set at the same sizes, byte for byte (308,284 B each, no
difference once `nm --size-sort --print-size` is aligned), and everything new is `.text` (+15,404 B
in the product). The 320 B between them is still the symbol-less stretch described above (the gap
between the start of `.data` and its first named symbol: 9,168 B in the product, 9,480 B in the
bench) — **so the two images' headroom cannot be compared against each other as if it were the
code**; only one image against its own earlier self.

**What the board said** (`[measured]`, 2026-10-16 late, S3, three boots):

| Boot | Image | Log |
| --- | --- | --- |
| 1 | bench (`IOT_PANEL_BENCH=1` with build-time credentials), `creds` erased | `[CREDS] no network saved on this device yet` → the bench types 13 characters on the panel → `[WIFI] associated with "<ssid>" on channel 11` → `[WIFI] this network is saved for the next boot` |
| 2 | the same bench image, `creds` erased again | the same two lines (this is the run behind the table in "Typing along the panel's own path") |
| 3 | **the product image** (no build-time credential at all) | `[CREDS] 1 saved network(s) on this device` → `[WIFI] this device saved a network; joining "<ssid>"` → `[WIFI] associated with "<ssid>"` → `[WIFI] this network was already saved, so the saved copy was updated` → `[WIFI] phase Connected, address Some(...)` |
| 4 | the same bench image, `creds` erased | typing and association → `[WIFI] this network is saved for the next boot` (the `*` appears) → the toolbar's `LEAVE` plus a confirmation → a long press on that row plus a confirmation → `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → `[BENCH] the saved mark is gone from the row the finger held` (the `*` is gone), see "The toolbar, the confirmation and forgetting" |

Boot 3 is this feature's **acceptance**: with no finger and no build input, the device reads the
passphrase typed on the panel last time out of flash and joins on its own. Boots 1 and 2 are what
proves the writing: **"nothing stored" to "stored" really happened on this part**
(`multicore_auto_park` does write here). Boot 4 is what proves the erasing: after the forget the
record really is gone from the part (`the next boot will not join it`), and in the same run the row's
`*` disappears on the spot.

`[not measured]`: the two warning arms — a read that fails (`Opened::Unreadable`) and a write that
fails (`the part would not keep the network`) — have no way to be built on this bench and are code
reading only.

## Features and where each layer sits

`wifi` is a **component feature (module axis)**: one file, one module in `iot-bsp-esp`,
gated once in `components/mod.rs`, aggregated into **both** board features (both boards have
a radio soldered on). `iot-core` has no cfgs — the capability arrives through the composition
point (`HasWifi`). `components/wifi.rs` is itself chip-agnostic: it takes only
`peripherals::WIFI` and the driver's types, and it now compiles for each part.

`Station`'s methods return `impl Future` (RPITIT) and deliberately **do not** carry
`+ Send`: the product joins those futures with input/render/control/feed on one task stack
with no spawn. (`Speaker` does need `Send` because its feed runs on a separate
higher-priority executor.)

`take_wifi()` cannot happen inside `Board::new`: **the driver's start needs the scheduler
running**, and `esp_rtos::start()` belongs to the app's entry point, so the board hands over
a **peripheral**, not a station.

`iot-chip-esp`'s two chip features each forward five chip features of their own
(`esp-hal`, `esp-rtos`, `esp-println`, `esp-backtrace`, `esp-bootloader-esp-idf`), with the
console as a separate axis (`console-auto` / `console-uart`) the app forwards to. The
reason: those three crates' build scripts each demand *exactly one* chip feature and a
console, all of which an app used to supply, so any build **without an app in it** died
inside someone else's build script — which is what `iot-feature-check.sh` hit the first time
it reached the `wifi` entry. That is `features.md` rule 5: **name every part you read.**

## The product page: a scan list and a 4×4 keyboard

`DisplayPage::Network` lands in two slices. **The first (read-only) is in**: the link across
the top (a phase word, the access point in use with its signal bars, the address) and the scan
list below it. **The second slice is in as well** (input and state machine,
panel drawing, a list that slides a row at a time, hysteresis and caching for the list, the
gesture cut, the selected row reversed, the passphrase revealed on request, open networks,
failure classes, the credential kept on the part, the action strip, the stored `*` mark and
forget-by-long-press), see "Where a credential lives" and the table below.

The page has four mutually exclusive states, decided by `NetworkSetup.keyboard`, `LinkStatus.phase`
and by whether a confirmation is up (the draft stays in `entry` across all of them):

| State | What is drawn | Gestures |
| --- | --- | --- |
| list (keyboard down) | a three-row header — `NETWORK`, `ST <phase>`, `IP <addr>` (the window `n-m/8` in the third column) — and below it **3 rows × 47 px** of list (name plus bars; an open network carries `OPEN` at its right edge; **a stored network carries `*` after its name**; **the selected row is reversed edge to edge**), then **an action strip** (`LEAVE` when there is a link, `SCAN` otherwise) | tapping a row picks it; **a row this device has saved (the one drawn with `*`) raises no keyboard** — it hands that row's **name** to the network task, which joins it with the copy the store keeps (and the keyboard comes back only when the store has no copy of that name, on a report from the radio); an **open network** raises nothing either, an empty passphrase is handed straight over; every other row raises the keyboard. Swipe up/down **slides the list one row**; **a long press on a row asks whether to forget it**; **tapping the strip** asks whether to leave when there is a link, and rescans at once (no question) when there is not; switching page is the **hardware triple click** only. **While a link can be left (`Connecting`/`Connected`) a tap on a row is refused outright**: nothing is picked, no keyboard rises and no credential is handed over (the `OPEN` rows too), while the long press and the slide are untouched (**they are not taken either while the joining box is up**: a list that is not drawn cannot be aimed at, see below) |
| typing (keyboard up) | **the same three rows** of header (`NETWORK` + `ST <phase>` / `AP <name>` + bars / `PSK <draft>`, one reading a row) and below it the **4 × 4** keyboard (**80 × 46 px** keys) | keys only: a character key tapped again cycles its candidates, `123`/`#&?` switch layer, Shift, backspace, OK, X, SHOW (reveal); every other intent is `Invalid` |
| confirmation (over the list) | **neither the list nor the strip is drawn** (`page::Part::hidden`); only a 256 × 118 px modal sits over the list: the title `LEAVE "<name>"?` / `FORGET "<name>"?`, the body `THE LINK GOES DOWN` / `THE SAVED KEY IS ERASED`, and below it two answers, `KEEP` and the verb (`LEAVE`/`FORGET`, 100 × 44 px each) | the two answers only; **every other gesture and tap is a cancel** (one `answer_at` decides) |
| joining (over the list, strip kept) | **the list is not drawn, the strip is** (`page::Part::hidden`): the same 256 × 118 px box, **with no answer in it**, titled `JOINING <name>` / `JOINED <name>` over the body `WAIT FOR THE NETWORK` / `WAIT FOR AN ADDRESS` | the strip only: `LEAVE` is still the one way out. **When a question is up at the same time the question is what the box holds** (`Overlay` is one rectangle, and the question wins); it is not drawn while a keyboard is up either — what the finger is doing then is typing |

The key layout is a table in `core/src/keyboard.rs` (`CELLS`), not arithmetic — the digits do
not sit in cell order:

```
1  2  3  | SH        ← the right column is four separate controls, not a merged one
4  5  6  | 123
7  8  9  | <-
X  0  OK | SHOW
```

`1 2 3` over `4 5 6` over `7 8 9` is the block a thumb knows, and it is worth the two cells the
block leaves empty at the panel's right edge; the `0` keeps the place the phone keypad put it
(between Cancel and OK). Those four right-column keys are the **same cells on all three layers**
(only the labels change), because a finger remembers cells rather than words.

The geometry writes no magic numbers: it all falls out of the footer in core's `overlay.rs`.
`FOOTER_ROWS = 8` (the global FPS badge, which no page owns) gives
`CONTENT_BOTTOM = PANEL_ROWS - FOOTER_ROWS = 232`, hence
`KEYBOARD_TOP = overlay_row_top(NETWORK_LIST_ROW = 3) = 47`, `KEY_H = (232 − 47)/4 = 46`,
`KEY_W = 320/4 = 80`, `LIST_TOP = overlay_row_top(3) = 47`, `LIST_ROW_H = 47` (the list is still
three rows, taken from 61 down to 47 to give up 42 px), `TOOLBAR_TOP = 47 + 3×47 = 188` and
`TOOLBAR_H = 44` (to `CONTENT_BOTTOM`). **The list state and the typing state start at the same
y**: under three header
rows the list is cut into three and the keyboard into four, so nothing already on the page
moves when the keyboard comes up — a second block simply appears below.
The `const _: () = assert!(...)` in the bsp tie "where a page ends" to that same number (neither
the list's last band, nor the strip's last row, nor the keyboard's last row may cross
`CONTENT_BOTTOM`) — a badge moved by a
row would be overdrawn by whatever control grew into it.

Every decision sits where a test can reach it:

| Decision | Value | Why |
| --- | --- | --- |
| When the page exists | the wiring fact the entry point reports (`iot_app::Link`: `Wired` / `Absent`) | the station is taken by the entry point — starting it needs the scheduler — so `run` cannot look at the radio any more. With a build fact (`cfg!(feature = "wifi")`) the host smoke could never reach this page, and what this page most needs checked is exactly "did the composition wire the capability in". `Link` has the same shape as `SpeakerRunner`: a wiring fact passed in. Not a new method on `Board`: that would be two ways to say one thing, and they can disagree |
| Where the scan list travels | its own single-slot plane (`ScanPlane` / `SCAN`), not the intent ring | `Intent` is `Copy` and every slot is priced by the widest message: an `ApList` (8 × 42 + 1 = 337 B) on the ring would be 8 slots ≈ 2.7 KB, against a ring whose window is `size_of::<AudioSample>()` (1,760 B). The size test in `app/src/input.rs` (`a_capture_window_costs_one_slot_beside_the_ring`) stops it outright |
| "Never scanned" against "scanned, found nothing" | `DeviceState.aps: Option<ApList>` | the two are different sentences on the page (`NOT SCANNED` / `NO NETWORKS`). A build holding its own credentials does not scan, so it can only say it never looked; a scan that found nothing is a statement about the room |
| How tall a list row is | 3 rows × 47 px (6.0 mm), the header still three rows, and a 44 px strip below the list | the panel is 320×240 = 40.6×30.5 mm (0.127 mm/px). 61 px (7.75 mm) met the usual minimum, but the strip has to cut 44 px out of those 232, and 3 rows × 47 px is what is left over. 47 px = 6.0 mm is under the 7 mm minimum touch target — the same **break that is known and accepted** as the keyboard's 46 px (both the digit block and the list are hit blind by a thumb) |
| Where the keyboard starts | at the **same y as the list** (row 3 = 47 px), with the header keeping its three rows | the typing state used to squeeze its header to one row to push the keyboard up; on the board those three readings (phase, network and signal, draft) turned out to be worth keeping, and the four-row keyboard can give up 6 px and still be pressable. Starting both states at one y also removes a jump between them |
| 4 × 4 rather than a 5 × 3 T9 | a 3 × 3 digit block plus four controls in the right column, keys **80 × 46 px** (10.2 × 5.8 mm) | a 5-column T9 gives each key only 64 px (8.1 mm) of width where four columns give 80. **The 46 px height is 5.8 mm, under the 7 mm minimum** — a break that is known and accepted: the digit block is hit blind by a thumb, and reaching 7 mm over four rows would need 28 px a row more than the panel has. The old T9's 64 × 66 px was 8.1 × 8.4 mm, but it had three rows only and `#&?` took a digit's place in the right column |
| The candidate table | the phone keypad's own: `1`→`.?!,`, `2`→`abc` … `9`→`wxyz`, `0`→space; the layer key cycles `abc` → `123` → `#&?` | a passphrase is mostly a word plus a couple of digits and a symbol, which those three layers cover, and the keypad is a layout a thumb already knows. `MAX_CANDIDATES = 4` is pinned by a compile-time `assert`: a key that grew a fifth candidate does not build. **The current key's candidates are drawn on the key**, the current one reversed (`Mark::Candidate`) — "which candidate am I on" has to be visible |
| How the passphrase is shown | the mask is all stars in a **tail window** of at most 30 glyphs (a leading `+` marks what it cut off); while revealed the characters themselves are drawn (a non-printable one as `?`); the `SHOW` key doubles as `HIDE` (state in `NetworkSetup.revealed`) | the panel is a monochrome reversal (`stamp_marked` fills a small rectangle, then stamps the text) with no second emphasis colour. A tail window rather than a head one: the character just typed is at the end, and a passphrase character to fix is near the end about as often as anywhere. `SHOW` and `HIDE` are one key because it is a **latch** — a key saying `SHOW` while the passphrase is on the panel would be a key whose word is not what it does. Thirty is what the canvas affords: the draft owns a whole row now, nearly twice the sixteen glyphs that fitted in three columns |
| Multi-tap | the first tap lands immediately; the same key, with more than one candidate, tapped again inside `MULTITAP_WINDOW_MS = 800` replaces the character just typed with the next candidate; another key, a backspace, a layer change, Shift or an expired window starts a new character; tapping a one-candidate key again is a new character | the first tap landing immediately is what removes any delay from typing; "one candidate tapped again is a new character" is what makes a passphrase like `00` typeable |
| Shift | one tap capitalises one letter (only letters consume it), a second tap inside the window locks it, a tap while locked releases it; a locked Shift reverses the whole key face. **Shift belongs to the whole multi-tap run, not to the first push** | the phone keyboard's own convention; `Connect`/`Cancel` end the multi-tap run. Per run and not per first tap because `Z` is the fourth tap of `9` and `S` the fourth of `7`: with Shift spent on the first push, a letter three or four taps in could never be capitalised |
| An open network | the row carries `OPEN` at its right edge (the name ends at x=222, the marker is right-aligned to 274, both pinned by compile-time asserts), and tapping the row **raises no keyboard** — an empty passphrase is handed over directly | there is no passphrase to type, and a keyboard would only suggest there is; `OPEN` and the full signal bars both sit at the row's right edge, and the two strings do not meet |
| A stored network carries `*` | one `*` after the name (right-aligned to 229, left of the `OPEN` word; the name slot ends at 222), sourced from **a one-bit-per-row mask carried by the list itself**, `ApList.saved: u8` | one character after the name saves room over squeezing another word into the row's right edge, and it does not meet `OPEN` (a compile-time assert pins all three apart). The mask travels with the `ApList` on the `SCAN` plane, and that plane therefore has to be a slot that **overwrites**: a dropped `SCAN.publish` leaves one row carrying a wrong mark, and **the next round does not heal it** (the history has already taken the correction as the list the page is showing, so the round after compares and says "the list is unchanged") — see "Tapping back into the row a link was left from, and one dropped list correction". That is what buys not keeping a second ~980 B stored-set in the app (the S3's static-memory wall, see "Where a credential lives") |
| Joining a saved row with the store's copy | `pick_row` reading "`secured` and that row's mark is saved" puts `JoinRequest::Held(name)` into `DeviceState.join` (no keyboard, the draft and the reveal cleared, not one passphrase byte through the page); the render loop takes it and publishes it on the `Join` plane, and the network task looks the name up with `SavedNetworks::copy_of(name)`: found, it joins with that copy (**writing nothing**); not found, it reports `BusinessIntent::NoCopy(name)` and the page raises the keyboard | the store already holds that passphrase, so asking for it again is asking a person to type in a secret the device already has. **The name rather than the passphrase**: the store has exactly one owner, the network task (`SavedNetworks` never enters `DeviceState`, the S3's static-memory wall — see the row above), so all the page can say is *which* network and never what its passphrase is; when the store has no copy, the only way back is to report that, and the page is the one place a passphrase can be asked for. The mark decides only **whether the store is asked first or the keyboard goes up first** — the answer itself comes from the store (the mark is the previous round's answer): a stale mark costs a person one retyping, never a wrong network. This row is where the store's two ownerships part: the store owns passphrases, the page owns "which network" |
| The selected row | the whole band is reversed a row at a time (`stamp_band_fill`), and the name, the marker and the bars are drawn on top of it | the panel only reverses black and white. Reversing the whole band rather than the name alone, because the row also holds the bars and possibly `OPEN`: reversing part of it would read as that part being what was picked |
| Where a leave starts | the `LEAVE` button on the strip (drawn only when the link **can be left**), which opens a confirmation and sets a `DeviceState.leave` latch only when the verb is answered | **deliberately moved away from "tap the connected row"**: that row was both "this network" and "leave it", and a mistap had no way back; the strip is the one entry and holds one action, and it asks once before acting. Whether a link can be left lives in `LinkStatus::is_leavable()` (`Connecting`/`Connected` can; `Off`/`Scanning`/`Failed` can only rescan), and the strip draws one of the two by it; **it looks at the phase alone**: `ap` is only set once the association succeeded, so making it a condition would turn the answer into "has the association been reported yet". The same predicate decides which word the strip draws, whether a row's tap is taken, and whether there is anything for `LEAVE` to ask about. The render loop takes the latch and publishes it on `LeavePlane`; the network task hears it in its window `select`, disconnects and reports `Off`. Never rejoining by itself in this session is the link's `auto` switch (still-to-do 5) |
| A list is read-only while there is a link | `pick_row` gates at its top: `link.is_leavable()` returns straight away (nothing picked, no keyboard, no credential handed over — the `OPEN` rows too), while the long press and the slide do not go through that gate | a tap on a row while a link is in flight has two meanings to offer — "switch to another network" and "take this one down first" — and taking the link down has already been narrowed to the strip's one entry. The reading is the only meaning the row has left, so it has no second one to give. A draft keyboard raised before the association is not affected (only `Connected` clears it, `Connecting` does not) |
| The confirmation covers the list | core's `page::Part` (`Header`/`Content`/`Controls`/`Overlay`, order = drawing order) declares what is drawn and in what order, and `page::walk` hands out only what is showing; while a question is up both `Content` and `Controls` are `hidden`; the surface is **written** solid by `stamp::fill_surface` rather than the page underneath being reversed | the panel has one kind of paper — the ground turned over — so a "reverse fill" turns whatever was already drawn (which is paper-coloured to begin with) into the ground colour: the list's own glyphs look printed on the modal, so what was missing was solidity rather than z-order. And the band of the row the question is about is the same paper as the question, so a band sticking out either side of the box would read as part of the box; the list is therefore not drawn at all. `Part::hidden` is the hit-testing rule too (`intent` resolves the question first, then reads `Part::showing` for the list) |
| The strip is not drawn while a finger types | `page::Part::hidden` gives `Controls` one more term, `\|\| setup.keyboard`, and the same predicates reach `intent` through `Part::showing` | the strip's box (y 188–232) and the keyboard's **last row** of keys (y 185–231) are the same band of the panel: with both up, `SCAN`'s three glyphs land on the row holding the `OK` that ends an entry (the label row is 206), and `intent` sends every point to the keys at that moment — so the strip was dead pixels. **This was not a z-order problem, it was drawing something that should not be drawn**, and its root cause is "drawn" and "hit" being said twice; the hit rule now reads the same `page` answer (one test in `page.rs` pins the overlap itself — once the two bands are apart, this rule has to be read again) |
| An attempt in flight has to be visible | `Station::connect` splits into a synchronous `begin_connect()` (sets the config and the phase, **including when setting it fails**) and `await_connected()` (waits for the radio's answer), with `hold` publishing `station.status()` between the two; the page draws a **non-modal** joining box and keeps the strip | the whole association used to sit behind `connect().await`: from the `OK` tap to the address arriving, the page's phase was the old one (the strip drawing `SCAN`, `ST` still the previous word) and `is_leavable()` was false, so `pick_row`'s gate was **open through that window** — a tap could start a new draft. The worst measured case is **37.6 s** (a wrong passphrase plus the driver's four-way-handshake timeout). Reporting a failed `set_config` as a phase fixed `NAME`/`DRV` never being drawn as well. The box is **not modal**: what is wanted here is not a confirmation but "it is under way" plus a way out, and the way out is the strip's own `LEAVE` |
| The two sentences in that box | the title `JOINING`/`JOINED` plus the name, the body `WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS` — all four decided by whether `LinkStatus.ap` has a value (the name is the page's `shown_ap`, the same one the header draws) | the two lines are two different waits that end differently (one ends in a link or a refusal, the other in an address or in a link with none), so one word for both would have the panel say the same thing about a radio that has not answered yet and a network with no lease. When the network came from a build input, nobody typed its name on the panel (`ap` is empty) and the box keeps the word alone rather than inventing a name |
| A long press on a row forgets it | `Touch::Hold` → `Intent::RowHold(band)`: only a row that is **stored** opens the `FORGET` confirmation, and the record is erased only when the verb is answered | forgetting is not reversible (the passphrase has to be typed from the start), which is why only it asks; a long press rather than another button, because it holds for few rows and drawing it would take the row's right edge. The name in the confirmation comes from that row's `ApList` entry, and the erase goes through `Store::forget` (the whole record rewritten, the two slots still written in turn) |
| A rescan does not ask | with no link the strip is `SCAN`, and a tap sends a `RESCAN` signal to the survey task at once | scanning is read-only and repeatable, so there is nothing irreversible to confirm; with a link up there is **no `SCAN`**, because `survey` is not reached at all while a link is held (`setup` is awaiting `hold` at that point), so the two actions never hold at once and the strip need not say "and" about them |
| Sliding the list | swipe up/down is `BusinessIntent::ListSlide`, moving `list_top ± 1` and pulling the end back to the last full window | paging left the last page of a short list blank and made the window reading jump by three; a row a slide makes every row reachable and keeps `n-m/8` continuous |
| Why the list does not jump | the ordering policy lives in core (`SurveyHistory`: 4 dB of hysteresis, a row surviving 2 missed rounds, published only when what is drawn changed) | every scan reading wobbles (σ≈3.1 dB on channel 11), so reordering on a single reading is the list jumping. The mechanism and the evidence are in "Why the list does not jump" |
| Failure classes | `NetworkError` splits into `NotFound` (`NOAP`) / `Credentials` (`AUTH`) / `Busy` (`BUSY`) / `InvalidArguments` (`NAME`) / `NoMemory` (`MEM`) / `Driver` (`DRV`); a failure **raises the keyboard again** and draws its word in the typing row's middle slot | the four-glyph words are drawn the same way the phase word is (the four-glyph test on `format_phase`). Only the driver's enumerable reasons that can be argued for are mapped; the rest fall to `Driver` — **the mapping as measured is below** |
| What the page's ground is | `DiagnosticsSink::draws_pages()` (default `false`) is overridden to `true` by `DisplayLight`; `app::render::LightRenderer` gives a black fill, 100% backlight and no breathing when "the surface draws pages && the page is Network" | deriving an appearance from the page in core would be wrong: not every light has a screen (the C6's WS2812 is light instance 0 too), and whether a surface draws pages is something only the surface knows |
| Hit testing and drawing share one source | `keyboard::{key_of, key_at, key_box, list_band_at}` | where a key is drawn and where a tap lands are the same functions, so a geometry change cannot miss the hits |
| An intent reports what the page shows | `BusinessIntent::Row(u8)` is the **band number on the page**, and `Key { key, at_ms }` carries the lift's clock | the state layer should not know which row the list is scrolled to; `at_ms` is the only clock the multi-tap rule needs. `ListSlide` reports no target either: one slide is one row, and whether the end was reached is the state layer's own business |

What gets drawn is decided by `diag_repaint_due`'s Network predicate: any change of phase,
access point, address, list or entry repaints, while the envelope and the touch counters do not
(`[test]` two tests, one per direction).

The arithmetic that turns readings into glyphs all lives in `bsp-esp/src/camera_readout.rs`
(where every page's drawing arithmetic lives, and where the host can test it; this page's
`[test]` ten): the phase word (a failure draws one of six — `NOAP`/`AUTH`/`BUSY`/`NAME`/`MEM`/`DRV`,
where `MEM` is the one reading that means "this build's heap budget is wrong"), the dotted
address (a dash when there is none, never `0.0.0.0`), the bars `#`/`.` (the same `bars_for`
scale the list uses), the name (the BSSID when the font cannot draw it; longer than 24
glyphs keeps its head and is marked with a `~`, because two networks folding at the same
point would otherwise draw the same row), the question's title with its two answers, and the
joining box's title and body (`JOINING`/`JOINED` plus the name,
`WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS`; a name too wide is what folds, the
`JOINING`/`JOINED` word never does).

### The gesture cut: the delay was in the classifier, not the panel

`[test]` Touch is now `Press` / `Tap` (fired on the lift that resolves it) / `Swipe` /
`LongPress` / `Ghost` only: `GestureEvent::DoubleTap` and `TripleTap` are gone from the
vocabulary, and the two counters in `TouchDiagnostics` went with them.

**The delay's root cause was a click being held in the mouth**: the old `TouchGestures` parked
a short lift in a `chamber` (`PendingTap`) and published it only in the `tick` that found the
`DOUBLE_CLICK_WINDOW_MS = 400` window expired — so **every click waited up to 400 ms to be
seen**, and the page reacted 400 ms late with it. With touch multi-tap gone, `Tap` is published
on **the sample whose lift resolved it**. The multi-click window stays where it belongs, on the
hardware button: the triple click is the only way to change page, and that is a real button
with its own window — still 400 ms.

`GestureEvent::Tap` carries `at_ms` (the lift's clock) and reports the **down edge**
(`down_x`/`down_y`) instead of the release coordinates, because the panel is coarse and a
release drifts. The keyboard uses `at_ms` to read a second tap inside 800 ms as "the next
candidate" rather than "another character". `TouchGestures`' queue went from 2 slots to 4 at
the same time (one sample can resolve at most one gesture per contact, two contacts, plus the
run's single phantom pulse), dropping the oldest when full.

### What the board said (`[measured]`, 2026-10-16, late)

Flashed to the S3 (a build with no credentials, `IOT_BOOT_PAGE=network`):
`[IOT] boot page is Network`, so it comes up on this page. Ninety seconds of readings
(reproduction in [`flashing.md`](@/development/iot/flashing.md) under "Boot page and the bench
image": the hand-written `IOT_BOOT_PAGE=network` build plus `espflash flash`, with
`espflash monitor` for the serial log):

| | value |
| --- | --- |
| Network page repaints | **one per scan round**: `[DISPLAY] Network 1 repaints in 15825 ms, held the executor 18 ms (0%)`, worst render 3 ms, worst write 15 ms (the boot window reads `Network 3 repaints in 7333 ms` — arriving at the page plus the first two rounds) |
| scan cadence | **5.8 s ×2 → 15.8 s ×3 → 30.8 s** (measured `survey found` timestamps: `2.046 → 7.802 → 13.624 → 29.449 → 45.268 → 61.088 → 91.909`), an empty round retried after 1 s (see "The rhythm of a round" and "When the page scans") |
| main stack | **68,600 B** at link time (the board reports `[DISPLAY] main stack 68600 B`), peak **37,281 B** (54%), **31,319 B** left |
| image | **885,744 B of 6 MB (14.08%)** |

*When* the peak is set matters as much as how big it is: at `1.222 s` it still read `0 B`, and by
`2.017 s` it was `37,281 B` — so the deepest stretch is the first scan and the first draw of a list,
and nothing after it raised the mark. The bench run bears that out (typing happened at `2.1–2.8 s`,
and the peak it reported afterwards was still the same number, `37,297 B of 68,408 B`): **the
keyboard and typing drawing path is not deeper than the boot one**.

The read-only slice measured **5–8 repaints per 2–2.8 s** on the same bench
(`/tmp/network.log`, 2026-10-16 17:02: `[DISPLAY] Network 5 repaints in 2324 ms, held the
executor 91 ms (3%)`). What changed is this page's own ground: the read-only version drew the
light's fill (its colour walk crossed the repaint threshold every few hundred milliseconds),
where this page is black and static — so it repaints only when the content really changes, and
the content changes once per scan round — whose spacing now really does follow the backoff (see the
790 ms mistake under "When the page scans").

**The touch coordinate frame on this end was wrong all along** (`IDENTITY`: the module reports the
controller's own 240×320 portrait window while the page is a 320×240 landscape one) — which is
exactly why a tap produced a different key's character. The fix and its evidence are in the
[input and touch record](@/records/iot/input.en.md) under "The touch coordinates need one frame
change"; the feel and the whole path below are only worth judging once that change is in.

### Typing along the panel's own path (`[measured]`, 2026-10-16, late)

Exactly **one stretch** of "tap a row → type the passphrase → OK" can be reached through the
glass: the coordinates travelling from the controller into a `GestureEvent`. Everything else is
decided in the page's own coordinates, so a build can be made to **tap itself at the cells it
draws**. An `IOT_PANEL_BENCH` build (`crates/app/src/panel.rs`) makes `credentials_from_build()`
return `None`, so the product comes up on the "wait for the keyboard to name one" arm; then
`PanelBench` publishes **one tap per pass** of the render loop, into the path a finger's taps take
— `intent::translate` and the same `INTENT_BUS` ring.

One tap a pass rather than the whole run at once: the ring holds 8 slots, and a tap is applied only
by the **next** pass's drain, so only the pass after that can decide the next tap against the state
it produced. Multi-tap therefore holds by itself — a tap's `at_ms` comes from the script's own clock
(`Tap::at_ms` from `keyboard::script_for`), not from the wall, so one tap a pass can never fold `Z`
(the fourth tap of `9`) into another character.

How to run it (`IOT_PANEL_BENCH` and `IOT_BOOT_PAGE=network` are set by the task — the latter is
part of the bench rather than a convenience, because the radio is only asked while the list is on
the panel):

    IOT_WIFI_SSID=… IOT_WIFI_PSK=… IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-panel-typing-s3

One boot's readings (S3; not one character of the passphrase reached the log):

| clock | log |
| --- | --- |
| 2.11 s | `[BENCH] replaying a credential through the panel: 0 is row 0 of page 0, 28 taps in all` |
| 2.15 s | `[WIFI] the panel named a network: "<ssid>"` — **what the keyboard typed reached the network task** |
| 2.78 s | `[WIFI] joining the network the panel named: "<ssid>"` |
| 5.34 s | `[WIFI] associated with "<ssid>" on channel 11` |
| 5.38 s | `[BENCH] the panel's own credential is on the air: 13 characters typed, 28 taps, and the radio has an address` |
| 7.38 s | `[WIFI] phase Connected, address Some([192, 168, 1, 136])` |

**28 taps typed 13 characters**, every one of them at a cell the panel itself drew, and the radio
associated on them and took an address — the end-to-end evidence for the whole coordinate and state
machine ("4 × 4 layout", multi-tap candidates, three layers, Shift belonging to the whole run, and
`OK` handing the credential over), with **no finger anywhere near the panel**. The passphrase itself
never reaches the log; the only thing to compare is the **character count** (13 = the length of the
passphrase in the build command line), and the radio accepting it is what says those 13 characters
are the passphrase.

Those 28 taps took **37 ms** of wall time (the bench is faster than any thumb), and this page's
repaint gate is `DIAG_REPAINT_STEP_MS = 40` plus "something this page draws really changed" — so the
repaint counters in that window staying small (`Network 3 repaints in 2255 ms`) is not a stalled
page: a thumb's taps are two hundred-odd milliseconds apart, and each one earns its own frame.

**Then again with a deliberately wrong passphrase** (the same command, only `IOT_WIFI_PSK` changed):

| clock | log |
| --- | --- |
| 3.54 s | `[WIFI] joining the network the panel named: "<ssid>"` |
| 41.10 s | `[WIFI] left "<ssid>": FourWayHandshakeTimeout at -67 dBm` — **the driver's own reason** |
| 41.10 s | `[WIFI] the radio would not associate: Credentials` |
| 41.12 s | `[BENCH] the radio refused what the panel typed: Credentials after 60 taps` |

Two things in the failure classes stop being as-reported and become **measured**: ① a wrong
passphrase lands on `FourWayHandshakeTimeout`, which `kind_of_disconnect` puts in `Credentials`, and
the panel draws `AUTH`; ② handing the credential over to being refused is **37.6 s** — what is being
waited on is the driver's own four-way handshake timeout, and the product (panel and list) runs
through it with `ST` reading `CONNECTING`. There is a lesson for the bench here too: on the first
round, when it had only seen the neighbour `ap1`, it reported that the named row was not in the
list — the second round carried it. That line went from `error` to `warn` and now says "not yet in
the list", because otherwise every healthy boot starts by scolding itself.

The bench's own arithmetic is pinned on the host too (seven tests in `panel.rs`): which page the
target is on, how many slides come first, which row a band's y lands on, **that the last tap is the
`OK` cell**, that an open network is not treated as something to type at, that no run is built
before there is a list, and **that the whole walk goes through in the real order**
(`the_walk_asks_before_it_leaves_forgets_and_looks_around`: a tap on a row under a link is not taken →
the strip asks once → `KEEP` keeps it → asks again and the verb is answered → the row the link was
left from is tapped (**the store's own copy brings the link back, with nothing asked**) → the link is
taken down again → a long press forgets → that same row is tapped once more (which now reads a
**fresh draft**) → the keyboard is cancelled → and a rescan at the end; the order itself is pinned
too, `visited` compared act by act, so a skipped act or two acts swapped goes red). Those are the
parts of the geometry
account most likely to be off by one quietly, and they need no board.

**Not verified yet**, because software cannot do it and a human has to touch the panel once: two
things only — the **feel** of the 4 × 4 keys, the list rows and the strip (80 × 46 px keys, 47 px
rows, a 44 px strip and confirmation answers, which
mean nothing until a finger lands on them), and the black ground with its 100% backlight by eye.
**Whether the confirmation covers the list is no longer one of the two**: it is four host-testable
facts — `page::Part::hidden` does not draw the list or the strip while a question is up,
`page::walk` never hands them out at all, `Part::IN_ORDER` pins the drawing order, and
`stamp::fill_surface` writes every pixel of the rectangle (`core/src/stamp.rs`, three tests, one of
which goes red if the fill is put back to "reverse").
The whole "tap a row → type the passphrase → OK → associated → leave → forget" path is now covered
by the bench above
and no longer needs one. What the host smoke covers is the same state machine and geometry:
in `iot:smoke-host`, `Row(0)` → `Char(b'4')`, `Char(b'4')`, `Char(b'2')`, `Shift`, `Char(b'8')`
types `haT`, the credential reaches the `Join` plane, and `NETWORK_TYPED` (the longest passphrase draft
any snapshot carried) reaches 3.

### The strip, the confirmation and forgetting (`[measured]`, 2026-10-17)

The same `IOT_PANEL_BENCH` build, this time walking the strip and forgetting to the end (`creds`
erased first; the script's `Act` order is: type and associate → tap `LEAVE` → answer `KEEP` → tap
`LEAVE` again → answer `LEAVE` → long-press the stored row → answer `FORGET` → tap `SCAN`). One S3
boot read (neither the passphrase nor the network name reaches the log):

| clock | log |
| --- | --- |
| 4.73 s | `[WIFI] this network is saved for the next boot` — **where the `*` comes from: this row entered the mask** |
| — | `[BENCH] tapping the strip, which asks to leave the link it is on` → `[BENCH] the strip asked about leaving, and nothing is down yet` |
| — | `[BENCH] KEEP left the question undone and the link up` — **after `KEEP` the link is still up** |
| — | `[BENCH] the strip asked again; the action's own answer is next` → `[WIFI] the panel asked to leave this network; the link is down` |
| — | `[BENCH] the link is down and the page is back on the list` |
| — | `[BENCH] holding band 0, which asks to forget that network` → `[BENCH] the hold asked to forget "<ssid>"` → `[WIFI] the panel asked to forget "<ssid>"` |
| — | `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → `[BENCH] the saved mark is gone from the row the finger held` — **the `*` is gone** |
| — | `[BENCH] tapping the strip with no link, which asks to look around` → `[WIFI] the panel asked to look around now` → `[BENCH] the strip's own round came back` |

These four steps are the two ends of **one mask**: when it was written that row was marked
(`this network is saved for the next boot`), and after the erase it is not
(`the saved mark is gone`); the line `holding band 0` can only be emitted when that row **is
stored**, which is the mask really existing on the part. The leave and the rescan go through the
strip's and the confirmation's own coordinates and down the finger's own path (`intent::translate`
plus the same ring). **After this run `creds` is empty**: the forget really erased the part, rather
than only rubbing the `*` off the page.

### A row under a link, and a box that covers the list (`[measured]`, 2026-10-17, second run)

Two defects, each closed in one place:

- **Tapping a list row while a link can be left does nothing.** The gate is at the top of
  `pick_row` (`link.is_leavable()` returns straight away): nothing is picked, no keyboard rises and
  no credential is handed over, the `OPEN` rows included; the long press (forget) and the slide do
  not go through that gate. Taking the link down is therefore left to the strip's one entry.
- **The confirmation really covers the list.** `stamp_fill` used to **reverse every pixel
  underneath** rather than write solid — so the list's white glyphs (paper-coloured to begin with)
  came out ground-coloured, which looks exactly like the list's text printed on the modal. What was
  missing was solidity, not z-order. The surface is now **written** by `fill_surface` in
  `core/src/stamp.rs`, and while a question is up the list and the strip are **not drawn at all**
  (`page::Part::hidden`).

The bench's `Act` order gained a step at the front (type and associate → **tap band 0 while the
link is in flight** → tap `LEAVE` → answer `KEEP` → …). One S3 boot read:

| clock | log |
| --- | --- |
| 4.66 s | `[WIFI] associated with "<ssid>" on channel 11` → `[WIFI] this network is saved for the next boot` |
| 4.72 s | `[BENCH] the panel's own credential is on the air: 13 characters typed, 28 taps, and the radio has an address` |
| 4.82 s | `[BENCH] tapping band 0 under the link, which must pick nothing` |
| 4.83 s | `[BENCH] the row under the link took no pick, and no keyboard came up` — **under a link that row is a reading** |

That line cannot be emitted by an implementation where the tap raises the keyboard (take the gate
out and the host walk test goes red at once: `a row took a pick while a link was up`). What it
reads back is **not** "the page has no selected row" — the typing step picked one, and a link coming
up clears the draft rather than the row the draft was for — so the line compares **before the tap
with after it**: this touch changed nothing.

The modal's half is **structural** and host-testable, four facts apiece: while a question is up
`page::Part::hidden` does not draw the list or the strip, `page::walk` never hands them out at all,
`Part::IN_ORDER` pins the drawing order, and `fill_surface` writes every pixel of the rectangle
(`core/src/stamp.rs`, three tests; put the fill back to "reverse" and that one goes red: at `(6,10)`
`left=4358 ground / right=61177 ink`).
**What is drawn, and in what order**, moved from the panel into core this round:
`page::walk(setup, draw)` hands out only what is showing (tests pin the order it hands them out
in, and that the list and the strip are **not handed out at all** while a question is up), leaving
the panel with nothing but a `match` over what each part looks like — because the panel is the one
place a wrong answer is invisible (the host cannot compile it). **The pixels themselves (how the
modal reads as a whole, the bands, the contrast) are still for an eye.**

This round's product image (no network given, `IOT_BOOT_PAGE=network`) also booted on the board:
`[CREDS] 0 saved network(s)` → `[IOT] boot page is Network` → 2 access points found →
`the list is unchanged, so the page is not redrawn` → and it sits there waiting for the panel. The
same page code runs on the branch without the bench, and its `[DISPLAY] main stack 65,912 B`.

### No strip while a finger types, and a box while the radio joins (`[measured]`, 2026-10-17, third run)

Two freshly reported defects, one in what is drawn and one in the phase — and both root causes are
**one fact being said twice**:

- **The strip was stamped across the keyboard's last row.** The strip's box (y 188–232) and the
  keyboard's fourth row of keys (y 185–231) overlap by 43 of 46 px, and `SCAN`'s three glyphs land
  at y=206 — the row holding the `OK` that ends an entry. `intent` sends every point to the keys
  while `setup.keyboard` is up, so those glyphs were **drawn and unpressable**. The rule moved into
  `page::Part::hidden` (`Controls` gained `|| setup.keyboard`) and the hit side reads the same
  answer through `Part::showing`; one test in `page.rs` pins the overlap itself.
- **The page said nothing while it was joining.** `Station::connect` split into a synchronous
  `begin_connect()` (sets the config and the phase) and `await_connected()`, with `hold` publishing
  the status between them; the page draws a **non-modal** joining box and keeps the strip's `LEAVE`.
  The split also fixed a failed `set_config` never reaching the phase, which is why `NAME`/`DRV`
  were never drawn.

All of the structure is on the host-testable side: `page::walk(setup, link, draw)` (`Content` is
hidden by a question **or by the joining box**; `Overlay` is not drawn when neither is up; when both
are, the question is what the box holds) and `camera_readout::{format_joining_title,
format_joining_body}` (`JOINING`/`JOINED` plus the name, `WAIT FOR THE NETWORK`/`WAIT FOR AN
ADDRESS`, all four decided by whether `LinkStatus.ap` has a value — two new tests).

One boot of the same `IOT_PANEL_BENCH` build (`creds` erased first, 28 taps):

| clock | log |
| --- | --- |
| 2.060 s | `[BENCH] replaying a credential through the panel: 0 is band 0 after 0 slides, 28 taps in all` |
| 2.064 s | `[BENCH] a finger is typing, and the strip is not drawn with the keys` — **the strip is simply not drawn while typing** |
| 2.098 s | `[WIFI] the panel named a network: "<ssid>"` |
| 2.100 s | `[WIFI] joining the network the panel named: "<ssid>"` |
| 2.142 s | `[BENCH] an attempt is in flight: the box is over the list and the strip is still offered` — **the box is on the panel before the radio has answered** |
| 4.664 s | `[WIFI] associated with "<ssid>" on channel 11` — **2.52 s after the box appeared** |
| 4.685 s | `[WIFI] this network is saved for the next boot` |
| 4.739 s | `[DISPLAY] Network 5 repaints in 4216 ms, held the executor 97 ms (2%), worst render 4 ms, worst write 15 ms` |
| 4.797 s | `[BENCH] the panel's own credential is on the air: 13 characters typed, 28 taps, and the radio has an address` |
| 4.851–6.652 s | last round's walk runs as before (a row under the link → the strip asks → `KEEP` → the verb → forget by long press → rescan) |

**The 2.142 s line is this round's reading**: it lands between `joining` and `associated`, which is
**exactly the window the page could not speak in last round** (`ST` still the previous word, the
strip drawing `SCAN`, and `pick_row`'s gate open). In the wrong-passphrase run that same window
measured **37.6 s**, so this line is not "it got 2.5 s faster" — it is "the page is no longer silent
for half a minute".

Neither line was judged by eye: **take the `setup.keyboard` term out of `Controls` and three
`page.rs` tests go red at once** (`the_page_under_a_keyboard_has_no_strip_and_no_box`,
`a_draft_is_the_pages_own_subject_over_an_attempt_too`,
`the_walk_leaves_the_strip_out_and_keeps_the_box_in_while_a_finger_types`); **drop the phase term
from `joining`** (leaving "not typing") and 24 tests across the state layer go red with it; **remove
either `Part::showing` gate** (the long press, the slide) and
`a_hold_on_the_list_under_the_join_box_asks_nothing` /
`a_slide_under_the_join_box_does_not_move_the_list` goes red on its own (all three tried on the
spot and green again once restored — and restore with an **exact-match** replacement rather than a
loose regex: a loose one rolled `joining`'s phase term off along with the mutant once, and these
tests were what caught it).

The box's **geometry** is not new: it reuses the question's `DIALOG_W/H/X/TOP` (256 × 118,
`DIALOG_X = 32`). What differs is that it holds no answer and does not take the strip.
**The static footprint did not move this round**: the bench image reads
`[DISPLAY] main stack 65,624 B` and the product image (no network given) `65,912 B` — the same pair
as last round, because the two new `bool`s fit in `PanelBench`'s existing padding (the assert's floor
is still 65,536 B, leaving 88 B and 376 B). The product image booted this time as
`[CREDS] no network saved on this device yet` → `[IOT] boot page is Network` → 2 access points found
→ waiting for the panel (`[DISPLAY] stack peak 38,992 B of 65,912 B`).

**What still needs an eye or a finger**: the keyboard's fourth row is clean now (`SCAN` is no longer
stamped over it), and what the joining box looks like as a whole, when it appears and when it goes,
plus the feel of the 80 × 46 keys and the 44 px strip — those are pixels and touch, not structure,
`[not done]`.

### Tapping back into the row a link was left from, and one dropped list correction (`[measured]`, 2026-10-17, fourth run)

Two defects: one was reported as a black page, and the other was **swept up** rather than reported —
leaving the second one in means the `*` on a row sometimes stays after the network is forgotten.

- **After leaving a link, tapping that row again leaves the page black below the header.** `pick_row`'s
  "the same row keeps the draft" test looked at `selected` alone, and a link coming up **drops the
  draft without dropping the row** (see the phase rules): once the link is gone the row is still the
  picked one, so a tap on it read as "the same row again" → **no draft was built** — while `keyboard`
  was set → the panel drew the keyboard's area over an empty draft and drew neither keys nor strip,
  **leaving nothing on the page that can be touched** (that is the black page). The test now carries
  `entry.is_some()`: with no draft there is no draft to keep, and "the draft a link dropped does not
  come back" is on purpose (it will not be handed over again), so that row opens a **fresh, empty
  draft**. The half of it a reader cannot see is in the code's own docs: `NetworkSetup::keyboard` and
  `pick_row` both state the invariant `keyboard ⇒ entry.is_some()`.
- **The `*` on a row sometimes stays after a forget.** The `SCAN` plane used to be a capacity-1
  `Channel`, whose `try_send` **throws the value away with an `Err`** when the slot is full — and the
  value thrown away is the **newer** one: a round publishes its list and, right behind it (within the
  same millisecond, because a forget is acted on in that round's own pause), the network task re-marks
  the list against a store that no longer holds the name and publishes it again — with the first one
  still in the slot, the second was dropped. The page kept a saved mark for a network that no longer
  existed, and **no round after put it right**: `SurveyHistory` had taken the correction as "the list
  the page is showing", so the next round compared, said "the list is unchanged", and drew nothing.
  The plane is now a slot that **overwrites** (a `Signal` holding the value plus a wake signal of its
  own; the wake is separate because the render loop **parks on it**, and a park must not take the list
  — the `take()` belongs to the top of the next pass).

**The mechanism meant to catch this class of defect next time landed together with the fix.** The
state layer had scattered tests only; it now has a **reachable-state sweep** —
`state::tests::reachable::every_state_the_page_can_reach_keeps_its_invariants` starts from the booted
state and walks breadth-first over 35 moves "the page really does receive" (each named in
`alphabet()`: a tap and a hold on three rows, the strip, the box's two answers, the button's three
shapes, seven keys, two slides, the radio's seven phases and three scans, and the four the render
loop carries from a latch to the radio). It reaches **8,800 states** in 0.90 s against a
`SWEEP_CAP = 20_000` bound it *asserts* it did not touch — touching it would be a test claiming a
cover it does not have — and checks four invariants on **every state it lands on**:

| Invariant | What the panel is if it breaks |
| --- | --- |
| `keyboard ⇒ entry.is_some()` | a keyboard drawn over an empty draft (the black page reported this round) |
| `entry.is_some() ⇒ selected.is_some()` | a draft with no owner: nothing names the network it would be handed to |
| `confirm == Some(Leave) ⇒ link.is_leavable()` | the page asking to leave a link that is not up |
| `rescan_wait ⇒ !link.is_leavable()` | waiting for a round under a link, whose strip only offers `LEAVE` |

The sweep's key writes out only **the fields a move branches on** (readable bytes rather than a hash:
a hash's collision would silently merge two states); the fields only the drawing reads (the reveal,
the last refusal, the strip's wait) stay out of the key, but **the invariants are checked on every
landing**, so what is lost is one branch of a same-key state rather than cover. The end of the test
holds **reachability assertions** ("the keyboard did come up over a draft", "a row really was left
picked with no draft", …) that guard against the alphabet quietly losing a move — more exact than a
floor on the state count, and untouched by any legitimate cut.

The `IOT_PANEL_BENCH` walk gained **two acts**, between "leave" and "forget": `ReopenTheRowJustLeft`
(tap back into the row just left, reading back "the keyboard is up over a draft of **0 characters**")
and `DropTheKeyboard` (press `CANCEL`, reading back "the page is on the list and the row is not picked
any more"). The old readings would have **passed this state by mistake** (the three-part reading took
"keyboard over an empty draft" for typing), so `read_parts` gained a `draftless_read` floor: a
keyboard over a slot with **no draft** fails.

What the board said is `bench-r7.log`, and the same walk's second run — rebuilt from the source as it
now stands — is `bench-r8.log`; both finished (`creds` erased first, one S3 boot, 28 taps):

| Time | Log |
| --- | --- |
| 5.023 s | `[BENCH] tapping band 0, the row the link was just taken off` |
| 5.027 s | `[BENCH] the row the link was left from opens a fresh entry: the keyboard is up over 0 characters, and the draft the link dropped is not back` — **the row tapped again after the leave: a fresh draft, and the keyboard is up over something** |
| 5.087 s | `[BENCH] the entry's own cancel puts the page back on the list` |
| 5.144 s | `[BENCH] holding band 0, which asks to forget that network` → `[BENCH] the hold asked to forget "<ssid>"` → `[WIFI] the panel asked to forget "<ssid>"` |
| 5.709 s | `[WIFI] survey found 2 access points` → `the list is unchanged, so the page is not redrawn` |
| 5.718 s | `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → **the re-mark and its publish come right behind** |
| 5.768 s | `[BENCH] the saved mark is gone from the row the finger held` — **the correction reached the page** |
| 5.804–6.403 s | `[BENCH] tapping the strip with no link, which asks to look around` → `[WIFI] the panel asked to look around now` → `[BENCH] the strip's own round came back` — **the whole walk finished, with no `[BENCH] … 120 passes` report** |

This run's "one round, one re-mark" did not meet the dropped publish (that round said "the list is
unchanged", so it published once), and neither did `bench-r8.log`, **so neither of these two
reproduces the drop** — the one that did is the previous `bench-r6.log`: 5.901 s
`survey found 3 access points` (that round would
redraw), 5.911 s `is forgotten` (the re-mark's publish was the one dropped), 6.653 s
`survey found 2 access points` + **`the list is unchanged`** (the history's list had no mark; the
page's did), 7.788 s `[BENCH] the action's own tap left the saved mark where it was in 120 passes`.
The same window holds `[INPUT] held the executor 3219 ms of 5008 ms … worst single hold 605 ms
(bound 30 ms) … capture 601 ms`: a camera capture held the executor for 0.6 s, and both publishes,
10 ms apart, were in the slot at once. **That interleaving is now pinned on the host**:
`render::tests::a_list_published_before_the_last_one_is_taken_replaces_it_rather_than_being_refused`
(two publishes, only the new one visible) and
`the_wake_a_park_waits_on_does_not_take_the_list_it_woke_for` (a park does not take the list).

**None of this was judged by eye**: roll the `entry.is_some()` out of `pick_row` and the new state
test and the sweep **go red together**, the sweep printing the path it reproduced it by — "the device
at boot, a triple click, a scan of two networks, a tap on the first band, the radio connected, the
radio reporting nothing on the air, a tap on the first band"; put the plane back to "full means drop"
and wire the wake up as one that takes the list, and the two plane tests above each go red (both tried
on the spot, green again once restored; a wake that is merely ready at once is **not** a mutation of
anything — it takes no list, and neither test went red on it — what counts is the version that really
takes the list). The static floor is still 65,536 B, and both images were
measured this round: the bench image `65,624 B` (the same figure as last round) and the product image
(no network given, `IOT_BOOT_PAGE=network`) **`65,944 B`** — 32 B above last round's 65,912 B, which
is the plane's two `Signal` wait futures landing in the render loop's own future, leaving 408 B. The
product image booted this time as `[CREDS] 0 saved network(s) on this device` → `[IOT] boot page is
Network` → 1–3 access points found → waiting on the list for a finger
(`[DISPLAY] stack peak 38,656 B of 65,944 B`).

### Tapping a saved row joins it with the copy the device keeps (`[measured]`, 2026-10-17, fifth run)

**Tapping a row this device has saved (the one drawn with `*`) joins it with the copy of the
passphrase the device keeps in `creds`**, instead of asking for it again. This row is where the two
**ownerships** on the credential path part: the store of passphrases has exactly one owner, the
network task (`SavedNetworks` never enters `DeviceState` — under the S3's static-memory wall those
980 B cannot become a value, see "Where a credential lives"), while all the page can say is *which*
network and never what its passphrase is; so a tap on the page hands over only a **name**, and the
answer (does the store have it, will it connect) comes from the radio.

- **The request is one plane with two shapes.** `CredentialPlane`/`CREDENTIALS` are renamed
  `JoinPlane`/`JOIN`, and the message goes from `Credentials` to
  `JoinRequest::{Typed(Credentials), Held(Ssid)}` — two degrees of "how much help" for one request,
  still one single-slot plane (no second plane, no extra `select` arm). `DeviceState.credential`
  therefore becomes `DeviceState.join: Option<JoinRequest>` (`take_join()`): one field still, four
  bytes wider at most (`Credentials` is already the wider of the two variants), and **no new
  `static`**.
- **With no copy in the store, the page is told.** The network task reads that plane in **two**
  places (`setup`'s own top, and `hold`'s window once the link is up) and both go through one
  `asked_for()`: `Typed` is used straight away (`Provenance::Named`); `Held(name)` goes through
  `SavedNetworks::copy_of(name)` — found, that copy is used (`Provenance::Stored`); not found,
  `BusinessIntent::NoCopy(name)` goes out (over the existing `INTENT_BUS`, the same road a `Link`
  report takes). The page's `no_copy()` acts only if the name is the one **currently selected** (a
  row tapped since is a newer question, and the stale answer is dropped) and raises the keyboard.
- **A refusal does not end in a loop.** That copy can be refused too (the access point changed its
  passphrase). `refused` then behaves as before, and `reopen_entry` adds one thing: **a selected row
  with no draft gets an empty one**. The keyboard is drawn over the draft and the strip is hidden by
  the keyboard, so "a keyboard over no draft" is a page with nothing to press — and this row is
  exactly the one that would only ever be tapped a second time (tap it → the store's copy → refused →
  no keyboard → the only thing left to tap is that row again). Filling in an empty draft is the only
  way out of that loop, and it is the one place `reopen_entry` **creates** a draft.
- **The write test parts with it** (`Provenance`): a passphrase from the build or from the panel is
  something the device **learned**, and it is written the moment the association succeeds (as
  before); the copy the store handed over is something the device **already has**, and it is not
  written. That also fixes a pointless write: the boot read's entry used to rewrite itself on every
  boot (two slots in turn means an erase and a program), which it no longer does; the price is that
  joining a saved row does **not** move it to the front of the saved list (so `remember`'s promotion
  now only happens for a passphrase typed into the panel again).

`[test]`: five new tests in `network_page` (a saved row leaves `Held` and raises no keyboard and
touches no passphrase; a saved **open** row still hands a credential straight over; `NoCopy` raises
the keyboard over an empty draft; a refused store copy raises the keyboard over that empty draft;
an answer about another row does nothing), two more moves in the reachable sweep (`Held` through to
an association, and `Held` through to `NoCopy`) with both pinned as reachable, one more bit in the
sweep's `key` (the two shapes are two states), and one `copy_of` test in `drivers::network` (it is
`None` exactly when it is not there, because an empty passphrase is a **legal** answer — an open
network).

**The bench's walk order changed with it.** It used to be `… LeaveGo →
ReopenTheRowJustLeft (tap the row back, reading "a fresh draft") → DropTheKeyboard → AskToForget →
Forget → Rescan`; it is now:

```
TapRowUnderLink → AskToLeave → KeepTheLink → LeaveAsk → LeaveGo
  → RejoinTheRowJustLeft    ← tapping the row back: the link comes up on the store's copy, page untouched
  → AskToLeave → KeepTheLink → LeaveAsk → LeaveGo    ← left again
  → AskToForget → Forget → ReopenTheRowJustLeft → DropTheKeyboard → Rescan
```

Both moves were forced by **the page's own rules**, not by taste: ① "tapping the row back" now reads
the **new** behaviour (the link comes up, no keyboard, no draft, and the row still carries its `*`),
so the act that reads "a fresh draft" had to move after the forget; ② while a link is in flight the
list is drawn under the joining box, and `network_hold` and `network_tap` both read
`Part::Content.showing` and call a long press `Invalid` — **a long press with a link up asks
nothing**, so forgetting has to come after the second leave. The order itself is pinned on the host
too (`visited`, compared act by act): an act skipped, or two acts swapped, goes red.

`[measured]`: the same `IOT_PANEL_BENCH` build, `creds` erased first (the 28-tap typing part of the
walk is unchanged), one S3 boot. **No finger touches the panel anywhere in it**: the whole path (tap
a saved row → the store's copy → the link comes back → leave again → forget → tap that row back →
rescan) is the build walking its own coordinates:

| Time | Log |
| --- | --- |
| 4.71 s | `[WIFI] associated with "<ssid>" on channel 11` → `[WIFI] this network is saved for the next boot` — this one is **the passphrase the bench typed into the panel** (`Provenance::Named`), and it is written as before |
| 5.13 s | `[BENCH] tapping band 0, the row the link was just taken off and this device has the passphrase for` → `[WIFI] the panel asked for the copy this device keeps of "<ssid>"` — **all the page handed over was the name** |
| 5.89 s | `[WIFI] joining the network the panel named: "<ssid>"` — the store's copy went to the radio; between the name arriving and this line there is **no keyboard and no draft** |
| 5.92 s | `[BENCH] the saved row brought the link back up with the copy on the part, and the page asked for nothing` — **this round's main reading**; and there is **no** `this network is saved for the next boot` anywhere near it: `Provenance::Stored` writes nothing, and the board agrees |
| 7.50 s | the second leave: `the panel asked to leave this network; the link is down` (the first was at 5.02 s) — **which is why the forget lands on the no-link side of the page** (a long press with a link up asks nothing) |
| 8.27 s | `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → `[BENCH] the saved mark is gone from the row the finger held` |
| 8.36 s | `[BENCH] the row the link was left from opens a fresh entry: the keyboard is up over 0 characters, and the draft the link dropped is not back` — after the forget that row is back in the "something to type for" arm |
| 9.11 s | `[BENCH] the strip's own round came back` — and **not one** `[BENCH] … 120 passes` anywhere: no act timed out |
| — | `[DISPLAY] main stack 65624 B` (the same figure as the round before), `stack peak 38944 B of 65624 B`; **no new `static`** this round, and the few bytes `DeviceState.join` widened by did not land on the main stack |

The product image (no network given, `IOT_BOOT_PAGE=network`) is **65,944 B** this round (408 B of
margin) and the bench image **65,624 B** — **exactly** last round's figure: the few bytes
`DeviceState.join` widened by landed on neither image's main stack. The same ruler (`nm`,
`_stack_start − _stack_end` — see "The main stack's account") and the two images' own
`[DISPLAY] main stack` logs **agree to the byte**.

`[measured]` (a reading taken on the way out, not a run of its own): **the build with no build input at
all** — the shipped start page, Ambient, and the one flashed back to the board last — is **65,976 B**
(440 B of margin). The `nm` symbols (`0x3fce9710 − 0x3fcd9558`) and the board's own boot log both say
`65976 B`; the two builds differ in exactly one place, the start page constant, and with
`IOT_BOOT_PAGE=network` it is the 65,944 B above — the 32 B sits in that one place.

`[not measured]`: the `NoCopy` cell **cannot be produced on the bench** — the mark and the store are
written by the same owner, so a mark being there means that round's store did have it; it is the
backstop for the "do not trust the mark, ask the store" rule and today only host tests reach it. A
**failed write** of the store's copy and the boot read's "does not write" are likewise code-reading
only (see still-to-do 5) — the latter is code-reading because this boot is the panel arm, with `creds`
erased, and never reaches it: what backs it is that `hold`'s call sites pass `Stored` (the boot entry
and the tapped row) and `Named` (a passphrase from the build or the panel), and `keep` is called on
the `Named` path alone (`app/src/network.rs`).

## The pure-logic part (host-testable)

Three rules in `iot-core::drivers::network` are product decisions rather than plumbing, so
they sit where a test can pin them:

- **A scan list is ranked, not raw**: dedupe on BSSID keeping the strongest report, order
  by signal, truncate to 8. Same-name access points are separate networks — a phone shows
  both — so the identity is the BSSID, not the name.
- **An SSID is bytes.** 802.11 allows arbitrary octets and the panel's glyph table is
  printable ASCII, so whether a name can be *drawn* is one decision with a fallback (draw
  the BSSID instead). A non-ASCII name is still carried byte for byte — connecting needs
  it — and only its drawing fails.
- **A round's rhythm** (a 5 s round, an empty round retried after 1 s up to three in a row, and a
  rest stretched to 15 s and then 30 s while nobody acts) is the same kind of policy, so it lives
  there too — see "The rhythm of a round".

`Credentials::from_text` **rejects** an over-long passphrase rather than truncating it,
because the driver rejects it too and a truncated passphrase authenticates against nothing
while reading as "wrong password".

Credentials come from **build-time environment variables** (`IOT_WIFI_SSID` /
`IOT_WIFI_PSK`, read with `option_env!`) rather than a secret store: a bench image should
say which network it joins, and a value compiled in is a value in the build command line.
The product's credentials are typed on the panel and kept in the `creds` partition (see "Where a
credential lives"); they must never be compiled into firmware. A build input outranks the stored
list: a build that names a network has that one path to try (changing it means changing the build),
where the stored list is the answer to "this device has been set up".

## Still to do (`[not done]`)

1. **Both parts' driver heaps have only about a third left** (C6 14,124 B of 49,152 B =
   28.7%; S3 15,820 B of 49,152 B = 32.2%, both measured). Anything that raises
   `DYNAMIC_RX_BUFFERS` or `STATIC_RX_BUFFERS` is a bill both boards pay — either measure both
   in the same commit that changes those constants, or raise that part's share to 56 KiB (the
   high end of the documented range: the S3 has the stack to pay the 8 KiB, the C6 does not).
2. **Why the camera's DVP DMA can eat a whole scan round**: 1 round in 12, no mechanism
   yet. A page that keeps scanning while the list is empty is fine, so it does not block
   delivery.
3. **The product page has to call `note_address`** — the only route to `Connected`.
   Leaving it out is what kept the panel saying "connecting" while the link was up.
4. **Re-measure** `[INPUT]` and the AEC figures — halving the data cache is a change that
   has already happened.
5. **The product page's second slice: the on-panel keyboard + credential storage + the stored
   mark and forgetting.** All three parts have landed: the keyboard and interaction are in "The
   product page", the credential in "Where a credential lives", the `*` mark and forget-by-long-press
   in "Tapping back into the row a link was left from, and one dropped list correction", and the
   tapped-row join in ⑦ below. What is left:

   - **① the board**: **the whole "tap a row → type the passphrase → OK → associated → addressed →
     leave → forget" path has been walked on the board by an `IOT_PANEL_BENCH` build**, including
     "a tap on that row while a link is in flight does nothing", "the strip is not drawn while a
     finger types" and "the joining box is on the panel before the radio answers" (see "Typing along
     the panel's own path", "The strip, the confirmation and forgetting", "A row under a link, and a
     box that covers the list" and "No strip while a finger types, and a box while the radio joins";
     no finger needed), and the fifth run's "tap the saved row back → the store's own copy brings the
     link up → the page asked nothing" (see "Tapping a saved row joins it with the copy the device
     keeps"). What is still a human's: the **feel** of the 4 × 4 keys, the list rows and
     the strip (the fourth key row is no longer under the strip now), the black ground with its 100%
     backlight, and how the joining box reads along with when it comes and goes — pixels and touch.
     Whether the modal **covers the list** and whether the **joining box is up** are no longer among
     those — they are host-testable facts (`Part::hidden`, `Part::showing`, `page::walk`,
     `Part::IN_ORDER`, `stamp::fill_surface`).
   - **② storage** (**landed**; the format, the slot rule and the write policy are in "Where a
     credential lives"): the three questions the wiring ran into are **settled without a flash**:
     ① **`espflash` writes the MD5 entry itself**: `espflash partition-table --to-binary
     partitions/lckfb-szpi-esp32s3.csv` puts the `ebeb ff…` entry after the three partitions, and the
     sixteen bytes it stores are the MD5 of the first 96 (`dd bs=1 count=96 | md5` =
     `5b7fb69b103d49aee6dd86b70ccdddcf`, byte for byte what the table holds) — the entry
     `esp-bootloader-esp-idf` with `default = ["validation"]` looks for, so **nothing to change**.
     ② **writing on a multicore part**: `esp-storage` 0.10's default refuses a write while the other
     core runs (`is_running(other_cpu)`), and this app **never starts a second core** (`esp_rtos::start`
     brings one up, the audio `InterruptExecutor` is a software interrupt on the same one), so the
     default would write too — `FlashStorage::multicore_auto_park()` is still the choice (it parks the
     other core through `CPU_CTRL` around each write and releases it after, a few milliseconds) for
     "starting a second core later cannot break this". ③ **an extra `data` partition**: the subtype has
     to be a **name** (`phy`, `nvs`, `coredump`, `nvs_keys`, `efuse`, `undefined` all parse) — a hex one
     (`0x40`) makes `esp-idf-part` panic on an `unwrap`, while `creds,data,undefined,,0x2000,` comes out
     as type/subtype `01 06` (`DATA_UNDEFINED`), placed after `factory`. Three boots on the board show
     the wiring too (an empty region written, and **a product image with no build input joining the
     stored network by itself**; the logs are in "Where a credential lives").
   - **⑥ Marking and forgetting a stored network** (**landed**): a `*` on a stored
     network's row in the list, and a long press on that row to forget it (`Touch::Hold` →
     `Intent::RowHold`, opening the `FORGET` confirmation only for a stored row). The
     implementation did **not** take the route of "a plane carrying the stored names into
     `DeviceState`": against the S3's static-memory wall, a `SavedNetworks` (~980 B) landing in the
     app either by value or as an `&EMPTY` blows it, so the **mask travels with the list itself**
     (`ApList.saved: u8`, one bit a row) and `SurveyHistory::{observe, marked}` take an
     `Option<&SavedNetworks>` so no value is built in the app. The price is that the state and the
     history each hold a list, so the `SCAN` plane has to be one that **takes every publish and drops
     none** (this round turned it into a slot that overwrites — see the fourth run). The erase goes
     through `Store::forget` and was checked on the part (boot 4 in "Where a credential lives").
   - **③ the details** (done): the selected row reversed edge to edge, the failure word in the
     typing row's middle slot, the keyboard raised again with the draft kept after a refusal, the
     `SHOW`/`HIDE` reveal, and `OPEN` plus the keyboard-free handover for an open network.
   - **④ the failure mapping as measured** (done): a wrong passphrase → the driver's reason
     `FourWayHandshakeTimeout` → `NetworkError::Credentials` → the panel draws `AUTH`, and handing
     the credential over to being refused is **37.6 s** (what is waited on is the driver's own
     four-way handshake timeout). `kind_of_disconnect` only maps the reasons that can be argued for
     (`NoAccessPointFound|BeaconTimeout` → `NotFound`,
     `AuthenticationFailed|FourWayHandshakeTimeout|HandshakeTimeout` → `Credentials`), the rest to
     `Driver`; the `NotFound` half is still **unmeasured** (it wants a bench with the access point
     switched off).
   - **⑤ a board reading for an open network**: even with the bsp's `Open` fixed, this arm has
     **no bench evidence** (there is no open network on the bench) — only the state-machine tests
     and a reading of the code. A WPA3-only network is out of reach because of the driver version
     (see "Three connection settings"), which is a scope limit rather than a to-do.
   - **⑦ tapping a saved row joins it with the store's copy** (**landed**, the details and readings
     are in "Tapping a saved row joins it with the copy the device keeps"): the page hands over a
     **name**, the network task looks it up with `SavedNetworks::copy_of` and takes the passphrase
     from there, and a name with no copy is reported back as `NoCopy` so the page raises the
     keyboard; the copy the store handed over is **not written** (`Provenance`). **With no bench
     reading**: the `NoCopy` backstop (the mark and the store share one owner, so "a mark without a
     store entry" cannot be produced on the bench), a failed write of that copy, and the boot read's
     own "does not write" (this boot is the panel arm, with `creds` erased) — code reading only.

   The list's own rule still stands: **one round of scanning is not the answer** — measured, a
   single round swings between 0 and 6 — which is why the page retries an empty round after 1 s
   (see "The rhythm of a round") and draws "never scanned" differently from "nothing found".
6. `esp-radio` is a beta: a version bump re-runs this whole page, **on both parts** — the
   per-part budgets above came out of what the driver actually took.
7. **Send the Wi-Fi blob's IRAM code to flash**: the only route to the S3's 32 KiB of
   `dram_seg` back, and this repository's script cannot sit between the two rules esp-hal
   generates — esp-hal needs a hook that can hold this first. See the driver's heap under
   "One source, two parts".
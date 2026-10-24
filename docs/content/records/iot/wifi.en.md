+++
title = "Wi-Fi Implementation Record"
weight = 45
sort_by = "weight"
[extra]
source_file_hash = "46b367cd47b2342260e5c56dcd0c720abcded822"
translated_at = "2026-10-23T07:57:48Z"
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
into the row a link was left from, and one dropped list correction"). **A board with no panel has a
front door now too**: `iot-core::provision` is the protocol (45 host tests), `iot-app::provision`
wires it to an open access point + DHCP + an answer-everything name server + a config page, and
`configure` races it against the panel's own path through `select` (see "How a board with no panel
gets configured"); the panel's path itself is untouched. **Both boards have since walked that door on
the bench** (the C6 end to end, the S3 after three fixes — see "The bench: from link, to phone, with
four corrections").**One way back into setup mode is the splash page**: both boards draw it for 1.5 s at boot, holding
BOOT during it asks for the mode and lands on the network page, and **no link is built during those
1.5 s** (see "Getting back to setup: hold BOOT on the splash page"). **No credentials means an offline
device**: no setup mode of its own and no access point, the panel's page scans only while somebody is
on it (it still offers `SCAN | AP`, and `AP` is what raises the access point — secured on a board
with a panel), and a saved network carries its BSSID/channel so the next boot goes straight there
(`fast_connect`). The config page is on its third version: **the first list is embedded, the icon is
inline, a reload costs one connection, and only the button wakes a scan** — the language switch sits in
the header, the passphrase is a bottom sheet and each row carries its BSSID (see "Seven or eight
reloads and the page goes white"); the panel's list rows gained that network's own BSSID on the same
day (a second line under the name, same section). The loose ends are at the
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
| address | `192.168.1.10` | `192.168.1.11` |
| driver's C heap | **32,204 B free of 65,536 B**, unchanged for 40 s | **14,124 B free of 49,152 B** |
| `phase Connected` | steady | steady |

> `[measured, updated 2026-10-16]` The S3's **65,536 B in that table is that day's budget.**
> From today both parts are at **49,152 B** (the S3's `dram_seg` half went from 32 KiB to
> 16 KiB — see the driver's-heap section below for why). Re-running the S3 probe with
> credentials, through the same scan + association + DHCP:
>
> - driver init plus six scan rounds pass at 48 KiB, with no `OutOfMemory`;
> - association takes **2,228 ms**, then **42 ms** to the address `192.168.1.10` (link up
>   9,337 ms after boot);
> - once connected the driver heap is **15,820 B free of 49,152 B** (32.2%), the same reading
>   38 times over 85 s — the table's 32,204 B on the S3 was a 64 KiB-budget reading;
> - the product image walks the same path: `[WIFI] this build joins the network named …`,
>   `associated`, `phase Connected`, no panic.

Both boards see the same access points (`<ssid>` strongest at −45…−50 dBm, then `ap1`,
`ap5`, `ap6`, `ap7`, `AA:00:00:00:00:01`), so what differs between the two columns
is the part, not the room.

esp-radio's documented heap need for a station is **47–57 KiB** with the default
`ControllerConfig` — the high end is the 6 dynamic RX buffers this repository sets — and
32 KiB makes `esp_wifi_init` fail with `OutOfMemory`. Measured, the driver takes 33,332 B
on the S3 and 35,028 B on the C6: **the C6 needs 1,696 B more**, so one source needs two
budgets. See [the section on that](#One_source,_two_parts).

Access points around the bench (union of 12 rounds and 6 more, strongest per row). **Names and
BSSIDs are placeholders** (`ap1`…, `AA:00:…`); channel, signal and security are measured:

| Name / BSSID | Channel | Strongest | Security |
| --- | --- | --- | --- |
| **`<ssid>`** | 11 | **−50 dBm** | secured |
| `ap2` | 11 | −80 dBm | secured |
| `ap1` | 6 | −81 dBm | secured |
| ap3 | 6 | −82 dBm | secured |
| `AA:00:00:00:00:01` | 1 | −78 dBm | secured |
| `AA:00:00:00:00:02` | 8 | −84 dBm | secured |
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

**When it is read and written** (both in `Board::new` and in `app/src/network/{link,store}.rs`):

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
| list (keyboard down) | a three-row header, **one subject a row**: `NETWORK` + **this board's MAC** (right-aligned) / `AP <name>` + `ST <phase>` + bars / `IP <addr>` + **that network's MAC** (right-aligned); the list's window `n-m/8` sits in the **action strip's left corner** — and below it **3 rows × 47 px** of list (**two lines per row: the name above, that network's own BSSID below** — a name that cannot be drawn *is* the address, and that row draws one line; an open network carries `OPEN` at its name line's right edge; **a stored network carries `*` after its name**; the bars sit at the right of the name line; **the selected row is reversed edge to edge**), then **an action strip** (`LEAVE` when there is a link, `SCAN` otherwise) | tapping a row picks it; **a row this device has saved (the one drawn with `*`) raises no keyboard** — it hands that row's **name** to the network task, which joins it with the copy the store keeps (and the keyboard comes back only when the store has no copy of that name, on a report from the radio); an **open network** raises nothing either, an empty passphrase is handed straight over; every other row raises the keyboard. Swipe up/down **slides the list one row**; **a long press on a row asks whether to forget it**; **tapping the strip** asks whether to leave when there is a link, and rescans at once (no question) when there is not; switching page is the **hardware triple click** only. **While a link can be left (`Connecting`/`Connected`) a tap on a row is refused outright**: nothing is picked, no keyboard rises and no credential is handed over (the `OPEN` rows too), while the long press and the slide are untouched (**they are not taken either while the joining box is up**: a list that is not drawn cannot be aimed at, see below) |
| typing (keyboard up) | **the same three rows** of header (`NETWORK` + this board's MAC / `AP <name>` + `ST <phase>` + bars / `PSK <draft>`, one reading a row; this board's MAC is drawn in this state too) and below it the **4 × 4** keyboard (**80 × 46 px** keys) | keys only: a character key tapped again cycles its candidates, `123`/`#&?` switch layer, Shift, backspace, OK, X, SHOW (reveal); every other intent is `Invalid` |
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
| A long press on a row forgets it | `Touch::Hold` → `BusinessIntent::AskForget(ap)`: only a row that is **stored** opens the `FORGET` confirmation, and the record is erased only when the verb is answered | forgetting is not reversible (the passphrase has to be typed from the start), which is why only it asks; a long press rather than another button, because it holds for few rows and drawing it would take the row's right edge. The name in the confirmation comes from that row's `ApList` entry, and the erase goes through `Store::forget` (the whole record rewritten, the two slots still written in turn) |
| A rescan does not ask | with no link the strip is `SCAN`, and a tap sends a `RESCAN` signal to the survey task at once | scanning is read-only and repeatable, so there is nothing irreversible to confirm; with a link up there is **no `SCAN`**, because `survey` is not reached at all while a link is held (`setup` is awaiting `hold` at that point), so the two actions never hold at once and the strip need not say "and" about them |
| Sliding the list | swipe up/down is `BusinessIntent::Scroll { rows: ±1 }`, moving `list_top ± 1` and pulling the end back to the last full window | paging left the last page of a short list blank and made the window reading jump by three; a row a slide makes every row reachable and keeps `n-m/8` continuous |
| Why the list does not jump | the ordering policy lives in core (`SurveyHistory`: 4 dB of hysteresis, a row surviving 2 missed rounds, published only when what is drawn changed) | every scan reading wobbles (σ≈3.1 dB on channel 11), so reordering on a single reading is the list jumping. The mechanism and the evidence are in "Why the list does not jump" |
| Failure classes | `NetworkError` splits into `NotFound` (`NOAP`) / `Credentials` (`AUTH`) / `Busy` (`BUSY`) / `InvalidArguments` (`NAME`) / `NoMemory` (`MEM`) / `Driver` (`DRV`); a failure **raises the keyboard again** and draws its word in the typing row's middle slot | the four-glyph words are drawn the same way the phase word is (the four-glyph test on `format_phase`). Only the driver's enumerable reasons that can be argued for are mapped; the rest fall to `Driver` — **the mapping as measured is below** |
| What the page's ground is | `DiagnosticsSink::draws_pages()` (default `false`) is overridden to `true` by `Screen`; `app::render::LightRenderer` gives a black fill, 100% backlight and no breathing when "the surface draws pages && the page is Network" | deriving an appearance from the page in core would be wrong: not every light has a screen (the C6's WS2812 is light instance 0 too), and whether a surface draws pages is something only the surface knows |
| Hit testing and drawing share one source | `keyboard::{key_of, key_at, key_box, list_band_at}` | where a key is drawn and where a tap lands are the same functions, so a geometry change cannot miss the hits |
| An intent reports what the page shows | `BusinessIntent::Choose(ap)` carries the full `AccessPoint`, and `Key { key, at_ms }` carries the lift's clock | the state layer looks the row up again by BSSID rather than trusting an index left by the page; `at_ms` is the only clock the multi-tap rule needs. `Scroll` reports no target either: one slide is one row, and whether the end was reached is the state layer's own business |

What gets drawn is decided by `diag_repaint_due`'s Network predicate: any change of phase,
access point, address, list or entry repaints, while the envelope and the touch counters do not
(`[test]` two tests, one per direction).

The arithmetic that turns readings into glyphs all lives in `apps/iot/crates/ui/src/text.rs`
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
are, the question is what the box holds) and `text::{format_joining_title,
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
the `Named` path alone (`app/src/network/{link,store}.rs`).

## How a board with no panel gets configured: an open AP behind a page (`[measured]`/`[test]`)

The C6 carries a WS2812 and a Boot key, and `LightRenderer::page_ground()` only lays down black when
`light.draws_pages()` — a WS2812 draws no pages, so `DisplayPage::Network` on that board is **neither
visible nor operable**. What was added is another front door: the device brings up an **open** access
point, hands out addresses over DHCP, answers **every** name with itself (which is what turns a
phone's "this network has no internet" check into the page opening by itself), and serves the config
page over HTTP. The panel's door is untouched: on the S3 the panel still leads with the page
alongside it, and the C6 has the page alone.

| Layer | Where | What it is | Who tests it |
| --- | --- | --- | --- |
| Protocol | `iot-core::provision` (`http`/`dns`/`route`/`scan`/`wire`, plus two `include_str!` HTML files and an icon under `assets/`) | request parsing, routing, answers, what a name gets answered with, the list as JSON — **and no allocation anywhere** (form fields decode into the caller's stack buffers) | host: **45** tests (the `#[test]`s in `provision/*.rs`) |
| Device | `iot-app::provision` | four loops (DHCP server, name server, page, `embassy-net` runner) wired to sockets | the bench (still-to-do 8) |
| Wiring | `configure` in `iot-app::network` | raises the AP, races the page against the panel, hands the credential back to the `JOIN` plane | the bench (still-to-do 8) |

Routes: `GET /` (and `/index.html`) for the page (**with the list embedded**), `GET /scan` for the
list as JSON, `GET /rescan` for that same list and **a wake for the next survey round**, `POST
/submit` to take a credential and answer **inline** with the done page, `GET /done` (and
`/done.html`) for that same page, `GET /favicon.svg` for the icon (cached for a day), `GET
/favicon.ico` for a `204`, and **everything else 302s to `BASE_URL`** — that last one is the feature
rather than a fallback: the paths a phone probes with (connectivity checks, assorted probes) get a
redirect, and the page is what that answer is for.

On the radio side the `Station` trait gained `begin_setup`/`end_setup`, and the bsp lands them on
`Config::AccessPointStation(StationConfig::default(), ap)` (**APSTA**, so scanning still runs while
the access point is up) and `Config::Station(...)`. The access point is
`AuthenticationMethodConfig::Open` (checked against `impl Default for AccessPointConfig` in
`esp-radio 1.0.0-beta.1`), and its name comes from the board's own address
(`vanling-<last two MAC octets>`, with three `[test]`s pinning the format, the dedupe and the zero
padding) rather than from a build constant: one image flashed to two boards is two networks in one
room.

### The answer reaches the phone before setup mode ends

`provision::serve` returns `Infallible`, so in `configure` it races the panel's own path
(`setup_credential`) through `select`: **whichever names a credential first ends setup mode**, the
page's future is dropped with the `select`, and the access point goes down with its `Runner`. The
credential travels the existing `JOIN` plane, so `hold`, the store and `Provenance` are reused
unchanged — the page and the panel are the same path at the level of "where a credential came from".

The page publishes the credential only after writing the answer **and waiting for the peer to
acknowledge it** (`TcpSocket::flush`, semantics read off `tcp.rs` and its docs in
`embassy-net 0.9.1`). That is not caution; it is a race that **loses every time**: publishing to
`JOIN` means `select` resolves at once and the page's future goes with the AP's `Runner`, so there is
**no next `interface.poll`** — bytes sitting in the socket's buffer that never reached the medium
vanish with the stack, and the phone sees a request that is never answered. `/submit` therefore
answers with the done page inline in a 200 rather than redirecting to `/done`: a redirect needs a
second request, and the stack that second request would wait on is already gone.

### An idle exit, and the `/exit` that is still not built

The previous version of this section said "the AP stays up until somebody hands over a credential":
the reference firmware's "reopen the AP after a while" and its `/exit` (`78__esp-wifi-connect` in
`../xiaozhi-esp32`) both read as identity operations. The first half stopped being true once setup
mode became two front ends — the access point is now **a front end raised for a phone**, and one left
up with nobody reading it is a configurable device for anyone in range; "reopen it" is still
meaningless, because the answer is to take it down:

- **`SETUP_IDLE_MS = 180_000`**: the clock starts when the access point front comes up and restarts
  with every request the page answers (`provision::page_read()` is a latest-value window, see
  `page_idle`). A phone filling the form in is not an idle access point.
- **The clock is always armed** (`page_idle()`, no more `armed`): a board with a screen goes back
  to the panel front; a board without one goes back to the network it keeps (`return None`, `link()`
  associates again); a board with **neither** (no screen, no saved network) takes the access point
  down too and lands in `browsing()`'s offline state. The old "neither → no clock" exception is gone
  with the third cut of the entry: a board with a screen also gets in through the offline page's `AP`
  cell, a board without one only through the explicit reset-then-hold, and a factory device's open
  access point should not keep beaconing (see "Getting back to setup: hold BOOT on the splash page").
- **`/exit` is still not built**: the page has no place for it, the panel's
  `LEAVE`/`CLOSE`/`CANCEL` say the same thing more completely, and the C6's way in is the hold on
  the splash page (see "Getting back to setup: hold BOOT on the splash page"), not a link in the
  page. (The page's own two timeouts wait on "the first byte" and "one request", not on idleness —
  see ④ under "The bench: from link, to phone, with four corrections".)

### Scanning no longer waits for the panel

`[measured]`: in setup mode `survey` scans **unconditionally** (the round's own pacing is unchanged —
see "The rhythm of a round"), replacing the "wait for the panel to arrive" gate
(`PAGE.wait_for_list()`) with a non-blocking `PAGE.shows_list()` plus a rising edge. The reason: on
the C6 nobody ever reaches that page, and the page is reading exactly this list.
`SurveyHistory::clear()` on arrival still runs, so "what a page entered shows is the room as it is"
is unchanged.

The list exists **twice**, one per reader: the panel's rides the `SCAN` plane (which the render loop
*takes*), the page's rides `provision::LATEST`. The page must not take the panel's copy, and
`LATEST` is a window where only the newest value means anything, so it is a `Mutex<RefCell<...>>`
rather than a channel.

### Dependencies and versions

`edge-dhcp 0.8.0` (`io` only) + `edge-nal 0.7.0` + `edge-nal-embassy 0.9.0` (`udp` and `proto-ipv4`
only), with `embassy-net`'s features gaining `dhcpv4`/`tcp`/`udp`. The versions have to line up with
`embassy-net 0.9`: `edge-nal-embassy` 0.8.x is built against `embassy-net 0.8`, and the two do not
compile together. `udp` is for DHCP alone (it takes its socket through `edge-nal`'s `UdpBind`); the
name server and the page use `embassy-net`'s own `UdpSocket`/`TcpSocket`, so `edge-nal-embassy`'s
`tcp`/`dns`/`icmp` are off — they would be code every board links and no board calls. HTTP is
hand-written (HTTP/1.0, `Connection: close`, `Content-Length` always set): **`picoserve` was
rejected**, it needs nightly's `impl_trait_in_assoc_type` and this repository pins stable 1.95.0.

`proto-ipv4` is **this crate's own** feature; having `embassy-net`'s copy on does not count — that is
what knocked it out when the dependency was written as "nothing but `udp`", and what the cost and the
reading were is in the next section.

Both HTML pages go into `.rodata` through `include_str!`, and on this target `.rodata` lands in DROM
(`0x3c01xxxx` under `nm`) — the flash-mapped region, which does not come out of `dram_seg`.

### Memory: the feature hit the static wall on the S3, and how it pays for itself (`[measured]`)

One ruler (`nm` for `_stack_start_cpu0 − _stack_end_cpu0`), one command (`--bin vanling
--no-default-features --features lckfb-szpi-esp32s3,console-auto`):

| Step | POOL | Main stack |
| --- | --- | --- |
| HEAD rebuilt (same command) | 43,336 B | 65,976 B (440 B of margin) |
| setup mode wired in, the page's future inline | 50,984 B | **link failure**: `stack.x:13 cannot move location counter backwards` |
| the page's future `Box::pin`ned | 43,552 B | 65,464 B (72 B short of the 65,536 B assertion floor) |
| `LATEST` boxed + `&Ssid`/`&ApList` | 43,408 B | 65,592 B (56 B of margin) |
| `SurveyHistory` boxed | 42,728 B | **66,264 B (728 B of margin)** |

Both boxings reuse the rule this record already has (see the `Store` under "The account for the main
stack"): **every frame that carries a large value by value is a fixed static's worth of the product's
one task**. The page's four loops plus their socket buffers are **7,432 B** of future, which
`Box::pin` moves wholesale to the heap (PSRAM on the S3, that 64 KiB app heap on the C6);
`SurveyHistory` is another ~700 B (the 344 B `LATEST` was boxed too at one point, and on the bench one
338 B allocation per round is what killed the S3; it is a static again — see the next section). The
last row ends up **288 B above HEAD's margin**: the whole feature
nets the static budget back about 300 B, and both the C6 and the S3 link.

**The ruler's resolution has to be on the record**: summing `.bss`+`.data` symbol by symbol through
`nm`, the second row differs from the first by only 80 B, while the location counter moved 384 B —
the difference is **section alignment**, not data. Shrinking `LATEST` from 344 B to 8 B mid-way
bought back only 128 B, for the same reason. So this budget is measured on `_stack_end`, not on the
sum of symbols.

Flash is a separate account, also measured: `moon run iot:image` puts the S3 at
`997,376 / 6,291,456 B` (15.85%, a 6 MB partition) and **the C6 at `776,016 / 1,048,576 B`
(74.01%, a 1 MB factory partition)**. The same tree with setup mode removed (after a `git stash`,
same command) puts the C6 at `706,704 B` (67.40%), so **the feature costs 69,312 B of flash
(+6.61 points)**. The two HTML pages are 4,725 B between them; the rest lands in smoltcp's
`tcp`/`udp` (the old image's networking stayed on the IP layer — a DHCP client touches no TCP) and in
`edge-dhcp`/`edge-nal`. That is inherent to "serve a page over HTTP", not part of the hand-written
half that could be trimmed.

### The bench: from link, to phone, with four corrections (`[measured]`, 2026-10-19)

The C6 went through first: a phone joined `vanling-1234`, took a lease, and the page opened **by
itself** (the name server answers every name with `192.168.4.1`); then `GET /` → `GET /scan` (the
neighbouring networks) → `POST /submit` (a real passphrase) → `phase Connected`. With the credential
on flash, **reflashing the same image still booted straight into the link**. The S3 then turned up two
problems that only show with a radio in the loop, and two more where the page's own answer was too
slow.

**① A 338 B allocation per survey round kills the S3**: the first `/scan` panicked — `memory
allocation of 338 bytes failed`, with 1,632 B free on the heap. That allocation was a copy of the list
(`Box<ApList>`, four neighbours). The fix is **not** to make it smaller but to remove it: the page's
copy of the list is a window where only the newest value means anything, so it should have been a
static all along, and it is a `Mutex<RefCell<ApList>>` again — a cost that was fatal at runtime traded
for one the linker accounts for. The same round took the page sockets' send and receive buffers from
512 down to 256.

**② One socket is one connection, and a phone holds more than one**: `Connection: close` makes every
fetch a new connection, while the phone also runs a connectivity check (`generate_204`), `mmtls` and
preconnects in the background. On one socket `/scan` waits for a slot to come free — 2 to 3 answers
in a 40 s window; two sockets were still 2 to 3; **three** is what it takes. The third socket's
`StackResources` comes from the **heap** (`Box::leak`), so `.bss` did not grow by a byte
(`SETUP_SOCKETS` is still 5).

**③ A socket in hand is being served, not re-armable**: `armable(state)` accepts only `Closed` and
`TimeWait`; a socket that came out of `accept()` stays in the loop's hand until its answer is
written, and an `accept()` in the meantime only gets `InvalidState`. Re-listening while writing was
expected to answer more — measured, the opposite: it refused the page's own `/scan`, its favicon and
the phone's probes.

**④ Two timeouts, not one**: from the connection coming up to its first byte waits
`PENDING_TIMEOUT = 1 s` (that is where a phone's preconnect and an abandoned connection sit), and
after the first byte `CONNECTION_TIMEOUT = 10 s` takes over. The pick order changed too: **a held
socket with data goes first** (`can_recv()`), then any held socket, and only then a new one — a real
request should not queue behind an idle connection.

**Readings** (S3): 14 `GET /`, 10 `/scan`, 1 `POST /submit` (the whole round 6,858 ms, of which the
page's own write was 1,900–2,100 ms), which is why the log reads
`… answered Ok, 4468 B of body, in 2111 ms`; a connection that cannot be read prints
`a request would not be read after N ms: <err>` (a socket timeout and a peer RST cannot be told apart
in the log, so the log is read by elapsed time). The page's own `/scan` retry ladder is 5 × 320 ms,
which is what crosses the queueing in ②.

**The static wall**: the same ruler (`nm` for `_stack_start_cpu0 − _stack_end_cpu0`) reads **65,880 B**
after these four, against the 65,536 B floor — **344 B of margin**; the render loop's own reading,
`[DISPLAY] stack peak 38592 B of 65880 B`, agrees with it. Trimming this branch's comments and shapes
afterwards (and making the POST path allocation-free) moved `[DISPLAY] main stack` to **65,912 B**
(**376 B** of margin), with the peak still 38,592 B. `[HEAP] … free with setup mode's future up` prints
on every boot: S3 1,632 B, C6 **41,056 B** (about a third of that 64 KiB window in use).

That round's `moon run iot:image` put the S3 at `1,004,848 / 6,291,456 B` (15.97%) and the C6 at
`782,272 / 1,048,576 B` (74.60%); rebuilding after the trim gives S3 `1,003,904` (15.96%), C6 `781,984`
(74.58%) — **smaller**, not bigger. Nearly all of the difference is `percent_decode`: both call sites
inlined it, and `#[inline(never)]` taking it back to one copy saves the C6 1,296 B of image (1,314 B of
`nm` symbol sum) and the S3 1,216 B. **Whether flash grew is a question for the symbol sum: one small
function inlined twice can eat a whole trim's gain.** That round's page, `assets/portal.html`, is
4,468 B (the third version is 14,819 B, see the next section), and its inline CSS/JS is
**deliberate**: one connection fetches the page, at the cost of not being able to fetch half of it.

### Seven or eight reloads and the page goes white: how many connections one reload is worth (`[measured]`/`[test]`, 2026-10-20)

What came back from the bench was "seven or eight reloads and the page goes white (spinning or an
error)" — **the page fails to arrive, the phone does not drop off**. The previous round raised each
page socket's buffers from 256 B to 1,460 B, which fixed **how many round trips one page costs**
(18 → 4); it did not touch any of this round's three accounts:

| Account | Before this round | Now |
| --- | --- | --- |
| Connections per reload | page + `/scan` + icon (including the `.ico` 204) + the phone's own connectivity checks = **3 to 6**, against 3 page sockets; the extra handshakes are refused **silently** by the stack | **1** (the page itself: the icon is inline as a `data:` URI and the first list is embedded in the document); the phone's own probes are still their own connections and get one 302 |
| What the page does to the radio | reads `/scan` every 4 s, and the device called `RESCAN.request()` on **every** `/scan` — a page left open meant a radio scanning continuously | only the **Scan again** button goes through `GET /rescan`; reading the list (`/scan`) no longer wakes the survey |
| What the first paint waits for | the document, then an XHR for the list before any row can be tapped | the list is in the document, so the first paint is already tappable |

**Why "the page scanning on its own" hurts**: the setup access point is APSTA, and a channel-by-channel
scan interrupts clients — exactly what ESP-IDF's documents warn about and what this record's comparison
table marked as "the difference was not reproduced here". Three to six connections per reload, each
followed by another immediate rescan, is how seven or eight reloads end with a phone looking at a
network nobody answers. This round removes both, which turns that `[as-reported]` from "scanning may
interrupt a client" into "a page must not press scan on its own".

**The embedded list is a slot, not a template**: `route::SEED_AT` (`/*scan*/`) marks the position, the
page puts a `[]` that **stands on its own** in that slot (unsubstituted it is an empty list, not a
syntax error), and at send time `send_page` writes `LATEST`'s JSON into the slot while
`Content-Length` is computed as `page_len(aps) = PAGE.len() − SEED_SLOT + scan_json_len(aps)`. This is
the one place where "what the head says" and "what goes out" can part ways, so both halves have tests:
`the_head_of_the_page_names_the_page_with_the_list_in_it` (against `json()`'s actual length) and
`the_page_leaves_one_slot_for_the_list_and_it_stands_on_its_own`. The address is still a
length-preserving substitution (`MAC_TOKEN`); the list is not — hence the fixed order, address first
and list second, each appearing once.

**The page, third version** (`portal.html`, 14,819 B; the inline CSS/JS is still deliberate): the
language switch moved from the footer to a segmented control in the **top right**; the passphrase went
from an inline field to a `<dialog>` sheet that slides up from the bottom (with show/hide and
`minlength=8`, backdrop or Esc to cancel, an `open`-attribute fallback for older browsers); a row is
now the name + **that network's own BSSID** (small, tabular, so two mesh or repeater radios stop
looking alike) + four signal bars + lock/open + a `Saved` chip; under the list is **Scan again**
(`GET /rescan` → wait 1.8 s for that round → read `/scan`). `done.html` only swapped its icon and
keeps reading the same language key.

The byte account: the page went from **4,468 to 14,819 B** (+10,351: the i18n dictionary, the dialog,
the card layout, a BSSID per row, the inline icon), while **a reload went from 3 to 6 connections down
to 1** and the first paint no longer waits for an XHR. `/favicon.svg` is still served by name (cached
for a day) and `/favicon.ico` is still a 204.

**The panel's list carries the BSSID too.** The phone's rows had the address, the panel's three did
not: a row is 47 px and the name (24 glyphs = 215 px) already spends the width, so 17 more glyphs
(149 px) do not fit beside it. A row is **two lines** now — the name above, the address below — and
the pair costs 20 px, centred with 13/14 px left top and bottom, so the 47 px tap is unchanged
(`keyboard::band_lines_top`, pinned by a host test and a compile-time assertion that the second line
does not reach the next row). The reason is usability, not typography: a mesh node and its own base,
or two repeaters with one name, used to be guessable only from the bars; the address can now be read
character by character against the phone's row.

An address is **upper case** (`AA:BB:CC:DD:EE:FF`). Three formatters used two spellings: the panel's
header and address line used `mac_text` (upper), while the page's per-row BSSID (`scan`'s
`hex_digit`) and the panel's own line for **a name that cannot be drawn, which falls back to the
address** (`text::write_bssid`) were lower — one person holding two screens read one value
in two spellings, and on the panel the two could meet **inside one row**. Both were brought to the
upper case the panel already used; `ap_name` (`vanling-a1b2`) was left alone, because that is the
access point's name rather than a drawing of its address and changing its case renames the device.
Each formatter gained a test with **a lettered octet**, since every existing fixture was all zeroes
or all digits and could not tell the two cases apart. One more thing fell out of it: when a name
cannot be drawn the name line *is* the address, so that row no longer draws a second line — the same
string printed twice a line apart is one printing too many.

**The header's board address moved a row.** It used to take the network's row (where `AP <name>`
goes), so picking a row or a link coming up pushed it aside — exactly when someone goes looking for
it. It now shares the `IP <addr>` row (address on the left, MAC right-aligned) and row 1 belongs to
`AP <name>` and the bars alone. The widths are accounted for: the address runs from the block's left
edge with `IP ` written inline (this row skips the four-glyph label slot, the one place on the page
that does) for 158 px, ending at 168, and the MAC's 17 glyphs start at 171 — a compile-time assertion
pins that they cannot meet (the first draft kept the label slot and the assertion went red by six
pixels). The typing state needs the whole row for `PSK <draft>`, so the address is not drawn there and
comes back when the keyboard closes. A hole in `diag_repaint_due` was closed at the same time:
`DisplayPage::Network` compared `link`/`aps`/`setup` and **not `mac`** — the address arrives after the
radio's first report, while the survey's normal answer is "the list is unchanged, so the page is not
redrawn", so that cell could stay blank for good. The diff now includes `prev.mac != next.mac`, with a
test that changes only the MAC and requires a Network repaint.

**The bench's first boot** (C6, `[measured]`): `Get / answered Ok, 15314 B of body, in 72 ms` (four
APs embedded; `page_len` computes 14,819 − 2 + 497), and the log holds **no `/scan` and no
`/favicon.svg`** — a reload is one connection and the list does not wait for an XHR. The boot's
`[HEAP] … free with setup mode's future up` is the ruler for that: **33,384 B** before the buffer
change, **32,104 B** at 2,048 — exactly 1,280 B less, which is what puts the buffer in setup mode's
future (the heap) rather than on the main stack. The same reading was 41,056 B the previous round.

**A real defect the same log caught**: `a request filled the buffer without ending`. Chrome Android's
request headers (user agent + client hints + `sec-fetch-*`) run 700–1,000 B against a
`REQUEST_BYTES` of 768 — a request that does not fit is **closed without an answer**, which on the
phone looks exactly like "spins once, white page, fine after another reload". Raised to **2,048 B**;
that buffer lives in setup mode's future (the heap), so the S3's static wall did not move by a byte.
The check is not reasoning: re-flash and re-capture, and that warning is gone.

**This round's sizes**: `iot:image` gave the S3 `1,020,848 / 6,291,456 B` (16.23%) and the C6
`796,864 / 1,048,576 B` (75.99%), up 5,456 B and 5,424 B from the previous round (the page's
+5,155 B is most of it; the rest is the panel's second line per row). The `IOT_BOOT_PAGE=network` S3
image is `1,022,736 B` (16.26%) and is what the bench board runs (it boots into the list). **The
static wall did not move**: `nm`'s `_stack_start_cpu0 − _stack_end_cpu0` is **65,816 B** against a
65,536 B floor, **280 B to spare** — the same number for both builds and before and after
`REQUEST_BYTES` (the buffer is on the heap), and the board's own boot prints
`[DISPLAY] main stack 65816 B` with a 38,672 B peak. Setup mode's heap on the S3 reads 25,560 B this
round (32,104 B on the C6).

### The header re-cut to one subject a row: the two addressed devices get a row each (`[test]`, pixels to be looked at)

What the bench asked for was row 0 `NETWORK … mac`, row 1 `AP <name> bars`, row 2 `IP mac`. The
previous cut could not do it, and **that is not taste but pixels on top of one another**:
`AA:BB:CC:DD:EE:FF` is 17 glyphs = 149 px and, right-aligned, occupies **x=171…319**; the phase word's
value starts at 150 and its longest word ends at **182**, overlapping the MAC by 11 px; the list's
window counter is drawn at **x=218**, inside the same span. A row is 35 cells, and the four items
(title 7 + phase 7 + window 5 + MAC 17 = 36 cells) never fit anyway.

Two things moved, both for a geometric reason:

- **The phase word moved to row 1** (`AP <name> ST <phase>`): it says where the link *to that access
  point* got to, so it belongs beside the name. The name slot is 17 glyphs, ending at 194, and the
  phase starts at a fixed **x=199** (the name's length does not move that column), seven glyphs at
  its widest ending at 258; the bars sit at 288…319.
- **The list's window counter moved into the action strip's left corner**: `1-3/6` starts at x=10
  while `SCAN`/`LEAVE` stays centred (about 139…180), so the two never meet. It is still drawn by the
  branch that draws the list (a question over the list takes the counter with it); only its position
  changed.

| Row | What it holds | Width (px) |
| --- | --- | --- |
| 0 | `NETWORK` + **this board's MAC** | title 10…69, MAC 171…319 |
| 1 | `AP <name ≤17>` + `ST <phase ≤4>` + bars | name 46…194, phase 199…258, bars 288…319 |
| 2 | `IP <addr ≤15>` + **the connected network's MAC** | address 10…168 (`IP ` inline, skipping the label slot), MAC 171…319 |
| strip | `n-m/8` … `SCAN`/`LEAVE` | counter 10…51, verb centred |

The two MACs are told apart **by their rows**: row 0 is this board (the title's row, given up by no
state, and drawn while typing too), row 2 is the network it is on (whose name the row above just
printed). Three compile-time assertions pin the pairs that must not meet (title vs this board's MAC,
name vs phase, address vs the network's MAC), and the phase-to-bars assertion counts the three glyphs
of `ST `.

**Why the phase could not stay on row 0**: its value starts at 150 and four glyphs end at 182, which
always overlaps this board's MAC at 171; narrowing the label slot only lets a two-glyph word like `UP`
through, while `SCAN`/`JOIN`/`AUTH` all run past. So row 0 can carry one of the two, and this board's
MAC is what the bench goes looking for.

The phone page's per-row line gained a `BSSID ` prefix (the page head already says `Device <this
board's MAC>`, so neither MAC is a bare value on its own any more). Since 2026-10-21 that label
reads `AP MAC` — same value, same small line, no term (see "The link-up cell goes back to
`LEAVE`").

**Sizes**: `portal.html` 14,819 → **15,128 B** (+309: the prefix and its comment). `iot:image` gave
the S3 `1,021,728 / 6,291,456 B` (16.24%) and the C6 `797,168 / 1,048,576 B` (76.02%), up 880 B and
304 B (the page's 309 B plus the panel header's new constants and code). The `IOT_BOOT_PAGE=network`
S3 image is **1,023,616 B** (16.27%). **The static wall did not move a byte**: `nm` still reads
**65,816 B** (280 B to spare) and the board's boot prints `[DISPLAY] main stack 65816 B`; setup mode's
heap on the C6 still reads 32,104 B (that buffer was not touched this round).

**Not yet read** (`[unfinished]`): whether the 100 px between the title and this board's MAC on row 0
reads as "this device", the gap between the name and the phase word on row 1, and whether the strip's
left-corner counter is mistaken for something tappable — only the panel can answer those three; the
phone page's prefix and its *Scan again* wait on the bench as well. The bench board runs the
`IOT_BOOT_PAGE=network` image with one network still in its `creds` partition, so it comes up **with a
link up** — which is exactly the state that shows both MACs (row 0 and row 2) at once; seeing setup
mode (typing, the dialog) needs that stored network erased first.

### `abort()` straight after the answer: the RST throws away what the phone has not read (`[measured]`, 2026-10-20, evening)

What came back from the bench was an **occasional** `ERR_CONNECTION_RESET`: the page sometimes failed
to appear, and reloading fixed it. Every one of those runs is `Get / answered Ok, 15,425 B of body, in
88–196 ms` on the device's side — the bytes really were written, and what was wrong is **how the
connection ended**:

- `close()` + `flush()` only waits for **our own FIN to be acknowledged** (`FinWait2`). The peer has
  not sent its FIN, which means it may still be reading.
- The `abort()` right after sets the socket `Closed`; the stack sees `remote_endpoint` still there and
  sends a **RST** (the `rst_pending` test in `flush()`).
- A RST makes the peer's kernel **discard receive-queue bytes its application has not read yet** — and
  the browser reports `ERR_CONNECTION_RESET`.

Three things beside it: 0 B answers (`generate_204`, the `/mmtls` 302) never failed once (one segment,
no "not read yet" window); a large page takes several segments every time; both boards share this
code, so both boards fail.

**The first fix was wrong, and usefully so**: waiting for the peer's FIN before releasing the socket,
bounded at 500 ms, brought `ERR_CONNECTION_REFUSED` straight back from the bench. A socket waiting for
a FIN **is not listening**: one of the three page sockets sits in that wait, the phone opens six
connections per reload, and by the time the page's own connection is fourth in line there is no
listener left, so its SYN is answered with a RST. Those minutes read exactly that way: three `Get /`
lines, everything else `generate_204` retries.

**The measurement settled what the wait costs**: the peer's FIN is **already in** when `flush()`
returns (`TimeWait after 0 ms`, 451 of 455 connections), so waiting for it costs nothing; the bound
came down to **100 ms**, which 4 of 455 reached (still `FinWait2`, and that RST lands long after the
peer stopped reading). Accepted on the bench: **55 page requests, 0 failures** (`could not accept` 0,
`did not finish` 0).

The same build turned `send`'s silent `Ok(0)` into `an answer stopped N B short`: it catches **three
peer-side** resets mid-body (the body stopped at 9,171 of 15,710 B), which used to be silent.
`CONNECTION_TIMEOUT` came down from 10 s to **3 s** as well: a request that only half arrived must not
hold one of three page sockets for ten seconds (a page takes ~100 ms).

**One hypothesis the same logs overturned**: the scan was worked on first — every answered request
raised `PAGE_READ`, and the survey's pause restarted on seeing it, on the grounds that a per-channel
scan takes the radio off the access point's channel (a round ≈1.15 s against a 5 s pause, so the first
few rounds spent ~19% of the time off-channel). It did take effect: **zero `survey found`** lines across
nine reloads, and the rounds either side moved from 31 s to 47 s. The page **failed anyway**, so the
scan was not the cause. The gate stays (a radio should not scan while somebody is reading a page), but
it is not what fixed this.

**Sizes**: the product image on the S3 is `1,025,680 / 6,291,456 B` (16.30%), the
`IOT_BOOT_PAGE=network` one **1,027,424 B** (16.33%). The static wall got **smaller** this round: the
product build reads **65,720 B** (184 B to spare) and the network build **65,688 B** (152 B to spare) —
the same board printed 65,816 B last round, and where those 96/128 B went is not measured (not the wait
for a FIN: that lives in setup mode's future, which is heap). With three digits to spare, it belongs in
the record.

### Against ESP-IDF's provisioning (three trades)

Read off ESP-IDF's own Wi-Fi Provisioning documents (`wifi_prov_mgr`, `wifi_prov_scheme_softap`).
Three places where we **differ from it**:

| Question | What ESP-IDF does | What this device does | The price |
| --- | --- | --- | --- |
| When the service stops | after the device **is connected**, with the client able to read the status | **as soon as a credential arrives** (first past `select` wins, see "The answer reaches the phone before setup mode ends") | a wrong passphrase drops the phone off the AP, and only the device itself can bring setup mode back. On the panel's path a refusal is measured at 37.6 s (`AUTH`, still-to-do 5④); the setup page's side is measured too (2026-10-24, C6): the device brought the AP back by itself after the refusal (by eye, no serial reading this round) |
| Scanning under a SoftAP | the documents warn that scanning channel by channel interrupts the client's connection, so a grouped sweep is offered (`[as-reported]`; the difference was not reproduced on this device) | channel by channel (APSTA) | the phone stayed on through several bench rounds (`[measured]`); worth re-checking on another board. On the page's side this is settled: the page no longer presses scan on its own (the third version drops the rescan every `/scan` used to trigger — see the previous section) |
| How a credential travels | a protocomm session + proof of possession + encryption | **an open AP and a plain-text form** | anyone on the same network can read that passphrase, and can hand the device a different one — a known trade; the splash entry's AP is open (the panel's `PASS` switches to WPA2), while a running panel starts it secured, so this trade sits on the default path only for the splash round; a one-time PIN would close it further |

## Setup mode becomes a gear the panel can see and leave (`[measured]`/`[test]`, 2026-10-20)

Until now setup mode was **implicit**: no credential, a `LEAVE`, or a refusal sent the network task
into `unconfigured()` for good, and the only thing the panel could say was the phase word on the
header. Two things were impossible because of it: **seeing that the device is being set up** (an AP
that is up and a link that is retrying drew the same page), and **coming back out** — after `LEAVE`
the only way on was scanning the list again.

This round turns it into a gear that is visible and reversible:

| Thing | Before | Now |
| --- | --- | --- |
| Into setup mode from a link | strip `LEAVE`, a box asking "leave?" | strip `LEAVE`, a box asking `LEAVE "cafe"?` / `THE LINK GOES DOWN` (it was called `SETUP` for one round and then put back — see "The link-up cell goes back to `LEAVE`") |
| Out of setup mode | not possible (`unconfigured` diverged) | strip `CANCEL` → the `CANCEL_SETUP` plane → `unconfigured` returns → `link` retries what it holds |
| Knowing the mode is on | no such field | `LinkStatus.setup` / `setup_cancellable`, header `ST SETUP` |
| The header's network | the link's access point (none in setup) | `AP vanling-xxxx` derived from the MAC — the same name the phone's list shows, not a second copy in the state |
| The address row | the station's address (none in setup) | `IP 192.168.4.1` |
| First entry | silent | one `SETUP MODE` / `AP vanling-xxxx` / `CLOSE` box, dismissed by any tap, once per boot |
| Sound | none | `Rise` on entering setup mode, `Tap` on connected, `Error` on refused (the existing MIDI cues; no PCM, no on-device TTS) |
| The button | `Click` on the Network page was `Invalid` | the strip's own verb (`LEAVE`/`CANCEL`/`SCAN`) |
| The `n-m/8` counter | the strip band's left corner | the title row's gap between `NETWORK` and the MAC |

Four rules were settled here, and each is pinned by a test in core:

- **`CANCEL` is only offered when there is a way back**: `setup_cancellable` is computed by the
  caller (`link` asks whether the saved list is non-empty; the build-named path is always true),
  not by the network task reading the store — the store is still mutable inside `unconfigured`,
  and "where does a cancel go" is the caller's question. Without a way back the strip falls back
  to `SCAN`: on a device that was never set up, `CANCEL` would only take its own AP down and raise
  it again, which a phone sees as the page disappearing.
- **The mode is not a phase**: `LinkPhase` stays the radio's (it is `Off`/`Scanning` in setup mode)
  and `setup` is a bit beside it; folding a non-phase into the phase enum would fork every
  `Connected`/`Failed` decision.
- **The first announcement is not "every time the AP comes up"**: `seen` is one bit per boot, and
  a refusal raises the AP again — that second time must not drop the box back over the keys a
  passphrase is being retyped on.
- **Sounds are started in the state's transitions**, not by a page: the network task is the only
  thing that knows "just connected / just refused", and all it publishes is a `LinkStatus`.

`[test]` Host-pinned behaviour: the strip's two verbs and `take_setup_cancel` handing over once;
no way back falling to scanning; the announcement appearing once, `Dismiss` closing it, a later
re-entry not bringing it back; `Part::hidden` covering the list and the strip while it is up;
`Click` on the Network page = `Toolbar`; any tap (a long press included) = `Dismiss` while it is
up; the `Rise`/`Tap`/`Error` transitions; `format_question(Leave)` = `LEAVE "cafe"?`; the
announcement's second line = `AP vanling-2fc8` (a synthetic MAC).

`[measured]` Gates: `cargo fmt --check`, `moon run iot:test` (core 651, bsp 112, app 34),
`iot:lint`, `iot:check-host`, `iot:check-s3`, `iot:check-c6` — all pass.

`[measured]` The static wall (one ruler: `nm` for `_stack_start_cpu0 − _stack_end_cpu0`, floor
65,536 B):

| Tree | Main stack | Spare |
| --- | --- | --- |
| HEAD `b61479d` rebuilt (`git stash` of the uncommitted work) | 66,008 B | 472 B |
| Before this round's work (an earlier reading this session) | 65,720 B | 184 B |
| After this round | **65,688 B** | **152 B** |

So the feature itself costs **32 B** (65,720 → 65,688), and most of the 320 B against HEAD is the
uncommitted portal/RST round. The board's own line agrees with `nm`: `[DISPLAY] main stack 65688 B`,
`[DISPLAY] stack peak 38520 B of 65688 B`.

Flash: `moon run iot:image` gives S3 **1,031,168 / 6,291,456 B (16.39%)**, **5,488 B** more than the
previous round's 1,025,680 B product image; the C6 image of the same name is in `dist/`.

`[measured]` On the board (product image, `IOT_PORT=/dev/cu.usbmodem1422301`):

```
[WIFI] this build was given no network and this device has saved none, so the setup access point and the panel are both open until one of them names one
[WIFI] setup mode for "vanling-2fc8" is open
[HEAP] 25560 B free with setup mode's future up
[WIFI] survey found 3 access points
[DISPLAY] main stack 65688 B
```

`[not done]` What this round did **not** do:

- **The headless board's entry** (xiaozhi's "press Boot while unlinked to enter setup + a blue
  blink"): the entry is the hold on the splash page (see the third cut of "Getting back to setup");
  **not done** is the click entry (the C6's click is still the light's colour walk — the LED *is* the
  light). The lamp itself is the next section.
- **Pixels and touch**: the box, `ST SETUP`, the `AP`/`IP` rows, the counter's new home and the
  width of `CANCEL` all need a human eye; `LEAVE`/`CANCEL` need a human thumb once.
- **A wrong passphrase in setup mode** (measured, 2026-10-24, C6): a wrong passphrase typed on the
  setup page is refused, the red lamp is normal, and the device brings setup mode back by itself (the
  AP is visible again); this round did not change that path — it only removed the reboot that used to
  be the way out of setup mode. By eye; no serial reading this round.

### The headless board's lamp (`[test]`/`[measured]`, 2026-10-23/24)

The C6 has one WS2812 and no screen, so provisioning state can only be spoken by that one lamp. The
language lives in core's state layer (`DeviceState::status_light`) and applies only to a board with a
radio and no panel; the S3 keeps drawing its own pages and boxes on its screen.

| Device state | Lamp | Why |
| --- | --- | --- |
| Never provisioned (after the splash) | Three amber flashes (160 ms on / 160 ms off, about 0.8 s), then the page light | Factory/offline and waiting to be set up; this is an announcement, not a mode, and the surface goes back to the page when it ends |
| AP up, waiting for a phone | Blue slow breath (2 s, 8→48) | This is setup mode; a slow breath says waiting is not an error |
| An unsaved credential associating/waiting for an address | Blue faster breath (0.8 s, 16→64) | The copy the phone just submitted; a saved credential's automatic reconnect does not take this path |
| Join succeeded (`Connected`) | Three green flashes (100 ms on / 100 ms off, about 0.5 s) | Success is an event, not a mode; the page light takes over when it ends |
| Failure: AP not found (`NotFound`) | Three amber flashes | A person fixes this by moving the device, not by retyping a passphrase |
| Failure: a refused credential or anything else (`Failed`) | Three red flashes (120 ms on / 120 ms off, about 0.6 s) | The AP is already down and the phone cannot see the device; red says "refused" |
| Provisioned and online | No override; the page's own light shows | Normal idle |
| Provisioned but temporarily offline | No override; one announcement per outage | "Has a network, merely off it" is an event, not a mode; retries do not flash again, and the page light is always underneath |

Priority is **one-shot notice > setup AP > unsaved-credential attempt > page light**. Success and
failure fire only on the phase transition; one outage is latched by `outage_announced`, announced
exactly once, and the latch is cleared by `Connected`, so a five-second retry does not keep covering
the lamp. A saved credential's automatic reconnect (`saved == true`) never shows the blue
"connecting" mode: the product light is there from the first second. The notice clock belongs to the
render loop (`tick_notice`); the state layer only keeps the frame to draw, which is what lets host
tests drive it with plain numbers. The last flash ends on its lit edge, with no dark pause after it.

When the access point's 180 s idle clock expires, a board with no panel and no saved network takes
the AP down and returns to offline `browsing`; that path now publishes one more link report with
`setup = false`, so the lamp goes from blue back to the **page light** instead of claiming a mode
that is no longer up.

`[test]`: `cargo test -p iot-core` pins the lamp's modes and priority, all three flash schedules
(including the last flash having no trailing dark), the `NotFound` versus refused split, the single
amber announcement at the splash's landing, a splash hold not reporting offline, a retried failure
being announced once and re-armed by a reconnect, and "the status light overrides only the first
surface and never a board with a screen".

`[measured]` (2026-10-23, C6, product image, reset without pressing the button): the splash ended at
`00:00:01.848` (1500 ms), `[WIFI] no panel and no saved network; waiting offline` followed, and the
lamp frames were —

| Time | Lamp |
| --- | --- |
| 01.848 | `[LAMP] solid 255 155 0 at 96` — first amber flash |
| 02.028 | `[LAMP] dark` |
| 02.188 | `[LAMP] solid 255 155 0 at 96` — second flash |
| 02.348 | `[LAMP] dark` |
| 02.508 | `[LAMP] solid 255 155 0 at 96` — third flash |
| 02.668 | `[LAMP] the page's own light` — the last flash ends on its lit edge and the page takes over |

The render loop logs `[LAMP]` only when the status light **changes** (a breathing page light does not
write every frame), so the bench can count the flashes and their lengths without an eye: the
180/160/160 ms above are each within one frame (`STEP_MS = 20`) of the designed 160 ms.

`[measured]` (same day, C6 reset into an automatic reconnect with one saved network): association
succeeded at `05.481`, and `[LAMP]` gave the three green flashes —

| Time | Lamp |
| --- | --- |
| 05.516 | `[LAMP] solid 0 255 0 at 64` — first green flash |
| 05.636 | `[LAMP] dark` |
| 05.736 | `[LAMP] solid 0 255 0 at 64` — second flash |
| 05.836 | `[LAMP] dark` |
| 05.936 | `[LAMP] solid 0 255 0 at 64` — third flash |
| 06.036 | `[LAMP] the page's own light` — handed back 100 ms later |

Every segment is exactly 100 ms, as designed; the reading also re-confirms that a saved credential's
automatic reconnect never shows the blue attempt and puts the product light up from the first second.

**This round of readings found a defect**: the announcement was originally timed on the render loop's
**frame counter**. The association holds the executor for a few hundred milliseconds, and the loop
then replays the skipped frames at full speed to catch up, so the three 100 ms green flashes were
compressed to 19 ms on the hardware (`03.581`–`03.600`, six `[LAMP]` lines). `tick_notice` now takes
the **wall clock** (the render loop passes `Instant::now()`), while the frame counter stays with the
renderers; the re-test is the 100 ms table above. Host tests still drive the state layer with plain
numbers and are unaffected.

`[measured]` (2026-10-23, C6, by eye): the three amber flashes did hand the lamp back to the product
light, a BOOT click changed the colour with the page (so nothing was holding the surface), and a long
press after the splash opened no AP. Brightness and colour are acceptable. **The red kind (a refused
credential) was confirmed on the bench** (2026-10-24, by eye; no serial reading was kept this round);
the green kind is pinned by the reconnect reading above.

## Two front ends of setup mode, one at a time (`[measured]`/`[test]`, 2026-10-20, evening)

> The row above that says "one `SETUP MODE` / `AP` / `CLOSE` box, dismissed by any tap, once per
> boot" is superseded by this section: that box is now **the access point front's face**, and it is drawn
> only while the access point front is up.
>
> The table's "a board with no panel boots on the access point front" is superseded in turn by the
> third cut of "Getting back to setup: hold BOOT on the splash page": **a device with no credentials
> boots offline**, with no scan and no access point. The access point front comes up only after the
> reset-then-hold, the panel's `LEAVE`, or a refusal, and the 180 s idle clock takes it down. The two
> fronts themselves are unchanged.

The last round made setup mode a gear on the panel, but both entrances were **live at once**: the AP
stayed up and the panel's list kept scanning. So one radio kept changing roles, and a device merely
sitting on a desk was broadcasting a configuration page anyone could join.

This round splits it into **two front ends, one at a time**:

| Thing | Before | Now |
| --- | --- | --- |
| A board with a panel entering setup mode | AP and panel list both live | the panel's front first (`Front::Panel`), no AP |
| Switching to a phone | join the AP | strip's right cell `AP`: the AP comes up, the list is covered by a box |
| Back to the panel | not possible | `CLOSE` in the box, or the strip's right cell `PANEL`: the AP goes down, the list is back |
| A board with no panel (C6) | AP always up | boots on `Front::AccessPoint`, an **open** AP (see the note after this section) |
| Whether the AP has a passphrase | always open | **per entry**: open from the splash page; on a running board with a panel, the AP starts on WPA2 with a random 10 characters (not remembered); the C6 has no screen, so it can only be open |
| Nobody reading the page | AP stays up | 3 minutes without a page request takes the AP down and returns to the panel |
| A broken touch panel | power-cycle only | a Boot long press **on any page** in setup mode switches fronts |

The rules settled here:

- **Two fronts, not two paths**: both share `setup_credential`, `SurveyHistory`, the list, the page,
  `JOIN`/`FORGET`/`CANCEL`; `Front` only decides which one is open. Splitting the code would make
  "switch back" a second implementation.
- **The station makes and holds the passphrase**: `begin_setup(ssid, secured)` says which kind of AP
  to raise in one call, and `status()` reports `setup_password`. The panel reads it off the link
  status and draws it; the page never sees it, and the log records its length only, never its value.
- **`SetupPass` is a fixed-width narrow type** (`[u8; 10]`), not an `Ssid`: the link status is copied
  every window, and a 32-byte name field holding a 10-byte value is 22 bytes the main task pays for
  nothing. It is also one of the ways this round pays for itself on the static wall (table below).
- **The alphabet drops `0/o/1/l`**: `23456789abcdefghjkmnpqrstuvwxyz`, 31 symbols × 10 characters
  ≈ 49.5 bits. Copying that off a 320 px line into a phone's keyboard, look-alike characters are the
  only place it goes wrong.
- **The default passphrase depends on the entry, and is not remembered**: the splash page's BOOT
  hold still raises an open AP, so a phone joins by tapping the network's name. Every entry made
  from a running device — the Network page's `AP`, a `LEAVE`, a refusal — starts on WPA2 with a
  random 10 characters on a board with a panel; the box shows `PASS …`, and `NO PASS` opens it for
  that entry. `PASS`/`NO PASS` last for one visit; going away and back restores the entry's default.
  The C6 has no screen to show a passphrase, so it can only be open. The price is that the splash
  entry takes anyone in range for 180 s and the form is plain text; the running entry at least costs
  a passer-by the ten characters read off the panel.
- **`CLOSE` and `CANCEL` are different things**: the box's right answer `CLOSE` only takes the AP
  down (back to the panel front); the strip's left cell `CANCEL` leaves setup mode to rejoin the
  saved network. The latter is drawn only when there is a way back (`setup_cancellable`); with no
  saved network the panel front's left cell becomes `SCAN` and the access point front's strip is one
  `PANEL` box — a blank device goes panel → access point → panel without ever touching "leave".
- **A stray tap in the box does not take the AP down**: `answer_at` returns `None` outside the two
  answers, so the strip underneath still answers. This front has no "any tap dismisses", because a
  finger wandering across the box must not tear down the AP a phone is being walked onto.
- **The access point front does not survey on a clock**: `survey_pause_after(Front::AccessPoint, …)` returns
  `None` — not a zero-length pause but no clock at all. Entering the front surveys once to fill the
  page; after that only the page's own `RESCAN` asks for a round.
- **3 minutes** (`SETUP_IDLE_MS`): every page read resets it, `CLOSE` takes the AP down at once. It
  prices the one thing the access point front costs that the panel front does not — a network **anyone in
  range can join**. The price paid for that is discoverability: a device straight out of the box is
  **invisible** in a phone's list until somebody taps `AP` on the panel. A board with no panel has no
  cell to tap, so its way in is the reset-then-hold on the splash page (see the third cut), and a
  device out of the box is just as invisible on it.

Geometry and wording:

- The strip's two cells live in core (`ToolbarCell` / `toolbar_cell` / `toolbar_cell_at`):
  `PANEL_COLUMNS/2 - TOOLBAR_GAP` = **152 px** per cell, and the 8 px seam is taken out of the cells
  rather than added outside them; `PANEL_COLUMNS` dividing evenly into two cells and one cell being
  at least `TOUCH_MIN` are both compile-time assertions. The seam belongs to neither cell, but taps
  are cut on the middle column: `x < PANEL_COLUMNS/2` is the left cell, the rest is the right one —
  no dead band inside the 8 px.
- Left cell: `CANCEL` when there is a way back, `SCAN` (`WAIT` while a round is in flight) on the
  panel front when there is not, and absent on the access point front with no way back; right cell always
  `AP`/`PANEL` — a switch names the destination, not the current state; the link-up cell is the
  exception, because `LEAVE` names what happens to the link (see "The link-up cell goes back to
  `LEAVE`").
- The box: title `SETUP MODE`, two body rows `AP vanling-xxxx` + `PASS …` (or
  `OPEN, NO PASSWORD`), two answers `NO PASS`/`PASS` and `CLOSE`. The body's second row must end
  above the answer box, a compile-time assertion
  (`DIALOG_BODY_TOP + ROW_PITCH + FONT_H <= answer.y`); so is `PASS ` + 10 characters fitting
  `DIALOG_GLYPHS`.
- The panel front draws **no fake** `AP`/`IP` rows: no AP means no name and no address, so the
  header keeps `ST SETUP` and the list.

`[test]` What the host pins: `LinkStatus.front` and `shows_setup_box()` (= setup mode and the access point
front); `survey_pause_after(AccessPoint, …) == None` while the other three gears keep their clocks; the
two cells' geometry (equal width, seam inside the cells, `toolbar_cell_at` on both halves and on the
boundary); `switch_front`/`take_setup_open` only acting in the right front, and `take_*` handing the
request over once; the box's two answers and "outside them is `None`"; the Boot long press on any
page while setup mode lands on the right cell; the box hiding `Part::Content` while **not** hiding
`Part::Controls`; `format_setup_pass` in both shapes (`PASS k3mq9t7w2x`, `OPEN, NO PASSWORD`).

`[measured]` Gates: `cargo fmt --check`, `moon run iot:test` (core **656**, app 34),
`iot:test-bsp` (bsp **113**), `iot:lint`, `iot:check-host`, `iot:check-s3`, `iot:check-c6`,
`moon run iot:image` all pass.

`[measured]` The static wall (same ruler: `nm`, `_stack_start_cpu0 − _stack_end_cpu0`, floor
65,536 B):

| Tree | Main stack | Spare |
| --- | --- | --- |
| After the last round | 65,688 B | 152 B |
| This round (two fronts + passphrase + merged request plane) | **65,592 B** | **56 B** |

A net **96 B** spent. Two savings are explicit: `CANCEL_SETUP` folded into `SETUP_ASK` (one plane,
one `Signal`, one static and one `select` arm fewer), and `setup_password` narrowed from
`Option<Ssid>` to `Option<SetupPass>` (34 → 11 bytes). The on-board reading matches `nm` exactly:
`[DISPLAY] main stack 65592 B`, `stack peak 38696 B of 65592 B`.

`[measured]` The access point front on the bench: to see this front without a finger, the S3 branch's
initial front was changed to `Front::AccessPoint` **temporarily** (one line, reverted after measuring; the
product still starts on the panel front):

```
[WIFI] this build was given no network and this device has saved none, so setup mode opens
       on the front this build starts with until it names one
[WIFI] setup access point up as vanling-1234 (under a passphrase)
[WIFI] setup mode for "vanling-1234" is open, under a passphrase
[WIFI] setup mode: an access point, a lease for whoever joins it, and the page at http://192.168.4.1/
[WIFI] survey found 1 access points
[DISPLAY] main stack 65592 B
```

(The AP name is written as the synthetic `vanling-1234`: it is derived from the MAC's last two
octets, and the real octets do not go into the record. The survey on entry is what fills the page;
there is no clock behind it.)

`[measured]` The **product** (an S3 starting on the panel front), same board and same empty `creds`
partition, broadcasts nothing on that first screen: there is no `setup access point up` line at all,
and the list surveys every 5–6 s as usual:

```
[WIFI] this build was given no network and this device has saved none, so setup mode opens
       on the front this build starts with until it names one
[WIFI] survey found 3 access points
[DISPLAY] main stack 65592 B
```

That is the evidence behind "the first screen does not broadcast": a device straight out of the box
is invisible in a phone's list until somebody taps `AP` on the panel.

`[measured]` The 3-minute idle close:

```
[WIFI] nobody read the page in 180 s; the access point goes down
```

After that the panel front's survey rhythm returns to the log — one round every 5–6 s at first,
slower as idle rounds accumulate — and neither `nm` nor the on-board main stack moves.

### Three things that did not hold on the real board: the switch cell, the scan cell, the box closing itself (`[measured]`/`[test]`, 2026-10-20, evening, second cut)

With the product image flashed, three things on the panel did not hold: no switch cell in sight, no
`SCAN`, and the box that opens on `AP` closing itself again. Each has a definite code path; none
of it is a matter of feel:

- **Left cell not drawn, right cell filling the whole strip**. `stamp_strip` used to draw only the
  right cell when `!setup_cancellable`, and that cell used `toolbar_box()` across the whole strip
  carrying `AP` alone — so `SCAN` vanished entirely and the switch did not look like a cell
  either. This board's `creds` is empty (no saved network), so `setup_cancellable == false` is the
  normal case here, not a corner.
- **The box closing itself**. `SetupAsk` is an `embassy_sync::Signal`: raising the AP takes ≈ 1 s,
  and a second tap on `AP` during it (or one tap read twice) leaves a `Front` request on the
  plane; the next reader is the **access point** front's loop, which reads `Front` as "back to the panel" —
  the box opens and closes at once. The plane does not remember **which front** a request was made
  on.
- **Fix one: two cells, always two cells** (strip option A). Left cell = `CANCEL` when there is a way
  back; on the panel front with no way back, `SCAN` (`WAIT` while a round is in flight — the same
  word the strip carries outside the mode); on the access point front with no way back the left cell is not
  drawn and the right one fills the strip — the access point front has no list to scan, and an empty half
  cell would be a control that does nothing. The right cell is always `AP`/`PANEL`.
  `state::toolbar` reads the same conditions: left cell = cancel / scan / switch.
- **Fix two: the request carries the front it was made on**
  (`SetupAsk::{Cancel, Front, Pass} { from: Front }`). The render loop publishes the current
  `link.front`; each front's loop takes only requests where `applies_to(its own front)`, drops the
  rest and logs one line: `dropped a setup request made on the panel's front; the access point stays
  up` (the other direction is `... made on the access point's front; the box is not open`). No time window:
  a request was either made on this front or it was not.

`[test]` What pins it: `a_setup_request_only_answers_the_front_end_it_was_made_on` (six
combinations); `the_panels_front_keeps_a_cell_for_looking_around_when_there_is_no_way_back` (panel
front, no way back: left cell = `SCAN`, `rescan_wait` set, right cell still the switch); the access point
front's test renamed to `..._has_only_the_front_switch_on_the_access_points_front`.

`[measured]` Gates: `cargo fmt --check`, `moon run iot:test` (core **657**, app 35),
`iot:test-bsp` (113), `iot:lint`, `iot:check-host`, `iot:check-s3`, `iot:check-c6`,
`moon run iot:image` all pass; `nm` is still **65,592 B** (56 B left), since this round touched a few
lines of code and two `Front` fields and no stack. Flashed and booted: `[DISPLAY] main stack 65592
B`, `stack peak 38696 B`, with no `setup access point up` (the product still starts on the panel
front).

`[not done]` What this round did **not** do:

- **Pixels and feel**: the two cells' widths, the glyph widths of `AP`/`PANEL`/`NO PASS`/`PASS`/
  `CLOSE`, where the box's second row sits, and the `AP`/`IP` rows left blank on the panel front —
  a person has to look; and a person has to press the `AP` cell, the box's two answers, and the
  Boot long press in setup mode.
- **The box no longer closing itself, and the two cells being visible**: the first needs a finger
  (the code path and its tests are there, but "a second tap inside the second the AP takes" only
  reproduces by hand); the second is pixels and needs an eye.
- **The C6's open AP**: the policy and the code are there (`serve(..., Front::AccessPoint, false)`),
  `iot:check-c6` compiles it, but the C6 has no product path, so it is **not measured**.
- **A no-hands walk of the front switch**: this round tried adding one to `IOT_PANEL_BENCH` (tap the
  right cell → the box rises → tap `NO PASS` → tap the right cell back to the panel, all through the
  intent ring), **but the bench variant no longer links**: `vanling-panel`'s `.data` is 160 B larger
  than the product's (identical symbol sizes — an alignment hole the bench's extra `format_args!`
  drags in), which drops the main stack to 65,432 B, 104 B under the floor. The walk was therefore
  not kept: a walk that cannot be built is worse than none. Restoring it means making the bench
  variant pay for those 160 B first.

### Every survey round wrote the panel front back: the station's status does not know its front (`[measured]`/`[test]`, 2026-10-21)

The previous section put "the box closes itself" on a stale `SetupAsk`; `applies_to` stopped that one
(there is a `dropped ...` in the log at 05:24), but what happened **every round** was another line:

```
[WIFI] setup access point up as vanling-1234 (under a passphrase)
[WIFI] setup mode for "vanling-1234" is open, under a passphrase
        <- the box appears here
[WIFI] survey found 2 access points
        <- the state's front is the panel again, the box is gone (the radio still beacons)
```

Entering the access point front surveys once, and `survey()` published
`LinkStatus { setup: true, ..station.status() }`. The station's own status only knows the radio half
(phase, AP, address, passphrase) and **does not know which front it is on**: `front` is the default
`Panel` and `setup_cancellable` is `false`. So every round overwrote those two fields — `shows_setup_box()`
read "not on the access point front" and the box vanished; on a board with a saved network, `CANCEL`
would have become `SCAN` every five seconds as well. It also explains the shape of the report: **nobody
touched it.**

Fix: `LinkStatus::on_setup_face(front, cancellable)` (core) writes the front and the way back in one
place and passes phase/AP/address/passphrase through untouched; the network task's four publishes
(panel front, access point front, survey ok, survey failed) all go through it, and `survey()` takes the
same `Face`.

`[measured]` The no-hands reading: the S3 branch's initial front was changed to `Front::AccessPoint`
temporarily (one line, reverted after measuring), and the serial log shows one front change and no
flip back after the scan:

```
[WIFI] setup mode is offered on the AccessPoint front
[WIFI] survey found 2 access points
```

The product image, back on `Front::Panel`, boots with no `setup access point up` and no front-change
line, and surveys every 5–6 s. The render loop now logs
`[WIFI] setup mode is offered on the {front:?} front` whenever the front changes — that line is the
reading that does not need an eye on the box.

`[test]` `a_publish_from_setup_reports_its_own_face_not_the_stations_default`: both fronts, cancellable
or not, covering `front`/`setup_cancellable`/`setup`, and asserting that phase, AP, address and
passphrase do not move.

`[measured]` Gates: core **658**, app 35, bsp **113**, `iot:lint` / `iot:check-host` / `iot:check-s3` /
`iot:check-c6` / `iot:image` all pass; `nm` is unchanged at 65,592 B (56 B left).

### The name: `PHONE` → `AP` (same round)

`PHONE` was the only one of the three names that named **what you happen to hold**: it is wrong for a
laptop and it is not the industry word. Espressif calls the whole thing *provisioning* (with SoftAP and
WEB transports), WLED, Tasmota and Shelly all say **AP mode**, and the box's first line was already
`AP vanling-xxxx`. So: the strip's right cell is `AP`, the variant is `Front::AccessPoint`,
`PHONE_LABEL` became `AP_LABEL`, and `phone_idle` became `page_idle` (it waits for page requests, not
for a phone). Prose about configuring **from** a phone stays as it is — that names the method, not the
front.

**Still needs eyes or a finger**: the above proves the front no longer flips, not that the box is drawn.
Still owed: one tap on `AP` with the box staying up, the two cells' widths and `AP`'s glyph width, the
box's two answers, and the strip being one `CANCEL` cell on the access point front against
`SCAN | AP` on the panel front.

### The link-up cell goes back to `LEAVE`; the page's row says `AP MAC` (`[measured]`/`[test]`, 2026-10-21)

What was wrong on the bench was two words, not two mechanisms:

- **`SETUP` did not say the link would go down.** The link-up cell had been named for its
  destination, but the panel front raises no access point, so entering setup mode *is* leaving the
  link — a word about the destination hid the cost. It is `LEAVE` again: the box asks
  `LEAVE "cafe"?`, the body is still `THE LINK GOES DOWN`, the two answers are `KEEP` | `LEAVE`.
  Where it lands has not moved (`Hold::Left` → `unconfigured()`'s panel front), and the left cell
  inside the mode is `CANCEL`, which joins the saved network back — `LEAVE`/`CANCEL` are a pair.
  The front switch still names destinations (`AP`/`PANEL`): you cannot judge a front you have not
  switched to, while the whole point of leaving **is** the cost, so this is the one exception.
- **The page's row label is `AP MAC` rather than `BSSID`.** The page head already says
  `Device <this board's MAC>`, and the row's value is the **access point's** MAC. `AP MAC` says
  whose it is without a term and without colliding with the head. The wire field stays `"bssid"` —
  that is the protocol, not a label.
- **Leaving a link lands on the panel front, not on the phone's page.** `front` was
  `let mut front = front;` in `link()`, as long-lived as the task, and only the panel's
  `AP`/`PANEL` cells changed it — a link coming up or going down never reset it. A device that had
  been configured through the phone's page therefore kept `front` on the access point, and a
  `LEAVE` tapped at the panel reopened the access point: a tap made at the panel opening the mode
  on the phone. `Setup`'s comment only promised the carry *across refused credentials* (a passphrase
  retyped on the phone stays on the phone's page), never across a successful join. The fix is
  `front_after_link(start, current, ended)`: `Left` and `Named` go back to the build's answer (the
  panel where there is one, the access point where there is not), and `Refused` stays where it is.
  Both entry logs now name the front (`on the {:?} front`), because the next report of this kind
  could not be answered from the serial log without it.
- **`AP MAC` and the address read as one run.** The label and the value were a **single**
  `textContent` (`'AP MAC ' + bssid`); one plain space at `.72rem` and `--dim` is about 3 px, so the
  line read `AP MACA4:CF:…`. They are two spans now (`.tag` + `.addr`), the space is a
  `gap: .5rem` the layout owns, and the address is `--ink`; the head's `Device <MAC>` lifted its
  `<b>` to the same colour, so both lines read "dim label, lit value".

`[test]` `a_leave_names_the_network_the_link_is_on`: `format_question(Leave, link)` is
`LEAVE "cafe"?`; a link the radio has not accepted yet (`Connecting`, no `ap`) is asked about with
the verb alone; `Confirm::Leave.network(link)` is the link's own access point, so the row the box
highlights and the row the question names cannot come apart. The verb and the body are unchanged
(`LEAVE` / `THE LINK GOES DOWN`). The cell now draws `format_verb(Confirm::Leave)` — the same
source as the box's answer, so the invitation and the button cannot drift apart (`the_boxes_words_are_the_verbs_own_and_fit_the_box` reads both).

`[measured]` Gates: core **658** / app 35 / bsp **113**; `iot:lint`, `iot:check-host`,
`iot:check-s3`, `iot:check-c6` and `iot:image` all pass; `nm` is still **65,592 B** (56 B left) —
this passed one more `LinkStatus` into two synchronous functions and added no state across an
`await`.

**Still owed to an eye or a thumb**: with a link up the cell reads `LEAVE`, its box asks
`LEAVE "cafe"?`, answering `LEAVE` lands back on the **panel front** (the list with `CANCEL` on the
left and `AP` on the right; the serial log should say `setup mode opens on the Panel front`); and
the page's `AP MAC` has a visible gap before the address, which is a shade brighter than the label.

`[test]` `front_after_link` used to have no host test: it lived in `network.rs` behind the `wifi`
feature, which pulls in `iot-bsp-esp/wifi`, and no host build compiled it (`iot:test`'s `host` has no
`wifi`). Splitting `network` into four files moved this pure decision into core (`LinkEnding` and
`front_after_link` in `core/src/drivers/network.rs`), and two host tests now pin it:
`a_leave_or_a_pick_opens_setup_where_the_build_put_it` (the six cells of start front × current front
× `Named`/`Left` all return to the start) and `a_refused_credential_keeps_the_front_it_was_typed_on`
(`Refused` keeps the current front). The `link`/`configure` loops themselves still have no host
harness: that would first need a fake-`Station` bench for the link task, so the evidence for them
remains the serial log of one `LEAVE` and the front the panel shows.

### A reload no longer earns `ERR_CONNECTION_REFUSED`: keep-alive, six listeners and half a second (`[measured]`/`[test]`, 2026-10-21)

On the C6 bench a phone opened the setup page and the browser said `ERR_CONNECTION_REFUSED`. A
capture after a reset (the phone came back by itself) showed:

```
[WIFI] Get /generate_204_<uuid> answered Redirect, 0 B of body, in 0 ms
[WIFI] a request would not be read after 0 ms: ConnectionReset
[WIFI] a request would not be read after 0 ms: ConnectionReset
```

The probe was answered with a 302, two connections beside it were reset by the phone, and not one
`Get /` appeared — the page never arrived. "One reload is one connection" counted the document and
not what the phone opens beside it: preconnects and the system's connectivity check send their SYNs
within milliseconds, and `SETUP_SOCKETS = 5` was DHCP 1 + DNS 1 + the page's **3**. The fourth SYN
finds no listener and **the stack answers it with a RST**, which the app never sees; Chrome marks the
host as refusing connections and drops the two it had already opened (those are the two
`ConnectionReset` lines).

Three changes:

1. **Keep-alive** (`[test]`): answers go from `HTTP/1.0 … Connection: close` to `HTTP/1.1 …
   Connection: keep-alive`, with `Response::write_head(out, close)` making the caller's word the only
   one; `answer()` reads the next request on the same connection, with an idle window of
   `KEEP_ALIVE = 2 s`. A phone's own `Connection: close` (`Request::close`, including a comma list
   that names `close`) or a request that names a network (`Action::Join`, the access point goes down
   right after the answer) ends it. A reload therefore reuses the connection it has instead of
   spending a listener. **Pipelining is not read**: a second request in the same segment is dropped
   with the connection's buffer, and no browser sends one.
2. **The page's listeners 3 → 6** (`PAGE_SOCKETS = 6`, `SETUP_SOCKETS = 2 + PAGE_SOCKETS = 8`): six
   is Chrome's own per-host ceiling, and it covers this burst of probe + preconnects + page.
3. **`PENDING_TIMEOUT` 1 s → 500 ms**: a preconnect that never says anything hands its listener back
   sooner.

**The heap** (`[measured]`, C6 boot): `[HEAP] 32048 → 22168 B free with setup mode's future up`, 9,880
B less — three pairs of 1,460 B buffers are 8,760 B, and the rest is the three extra slots in
`StackResources`. The page's future is still boxed on the heap, so **the static wall did not move**:
`nm` over `_stack_start_cpu0 − _stack_end_cpu0` reads **65,592 B** (65,536 B floor, 56 B left). The S3
reading was not retaken (that board keeps a saved network and does not enter setup mode); the same
difference puts it at 25,560 − 9,880 ≈ 15,680 B.

**Sizes**: `iot:image` puts the S3 at `1,049,120 / 6,291,456 B` (16.68%) and the C6 at
`813,424 / 1,048,576 B` (77.57%).

`[unfinished]` The phone side: the C6 was flashed with this image (boot prints `[HEAP] 22168 B`) and
needs a person to open the page and reload it ten times — the expectation is ten
`Get / answered Ok` lines, no `ConnectionReset` and no `would not be read`; the S3 setup-mode heap
reading belongs to the same pass.

### Submit: the access point outlives the answer by two seconds (`[measured]`, 2026-10-21)

With keep-alive in place a reload was fine, but the **submit** reported `ERR_CONNECTION_RESET` on the
phone. The capture (C6, 44.8 s after a reset) has a device side that did everything right:

```
[WIFI] Post /submit answered Ok, 2469 B of body, in 10 ms
[WIFI] the page named a network, so the link is joining it
[WIFI] a request would not be read after 0 ms: ConnectionReset   ×5
[WIFI] joining the network setup named: "…"
[WIFI] setup mode is offered on the Panel front
```

- No `the answer's socket was still FinWait2 after 100 ms` and no `an answer did not finish`: the
  2,469 B done page went out, the phone acknowledged it, and **the phone's own FIN was in within
  7 ms** — the 100 ms `CLOSE_WAIT` was never used;
- the five `ConnectionReset` lines are not the submit's connection: the same `×5` shape appears after
  the 302 for `generate_204` and after the one for `/mmtls`, and it is Chrome dropping its pooled
  connections as the navigation starts;
- the gap is in the last two lines: **68 ms** between the answer and the access point coming down
  (`…853` → `…921`). An ACK is the phone's kernel holding the bytes, not the browser having drawn
  them — and "the setup network goes away with it" was made true before the done page was on screen.

The change is `LEAVE_GRACE = 2 s`: `page()` waits it out before `JOIN.publish`, so the access point
stays two seconds longer. After publishing it parks on `pending().await`, so a second submit inside
the grace cannot put another credential on the join plane — the link would otherwise be pulled away
by the request that arrived late. The cost is one timer and **the static wall does not move** (`nm`
still 65,592 B); the C6's `[HEAP]` goes 22,168 → 22,048 B (−120 B) and `iot:image-c6` **813,904 /
1,048,576 B** (77.62%), `iot:image-s3` **1,049,568 / 6,291,456 B** (16.68%).

**Accepted** (`[measured]`, phone side and serial):

```
[WIFI] Post /submit answered Ok, 2469 B of body, in 17 ms
[WIFI] the page named a network, so the link is joining it
        ← 2,058 ms
[WIFI] joining the network setup named: "…"
[WIFI] setup mode is offered on the Panel front
[WIFI] phase Connected, address Some([…]), 0 windows without one
```

The done page appeared and the device joined the network. The phone side cannot tell whether those
68 ms failed because the browser had not drawn the page or because Android dropped the
captive-portal navigation; the only thing that changed between the two runs is the two seconds, so it
is recorded as too fast a teardown. The same window has four completed `Get /` answers and a few
`an answer stopped … B short: ConnectionReset` lines — a reload cancels the load still being written,
and the new one is answered as usual (a GET is retried, a POST is not, which is why only submit
showed it).

### Getting back to setup: hold BOOT on the splash page (`[test]`/`[measured]`, 2026-10-21, third cut)

The entry point is unchanged: **both boards draw a 1.5-second splash page at boot, and holding Boot
during it is how setup mode is asked for**. So are the reasons — on a working device the same finger
is the page's own verb (Ambient cycles the mode, Speaker mutes), and the C6 wants a **deliberate
ceremony** (reset, then hold inside the window), not a side door any long press could spend every
three minutes.

What this cut changes is **where that press lands** and **what the radio does during the splash**:

| Thing | Previous cut | This cut |
| --- | --- | --- |
| Long press on the splash | `Toolbar(Right)`: leave the link if there is one, `Invalid` otherwise | `BusinessIntent::RequestSetup`: the request is only **latched** (`setup_wanted`); no page change, no access point |
| The splash closing | back to `boot_page` | asked for and `network_enabled` → **`Network`**; otherwise `boot_page` |
| The link during the splash | read a credential and associate; the hold then tears it down | **no link at all**: `link()` waits on `PAGE.wait_for_splash_end()` before reading a credential |
| After the request | associate, then leave | skip association: `Front::AccessPoint` goes straight into setup mode |
| What decides it | `is_leavable()` on the link | nothing about the link: a press inside the window counts, the same answer on both boards |
| A device with no credentials at boot | enters setup mode by itself and raises the access point | **works offline**: no setup mode, no access point |
| The page the press lands on | the box drawn on top of the splash | the page lands on `Network` first; `link()` raises the access point front afterwards, and nothing is ever stacked on the splash |

The rules:

- **`SPLASH_MS = 1_500`**. Down from 3 s: the long-press threshold is 600 ms, so 1.5 s still fits a
  full hold, while a finger that just left reset can already reach the button; anything longer only
  makes every boot wait. The window is "begin the hold within 1.5 s", not "finish it within 1.5 s".
- **What counts is the press starting inside the window, not the release**: when the clock runs out
  with a finger still down (`InputSource::held()`, including a release still in debounce), the splash
  **waits for the release before closing** (`app/src/input.rs`). A press at 1.4 s released at 2.1 s
  still counts; the clock does not pull the page out from under the finger.
- **One-shot is structural**: `Splash` is not in `CYCLE`, `next_page(Splash)` answers `Ambient`,
  `with_boot_page` refuses it, and the only exit is the input task's `BusinessIntent::SplashOver`
  when the clock runs out (retried on the next tick if the bus is full). No gesture can get back to
  the page once the device has booted.
- **The landing is `boot_page` unless the hold asked**: `with_boot_page()` still only records the
  page the splash lands on (the build input `IOT_BOOT_PAGE`, default `Ambient`; a page the wiring
  cannot back falls back to `Ambient`). `SplashOver` reads `setup_wanted`: asked for and the board
  wired a network means `Network`, anything else means `boot_page`. The splash itself never writes
  `state.page`, so a bench build with `IOT_BOOT_PAGE=network` still lands on the network page.
- **No link is built during the splash** (`[measured]`): the first thing `link()` does is wait on
  `PAGE.wait_for_splash_end()`; until it resolves, neither a credential is read nor the radio
  touched — the splash window produces no `joining`/`leaving` line. When the request is there the
  path goes straight into `Front::AccessPoint`, with no association at all; an "associate, then tear
  down" implementation would leave two lines in that window and put the access point front 2–3 s
  later.
- **No credentials means an offline device**: when `first_saved()` is empty, `browsing()` takes the
  device. It scans only while `PAGE.shows_list()` (the panel sitting on Network), stops when the page
  leaves, and only `JOIN` leads to `hold` — which is where the write happens. No setup mode, no
  access point: an unwrapped device is invisible in a phone's list. The panel page still carries both
  cells, and `AP` is what raises the access point. The C6 has no panel, so configuring it still means
  reset, then hold BOOT on the splash; that is its only way in.
- **The panel's page is resident**: scanning, the list, the keyboard and `JOIN` were split out of the
  setup loop (`setup_credential`), and `link.setup` now means **the page is on the setup face**: the
  panel front raises no access point, the access point front does. On an S3 with no credentials,
  three taps to Network give a live list (`browsing()`) with `SCAN | AP`; tapping `AP` raises it, and
  a row picked sends `JOIN`, which `hold` receives.
- **The ways in remain**: the offline page's `AP` cell, the panel's `LEAVE` on a live link
  (`Hold::Left`) and a saved credential that is refused (`Hold::Refused`) all enter setup mode — each
  is the finger's own doing.
- **The idle clock is always armed**: `page_idle()` no longer takes `armed`; the access point front
  starts 180 s, every page request restarts it, and the exit goes back to the panel front or, on a
  board without one, to the saved network. The old "no screen and nothing saved → no clock" exception
  is gone: a board with no screen gets in only by reset-then-hold, while a board with a panel also has
  the offline page's `AP` cell, and a factory device's open access point should not keep beaconing.
- **A board with a screen draws three lines** (`VANLING` / `HOLD BOOT` / `SETUP`, `stamp_splash` in
  `bsp-esp`), `3 × FONT_H` apart and centred from `width`/`height`. A board without a screen (the C6)
  has no page to look at; the state still moves, and the Boot press latches the same request.

`fast_connect` (new in this cut, `[test]`):

- Every saved network also carries a `LinkHint { bssid: [u8; 6], channel: u8 }` (`NONE` = unknown).
  An association hands it to the driver through `Station::begin_connect_hinted`: `EspStation`
  overrides it with `config_for(credentials, hint)` — `ScanMethod::Fast` plus
  `with_bssid`/`with_channel` — so the driver goes to that channel for that BSSID instead of scanning
  every channel.
- **A failed hint falls back and is re-learned**: when the hinted attempt fails (the driver refuses
  it, or the association times out) the hint is `take()`n and the loop `continue`s into a full scan;
  a successful association writes the new BSSID/channel back from `status().ap` (`learn`). `learn`
  writes only when the value changed — the same network on the same channel costs no flash wear.
- **Against ESP-IDF**: `wifi_sta_config_t` carries the same three facts — `bssid_set`, `bssid`,
  `channel` — plus `scan_method = WIFI_FAST_SCAN`; this repository stores those three next to the
  credential but does not "trust what was remembered": when the access point moves channel, or a
  mesh node with the same name changes BSSID, the fallback makes the hint one attempt rather than a
  hard constraint. That is the half step the reference implementation leaves out.
- **The credential record moves to v2**: `VERSION = 2`, `ENTRY_BYTES = 108` (the hint takes 8 B: six
  for the BSSID, one for the channel, one for alignment). **No v1 compatibility in development**: a
  v1 record reads as `Opened::Blank` (the same as no saved network) and has to be provisioned again;
  `a_version_one_record_is_read_as_nothing` pins that answer.

`[test]`: `the_splash_page_is_where_the_button_asks_for_setup_mode`,
`the_splash_page_answers_nothing_but_the_hold`,
`a_splash_long_press_asks_for_setup_mode_on_every_board`,
`the_splash_lands_on_the_page_that_offers_setup_when_it_was_asked_for`,
`the_splash_lands_on_the_build_page_when_nothing_was_asked`,
`a_board_with_no_radio_lands_on_its_page_even_after_the_hold`,
`a_long_press_after_the_splash_is_the_pages_own_verb`, `the splash's clock` in the state layer's
reachability sweep, and the three on the hint side: `a_hint_survives_a_reset_and_a_retyped_password`
(a changed passphrase keeps the hint), `learning_a_channel_writes_once_and_only_when_it_moves`,
`a_version_one_record_is_read_as_nothing`.

`[measured]` On the bench (2026-10-23, product images, both boards reset once without BOOT held):
the C6 prints `the splash page is over 1500 ms into the input clock` at `00:00:01.848`, followed by
`[WIFI] no panel and no saved network; waiting offline`; the S3 closes it at `00:00:02.752` (its
input clock starts about 1.25 s in), prints `[DISPLAY] main stack 66200 B`, and takes the offline
`browsing` path. Neither board logs `joining`/`leaving` during the splash, and no access point comes
up. (`66200` is the reading after `network` was split into four files
(`network/{mod,link,setup,survey,store}.rs`), 64 B less than before the split; it is the same device
code, only the module boundary moved.)

`[measured]` (2026-10-24, the same split build, both boards flashed once and their serial read):
the S3 prints `[DISPLAY] main stack 66200 B`, exactly what `nm` gives for
`_stack_start_cpu0 − _stack_end_cpu0` (`0x3fce9710 − 0x3fcd9478` = **66,200 B**, floor 65,536 B,
**664 B** of margin), then `[CREDS] no network saved on this device yet` and the offline `browsing`
path; the C6 keeps its `creds` partition across the flash (`[CREDS] 1 saved network(s)`) and
reconnects on reset — `joining` → `associated ... on channel 11` → `Connected` — with the three
green flashes still 100 ms each (starting at `03.622`/`03.856`/`04.056`, a `dark` segment between).
Images: S3 `1,059,888 / 6,291,456 B` (16.85%), C6 `826,976 / 1,048,576 B` (78.87%). The split's own
cost was measured too: folding `network` back into one file on the same tree (keeping the
`tick_notice` wall-clock change) builds the C6 at `826,752 B`, against `826,976 B` split, so the
module boundary costs **+224 B**.

`[measured]` Gates and the static wall: after the splash round, this round's futures grew past the
wall again — **72 B** short. The fix is not cutting a feature but keeping a credential out of every
frame: `hold` now takes `&Credentials`, `Hold::Named` carries `Box<Credentials>`, and the credential
`link` holds is boxed (one allocation per credential, not per window). `nm` reads
`_stack_end_cpu0 = 0x3fcd9658`, `_stack_start_cpu0 − _stack_end_cpu0` = **65,720 B** (floor 65,536,
184 B left), and the main task's `POOL` = 42,704 B. `.cargo/config.toml` carries no
`--noinhibit-exec`: `iot:build-s3` links on its own. Images: S3 `1,057,424 / 6,291,456 B` (16.81%),
C6 `820,384 / 1,048,576 B` (78.24%).

`[not done]`:

- **The hold itself needs a finger**: hold BOOT within 1.5 s of reset, and the landing should be
  `Network` in `Front::AccessPoint`, with no `joining` in the log and `setup mode ... is open`
  instead. Once on each board.
- **The S3's three-line pixels need a person**: the layout is covered by tests (the three lines land
  on the panel, the pitch and the centring) and the 1.5 s timing is measured (closes at
  `00:00:02.748`), but whether the font reads well and whether 1.5 s feels too rushed is a matter of
  eyes.
- **The boot timing after provisioning**: both boards' `creds` are empty right now, so where
  `joining`/`associated` land right after the splash has no reading yet; each needs one provisioning
  pass first.

### The default passphrase depends on the entry; the offline page keeps both cells; the glyph table is Spleen 5×8 (`[test]`/`[measured]`, 2026-10-22)

- **The default passphrase depends on the entry**: `setup_ap_starts_secured(panel, from_splash)`
  decides `secure_on_entry`. The splash entry is open (`OPEN, NO PASSWORD` + `PASS`); the running
  entries — the offline page's `AP`, a `LEAVE`, a refusal, a named build's `LEAVE` — start on WPA2
  with a random 10 characters where there is a panel (`PASS …` + `NO PASS`). Both buttons last for
  one visit; going away and back restores the entry's default. The C6 has no screen, so it is
  always open.
- **The offline page keeps both cells**: `Face::browsing()` reports `browsing: true` and
  `setup: false` (`front` stays `Panel`), so the Network page still offers `SCAN | AP`, but the
  header draws the link's phase rather than `ST SETUP`, and a hold no longer switches fronts.
  `SETUP_ASK` catches `AP` and hands back to `link()`, which enters `unconfigured()` with
  `Front::AccessPoint`; only then does the AP come up. Offline still means no automatic setup mode
  and no AP, and the 180 s idle clock still closes it.
- **Glyphs**: Spleen 5×8 (Frederic Cambus, BSD-2-Clause), natively five columns by eight rows, so
  `m/M/w/W`, `0` and `\\` share one face; `FONT_H` stays 8, `OVERLAY_ROW_GAP` 5, pitch 9×13. The
  licence and regression are in `hardware-constraints.md` under "The panel's glyph table".
- **On the boards** (2026-10-21 product image, both reset once without BOOT held): the S3's splash
  ends at `00:00:03.367`, `[DISPLAY] main stack 65720 B` matches `nm`, and
  `[WIFI] this build was given no network and this device has saved none; it runs offline` follows;
  the C6's splash ends at `00:00:03.346` and is likewise offline with no AP. Both boards' `creds`
  are still empty.
- **Image and wall** (2026-10-22 cut): S3 `1,057,616 / 6,291,456 B` (16.81%), C6
  `821,328 / 1,048,576 B` (78.33%); the `+128 B` / `+64 B` against the previous cut is code layout
  across the entry rule and the reset line, not itemised. `_stack_start_cpu0 − _stack_end_cpu0` is
  **65,688 B** (`0x3fce9710 − 0x3fcd9678`, 152 B left).
- **Not measured**: the normal entry's `[WIFI] setup mode for "vanling-1234" is open, under a
  passphrase` and the splash hold's `..., open` each need one finger; how the table reads at 1× on
  the panel (the counters, the density of `m/w`, the descenders) only a person can answer; and the
  offline page's `SCAN | AP` cells and the `PASS`/`NO PASS` box behind `AP` still need one tap.

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

## A hold outside the splash no longer opens the AP; the C6 keeps its credential and rejoins after reset (`[test]`/`[measured]`, 2026-10-23/24)

Both defects came from the same habit: a boolean standing in for a mode, then a page reading the
approximation.

- **A hold outside the splash opened the AP**: `Face::browsing()` reported the offline list as
  `setup: true`, so `link.setup` stayed true after setup mode ended; the hold gate in `intent.rs`
  reads only `link.setup`, so every hold became `SwitchFront`. The lamp stayed amber because
  `status_light()` only says `SETUP` for `setup && front == AccessPoint` — hence "not in setup
  mode, yet a hold raises the AP".
- **The C6 did not rejoin after reset**: `iot:flash` ran `cargo run`, whose runner is
  `espflash flash --monitor` without `--partition-table`. espflash's default table has no `creds`
  partition, so `Store::open` found nothing on every boot and the device read as "never
  provisioned". The S3's `flash-s3` and `iot:image` both pass the table, which is why only the C6
  was affected.

The fix:

- `LinkStatus` gains `browsing: bool`; `Face::browsing()` reports `setup: false`, `browsing: true`.
  `controls::strip` draws `SCAN | AP` for `browsing`, and the header draws the link's phase rather
  than `ST SETUP`.
- `SwitchFront` accepts `setup || browsing` but still requires `panel.is_wired()`. The hold gate
  stays on `link.setup`, so only the splash's 1.5 seconds and real setup mode answer a hold; the
  offline page enters setup only by an explicit tap on `AP`.
- With no panel and nothing saved, `link()` no longer enters `browsing()`: it waits in place.
  There is no page to browse and no cell to tap, and the splash is the only entry this board
  accepts. Everything else in the C6's link path is the S3's.
- `iot:flash` is now an explicit `espflash flash --chip esp32c6 --flash-size 8mb
  --partition-table partitions/esp32c6-devkitc-1.csv --monitor`, with `moon.yml` saying why
  `cargo run` cannot carry the table.

`[test]`: `a_board_with_no_panel_has_no_front_to_switch_to` (`SwitchFront` is a no-op without a
panel), `an_offline_page_keeps_its_ap_cell_but_a_hold_does_not_raise_one` (the `AP` cell still
works, the hold does not switch), and `controls::tests::the_offline_list_keeps_both_cells`. All
691 `iot-core` tests pass, and `check-s3`, `check-c6` and `check-host` pass.

`[measured]` (2026-10-23, C6, the new `iot:flash`):

- The flashed partition table's third entry is `creds` (`00110000 00002000`), and boot prints
  `[CREDS] no network saved on this device yet` rather than "this board's table has no `creds`
  partition".
- After one network was provisioned and saved, a reset printed `[CREDS] 1 saved network(s)` →
  `joining ...` → `associated ... channel 11`; the first DHCP wait timed out at 30 s, the retry
  associated again and reached `Connected` with an address. Auto-reconnect holds.
- With `creds` erased and the board reset, `waiting offline` follows; a button press after the
  splash produced no `setup mode ... is open` and no AP. The lamp stayed at the unprovisioned
  status colour, because a headless board's status lamp overrides the page light, so lamp colour
  cannot decide this test. The log's `page is now Ambient` shows the press reached the state
  machine; the hold gate itself is covered by `a_board_with_no_panel_has_no_front_to_switch_to`
  and the offline-page test.

`[not done]`: after provisioning the C6 again, a hold on Ambient should visibly cycle the page light
(the status lamp yields), which needs a person's eye. Both boards' links are confirmed working on the
bench (2026-10-24; the S3 is provisioned — **auto-reconnect was not read off the serial this round**).
The C6's `creds` holds one network — the 2026-10-24 reset reconnected (`[CREDS] 1 saved network(s)` →
`joining` → `associated ... channel 11` → `Connected`, see the reading above) — so it is no longer
the erased state.

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
     `BusinessIntent::AskForget(ap)`, opening the `FORGET` confirmation only for a stored row). The
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
8. **Setup mode**: **both boards have walked it on the bench** (the C6 end to end; the S3, after the
   three fixes — "a 338 B allocation per round", "one socket is not enough", "the timeouts and the
   pick order" — 14 `/` and 10 `/scan` with no panic; see "The bench: from link, to phone, with four
   corrections"). On the panel it is now an **explicit gear**: it can be entered (`LEAVE`), left
   (`CANCEL`, offered only while the device keeps a network) and seen (`ST SETUP` plus the
   announcement box — see "Setup mode becomes a gear the panel can see and leave", and this round's
   "Two front ends of setup mode, one at a time"). Several things still have no reading: one real
   reading of an **open** access point (the bench's neighbours all have passphrases — the same item
   as 5⑤), the **headless board's entry** (reset, then hold Boot on the splash page — see the third
   cut of "Getting back to setup: hold BOOT on the splash page"; the previous cut's "a Boot long
   press leaves the link" is gone), and **fingers**: the front-switch cell, the box's two answers,
   and the Boot long press on any page in setup mode. **Whether the device brings setup mode back by
   itself after a wrong passphrase** does have a reading now: 2026-10-24 on the C6's setup page, the
   AP was visible again after the refusal.
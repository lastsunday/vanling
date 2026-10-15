+++
title = "Wi-Fi Implementation Record"
weight = 45
sort_by = "weight"
[extra]
source_file_hash = "cdcd8cc7b00846fc6800dcdae340a0115438a14f"
translated_at = "2026-10-15T15:52:28Z"
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
has not started; the loose ends are at the [bottom](#Still_to_do_([not_done])).

## What the bench said

`[measured, 2026-10-15]` One probe source, built for each part and flashed to each: the
S3 booted with `-a watchdog-reset`, the C6 on a plain reset (why the difference, and the
commands, are in `@/development/iot/flashing.md`). Both joined `whitehell` with build-time
credentials.

| | S3 (LCKFB SZPI) | C6 (DevKitC-1) |
| --- | --- | --- |
| six scan rounds | 0.65–1.0 s each, 1–6 APs | 0.73–0.80 s each, 1–4 APs |
| **association** | **2184 / 2203 / 2189 ms** (three boots) | **3246 / 3261 ms** (two boots) |
| **DHCP** | **546 / 22 / 44 ms** | **31 / 548 ms** |
| link up | 9428–9789 ms after boot | 10325–10995 ms after boot |
| address | `192.168.1.136` | `192.168.1.158` |
| driver's C heap | **32,204 B free of 65,536 B**, unchanged for 40 s | **14,124 B free of 49,152 B** |
| `phase Connected` | steady | steady |

Both boards see the same access points (`whitehell` strongest at −45…−50 dBm, then `2305`,
`CMCC-6yVk`, `888`, `6802`, `94:be:09:98:cf:29`), so what differs between the two columns
is the part, not the room.

esp-radio's documented heap need for a station is **47–57 KiB** with the default
`ControllerConfig` — the high end is the 6 dynamic RX buffers this repository sets — and
32 KiB makes `esp_wifi_init` fail with `OutOfMemory`. Measured, the driver takes 33,332 B
on the S3 and 35,028 B on the C6: **the C6 needs 1,696 B more**, so one source needs two
budgets. See [the section on that](#One_source,_two_parts).

Access points around the bench (union of 12 rounds and 6 more, strongest per row):

| Name / BSSID | Channel | Strongest | Security |
| --- | --- | --- | --- |
| **whitehell** | 11 | **−50 dBm** | secured |
| `15876352517` | 11 | −80 dBm | secured |
| `2305` | 6 | −81 dBm | secured |
| H3C_BC5DB8 | 6 | −82 dBm | secured |
| `94:be:09:98:cf:29` | 1 | −78 dBm | secured |
| `50:5d:7a:db:5e:6a` | 8 | −84 dBm | secured |
| 8249 | 1 | −87 dBm | secured |

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
| S3 `wifi-probe` | 102,872 B | +37,336 B |
| C6 `wifi-probe` | 247,400 B | +181,864 B |
| C6 product `vanling` | 348,124 B | +282,588 B |

The C6's margin is that large for two structural reasons: it has 400 KiB of internal DRAM
(of the S3's 512 KiB, 150 KiB is the camera's frame arena) and this board mounts no panel
and no camera.

S3 product image 497,744 B against a 6 MB partition (7.91%); its probe is 660,992 B
(10.51%), larger because it links the driver's IRAM segments too. The C6 does not need the
6 MB: its probe merges to 597,264 B against 1 MB (56.96%) and its product to 223,584 B
(21.32%) — a RISC-V blob is smaller than an Xtensa one. Wi-Fi's own 14,092 B on the S3 is
the driver's statics: `esp_phy::PHY_STATE` 2,008 B, the blob's `s_wifi_nvs` 1,440 B,
`esp_rtos`'s scheduler 1,416 B, `g_ic`/`g_pm` 760/584 B, `phy_param` 740 B, the rest under
512 B each.

**The product images do not carry that 64/48 KiB driver heap yet**: only the probe calls
`init_radio_heap()`, the product has no `take_wifi()` yet, so the linker drops the regions
as dead code (no `INTERNAL_MEM` / `RECLAIMED_MEM` in the S3 ELF, no `INTERNAL_MEM` in the
C6's, while the driver's own statics are there — 17 symbols on the S3). Once Stage B wires
it up, the 32 KiB `dram_seg` region returns to the S3's budget: 125,788 − 32,768 =
**93,020 B**, still 27,484 B above the floor.

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

### The driver's heap: 32 KiB reclaimed + 32 KiB `dram_seg`

`[measured]` `esp-radio` is a closed blob that allocates through `malloc`, so the symbols
have to exist in the final image. `esp-alloc` provides them; this module is the memory
behind them.

- **The reclaimed region** (above 0x3FCF0000) is DRAM the second-stage bootloader used
  for its flash cache, which esp-hal's linker leaves to the heap. Its size is 64 KiB minus
  the data cache, so **it is empty at the default 64 KiB cache** (the existing ELF in this
  tree has no symbol above 0x3FCF0000). `scripts/iot-xtensa.sh` halves the cache to buy
  32 KiB back.
- **The 32 KiB in `dram_seg`** is affordable only because the app's own heap moved to
  PSRAM (next): that took the main stack from 63,788 B to 125,788 B with 39,809 B in use,
  so this is measured headroom rather than a hope.

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

`[measured]` The S3 has 32 KiB reclaimed plus 32 KiB of `dram_seg`; the C6 has `dram_seg`
only, **48 KiB** (the driver takes 35,028 B, leaving 14,124 B — 28.7% headroom).

Three constraints produce those two numbers:

- **`#[ram(unstable(dcache_reclaimed))]` cannot compile on the C6.** The procmacro feature is
  opened by `esp-hal/esp32s3` alone (it is one line of the `esp32s3 = [...]` group in
  esp-hal's `Cargo.toml`; esp32s2 does not open it either), so the reclaimed block is the
  S3's alone rather than something that was forgotten on the C6.
- **The C6 has no third place.** It has 400 KiB of internal DRAM; with 64 KiB for the app's
  heap and 48 KiB for the driver's, the main stack **measures** 247,400 B — 181,864 B above
  the floor — and the rest is the driver's statics. The S3's 32 KiB came from moving its app
  heap to PSRAM.
- **The headroom is wide on the S3 and tight on the C6, and no amount of tuning closes
  that.** The S3's 49% was bought with PSRAM; 28.7% is what the C6 can offer. **Raise
  `DYNAMIC_RX_BUFFERS` and the C6 is what fails first** — the high end of the documented
  47–57 KiB range is exactly the 6 dynamic buffers this build sets, and the C6's driver
  measured 1,696 B hungrier than the S3's. It is on the list at the bottom.

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

## The pure-logic part (host-testable)

Two rules in `iot-core::drivers::network` are product decisions rather than plumbing, so
they sit where a test can pin them:

- **A scan list is ranked, not raw**: dedupe on BSSID keeping the strongest report, order
  by signal, truncate to 8. Same-name access points are separate networks — a phone shows
  both — so the identity is the BSSID, not the name.
- **An SSID is bytes.** 802.11 allows arbitrary octets and the panel's glyph table is
  printable ASCII, so whether a name can be *drawn* is one decision with a fallback (draw
  the BSSID instead). A non-ASCII name is still carried byte for byte — connecting needs
  it — and only its drawing fails.

`Credentials::from_text` **rejects** an over-long passphrase rather than truncating it,
because the driver rejects it too and a truncated passphrase authenticates against nothing
while reading as "wrong password".

Credentials come from **build-time environment variables** (`IOT_WIFI_SSID` /
`IOT_WIFI_PSK`, read with `option_env!`) rather than a secret store: a bench image should
say which network it joins, and a value compiled in is a value in the build command line.
The product's credentials are typed on the panel and kept in NVS; they must never be
compiled into firmware.

## Still to do (`[not done]`)

1. **The C6's driver heap has 28.7% headroom** (14,124 B of 49,152 B), against the S3's
   49%. Anything that raises `DYNAMIC_RX_BUFFERS` or `STATIC_RX_BUFFERS` breaks the C6 first
   — it has no PSRAM to buy memory with. Either measure the C6 in the same commit that
   changes those constants, or raise the C6's `INTERNAL_BYTES` to 56 KiB (which costs 8 KiB
   of a stack margin still in the hundreds of kilobytes).
2. **Why the camera's DVP DMA can eat a whole scan round**: 1 round in 12, no mechanism
   yet. A page that keeps scanning while the list is empty is fine, so it does not block
   delivery.
3. **The product page has to call `note_address`** — the only route to `Connected`.
   Leaving it out is what kept the panel saying "connecting" while the link was up.
4. **Re-measure** `[INPUT]` and the AEC figures — halving the data cache is a change that
   has already happened.
5. The product page (`DisplayPage::Network`, intents, the on-screen keyboard, credential
   persistence) has not started. It **cannot take one round of scanning as the answer**:
   measured, a single round swings between 0 and 6 APs.
6. `esp-radio` is a beta: a version bump re-runs this whole page, **on both parts** — the
   per-part budgets above came out of what the driver actually took.
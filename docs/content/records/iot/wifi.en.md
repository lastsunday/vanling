+++
title = "Wi-Fi Implementation Record"
weight = 45
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# Wi-Fi Implementation Record

The Wi-Fi link in `apps/iot`, from "which networks are out there" to "the device has
an address". **AI output, not reviewed by a human** — see the
[parent note](@/records/_index.md).

Each judgement carries its basis: **`[test]`** a unit test pins it, **`[measured]`**
a hardware measurement, **`[as-reported]`** not reproduced, left as a trace.

## State

The probe has run on the bench end to end (scan, associate, DHCP). The product page has
not started; the two loose ends are at the [bottom](#still-to-done-not-done).

## What the bench said

`[measured, 2026-10-15]` `wifi-probe` flashed and booted with `-a watchdog-reset`
(commands in `@/development/iot/flashing.md`), joining `whitehell` with build-time
credentials:

| Stage | Time |
| --- | --- |
| six scan rounds | 0.65–1.0 s each |
| **association** | **2184 / 2203 / 2189 ms** (three boots) |
| **DHCP** | **546 / 22 / 44 ms** |
| link up | 9428–9789 ms after boot |
| address | `192.168.1.136` |
| driver's C heap | **32,204 B free of 65,536 B**, unchanged for 40 s |

esp-radio's documented heap need for a station is **47–57 KiB** with the default
`ControllerConfig`: 32 KiB makes `esp_wifi_init` fail with `OutOfMemory`, 64 KiB works,
leaving about 49% headroom.

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

At link time (`[measured]`, from link symbols):

| | main stack | against the 65,536 B assertion |
| --- | --- | --- |
| product, no Wi-Fi | 77,880 B | +12,344 B |
| product, Wi-Fi, app heap still in `dram_seg` | 63,788 B | **−1,748 B, does not link** |
| product, Wi-Fi, app heap in PSRAM | **125,788 B** | +60,252 B |
| `wifi-probe` | 102,872 B | +37,336 B |

Product image 497,744 B against a 6 MB partition (7.91%); the probe is 660,992 B
(10.51%), larger because it links the driver's IRAM segments too. Wi-Fi's own 14,092 B
is the driver's statics: `esp_phy::PHY_STATE` 2,008 B, the blob's `s_wifi_nvs` 1,440 B,
`esp_rtos`'s scheduler 1,416 B, `g_ic`/`g_pm` 760/584 B, `phy_param` 740 B, the rest under
512 B each.

**The product image does not carry that 64 KiB heap yet**: only the probe calls
`init_radio_heap()`, the product has no `take_wifi()` yet, so the linker drops both
regions as dead code (no `INTERNAL_MEM` / `RECLAIMED_MEM` in the `vanling` ELF, while the
driver's own statics are there — 17 symbols). Once Stage B wires it up, the 32 KiB
`dram_seg` region returns to the budget: 125,788 − 32,768 = **93,020 B**, still 27,484 B
above the floor.

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

## Memory: three decisions

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

### The app's heap: PSRAM, because there is no third place

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

### `wifi_force_pd`: doing the `init()` esp-radio never calls

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
stays: it is where the driver writes its calibration on the first boot.

The tables are named after the **board**: `scripts/iot-image.sh` has always looked for
`partitions/<board>.csv`, and the layout is a board fact anyway — another S3 board with an
8 MB part is another table. A 6 MB slot does not fit inside espflash's assumed 4 MB, so
every S3 flash and image task passes `--flash-size 16mb`. The C6 table is equivalent to
`espflash`'s default but is a file too: the image task and the flash task then have exactly
one place that knows the layout.

## Features and where each layer sits

`wifi` is a **component feature (module axis)**: one file, one module in `iot-bsp-esp`,
gated once in `components/mod.rs`, aggregated into the S3 board feature. `iot-core` has no
cfgs — the capability arrives through the composition point (`HasWifi`).

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

1. **Why the camera's DVP DMA can eat a whole scan round**: 1 round in 12, no mechanism
   yet. A page that keeps scanning while the list is empty is fine, so it does not block
   delivery.
2. **The product page has to call `note_address`** — the only route to `Connected`.
   Leaving it out is what kept the panel saying "connecting" while the link was up.
3. **Re-measure** `[INPUT]` and the AEC figures — halving the data cache is a change that
   has already happened.
4. The product page (`DisplayPage::Network`, intents, the on-screen keyboard, credential
   persistence) has not started. It **cannot take one round of scanning as the answer**:
   measured, a single round swings between 0 and 6 APs.
5. `esp-radio` is a beta: a version bump re-runs this whole page.
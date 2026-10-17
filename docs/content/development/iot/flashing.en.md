+++
title = "Firmware Installation"
weight = 30
[extra]
source_file_hash = "2d4458c4f9357042b46534636b159164b670ee65"
translated_at = "2026-10-16T16:46:22Z"
+++

# Firmware Installation

This page explains how to install vanling firmware onto a development board. A [browser one-click installer](../../../../flasher/index.html) is also available.

## Prerequisites

- A supported dev board (esp32c6: `esp32c6-devkitc-1`; esp32s3: `lckfb-szpi-esp32s3`)
- A firmware artifact. Release artifacts (incl. `merged.bin`) come from the CD release (`tag vanling-iot@x.y.z`); for local builds:
  ```sh
  # under apps/iot
  moon run build
  espflash save-image --chip esp32c6 --merge \
    --flash-size 8mb --flash-mode dio --flash-freq 40mhz \
    target/riscv32imac-unknown-none-elf/debug/vanling \
    vanling-merged.bin
  ```
  > **8mb is the measured value** (`espflash board-info`; the part really is 8 MB). The `4mb` written here before did not match the board.
  > Measured for both boards: **C6 = 8 MB**, **S3 = 16 MB** (the `Flash size` line of `espflash board-info`).

## What is on the bench, and what each board is for

Do not assume there is no hardware — two boards stay plugged in. **Port names change on every replug and cannot be used as identifiers.**

| Board | Repository feature | Role |
|---|---|---|
| LCKFB Shizhan ESP32-S3 | `lckfb-szpi-esp32s3` | **Product target**: 320×240 panel, FT6336 touch, two mics and a speaker, QMI8658, PCA9557, GC2145 camera |
| ESP32-C6-DevKitC-1 | `esp32c6-devkitc-1` | **Smoke/emulation target**: a WS2812 and the boot key, nothing else. `UnwiredSpeaker`, the panel and the Audio page are all `None`, so **the product path does not exist on it**; the two are not interchangeable |

Measured hardware facts (conclusions and reproduction in the [development records](@/records/iot/_index.md)):

- **The camera is a GC2145, not the GC0308 the LCKFB wiki names** (`sensor id 0x2145`, read window 1616×1208 @5:5)
- **AEC's delay locks; the amount cancelled still has no reading**: the delay search moved exactly once across two measurements, while the firmware's ERLE meter has never returned a non-zero number on hardware — the one 12.3 dB came from an offline dump that has since been deleted and cannot be reproduced

## With two boards plugged in

`espflash` picks a port on its own and **can pick the wrong one** (`--chip esp32c6` against the S3 fails outright). Identify, then flash — what identifies is the `Chip` line of `board-info`:

```sh
espflash board-info --port /dev/cu.usbmodem14401   # Chip: esp32c6 → C6
espflash board-info --port /dev/cu.usbmodem1422301 # Chip: esp32s3 → S3

IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-s3
IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-probe-echo-s3
```

With `IOT_PORT` unset the tasks behave as they did before (`espflash` chooses), but it must be set while both boards are attached.

## Local artifact generation

The artifact pipeline runs equivalently on GitHub Actions (`reusable-iot-build.yml`) and locally, so a failed Action can be substituted by generating artifacts locally and uploading them to the release. One-command generation (under `apps/iot`):

```sh
moon run iot:image           # all boards: ELF + merged.bin
moon run iot:image-c6        # esp32c6-devkitc-1 only
moon run iot:image-s3        # lckfb-szpi-esp32s3 only
```

> s3 builds dispatch via `scripts/iot-xtensa.sh`: a fully installed espup `esp` 1.95.0.0 toolchain (CI/release) compiles natively; otherwise the `espressif/idf-rust:esp32s3_1.95.0.0` container is used locally (macOS Intel). Both use the same toolchain version, so artifacts are byte-identical.

> Dependency cache: container builds resolve `-Z build-std` against the esp toolchain's own `library/Cargo.lock` (e.g. memchr 2.7.6, which differs from the project `Cargo.lock`). When the offline cache is incomplete, the first build boots online once automatically (see `bootstrapping once online`); later builds stay `--offline` and byte-identical, so no manual pre-warming is needed.

`iot:image` pulls the per-board `build-*` tasks and writes artifacts to the repo-root `dist/`, with names identical to the CI ones:

| Artifact | Name                                         |
| -------- | -------------------------------------------- |
| ELF      | `vanling-iot-<board>-<version>-<target>.elf` |
| merged   | `vanling-iot-<board>-<version>-merged.bin`   |

`version` comes from `scripts/version.sh` (reading `apps/iot/Cargo.toml`), the same source as the CI `DEV_VERSION` (`iot-dev-release.yml`); locally it is `x.y.z-dev.<run>.<date>.<sha>`.

## Single-device installation

### 1. Browser flashing (recommended)

Open [Vanling Firmware Installer](../../../../flasher/index.html) (desktop Chrome / Edge):
1. Drop in `merged.bin`
2. Verify the parsed target chip and version info
3. Connect the board over USB
4. Click “Connect device”, confirm chip match, then click “Write firmware”

Browser flashing uses WebSerial + esptool-js; no local tools needed.

### 2. ESP Flash Download Tool (GUI)

- Windows: official [ESP Flash Download Tool](https://www.espressif.com/en/support/download/other-tools)
- Address matches `save-image --merge`: write the whole image at `0x0`
- Pick matching SPI settings (4MB / DIO / 40MHz)

### 3. esptool CLI

```sh
python -m esptool --chip esp32c6 write_flash 0x0 vanling-merged.bin
```

`merged.bin` is already a whole-flash image; write a single address `0x0`.

## Batch installation

- Run esptool in parallel per board (multiple USB ports on one PC):
  ```sh
  for p in /dev/ttyUSB{0..3}; do
    python -m esptool --port "$p" --baud 921600 write_flash 0x0 vanling-merged.bin &
  done; wait
  ```
- Factory mode: flashers with CRC32/data verification can be wired into a fixture pipeline; esptool ships `mass_mfg` (read-only factory-image batch template) with MAC/serial variables.

## Development flashing

```sh
# under apps/iot, compile and flash (espflash is in devShell)
moon run flash
# or manual
cargo build && espflash flash target/.../vanling
espflash monitor   # press enter in console to restart
```

`espflash monitor` needs a real tty (picocom cannot make sense of its own arguments on this machine), and macOS's `script -q <log> <cmd>` hands it one while recording a transcript — so wrap the monitor in `script`.

> `[measured, corrected 2026-10-15]` **Do not ask anyone to press RESET, and do not "attach
> right after flashing"** (the second was my own earlier conclusion and it is wrong).
>
> This board runs **native USB** (USB-Serial/JTAG), whose DTR/RTS is a *core reset*, and a
> core reset **does not re-sample the boot strapping pins** — so every software reset lands
> on `boot:0x23 (DOWNLOAD(USB/UART0))` and the application never starts after a flash.
> esptool's ESP32-S3 documentation (*Leaving Download Mode in USB-Serial/JTAG Mode*) names
> the way out: `-a watchdog-reset`, which hijacks the RTC WDT to force a real system
> reset, so the strapping pins are re-sampled and the chip boots normally (the ROM prints
> `boot:0x2b (SPI_FAST_FLASH_BOOT)`).
>
> Capturing the whole boot log — attach first, then reset, so nothing is missed:
>
> ```sh
> espflash flash --chip esp32s3 --flash-size 16mb \
>   --partition-table partitions/lckfb-szpi-esp32s3.csv \
>   --port /dev/cu.usbmodem1422301 -a watchdog-reset target/.../wifi-probe
>
> ({ sleep 4; printf '\\x12'; sleep 30; } | script -q -t 35 /tmp/boot.log \
>    espflash monitor --chip esp32s3 --port /dev/cu.usbmodem1422301 -a no-reset) >/dev/null 2>&1
> ```
>
> That `printf '\\x12'` is the monitor's `CTRL+R Reset chip` fed into the same pty: attached
> and reset in one breath. It lands a few seconds later than the flash's own reset, so the
> ROM's first line is in the transcript too. `script` hands the monitor the tty it demands
> and records a copy; redirect rather than piping.

`[measured]` **The C6 is the other way round**: it does **not** want `-a watchdog-reset` — a
plain DTR reset boots it, and the ROM prints `rst:0x15 (USB_UART_HPSYS),boot:0x1c
(SPI_FAST_FLASH_BOOT)`. espflash refuses the option outright (`The esp32c6 does not support
watchdog reset`), so no `flash-probe-*-c6` task may carry it: a task that did would write the
image and then fail on its last step.

## When a build fails, look at Docker first

S3 builds go through `scripts/iot-xtensa.sh`: it tries the machine's esp toolchain first and falls back to a container when that one is incomplete. The local toolchain usually *is* incomplete — `~/.espressif` has `xtensa-esp-elf`'s gcc but neither `bin/cargo` nor `lib/rustlib/src/rust/library`, so `has_native_esp` says no — which means **building depends on the Docker daemon being up.**

> `[measured]` When the daemon is down the message is `docker: request returned 500 Internal Server Error ... /docker.sock/_ping` with exit code 125, and **not** any compile error. `open -a Docker` does not start it from a shell with no GUI session; run `/Applications/Docker.app/Contents/MacOS/Docker` directly and wait for `com.docker.backend`.
>
> The dangerous part is that **the stale binary survives a failed build**, so `espflash flash` cheerfully writes the previous image to the board. Filtering the output through `grep '^error'` hides that completely, which cost three rounds of measuring nothing. **Before every flash, confirm the artefact is new with `strings <elf> | grep <a log literal only the new code emits>`** — never trust the exit code alone.

## Boot page and the bench image

`IOT_BOOT_PAGE` (`.cargo/config.toml`, default `ambient`) picks the page the firmware comes up on — `ambient` / `attitude` / `audio` / `speaker` / `camera` / `network`, with an unrecognised name falling back to `ambient`. It is an `option_env!`, expanded at **compile** time, so a Docker build has to forward it into the container (`FORWARDED_ENV` in `scripts/iot-xtensa.sh`); `moon run iot:build-s3-camera` sets it to `camera`, paired with `moon run iot:flash-s3-camera`. `network` has no moon task of its own; the same command by hand:

```sh
cd apps/iot
IOT_BOOT_PAGE=network bash ../../scripts/iot-xtensa.sh build --release \
  -p iot-app --bin vanling --target xtensa-esp32s3-none-elf -Z build-std=core,alloc \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto
```

`[measured]` That build carries **no build-time credentials**, so the device sits in `setup` and keeps scanning; `moon run iot:flash-s3` rebuilds the product image and overwrites `target/xtensa-esp32s3-none-elf/release/vanling` again. The boot log carries `[IOT] boot page is Network` (nothing is logged when it stays on `ambient`), so which page this image really starts on is not a guess.

> `[measured]` Those two tasks **must not** share one artifact path. `build-s3-camera` and `build-s3` both used to declare `target/.../release/vanling`, and `flash-s3` and `scripts/iot-image.sh` both read that file — so whichever ran last decided what got flashed, and a cached task did not put its own back, meaning "flash the product firmware" flashed the camera firmware. The bench build now saves and restores the product image around the build and keeps a separate `vanling-camera`, so both images coexist.

> `[measured]` `option_env!` is tracked by cargo's fingerprint: changing only `IOT_BOOT_PAGE` does rebuild and the two artifacts differ byte-for-byte. The problem above was therefore the shared path alone, not a cache key — no `touch` of the source is needed.

## Firmware layout

Release `merged.bin` (`save-image --merge`) is a whole-flash image:

| Segment    | Address    | Description                |
| ---------- | ---------- | -------------------------- |
| bootloader | `0x0`      | 2nd-stage bootloader       |
| partition  | `0x8000`   | partition table            |
| app        | see table  | vanling app + data         |

> The layout follows the partition table used when producing the bin; flash the whole image at `0x0`, no need to write each segment separately.

**The partition table comes from this repository**: `apps/iot/partitions/<board>.csv`, passed
explicitly as `--partition-table` by both `flash-*` and `scripts/iot-image.sh`. Both must name
the same file — with a table written down in two places, whichever task ran last decides what
the device gets, and neither side fails to compile.

The S3 table departs from the default in two places, each for a reason spelled out in the file
itself and in the [Wi-Fi record](@/records/iot/wifi.en.md):

- **factory 6 MB** (`espflash` defaults to 1 MB): the firmware links the Wi-Fi driver's prebuilt
  blobs and does not fit in 1 MB. The S3 part has 16 MB.
- **no `phy_init` partition**: esp-radio's Xtensa build implements the driver's calibration
  restore hook as `todo!()`, and the driver looks for a calibration partition; without one the
  path is not taken, at the cost of calibrating at every boot.

## How browser flashing works

The installer page is fully static (hosted on GitHub Pages). `docs/static/flasher/` contains:

- `index.html` + `main.js`: parse the image header client-side (magic `0xE9`, chip ID, flash params, segment table, XOR checksum, app descriptor `0xABCD5432`).
- `vendor/esptool-js@0.6.0.bundle.js`: Espressif's official WebSerial flashing core (Apache-2.0).

Independent of OTA (network upgrade): the installer covers first-time flashing and full restore; OTA covers later incremental updates.
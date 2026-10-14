+++
title = "Firmware Installation"
weight = 30
[extra]
source_file_hash = "90ca21991d90c29a9d3c317e381e761214a740c4"
translated_at = "2026-10-14T15:19:21Z"
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
  > Measured for both boards: **C6 = 8 MB (MAC `98:a3:16:7e:55:08`)**, **S3 = 16 MB (MAC `30:ed:a0:a8:2f:c8`)**.

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

`espflash` picks a port on its own and **can pick the wrong one** (`--chip esp32c6` against the S3 fails outright). Identify, then flash:

```sh
espflash board-info --port /dev/cu.usbmodem14401   # C6 → 98:a3:16:7e:55:08
espflash board-info --port /dev/cu.usbmodem1422301 # S3 → 30:ed:a0:a8:2f:c8

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

`espflash monitor` needs a real tty (picocom cannot make sense of its own arguments on this machine). Native USB does not support DTR/RTS, so **attach immediately after flashing** to catch the ROM's own boot; redirect with `> /tmp/x.log` rather than piping.

## When a build fails, look at Docker first

S3 builds go through `scripts/iot-xtensa.sh`: it tries the machine's esp toolchain first and falls back to a container when that one is incomplete. The local toolchain usually *is* incomplete — `~/.espressif` has `xtensa-esp-elf`'s gcc but neither `bin/cargo` nor `lib/rustlib/src/rust/library`, so `has_native_esp` says no — which means **building depends on the Docker daemon being up.**

> `[measured]` When the daemon is down the message is `docker: request returned 500 Internal Server Error ... /docker.sock/_ping` with exit code 125, and **not** any compile error. `open -a Docker` does not start it from a shell with no GUI session; run `/Applications/Docker.app/Contents/MacOS/Docker` directly and wait for `com.docker.backend`.
>
> The dangerous part is that **the stale binary survives a failed build**, so `espflash flash` cheerfully writes the previous image to the board. Filtering the output through `grep '^error'` hides that completely, which cost three rounds of measuring nothing. **Before every flash, confirm the artefact is new with `strings <elf> | grep <a log literal only the new code emits>`** — never trust the exit code alone.

## Boot page and the bench image

`IOT_BOOT_PAGE` (`.cargo/config.toml`, default `ambient`) picks the page the firmware comes up on — `ambient` / `attitude` / `audio` / `speaker` / `camera`, with an unrecognised name falling back to `ambient`. It is an `option_env!`, expanded at **compile** time, so a Docker build has to forward it into the container (`FORWARDED_ENV` in `scripts/iot-xtensa.sh`); `moon run iot:build-s3-camera` sets it to `camera`, paired with `moon run iot:flash-s3-camera`.

> `[measured]` Those two tasks **must not** share one artifact path. `build-s3-camera` and `build-s3` both used to declare `target/.../release/vanling`, and `flash-s3` and `scripts/iot-image.sh` both read that file — so whichever ran last decided what got flashed, and a cached task did not put its own back, meaning "flash the product firmware" flashed the camera firmware. The bench build now saves and restores the product image around the build and keeps a separate `vanling-camera`, so both images coexist.

> `[measured]` `option_env!` is tracked by cargo's fingerprint: changing only `IOT_BOOT_PAGE` does rebuild and the two artifacts differ byte-for-byte. The problem above was therefore the shared path alone, not a cache key — no `touch` of the source is needed.

## Firmware layout

Release `merged.bin` (`save-image --merge`) is a whole-flash image:

| Segment    | Address    | Description                |
| ---------- | ---------- | -------------------------- |
| bootloader | `0x0`      | 2nd-stage bootloader       |
| partition  | `0x8000`   | partition table            |
| app        | `0x10000`  | vanling app + data         |

> The layout follows the partition table used when producing the bin; flash the whole image at `0x0`, no need to write each segment separately.

## How browser flashing works

The installer page is fully static (hosted on GitHub Pages). `docs/static/flasher/` contains:

- `index.html` + `main.js`: parse the image header client-side (magic `0xE9`, chip ID, flash params, segment table, XOR checksum, app descriptor `0xABCD5432`).
- `vendor/esptool-js@0.6.0.bundle.js`: Espressif's official WebSerial flashing core (Apache-2.0).

Independent of OTA (network upgrade): the installer covers first-time flashing and full restore; OTA covers later incremental updates.
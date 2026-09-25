+++
title = "Firmware Installation"
weight = 30
[extra]
source_file_hash = "10f57b94a030435c8eba595f4cecbff89e5a715a"
translated_at = "2026-09-23T04:30:00Z"
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
    --flash-size 4mb --flash-mode dio --flash-freq 40mhz \
    target/riscv32imac-unknown-none-elf/debug/vanling \
    vanling-merged.bin
  ```

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
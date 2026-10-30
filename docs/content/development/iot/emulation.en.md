+++
title = "Hardware-free Emulation and Regression Smoke"
weight = 40
[extra]
source_file_hash = "ef56169a681ea9fb1b9ef4b1a48689387280fe57"
translated_at = "2026-10-24T15:16:46Z"
+++

# Hardware-free Emulation and Regression Smoke

Hardware-free CI regression smoke testing works on two layers, prioritizing behavior coverage with optional silicon-layer smoke:

- **Host harness** (mainline): iot-app runs on the host machine with the std backend; a fake `HostBoard` drives the real `run<B>` pipeline and asserts intent → state → render. C6 and S3 share the same logic, so behavior coverage and architecture consistency are validated here.
- **esp-emu silicon smoke** (optional enhancement, esp32c6 only): the real firmware image boots in the esp-emu emulator up to `[IOT] boot complete`. esp-emu does not support Xtensa, so S3 behavior relies on the host harness and the driver loop relies on real hardware.

Real-device HIL (espflash flashing + on-device smoke) stays on the roadmap (P1) and is not a CI gate.

## Decision matrix

| Approach                | Chip support        | Board fidelity | CI cost | Maintenance | Behavior coverage | esp-hal integration effort | Decision      |
| ----------------------- | ------------------- | -------------- | ------- | ----------- | ----------------- | -------------------------- | ------------- |
| Real-device HIL         | all (as boards held)| complete       | high    | high        | high              | none                       | roadmap P1    |
| Host harness (this)     | all (logic layer)   | no peripherals | low     | low         | high              | none (std backend)         | **mainline CI** |
| esp-emu                 | esp32c6 etc.        | fairly complete| medium  | medium      | medium            | runs firmware directly     | **C6 enhancement** |
| QEMU (espressif fork)   | esp32c3/c6/s3 etc.  | fairly complete| medium  | medium      | medium            | runs firmware directly     | not chosen: esp-emu fits better |
| Wokwi                   | multiple            | component-level| medium  | medium      | low               | requires firmware changes  | not chosen: requires changes |
| Renode                  | multiple vendors    | board-level    | medium  | high        | medium            | requires peripheral modeling | not chosen: high modeling cost |

Rationale:

- **Host harness as mainline**: the logic layer has zero esp-hal dependencies (`iot-core`'s `Board`/`HasSurface`/`HasInput` are pure traits), and `input_task`/`run_loop`/`run<B>` are all host-clean; the only ESP coupling is at the binary entry. Running with std features of existing dependencies adds no new crate and no peripheral timing, making it the most stable CI regression net.
- **esp-emu as supplement**: it boots real merged firmware images directly (ROM → 2nd-stage bootloader → partition → app → embassy executor), validating the chip boot chain rather than business logic; esp-emu is Espressif-official and fits the esp-hal ecosystem.
- Other options either require firmware changes (Wokwi), high peripheral modeling cost (Renode), or are less aligned with the esp-hal ecosystem than esp-emu (the QEMU fork is more complete, but redundant for a boot-only smoke).

## Usage

Smoke tasks are orchestrated via moon, equally runnable in CI (`reusable-iot-build.yml`) and locally:

```sh
moon run iot:smoke-host      # host harness: intent → state → render behavior assertions
moon run iot:emu-smoke       # C6 silicon: esp-emu boots merged image to [IOT] boot complete
```

- `smoke-host` depends on `check-host` (`cargo check --no-default-features --features host`); `emu-smoke` depends on `build-c6-emu`.
- `iot:check`/`iot:build` compile device firmware with default features (including `esp32c6-devkitc-1` + `console-auto`); `check-s3`/`build-s3` use `lckfb-szpi-esp32s3,console-auto`.

### Scripts and artifacts

| Script                  | Purpose                                                         | Artifact                                          |
| ----------------------- | --------------------------------------------------------------- | ------------------------------------------------- |
| `scripts/iot-emu.sh`    | Caches esp-emu (pinned v0.42.0), selects merged.bin, asserts via `--exit-on` | —                                                 |
| `scripts/iot-emu-image.sh` | Isolated build with `CARGO_TARGET_DIR=target/emu` + `espflash save-image --merge` | `dist/vanling-iot-esp32c6-devkitc-emu-merged.bin` |

- The isolated `target/emu/` build does **not** clobber the default C6 build (`image-c6`/flashing paths).
- esp-emu runs natively on Linux (CI); macOS (Intel) automatically goes through docker `linux/amd64`.
- Artifact naming aligns with CI; `dist/` is gitignored.

## Console (UART) constraint

- esp-emu only scans UART0 for `--exit-on`, so the **C6 smoke build must use `console-uart`**; the device default `console-auto` (USB-Serial/JTAG adaptive) stays unchanged.
- Split into two iot-app features: `console-auto` = device production default (old behavior), `console-uart` = esp-emu smoke only. `esp-println`'s `auto`/`uart` are mutually exclusive, so the features inherently cannot coexist.
- The host harness uses the std backend and needs no console; it is not bound by this constraint.

## Limitations

- esp-emu does not support Xtensa/esp32s3: native S3 behavior is covered by the host harness; the driver loop requires real hardware.
- The host harness does not model real peripheral timing (RMT, flash, Wi-Fi, etc.), covering only the logic composition layer.
- esp-emu's QEMU layer is usually run against release-optimized firmware (time-sensitive); the dev profile serves as compile-only check.
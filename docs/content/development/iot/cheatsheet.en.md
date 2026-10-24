+++
title = "Cheat Sheet"
weight = 5
[extra]
source_file_hash = "abf8d7253bbce8ee202ab24b402a3d21800b9f99"
translated_at = "2026-10-24T00:00:00Z"
+++

# Cheat Sheet

One page of copy-paste. Why things are this way, and the measured numbers, live in the pages linked at the bottom.

## Daily loop

Under `apps/iot`, or from anywhere in the repository with `moon run iot:<task>`:

| Goal | Command |
| --- | --- |
| Check S3 after a change (type-check only, no link) | `moon run iot:check-s3` |
| Check C6 | `moon run iot:check-c6` |
| Host tests (core / board / recipes) | `moon run iot:test`, `iot:test-bsp`, `iot:test-tool` |
| No-board smoke | `moon run iot:smoke-host` (hits the cache; replay with `--force`), `iot:emu-smoke` |
| Build + flash (C6 smoke target, enters monitor afterwards) | `moon run iot:flash` |
| Build + flash (S3 product target) | `moon run iot:flash-s3` |
| Build only | `moon run iot:build-s3` |
| Release artifacts into `dist/` | `moon run iot:image` |
| Every task name | `moon query tasks --project iot \| jq -r '.tasks.iot\|keys[]'` |

## Boards and ports

Two boards sit on the bench permanently and are not interchangeable. **The port name changes on every replug — it is not an identifier.**

| Board | Feature | Reset |
| --- | --- | --- |
| LCKFB ESP32-S3 (product target, 16 MB) | `lckfb-szpi-esp32s3` | Native USB: flashing needs `-a watchdog-reset`; stuck in the downloader means `moon run iot:reset-s3` |
| ESP32-C6-DevKitC-1 (smoke target, 8 MB) | `esp32c6-devkitc-1` | A plain DTR reset boots it; espflash refuses `watchdog-reset` outright |

```sh
espflash list-ports                                    # what is plugged in
espflash board-info --port /dev/cu.usbmodem1422301     # Chip type / Flash size / MAC address
IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-s3 # mandatory when both boards are plugged in
```

## Variables that change the firmware (build-time — rebuild after changing one)

| Variable | Effect | When unset |
| --- | --- | --- |
| `IOT_BOOT_PAGE` | Which page the device comes up on | `ambient` |
| `IOT_DEMAND` | Which data planes, at what cadence | The firmware's defaults |
| `IOT_LOG_LEVEL` | How much the serial port prints: `error` / `warn` / `info` / `debug` / `trace` | `info` |
| `IOT_WIFI_SSID` / `IOT_WIFI_PSK` | Credentials compiled in | None; the device stays on the setup page |
| `IOT_PANEL_BENCH` | Force the unconfigured state so a password has to be typed on the panel | None |

`IOT_PORT` is not one of these: it only tells espflash where to write, and never enters the firmware.

## Logging

The product defaults to `info`: the boot inventory, each state transition and each user action are one line, and **a steady state may print nothing at all** — a quiet port is normal, not a hang. Periodic telemetry (frame cost and stack peak, feed cadence, link phase and address, scan lists) is all `debug`:

```sh
IOT_LOG_LEVEL=debug IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-s3
```

The `[DISPLAY] stack peak …`, `[PLAY] … feeds …` and `[WIFI] … address …` lines a record quotes only appear per window in a `debug` image; rebuild with the line above to reproduce one. `warn` is reserved for a bound actually crossed (a single executor hold over 30 ms, a frame write over 20 ms or 25% duty, a late feed or under 95% of the nominal rate), so **any warn line is worth reading**. The first line, `[IOT] log level …`, is the level this image really carries — check it before suspecting a missing log.

## Before flashing

- A failed build **leaves the previous binary in place**, and its timestamp says nothing. Add a log line, then `strings <elf> | grep <literal>` — only a match proves the artifact is new.
- `iot:flash` runs `--monitor` in its script, so it keeps the terminal after flashing. Without a tty it ends with `Failed to initialize input reader`; the image is already written.
- S3 builds go through Docker; a `docker.sock/_ping` 500 means the daemon is down, not a compile error.
- `moon` will not re-run a cached task; `moon run --force <task>` is what actually re-runs it.

## Symptom → first check

| Symptom | Look at |
| --- | --- |
| Flashed but does not start, log shows `boot:0x23 (DOWNLOAD(USB/UART0))` | The S3 flash is missing `-a watchdog-reset` |
| Not a single log line | The console channel (`console-auto` / `console-uart`); monitor needs a real tty, so wrap it in `script` |
| The device does not match the code | Stale artifact; check the literal with `strings` |
| Link-time main-stack assertion | `.bss` grew; `check-s3` does not link, so it cannot see this |
| A probe runs, but the product no longer knows its saved network | The probe flash tasks pass no `--partition-table`, so the device gets espflash's default table; reflash the product image |

## Handy commands

```sh
# Live log: read the port directly, not through the flash stub
timeout 5 cat /dev/cu.usbmodem1422301

# Full boot log: monitor needs a pty, and CTRL+R (0x12) only counts once it has connected.
# The scripted recipe is in flashing.md.
({ sleep 6; printf '\x12'; sleep 10; } | script -q -t 1 /tmp/boot.log \
  espflash monitor --chip esp32s3 --port /dev/cu.usbmodem1422301 -a no-reset) >/dev/null 2>&1

# Forget the saved network without reflashing; espflash does not read the device's table, so pass the csv
espflash erase-parts --chip esp32s3 --port /dev/cu.usbmodem1422301 \
  --partition-table partitions/lckfb-szpi-esp32s3.csv creds

# Any cargo subcommand on the S3 target has to go through this dispatcher
bash ../../scripts/iot-xtensa.sh tree -p iot-app --target xtensa-esp32s3-none-elf \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto

# One test, with its println output
cargo test -p iot-core <test-name> -- --nocapture

# Total main-task stack = the difference between these two symbols (what esp-rtos uses)
nm <elf> | grep -E '_stack_(start|end)_cpu0'

# Changed a feature in Cargo.toml? Build every subset on its own
moon run iot:check-features

# Run the pre-commit hooks without committing (`--force` keeps them from skipping)
lefthook run pre-commit --force
```

## What the checks do not cover

- `check-s3` only type-checks: running out of memory shows up at `build-s3` / `iot:image`, not here.
- Build-time variables are not part of moon's cache key; `build-s3` sets `cache: false`, the other tasks have no such protection.
- Whether each feature compiles on its own is only answered by `check-features`.
- The log level is pinned to `Info` (`set_max_level` in `chip-esp/src/esp32c6.rs` and `esp32s3.rs`), so `log::debug!` prints nothing.
- **There is no gdb / openocd**: debugging means logs plus one probe per peripheral (`probe-*-s3`).
- `apps/iot/target` grows into the tens of GB; `cd apps/iot && cargo clean` costs a rebuild, and the S3 half costs the most.

## Full details

- [Layering and composition](@/development/iot/architecture.en.md), [feature criteria](@/development/iot/features.en.md)
- [Firmware installation](@/development/iot/flashing.en.md) — artifacts, partition tables, bench images, the full boot-log recipe
- [Hardware-free emulation](@/development/iot/emulation.en.md)
- [Motion](@/development/iot/motion.en.md), [audio metering](@/development/iot/audio.en.md), [playback](@/development/iot/playback.en.md)
- [Implementation records](@/records/iot/_index.en.md) — why it is this way and the measured numbers (AI output, not human-reviewed)

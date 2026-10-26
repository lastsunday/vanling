+++
title = "Cheat Sheet"
weight = 22
[extra]
source_file_hash = "523c655bab2cbf2358d40566597950cdaec0377d"
translated_at = "2026-10-26T00:00:00Z"
+++

# Cheat Sheet

This page is the command entry point for every project in the repository: the common workflows first (multi-step, meant to be pasted and run in order), then the individual commands per project, followed by the firmware sections, the impact of each kind of change, where to start on a failure symptom, and what the checks do not cover.

Only commands and constraints are recorded here. Why things are this way, and the measured numbers, are in the pages linked at the end.

## Contents {#contents}

- [Conventions](#conventions): argument forwarding, caching, and the boundary of the task list
- [Common workflows](#workflows): 12 multi-step sequences, each with prerequisites, commands, and a success marker
- [Command reference](#commands): [server](#server), [server-ui](#server-ui), [server-ui-e2e](#server-ui-e2e), [app](#app), [iot](#iot), [docs](#docs)
- [Release and maintenance](#release)
- [Firmware](#firmware): [Boards and ports](#boards), [Build-time variables](#build-time-vars), [Logging](#logging), [Before flashing](#before-flashing), [Handy commands](#handy-commands)
- [Impact of a change](#impact)
- [Symptoms and where to start](#troubleshooting)
- [What the checks do not cover](#blind-spots)
- [Full details](#further)

## Conventions {#conventions}

- Run every command from the repository root; `moon run <project>:<task>` also works from any subdirectory.
- Pass arguments to a task after `--`: `moon run <project>:<task> -- <arguments>`. They are appended to the end of the task's command, not inserted anywhere inside it.
- A task that hits the cache is not executed; `moon run --force <project>:<task>` re-runs it.
- The complete task list is whatever `moon query tasks --project <project>` reports. This page omits tasks that exist only as dependencies of another task; moon runs those on its own.

## Common workflows {#workflows}

### 1. Start the server for the first time {#flow-first-run}

Prerequisite: a fresh clone has no `application.toml`, and `data/` holds no model assets. Run these three in order.

```sh
moon run server-ui:build
moon run server:downloader-install
moon run server:run
```

Success: the terminal prints `listening on 0.0.0.0:3000`, and `http://127.0.0.1:3000/login` accepts the default account `root` / `Change_Me`.

- `server-ui:build` installs dependencies through `ensure-deps`; no manual `pnpm install` is required.
- Without model assets the server stops during TTS initialisation with `Matcha acoustic model file (.onnx) not found in …`.
- Both `application.toml` and `application-dev.toml` are gitignored, and a missing file is skipped silently by figment: the server starts on defaults without asking and without reporting anything.

### 2. Develop the frontend and backend together {#flow-dual-terminal}

```sh
# Terminal A
moon run server:dev
# Terminal B
moon run server-ui:dev
```

Success: terminal A prints `listening on 0.0.0.0:3000`; terminal B prints `http://localhost:4200` and the page loads data.

- The frontend uses no Vite proxy; its API address comes from `VITE_BASE_URL=http://localhost:3000` in `apps/server-ui/.env.development`. With the backend down the page stays blank — there is no cross-origin error to read.
- `server-ui:dev` does not depend on `ensure-deps`, so `pnpm i` is required once.
- `server:dev` reads `application-dev.toml`; a missing file means defaults.

### 3. Start without model assets {#flow-minimal}

```sh
moon run server:dev-minimal
```

VAD and ASR are set to `void`, TTS to `mute`, and the LLM to `local_echo`, so no model asset is needed; the voice path is unavailable while the rest of the API and the pages work as usual. The success marker is the same as in workflow 1.

`moon run server:dev-lite` keeps VAD, ASR, and TTS and only swaps the LLM for `local_echo`; it still needs model assets.

### 4. Change only the frontend and verify the embedded build {#flow-embedded-ui}

```sh
moon run server-ui:build
moon run server:run
```

Success: `http://127.0.0.1:3000/` shows the new interface.

`apps/server/web/dist` is a symbolic link to `apps/server-ui/dist/`, and the backend embeds the build product on disk; running `server-ui:dev` alone does not change the page served on port 3000.

### 5. End-to-end test {#flow-e2e}

```sh
moon run server-ui-e2e:e2e
```

Prerequisite: nothing has to be started by hand. The task depends on `server-ui:build` and `install-playwright`, and Playwright runs `vite preview` on port 4300 itself.

Success: the cases pass and the HTML report is written to `apps/server-ui-e2e/playwright-report`.

- To target another address, set `BASE_URL`: `BASE_URL=http://127.0.0.1:3000 moon run server-ui-e2e:e2e`.
- If something already listens on 4300, Playwright reuses it (`reuseExistingServer`, disabled under CI).

### 6. Check the firmware after a change {#flow-firmware-checks}

```sh
moon run iot:check-s3
moon run iot:check-c6
moon run iot:check-host
```

None of the three replaces another: S3 goes through the Xtensa toolchain, C6 through `riscv32imac`, and `check-host` covers the `host-smoke` binary. The third may be skipped when only host-side code changed.

### 7. Flash the product image {#flow-flash}

```sh
espflash list-ports -n
IOT_PORT=<port> moon run iot:flash-s3
```

Success: the monitor shows `[IOT] log level INFO (IOT_LOG_LEVEL)` first, then `[IOT] boot complete`.

- `IOT_PORT` is mandatory when both boards are plugged in; otherwise espflash picks a port itself and may write the C6.
- The task's script passes `--monitor`, so it keeps the terminal after flashing.

### 8. Run a single-peripheral probe {#flow-probe}

```sh
IOT_PORT=<port> moon run iot:flash-probe-audio-s3
```

Each probe image enables one peripheral only, so a fault belongs to a single path. `flash-probe-*` already builds; there is no need to run the matching `probe-*` first, which only builds and serves the no-flash case.

- The full set of subjects: `aec`, `audio`, `camera`, `clock`, `demand`, `echo`, `multicore`, `wifi-s3`, `wifi-c6`.
- Probe flash tasks pass no partition table, so the device receives espflash's default table; reflash the product image to restore product behaviour.

### 9. Without hardware {#flow-no-hardware}

```sh
moon run iot:smoke-host
moon run iot:emu-smoke
```

`smoke-host` runs `host-smoke` on the host and hits the cache, so a replay needs `--force`; `emu-smoke` boots the merged C6 image through `esp-emu` and asserts `[IOT] boot complete`, and is not cached — it rebuilds and boots on every run.

### 10. Before committing {#flow-pre-commit}

```sh
lefthook run pre-commit --force
moon run server:test
moon run docs:check-translation
```

pre-commit carries formatting and static analysis only: `cargo fmt --check`, `cargo clippy` (once for the server, three times for the firmware), `dart format`, and `flutter analyze`. It runs no tests and checks no translation, so the last two commands are separate obligations.

- With no changed files the hooks skip themselves; `--force` runs them anyway.
- `server:test` depends on `test-download-resources` (`cache: false`), which installs any missing model assets for real.

### 11. Match CI {#flow-ci}

```sh
moon ci --base=origin/dev
moon run docs:check-translation
```

CI's `ci` job runs `moon ci` (affected tasks only) and its `docs-check` job runs `docs:check-translation` on its own. Both must pass to equal CI.

### 12. CUDA {#flow-cuda}

moon cannot forward `--features` to `server:run`: that task's command already contains `--`, so an appended argument reaches the binary's own command line. Call cargo directly:

```sh
cd apps/server && cargo run -p vanling-server --features cuda -- \
  -O 'address=0.0.0.0' -O 'data_dir=../../data' --config=application.toml
```

## Command reference {#commands}

### server {#server}

`apps/server`, listening on 3000. `dev` reads `application-dev.toml`, `run` reads `application.toml`; both are gitignored and both fail silently when missing.

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run server:dev` | Start on `application-dev.toml`, listening on 3000 | Starts on defaults when the config is missing |
| `moon run server:run` | Start on `application.toml` | Same |
| `moon run server:dev-lite` | Disable the LLM (`local_echo`), keep VAD, ASR, TTS | Still needs model assets |
| `moon run server:dev-minimal` | Disable VAD, ASR, TTS, LLM | Needs no model assets |
| `moon run server:downloader-install` | Install model assets into `../../data` | Required once before the first start on a fresh clone |
| `moon run server:downloader-list` | List installable entries | — |
| `moon run server:format` | `cargo fmt --check` | Check only, does not write |
| `moon run server:lint` | `cargo clippy --workspace -- -D warnings` | The root workspace excludes `apps/iot`; firmware is not checked |
| `moon run server:test` | Run the `vanling-server` and `api` tests | Downloads model assets for real; moon sets `RUST_TEST_THREADS=2` |
| `moon run server:test-pg` | Same, on the PostgreSQL path | Sets `TEST_DATABASE=pg` |

### server-ui {#server-ui}

`apps/server-ui`, dev server on 4200, preview on 4300.

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run server-ui:dev` | Start the Vite dev server | Installs no dependencies; `pnpm i` first |
| `moon run server-ui:build` | Build into `dist/` | Runs `pnpm install` through `ensure-deps` |
| `moon run server-ui:preview` | Serve the build product | `e2e` starts it on its own |
| `moon run server-ui:test` | `vitest run` | — |
| `moon run server-ui:typecheck` | `tsc --noEmit -p tsconfig.app.json` | Covers `src/**` only, not `vite.config.ts` or `vitest.config.ts` |

### server-ui-e2e {#server-ui-e2e}

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run server-ui-e2e:e2e` | Run the Playwright cases, starting 4300 itself | Depends on `server-ui:build` and the browser install |
| `moon run server-ui-e2e:e2e-install` | `playwright install --with-deps` | Run on first use or after a Playwright upgrade |

### app {#app}

`apps/app`, the Flutter client. Build tasks take their version from `scripts/version.sh`.

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run app:run` | Run on the target device | — |
| `moon run app:test` | `flutter test` | — |
| `moon run app:analyze` | `flutter analyze` | — |
| `moon run app:format` | `dart format .` | Writes to the sources; it is not a check |
| `moon run app:build-android` | Build the release APK | — |
| `moon run app:build-ios` | Build release iOS, unsigned | macOS only |
| `moon run app:build-macos` | Build release macOS | macOS only |
| `moon run app:build-web` | Build release web | — |
| `moon run app:build-linux` | Build release Linux | Linux only |
| `moon run app:build-windows` | Build release Windows | Windows only |

### iot {#iot}

`apps/iot`. `check-s3` and `build-s3` are dispatched by `scripts/iot-xtensa.sh`: a complete espup toolchain is used directly, otherwise the image built by `iot:xtensa-image` is used.

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run iot:check-s3` | Type-check the S3 target, every probe binary included | Does not link; memory exhaustion is invisible here |
| `moon run iot:check-c6` | Type-check the C6 target, `wifi-probe` included | — |
| `moon run iot:check-host` | Type-check the `host-smoke` binary | — |
| `moon run iot:check-features` | Compile every component feature on its own | The only task that covers single-feature builds; about 15 subsets |
| `moon run iot:format` | `cargo fmt --check` | Check only, does not write |
| `moon run iot:lint` | Clippy against the S3 target | Needs the Xtensa toolchain or the image |
| `moon run iot:lint-host` | Clippy for `iot-core` and `iot-ui` | — |
| `moon run iot:lint-smoke` | Clippy for `host-smoke` | — |
| `moon run iot:test` | core, ui, and app (host backend) tests | — |
| `moon run iot:test-bsp` | Board-level tests with every component feature | — |
| `moon run iot:test-tool` | Check the sound catalogue against its generated table | — |
| `moon run iot:smoke-host` | Host smoke run | Hits the cache; replay with `--force` |
| `moon run iot:emu-smoke` | Boot the merged C6 image through `esp-emu` | Not cached; rebuilds and boots on every run |
| `moon run iot:build-s3` | Build the S3 product image | Builds without flashing; memory exhaustion shows up here |
| `moon run iot:build-s3-camera` | Build the image that boots onto the camera page | Shares the product output path; the task saves and restores it |
| `moon run iot:build-s3-thrift` | Build the throttled image (`audio=off`, `motion=on:30000`) | Same |
| `moon run iot:flash` | Build, flash the C6 against the repository partition table, then monitor | Keeps the terminal |
| `moon run iot:flash-s3` | Build, flash the S3, then monitor | Needs `IOT_PORT` when both boards are plugged in |
| `moon run iot:flash-s3-camera` | Flash the camera-page image | — |
| `moon run iot:flash-s3-thrift` | Flash the throttled image | — |
| `moon run iot:flash-panel-typing-s3` | Flash the bench image that types a credential on the panel | Needs `IOT_WIFI_SSID` and `IOT_WIFI_PSK` |
| `moon run iot:reset-s3` | Pull the S3 out of the ROM downloader with a watchdog reset | Reset only, no flashing |
| `moon run iot:flash-probe-aec-s3` | Build and flash the AEC probe | Passes no partition table |
| `moon run iot:flash-probe-audio-s3` | Build and flash the speaker probe | Same |
| `moon run iot:flash-probe-camera-s3` | Build and flash the camera and panel probe | Same |
| `moon run iot:flash-probe-clock-s3` | Build and flash the clock probe | Same |
| `moon run iot:flash-probe-demand-s3` | Build and flash the data-plane probe | Same |
| `moon run iot:flash-probe-echo-s3` | Build and flash the echo-cancellation probe | Same |
| `moon run iot:flash-probe-multicore-s3` | Build and flash the dual-core and main-stack probe | Same |
| `moon run iot:flash-probe-wifi-s3` | Build and flash the Wi-Fi probe (S3) | Credentials come from `IOT_WIFI_SSID` / `IOT_WIFI_PSK` |
| `moon run iot:flash-probe-wifi-c6` | Build and flash the Wi-Fi probe (C6) | Same |

### docs {#docs}

`docs`, the Zola site.

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run docs:dev` | Serve locally and open a browser | Live reload |
| `moon run docs:build` | Build into `public/` | Reports broken internal links and orphan pages |
| `moon run docs:check-translation` | Verify `source_file_hash` in `.en.md` | Walks existing `.en.md` files only; a missing translation is not an error |

## Release and maintenance {#release}

Rare tasks, run on demand.

| Command | Purpose | Notes |
| --- | --- | --- |
| `moon run server:bump` | Compute and write back the server version | One package at a time |
| `moon run app:bump` | Compute and write back the app version | Same |
| `moon run iot:bump` | Compute and write back the firmware version | Same |
| `moon run server:build-release` | Release build of the server | Used by the release pipeline |
| `moon run server:sweep` | `cargo sweep --maxsize 20GB` | Cleans `target` |
| `moon run server:downloader-all-and-checksums` | Install every asset and update the checksums | Used when maintaining the download manifest |
| `moon run iot:image` | Produce the ELF and `merged.bin` in `dist/` | Depends on `image-s3` and `image-c6` |
| `moon run iot:xtensa-image` | Build the Xtensa toolchain image | Once on a host without espup; `build-s3` falls back to it afterwards |
| `moon run iot:gen-midi-assets` | Regenerate `assets/*.mid` from the recipes | Writes the source tree; `test-tool` checks it against the table |
| `moon run docs:build-gh` | Build into `public-gh/` with the GitHub Pages base URL | Used by the CI deploy |
| `moon run tools:update-moon` | Update the moon version and hashes | Modifies `flake.nix` |
| `moon run tools:update-moon-check` | Check whether moon is up to date | Check only |

## Firmware {#firmware}

### Boards and ports {#boards}

Two boards sit on the bench permanently and are not interchangeable. **The port name changes on every replug; it is not an identifier.**

| Board | Feature | Reset semantics |
| --- | --- | --- |
| LCKFB ESP32-S3 (product target, 16 MB) | `lckfb-szpi-esp32s3` | Native USB: flashing needs `-a watchdog-reset`; when stuck in the downloader, run `moon run iot:reset-s3` |
| ESP32-C6-DevKitC-1 (smoke target, 8 MB) | `esp32c6-devkitc-1` | A plain DTR reset boots it; espflash refuses `watchdog-reset` |

```sh
espflash list-ports -n                                # List attached ports, names only
espflash board-info --port <port>                      # Chip type / flash size / MAC address
IOT_PORT=<port> moon run iot:flash-s3                  # Mandatory when both boards are plugged in
```

### Build-time variables {#build-time-vars}

The following are read at build time; a change requires a rebuild.

| Variable | Effect | When unset |
| --- | --- | --- |
| `IOT_BOOT_PAGE` | Which page the device comes up on | `ambient` |
| `IOT_DEMAND` | Which data planes, at what cadence | The firmware's defaults |
| `IOT_LOG_LEVEL` | Serial output level: `error` / `warn` / `info` / `debug` / `trace` | `info` |
| `IOT_WIFI_SSID` / `IOT_WIFI_PSK` | Credentials compiled in | Not compiled in; the device stays on the setup page |
| `IOT_PANEL_BENCH` | Force the unconfigured state so a passphrase has to be typed on the panel | None |

`IOT_PORT` is not one of these: it only tells espflash which port to use and never enters the firmware.

`IOT_DEMAND` is parsed per term and leniently: a term it does not recognise costs only that term, and the build still succeeds; when the whole string names no plane at all, the firmware prints `[IOT] IOT_DEMAND named no plane this firmware recognises: ignored`. An `IOT_BOOT_PAGE` that is not a page name falls back to `ambient` and prints `[IOT] IOT_BOOT_PAGE=… is not a page, booting on Ambient`.

### Logging {#logging}

The product defaults to `info`: the boot inventory, each state transition, and each user action are one line, and **a steady state may print nothing for a long time** — a quiet port is normal, not a hang. Periodic telemetry (frame cost and stack peak, feed cadence, link phase and address, scan lists) is all `debug`:

```sh
IOT_LOG_LEVEL=debug IOT_PORT=<port> moon run iot:flash-s3
```

The `[DISPLAY] stack peak …`, `[PLAY] … feeds …`, and `[WIFI] … address …` lines a record quotes appear per window in a `debug` image only; rebuild with the line above to reproduce one. `warn` is reserved for a bound actually crossed (a single executor hold over 30 ms, a frame write over 20 ms or 25% duty, a late feed or under 95% of the nominal rate), so **any warn line is worth reading**. The first line, `[IOT] log level …`, is the level this image really carries; check it first when a log seems to be missing.

### Before flashing {#before-flashing}

- A failed build **leaves the previous binary in place**, and its timestamp proves nothing. Add a log line, then run `strings <elf> | grep <literal>`; only a match shows the artifact is new.
- `iot:flash` runs `--monitor` in its script and keeps the terminal after flashing. Without a tty it ends with `Failed to initialize input reader`; the image is already written.
- S3 builds go through Docker; a `docker.sock/_ping` 500 means the daemon is down, not a compile error.
- `moon` does not re-run a cached task; `moon run --force <task>` re-runs it.

### Handy commands {#handy-commands}

```sh
# Live log: read the port directly, not through the flash stub
timeout 5 cat <port>

# Full boot log: monitor needs a pty, and CTRL+R (0x12) only counts once it has connected.
# The full recipe is in flashing.md.
({ sleep 6; printf '\x12'; sleep 10; } | script -q -t 1 /tmp/boot.log \
  espflash monitor --chip esp32s3 --port <port> -a no-reset) >/dev/null 2>&1

# Forget the saved network without reflashing; espflash does not read the device's table, so pass the csv
espflash erase-parts --chip esp32s3 --port <port> \
  --partition-table partitions/lckfb-szpi-esp32s3.csv creds

# Any cargo subcommand on the S3 target has to go through this dispatcher
bash ../../scripts/iot-xtensa.sh tree -p iot-app --target xtensa-esp32s3-none-elf \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto

# One test, with its println output
cargo test -p iot-core <test-name> -- --nocapture

# Total main-task stack = the difference between these two symbols (the same arithmetic esp-rtos uses)
nm <elf> | grep -E '_stack_(start|end)_cpu0'
```

## Impact of a change {#impact}

| Change | Must re-run |
| --- | --- |
| Any Rust code in server | `moon run server:format`, `server:lint`, `server:test` |
| server-ui sources | `moon run server-ui:typecheck`; add `server-ui:build` when the backend must embed it |
| An IoT component feature or a `Cargo.toml` | `moon run iot:check-features` |
| `scripts/iot-xtensa.sh` | `moon run iot:check-s3`, `iot:check-c6`, `iot:build-s3` |
| A Chinese docs page | Recompute `source_file_hash` (`git hash-object <zh-page>.md`), then run `docs:check-translation` |
| An English docs page, a template, or `zola.toml` | `moon run docs:build` |
| Any `moon.yml` | The matching command table on this page |

## Symptoms and where to start {#troubleshooting}

| Symptom | Look at first |
| --- | --- |
| Exits on start with `Matcha acoustic model file (.onnx) not found in …` | No model assets installed; run `moon run server:downloader-install` |
| The page on port 3000 is blank | The backend is not running, or the embedded build is stale (workflow 4) |
| Flashed but does not start, log shows `boot:0x23 (DOWNLOAD(USB/UART0))` | The S3 flash is missing `-a watchdog-reset` |
| Not a single log line | The console channel (`console-auto` / `console-uart`); monitor needs a real tty, so wrap it in `script` |
| The device does not match the code | Stale artifact; check the literal with `strings` |
| Link-time main-stack assertion | `.bss` grew; `check-s3` does not link, so it cannot see this |
| A probe runs, but the product no longer knows its saved network | Probe flash tasks pass no `--partition-table`, so the device holds espflash's default table; reflash the product image |
| `server:test` sits in a download for a long time | The first run installs model assets (`test-download-resources`, `cache: false`) |

## What the checks do not cover {#blind-spots}

- `check-s3` only type-checks and never links: running out of memory shows up at `build-s3` or `iot:image`.
- Build-time variables are not part of moon's cache key; `build-s3` sets `cache: false`, the other tasks have no such protection.
- Whether each feature compiles on its own is answered by `check-features` alone.
- The log level is a build-time constant (`configured_level` in `chip-esp/src/logging.rs`): changing `IOT_LOG_LEVEL` without a rebuild has no effect.
- **There is no gdb and no openocd**: debugging means logs, and each peripheral added means one more probe (`flash-probe-*-s3`).
- `server:lint` is `cargo clippy --workspace`, and the root workspace excludes `apps/iot`; the firmware is not checked.
- `server-ui:typecheck` covers the `include` of `tsconfig.app.json` (`src/**`) only; `vite.config.ts` and `vitest.config.ts` belong to `tsconfig.spec.json` and are not included.
- pre-commit runs no tests and checks no translation.
- `apps/iot/target` grows into the tens of GB; `cd apps/iot && cargo clean` costs a rebuild, and the S3 half costs the most.

## Full details {#further}

- [Layering and composition](@/development/iot/architecture.en.md), [feature criteria](@/development/iot/features.en.md)
- [Firmware installation](@/development/iot/flashing.en.md) — artifacts, partition tables, bench images, the full boot-log recipe
- [Hardware-free emulation](@/development/iot/emulation.en.md)
- [Motion](@/development/iot/motion.en.md), [audio metering](@/development/iot/audio.en.md), [playback](@/development/iot/playback.en.md)
- [Implementation records](@/records/iot/_index.en.md) — why it is this way and the measured numbers (AI output, not human-reviewed)

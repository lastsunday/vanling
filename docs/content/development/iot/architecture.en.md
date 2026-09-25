+++
title = "Layering and Composition"
weight = 20
[extra]
source_file_hash = "5d65f19706c9b7844f90a1012f6a7fe1e444f0c1"
translated_at = "2026-09-12T11:59:13Z"
+++

# Layering and Composition

## Layers

`iot-core (pure logic) → iot-chip-esp (esp family runtime) → iot-bsp-esp (per-board wiring) → iot-app (single-task binary)`

- **Wiring lives only in the `bsp/` board modules**: fixed pins inside `Board::new(Peripherals)`; business code must not reference pin numbers.
- **iot-chip-esp**: chip init / logging / RTOS / panic definitions; the app-level entry macro `#[esp_rtos::main]` stays in app `main.rs` (esp-hal/esp-rtos are feature-gated in the app purely so the entry point resolves).
- **iot-bsp-esp**: board differences converge in the `type Board` alias plus run dispatch; one `#[cfg(feature)]` branch per board.
- **iot-app**: the application layer, product name `vanling`; zero chip dependencies in business code; flash/RAM budgets go through linking and partitions, not the type system.
- **Naming convention**: board layer uses board names, application layer uses the product name `vanling`; never duplicate.
- **main entry**: dispatches per family feature, one entry block per family (esp family alias `esp32c6`).

## Composition (three orthogonal axes)

Hardware (board feature) ⊥ module (module feature) ⊥ capability (`iot-core` trait). Modules only see capabilities; boards only provide them.

- **Composition point = bin board manifest**: capabilities injected by move, generic over capabilities, shared by boards with the same capability set; enabling a module that lacks a capability is a compile error, never silent.
- **Product tier = feature alias**: the tier fixes the module set, the board fixes the supported set, and the intersection is trimmed and checked at compile time.
- **Module configurability** = feature switch + const parameters.
- **Runtime-pluggable render layer**: `iot-core`'s allocation-free central `RenderController` (reconcile closure callback) plus `iot-app`'s heap `Vec<Box<dyn Renderer>>` registry (`embedded-alloc` 32KB heap; boot LED registered in-task; cross-task Web/Audio renderers go over `RENDER_BUS` + `Send`).

## Logging

Only the `log` facade (`log::info!` etc.); the output channel is initialized by the family crate `iot-chip-esp` (esp uses `esp_println::logger`); business code must not use `println!` / `esp_println::println!`.

## Adding a board

1. Add the board module to `bsp-esp/` (Board + HasXxx traits) with feature gating.
2. Add the `type Board` / run-dispatch cfg branch in `app/src/main.rs`.
3. Forward the app feature `iot-bsp-esp/<board>`.
4. Add the board to the `platforms` default JSON in `reusable-iot-build.yml` and the `platforms` input of `iot-dev-release.yml` (xtensa boards need `use-xtensa-toolchain: true` and go through the `build-s3` task: the espup native `esp` 1.95.0.0 toolchain on CI/release, falling back to the same-version Docker image locally on macOS Intel).
5. Verify: `cargo build -p iot-app --bin vanling --target riscv32imac-unknown-none-elf --no-default-features --features <board>`

## Adding a chip family

The esp family is fixed as `iot-chip-esp`/`iot-bsp-esp`; adding a non-esp family:

1. Add the `iot-chip-<family>`/`iot-bsp-<family>` crates.
2. Gate that family's entry dependencies behind the app family-alias feature; add the family entry block to `main` (`#[cfg(feature)]`).
3. Toolchain target + `rust-toolchain.toml` + `platforms` in `reusable-iot-build.yml` (`target`/`use-xtensa-toolchain`) and the `platforms` input of `iot-dev-release.yml`.
4. Switch `moon`/`lefthook` `--workspace --target` to the firmware scope; the toolchain target flows into moon tasks via `CARGO_ESP_TARGET` (set in `.envrc`/CI env and workflow inputs; tasks take `- '$CARGO_ESP_TARGET'` in `inputs` to keep cross-chip build caches isolated).
5. Verify.
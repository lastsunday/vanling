+++
title = "Cargo Feature Criteria and Cohesion"
weight = 10
[extra]
source_file_hash = "2cc0dea6f66a1a58ee1699ca14e1e05c96797221"
translated_at = "2026-09-12T11:59:13Z"
+++

# Cargo Feature Criteria and Cohesion

## Background

`apps/iot` once defined a set of "capability features": `led`/`button` in `iot-core`, and `breath`/`button` in `iot-app`. They were scattered across `drivers/mod.rs`, `state.rs`, `render.rs` (two render loops), `main.rs`, and the bsp as `#[cfg(feature=...)]`, and were ultimately removed entirely.

The removal is not a stopgap: the features failed every introduction criterion. Vanling is a single product on a single board; the capability set each build needs is fully determined by the **board** (the hardware axis), and a second software axis with two real configurations never existed.

## Hard features vs soft features

Cargo has no official hard/soft classification, but the ecosystem practice (esp-hal, embassy, bevy, smithy-rs RFC-0015) draws exactly the two categories we mean:

| | Hard feature (hardware axis) | Soft feature (capability/behavior axis) |
|---|---|---|
| Meaning | Selects the target: chip / board / ABI / toolchain | Which optional capabilities/behaviors/dependencies are compiled for the same fixed hardware |
| Semantics | Mutually exclusive, exactly one at a time (set by physical reality) | Additive (union); any combination should be valid |
| Examples | `esp32c6`, `esp32c6-devkitc-1` | `wifi`, a codec, `bt-stack`; esp-radio's `wifi`/`ble`/`csi` |
| Ecosystem reference | esp-hal chip features, embassy per-chip split, cargo #2980 | Cargo Book "Features examples", bevy profiles, pyo3 `std`/`abi3` |

Judgment: **it varies by chip/board/ABI → hard feature; it varies by product tier or size goal on the same hardware → soft feature candidate**.

## Four criteria for introducing a soft feature (require all)

1. **Optional dependency gating** — saves compiling a whole backend/driver stack plus flash and RAM (e.g. TLS, codec, radio stack).
2. **Official SKU tiers** — real variants of the same hardware (lite/free), where consumers must not even see the API.
3. **Significant binary size reduction** — optional large tables/algorithms (e.g. Unicode tables, codec formats) that genuinely cost flash.
4. **Runtime support switches** — std/no_std, `rt`, logging backends, `unstable`/nightly gating.

In particular: if a behavior switch is equivalent to constant folding at opt-level ≥ 2 and only one configuration is ever shipped, do not use a feature (see nullderef, *Why you shouldn't obsess about Rust features*, for a compiler-verified argument). The antipattern list is at the end.

## Six cohesion rules

Cargo features are **additive** (union; enabling one must not disable another) and **global at the library level** (within one dependency graph you cannot have "this instance on, that instance off"). Cohesion therefore means treating every feature as a **cuttable module axis**, not scattered ifdefs:

1. **One feature = one module**: the cfg gates once at the `mod` declaration; all capability code lives in the owning file; off means the file does not exist. A feature touching multiple files is not cohesive — extract that layer into one module.
2. **Zero cfg in consumers**: when off, provide the same API via stubs/no-ops, or inject the capability through a trait at the composition point; any `#[cfg(feature=...)]` at a call site is a cohesion leak.
3. **Single source of truth in Cargo.toml**: declarations, `dep:` names, and composition live in `[features]`; bins/tests use `required-features` to skip the whole unit.
4. **A pyramid, not a flat pile**: leaves = capabilities or `dep:`; collection features only compose leaves; meta features (`full`/default) contain only `["..."]` and never gate code themselves.
5. **The matrix enforces valid combinations**: adding or changing a soft feature must guarantee every subset compiles and its tests pass independently (`cargo hack --feature-powerset` / `cargo fc`); a subset that is meaningless on its own means the feature is not a unit.
6. **Naming and docs carry the role**: capabilities/collections/hardware use distinct naming roots; the feature list and purposes live in one docs page; empty "fake features" are banned (prefer `dep:` or a backing module).

## Antipattern list (the removed items are examples)

- `led = []`, `button = ["led"]`, `breath = []`: empty features, pure cfg switches, no dependency, no backing module (violate 3/6).
- cfg appearing in `drivers/mod.rs`, `state.rs`, `render.rs` (two render loops), `main.rs`, and the bsp at once (violate 1/2).
- The only consumer being a single product on a single board with no two software states (fails the four criteria; see the constant-folding equivalence argument).
- No feature-combination matrix validation (violate 5).

## Current state

`apps/iot` spans two feature axes:

```
iot-core     — no [features]
iot-bsp-esp  — component features (button / ft6336 / pca9557 / ws2812 / st7789 / display-light)
               board features (esp32c6-devkitc-1 / lckfb-szpi-esp32s3) = component aggregate
iot-app      — esp32c6 (chip alias) / esp32c6-devkitc-1 (default) / lckfb-szpi-esp32s3
```

- **Component features are a module axis**: one feature = one component module (gated once at the `mod` declaration in `components/`; off means the file does not exist). Board features only aggregate the components their wiring needs, with no loose dependencies. Empty features (e.g. `button = []`) are legalized by module endorsement (rule 1/6 "module endorsement"); they are not the pure cfg switches removed earlier.
- **Two-axis naming** (rule 6 "distinct roots" in practice): the hardware axis uses chip/board product names as roots (`esp32c6`, `esp32s3`, `esp32c6-devkitc-1`, `lckfb-szpi-esp32s3`); the module axis uses bare component names (`button`, `ft6336`, `pca9557`, `st7789`, `ws2812`, `display-light`). Mutually-exclusive vs additive semantics are clear from the name alone; no `cmp-`/`board-`/`chip-` prefix; only consider a prefix after a real collision (e.g. a board name that equals a component name).
- **Board features are the hardware axis**: mutually exclusive, exactly one at a time, set by the physical board; `iot-core` still has zero cfg, with capabilities injected via traits at the composition point (board wiring).
- CI uses a fixed `--no-default-features --features <board>`, building the fully-featured firmware for that board. Any future capability/behavior-axis soft feature must satisfy the four criteria plus the six rules again, and add matrix validation.
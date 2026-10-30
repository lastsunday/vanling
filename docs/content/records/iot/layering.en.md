+++
title = "Layering review of the app crate: what landed"
weight = 90
sort_by = "weight"
[extra]
source_file_hash = "c9735a24cb95fc0b3a360bb2ab28dae7c1998f78"
translated_at = "2026-10-30T11:55:28Z"
+++

<!-- doc-audience: ai -->

# Layering review of the app crate: what landed

A layering review of `apps/iot/crates/app`: general mechanisms went back to `iot-core`, page and feature knowledge went back to their own modules. This records the judgements and the one blocker, not the code.

## What moved where

- A breath's colour and backlight envelope are `Breath` properties, in `core::state::light`; a renderer only asks `rgb()` / `backlight()` instead of recomputing hue and brightness itself.
- `LightRenderer` and the renderer registry live in `core::render`: the `Renderer` contract was already in core, while `iot-ui` is the page layer, so the renderer does not go into the UI.
- Playback commands (`Play` / `SetMuted`) and their edge decisions (`mute_command` / `play_command`) belong to `PlaybackState`; the app's dispatch only queues and retries a full queue.
- `InputSource::name`, `PollEntry::name` and `DemandKind::name` are answered by the driver and demand layers; the app no longer keeps a name table indexed by position.
- Grounding the whole panel, and "the frames come from the camera", are page properties (`DisplayPage::ground` / `draws_frames`), delegated by the bsp screen instead of repeated board-side `matches!`.
- The online judgement `AddressWatch` and `ADDRESS_LOST_WINDOWS` live in `core::drivers::network`; the link loop only feeds observations in.
- The sweep parameters belong to the echo-room probe and stay in `bench/probe/echo_room`; the product sound catalogue no longer generates a sweep, and the probe plays its own excitation through `audio::start_source`.
- The build-time network name is read in one place (`boot::build_wifi_credentials`); the bench exception is decided by the product-side `boot::wifi_credentials`. A probe always wants the name its build was given, so it reads the former.
- Link-status delivery and de-duplication live in `bridge::deliver` (`LinkWatch`); `PageWatch` is parameterized by page and injected into `net`; a scan list has one double publish (`net::publish_scan`); saved-mark recomputation returns a list from `store::remark`, so the store no longer publishes.
- The product heap size belongs to the chip layer (`iot-chip-esp::PRODUCT_HEAP_BYTES`) and the starting setup front to the board layer (`iot-bsp-esp::FRONT`); `main.rs` only reads them.

## Blocker: the probe's board `#[cfg]` did not move down

`bench/probe` still picks probe entry points with `#[cfg(feature = "...")]`. Moving that gate into bsp is not possible: `echo_room` depends on `crate::audio`, the product's playback and capture implementation, and bsp cannot depend on the app. `bench/probe/mod.rs` already gates once as the module owner, which is what the soft-feature rules ask for, so it stays as it is.

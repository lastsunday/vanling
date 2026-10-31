+++
title = "Layering review of the app crate: what landed"
weight = 90
sort_by = "weight"
[extra]
source_file_hash = "8b3bcdcdfd9ee9cff502db873bf4c265ccffdd4a"
translated_at = "2026-10-31T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Layering review of the app crate: what landed

A layering review of `apps/iot/crates/app`: general mechanisms went back to `iot-core`, page and feature knowledge went back to their own modules. This records the judgements and the one blocker, not the code.

## What moved where

- A breath's colour and backlight envelope are `Breath` properties, in `core::state::light`; a renderer only asks `rgb()` / `backlight()` instead of recomputing hue and brightness itself.
- `LampRenderer` / `ScreenRenderer` and the renderer registry live in `core::render`: the `Renderer` interface was already in core, while `iot-ui` is the page layer, so the renderer does not go into the UI.
- Playback commands (`Play` / `SetMuted`) and their edge decisions (`mute_command` / `play_command`) belong to `PlaybackState`; the app's dispatch only queues and retries a full queue.
- `InputSource::name`, `PollEntry::name` and `DemandKind::name` are answered by the driver and demand layers; the app no longer keeps a name table indexed by position.
- Grounding the whole panel, and "the frames come from the camera", are page properties (`DisplayPage::ground` / `draws_frames`); the renderer asks the page directly, so neither the board nor the device repeats a `matches!`.
- The online judgement `AddressWatch` and `ADDRESS_LOST_WINDOWS` live in `core::drivers::network`; the link loop only feeds observations in.
- The sweep parameters belong to the echo-room probe and stay in `bench/probe/echo_room`; the product sound catalogue no longer generates a sweep, and the probe plays its own excitation through `audio::start_source`.
- The build-time network name is read in one place (`boot::build_wifi_credentials`); the bench exception is decided by the product-side `boot::wifi_credentials`. A probe always wants the name its build was given, so it reads the former.
- Link-status delivery and de-duplication live in `bridge::deliver` (`LinkWatch`); `PageWatch` is parameterized by page and injected into `net`; a scan list has one double publish (`net::publish_scan`); saved-mark recomputation returns a list from `store::remark`, so the store no longer publishes.
- The product heap size belongs to the chip layer (`iot-chip-esp::PRODUCT_HEAP_BYTES`) and the starting setup front to the board layer (`iot-bsp-esp::FRONT`); `main.rs` only reads them.

## Blocker: the probe's board `#[cfg]` did not move down

`bench/probe` still picks probe entry points with `#[cfg(feature = "...")]`. Moving that gate into bsp is not possible: `echo_room` depends on `crate::audio`, the product's playback and capture implementation, and bsp cannot depend on the app. `bench/probe/mod.rs` already gates once as the module owner, which is what the soft-feature rules ask for, so it stays as it is.

## Splitting the lamp and screen interfaces (round two)

Round one moved `page_ground` / `page_draws_frames` from the board into `Screen`, but the interface was still "a lamp": screen abilities hung off `RgbLight`, so a board without a panel could only write empty implementations. This round splits the device interface by capability.

- `RgbLight` keeps only `set_rgb(Rgb)`; `PageSurface: RgbLight + DiagnosticsSink + CameraTarget` owns the backlight, the repaint throttle and the camera, and only a panel implements it.
- `page_ground` / `page_draws_frames` / `CameraTarget::takes_frames` are gone. Grounding and "the frames come from the camera" were already `DisplayPage` properties, and the role was already given by `WiredSurface::Pages/Lamp` at wiring time; asking the device again copied the same answer twice.
- `WiredSurface` is now `Pages(Box<dyn PageSurface>) | Lamp(Box<dyn RgbLight>)`, and `HasSurface` no longer has an associated type; the WS2812 therefore drops its `DiagnosticsSink`, `CameraTarget`, backlight and repaint stubs. `surface::wired()` hands the camera to the first `Pages`, without asking `takes_frames`.
- The renderer splits into `LampRenderer` / `ScreenRenderer`, sharing `render::colour::ColourDriver` (breathing, solid, off, display-demand gate). The registry is still `Box<dyn Renderer>`, and late registration is unchanged.
- `Rgb` and the colour maths move to `core::paint`; `MAX_LIGHTS` belongs to `state` (it measures state slots, not a driver fact). `ui` and `stamp` no longer take colours from `drivers`.
- `Fill` and `Fill::VerticalGradient` are deleted: the production path only sends `Uniform`, and the only job `Fill` had left was an extra bit in `FrameOwner`. `FrameOwner` now records just `Rgb`, `Canvas::ground()` returns `Option<Rgb>`, and `stamp::fill_surface` loses its fill parameter.
- Behaviour preserved word for word: the camera owner's release order, the `Activity::TimeDriven` return, the `Mode::Off` branch, the place `Diagnostics` is cloned whole, and the `accepts_color`-before-`paint` order.
- Image: C6 845,440/1,048,576 B (80.63%, +2,448 B over the pre-split build), leaving about 198 KiB.

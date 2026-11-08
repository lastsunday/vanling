+++
title = "Layering review of the app crate: what landed"
weight = 90
sort_by = "weight"
[extra]
source_file_hash = "de9367af8231ca50a8c25a214afd845a13b4b8cf"
translated_at = "2026-11-05T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Layering review of the app crate: what landed

A layering review of `apps/iot/crates/app`: general mechanisms went back to `iot-core`, page and feature knowledge went back to their own modules. This records the judgements and the one blocker, not the code.

## What moved where

- A breath's colour and backlight envelope are `Breath` properties, in `core::state::light`; a renderer only asks `rgb()` / `backlight()` instead of recomputing hue and brightness itself.
- `LampRenderer` / `ScreenRenderer` and the renderer registry live in `app::render`: `core::ui` is the pixel and glyph layer, so the renderer does not go into the UI; the `Renderer` interface later moved into the app with its implementations.
- Playback commands (`Play` / `SetMuted`) and their edge decisions (`mute_command` / `play_command`) belong to `PlaybackState`; the app's dispatch only queues and retries a full queue.
- `InputSource::name`, `PollEntry::name` and `DemandKind::name` are answered by the driver and demand layers; the app no longer keeps a name table indexed by position.
- Grounding the whole panel, whether frames come from the camera, and the repaint step are render policy (`render::screen::policy`); the renderer asks the policy per page, so neither the board nor the device repeats a `matches!`. The route table (`shared::route`) keeps only the name, needs, claim, bootability and lifetime.
- The online judgement `AddressWatch` and `ADDRESS_LOST_WINDOWS` live in `core::drivers::network`; the link loop only feeds observations in.
- The sweep parameters belong to the echo-room probe and stay in `bench/probe/echo_room`; the product sound catalogue no longer generates a sweep, and the probe plays its own excitation through `audio::start_source`.
- The build-time network name is read in one place (`boot::build_wifi_credentials`); the bench exception is decided by the product-side `boot::wifi_credentials`. A probe always wants the name its build was given, so it reads the former.
- Link-status delivery and de-duplication live in `network::publish_link` (`LinkWatch`); `PageWatch` is parameterized by page and injected into `network`; a scan list is held only by `NETWORK.scan`, and both each `scan` round and `store::remark_and_publish` publish there, so the panel and the phone see the same marks.
- The product heap size belongs to the chip layer (`iot-chip-esp::PRODUCT_HEAP_BYTES`) and the starting setup front to the board layer (`iot-bsp-esp::FRONT`); `main.rs` only reads them.
- The link's business state and the Network page's state are two records: `NetworkState` is written only by `LinkIntent`, `NetworkDraft` only by the fingers' `NetworkPageIntent`, and the page reads `&NetworkState` while the link never touches a page field. `Effects` no longer carries an outbox: the page domain returns `NetworkCommand` directly, so there is one path from an intent to the worker.
- The network worker no longer reaches `run` through a `WorkerStart`: its driver types are only known to the entry point, so it builds `Wifi::Worker(NetworkWorker)` there — a closure that takes only a `Spawner` — and `run` starts it before `run_loop` without holding a worker list. The splash's verdict travels as `NetworkCommand::SplashOver { setup }`; `PageWatch` is only a page level and no longer carries a splash edge.
- `state` and `intent` are **deliberately two-way**: `state` defines and owns the payload types an intent carries (`Confirm`, `Page`, `AudioPhase` and so on), while `translate` — which turns a finger into an `ActionIntent` — now lives in `controller`. Neither of them is a render or snapshot layer, so the structural test forbids only `state → {render, diagnostics}` and `intent → {page, render, diagnostics}`; an audit should not read this as a new hole.
- The coordinator's network machinery moved into `coordinator/systems/network.rs`: `NetworkOutbox` holds the one command that did not fit in the queue, `NetworkMirror` publishes the page level, records the front and lamp edges and reclaims the scan list, and `run_loop` keeps only intent orchestration. On the `core` side the splash verdict was folded back into `apply_page`'s return value and the `take_splash_end` side channel is gone, so from an intent to the worker there really is one path.
- The coordinator is split by boundary: one `System` per file under `coordinator/systems/` (declaration order in `coordinator::systems::Schedule` is execution order), each holding only what must survive a frame; `Frame` hands it the state and the worker handles. The intent → state → command recipe moved into `DeviceManager::apply_intent` so tests stop copying it, and the demand slot's lock moved back to `workers::Input::publish`. `[measured]` On the S3 product image `_stack_end` sits 96 B lower than HEAD, `.text` is 2,188 B smaller, and `.data`/`.bss` are unchanged.

## Blocker: the probe's board `#[cfg]` did not move down

`bench/probe` still picks probe entry points with `#[cfg(feature = "...")]`. Moving that gate into bsp is not possible: `echo_room` depends on `crate::audio`, the product's playback and capture implementation, and bsp cannot depend on the app. `bench/probe/mod.rs` already gates once as the module owner, which is what the soft-feature rules ask for, so it stays as it is.

## Splitting the lamp and screen interfaces (round two)

Round one moved `page_ground` / `page_draws_frames` from the board into `Screen`, but the interface was still "a lamp": screen abilities hung off `RgbLight`, so a board without a panel could only write empty implementations. This round splits the device interface by capability.

- `RgbLight` keeps only `set_rgb(Rgb)`; `PageSurface: RgbLight + DiagnosticsSink + CameraTarget` owns the backlight, the repaint throttle and the camera, and only a panel implements it.
- `page_ground` / `page_draws_frames` / `CameraTarget::takes_frames` are gone. Grounding and "the frames come from the camera" were already answered by the page (now in `render::screen::policy`), and the role was already given by `WiredSurface::Pages/Lamp` at wiring time; asking the device again copied the same answer twice.
- `WiredSurface` is now `Pages(Box<dyn PageSurface>) | Lamp(Box<dyn RgbLight>)`, and `HasSurface` no longer has an associated type; the WS2812 therefore drops its `DiagnosticsSink`, `CameraTarget`, backlight and repaint stubs. `surface::wired()` hands the camera to the first `Pages`, without asking `takes_frames`.
- The renderer splits into `LampRenderer` / `ScreenRenderer`, sharing `render::colour::ColourDriver` (breathing, solid, off, display-demand gate). The registry is still `Box<dyn Renderer>`, and late registration is unchanged.
- `Rgb` and the colour maths move to `core::paint`; `MAX_LIGHTS` belongs to `state` (it measures state slots, not a driver fact). `ui` and `stamp` no longer take colours from `drivers`.
- `Fill` and `Fill::VerticalGradient` are deleted: the production path only sends `Uniform`, and the only job `Fill` had left was an extra bit in `FrameOwner`. `FrameOwner` now records just `Rgb`, `Canvas::ground()` returns `Option<Rgb>`, and `stamp::fill_surface` loses its fill parameter.
- Behaviour preserved word for word: the camera owner's release order, the `Activity::TimeDriven` return, the `Mode::Off` branch, the place `Diagnostics` is cloned whole, and the `accepts_color`-before-`paint` order.
- Image: C6 845,440/1,048,576 B (80.63%, +2,448 B over the pre-split build), leaving about 198 KiB.

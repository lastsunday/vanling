+++
title = "Layering review of the app crate: what landed"
weight = 90
sort_by = "weight"
[extra]
source_file_hash = "ae20070b972a8a23b41e3f783022fa11eb66c5da"
translated_at = "2026-11-20T00:00:00Z"
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
- Link-status delivery and de-duplication live in `LinkWatch::deliver`; `PageWatch` is parameterized by page and injected into `network`; a scan list is held only by `Network.scan`, and both each `scan` round and `store::remark_and_publish` publish there, so the panel and the phone see the same marks.
- The product heap size belongs to the chip layer (`iot-chip-esp::PRODUCT_HEAP_BYTES`) and the starting setup front to the board layer (`iot-bsp-esp::FRONT`); `main.rs` only reads them.
- The link's business state and the Network page's state are two records: `NetworkState` is written only by `LinkIntent`, `NetworkView` only by the fingers' `NetworkPageIntent`, and the page reads `&NetworkState` while the link never touches a page field. `Effects` no longer carries an outbox: the page domain returns `NetworkCommand` directly, so there is one path from an intent to the worker.
- The network worker no longer reaches `run` through a `WorkerStart`: its driver types are only known to the entry point, so it builds `Wifi::Worker(NetworkWorker)` there — a closure that takes only a `Spawner` — and `run` starts it before `run_loop` without holding a worker list. The splash's verdict travels as `NetworkCommand::SplashOver { setup }`; `PageWatch` is only a page level and no longer carries a splash edge.
- `state` and `intent` are **deliberately two-way**: `state` defines and owns the payload types an intent carries (`Confirm`, `PageId`, `AudioPhase` and so on), while `translate` — which turns a finger into an `BusinessIntent` — now lives in `controller`. Neither of them is a render or snapshot layer, so the structural test forbids only `state → {render, diagnostics}` and `intent → {page, render, diagnostics}`; an audit should not read this as a new hole.
- The coordinator's network machinery moved into `coordinator/systems/network.rs`: `NetworkOutbox` holds the one command that did not fit in the queue, `NetworkMirror` publishes the page level, records the front and lamp edges and reclaims the scan list, and `run_loop` keeps only intent orchestration. On the `core` side the splash verdict was folded back into `apply_navigation`'s return value and the `take_splash_end` side channel is gone, so from an intent to the worker there really is one path.
- The coordinator is split by boundary: one `System` per file under `coordinator/systems/` (declaration order in `coordinator::systems::Schedule` is execution order), each holding only what must survive a frame; the frame's parameters hand it the state, and the composition root hands it the worker handle when it registers. The intent → state → command recipe moved into `DeviceManager::apply_intent` so tests stop copying it, and the demand slot's lock moved back to `workers::Input::publish`. `[measured]` On the S3 product image `_stack_end` sits 96 B lower than HEAD, `.text` is 2,188 B smaller, and `.data`/`.bss` are unchanged.

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

## Implicit handles down to zero (round three)

The requirement was "every function's inputs and outputs are in its signature". This round removed the module-path singletons — `INTENT_BUS`, `WAKE`, `NETWORK`, `PLAYBACK`, `READINESS`, `DROPPED_INTENTS`, `HEAP_FREE`, `PAGE_READ`, `LINK` — and made them handles the entry builds and passes down.

- The composition root holds one `static RUNTIME: Runtime` (one each in the product entry and `host-smoke`); `Wiring` carries `&'static Runtime`, `Io` carries the bundle that crosses cores. Below that, only a specific handle or one coherent companion value travels: the network worker receives `NetCtx { bus, network, readiness, heap_free }`, and a frame received five handles — `bus`, `capture`, `network`, `dropped`, `readiness` — collected at the time into `Frame`, which round four took apart again (below). There is **no** `runtime()` accessor — that would be a service locator under a new name.
- `Playback`, `Network` and `CaptureWindow` own their channels (`const fn new()`), instead of a static channel plus a handle pointing at it. The only cross-core lock left is `Shared` (the speaker).
- `LinkWatch`'s last report and unsent MAC moved onto `serve`'s stack as plain `Cell`s; `PageRead` became a signal created each time a setup AP is mounted, so a read left over from a previous access point no longer makes the new one's idle timeout fire at once.
- Pure decisions return outcomes: `resolve_join` now returns `#[must_use] enum Join::{Found, Missing}` and the caller decides what to send, instead of the function writing to the bus from inside.
- Full-ring policy lives in one place: `input::hand_over` awaits `send()`, because only that guarantees the copy asked once is never lost; status reports still use `try_send` plus a retained value, which the next pass can retry. New host test: `a_full_ring_delays_a_hand_over_instead_of_losing_it` (round four moved it to `IntentBus::send` and renamed the test to match).
- That test flushed out a real bug: `DroppedIntents::note` used `then_some(count + 1)`, which overflows before the check at `u32::MAX`; it is back to `then(|| count + 1)` (round four moved that logic into `IntentBus::try_send` with the count).
- Behaviour preserved: the order inside a frame, the three ready lines a parked frame waits on, the wifi soft-feature gate, and the C6's lack of a product path. The host scenario now awaits `send()` per intent, and `dropped.count() == 0` is still asserted.
- `[measured]` S3 product image (`nm` / `size -A`, the same credential-free build): main stack `_stack_start − _stack_end` = 86,536 B (HEAD 87,024 B, −488 B), `.bss` 240,840 B (−3,128 B), `.data` 23,840 B (+3,496 B), `.text` 919,113 B (+1,100 B); the three together are a net −120 B of RAM. Image 1,136,128/6,291,456 B; C6 954,048/1,048,576 B (90.99%). Both boards booted on hardware: the S3 joined its saved network and got an address, the watchdog reported healthy, and the stack peaked at 18,948 B of 86,536 B.

## The refusal count goes back to the ring (round four)

Round three pulled every singleton into a signature, and paid for it with a `Frame { bus, capture, network, dropped, readiness }`: five fields with no shared concept, bundled only because `run_loop` used them together, so `run_loop`'s signature no longer said what it read. The sources agree with each other — clippy's `too_many_arguments` threshold is 7 (the eighth argument is what it reports, and trait impls are exempt), a parameter object pays only when the values come from different objects and form a real concept, and a bag of unrelated handles *hides* dependencies. This round cuts the real dependency count instead of repackaging it.

- `Frame` is gone; `run_loop` takes seven named parameters: `bus`, `capture`, `scan`, `readiness`, renderers, the model and the schedule. A parked frame only waits on `scan.ready()`, so it receives `&'static Scan` rather than the whole `Network`. Seven lands exactly inside clippy's default threshold with no new `#[allow]`: one more dependency and the compiler refuses it.
- The count moved into `IntentBus`: the refusal happens at the ring, so `try_send` records it when it is full and the ring cannot refuse silently; `dropped_count()` is folded into the snapshot at the head of `Schedule::process_intents`, once per frame. `DroppedIntents`, `input::forward` and `input::hand_over` went with it — waiting for room is `IntentBus::send`, `input_task` drops from six parameters to five, and the per-window figure in the `[INPUT]` log is now a difference of the total, so there is one source of truth.
- New fence, `the_refusal_count_is_read_where_the_ring_is_drained`: the only readers of `dropped_count` may be the ring itself, that one fold and the host scenario's assertion; a second reader would give the number a second meaning.
- Behaviour preserved: the order inside a frame, the three ready lines a parked frame waits on, the wifi soft-feature gate and the C6's lack of a product path. The host scenario's `dropped_count() == 0` is still asserted.
- `[measured]` S3 product image (`nm` / `size -A`, the same credential-free build): main stack `_stack_start − _stack_end` = 86,344 B (86,536 B in round three, −192 B; 87,024 B at HEAD, −680 B), `.bss` 241,032 B (+192 B), `.data` 23,840 B (unchanged) and `.text` 919,457 B (+344 B) — the stack and `.bss` give way to each other, so RAM totals 351,216 B, the same as round three. App partition 1,136,464/6,291,456 B (18.06%); C6 953,968/1,048,576 B (90.98%, −80 B). On hardware after flashing: the S3's boot log prints `[DISPLAY] … main stack 86344 B`, the watchdog reports healthy, it joins its saved network and gets an address, and the stack peaks at 18,932 B of 86,344 B; the C6 takes the offline path (no saved network), breathes its lamp, reports `0 crashed boot(s)` and the same healthy boot.


## When a companion value is legitimate (round five)

Round three introduced `NetCtx`, round four deleted `Frame`; both were about whether a set of values should be bundled, and neither wrote the criterion down, so every site argued it again. This round fixes the criterion first, sweeps the crate with it, then removes the duplicated plumbing.

Seven tests, and a bundle has to pass all of them, or it is only the parameters under a new name:

1. one owner, and exactly one construction site;
2. its fields are fixed a priori by the boundary it crosses, not assembled for one call;
3. it outlives a single signature — across a task, across a frame, or past the boundary;
4. its contents come from the receiver or the owner, not from whatever the caller happened to have;
5. its name is a domain noun, not `Args`, `Params` or `Frame`;
6. it removes real dependencies, or crosses a boundary a signature cannot;
7. it holds no optional or rarely-used field — such a field is itself evidence that this is not one concept.

The sources agree: Fowler's *Data Clump* (delete one field and reread the signature) and *Flag Argument*; the J2EE Context Object; Go's `context` rules (the entry creates it, it only travels down, required values are parameters rather than context); ISP's "clients should not depend on what they do not use"; and connascence dropping from position to name. `Frame` was the reverse: a name that hid the positional relation and added connascence of meaning on top. Clippy reports `too_many_arguments` at 7, so "too many parameters" is not an argument by itself.

`BootFacts` is the one type this criterion newly splits out:

- Safe mode's `run` feeds two sessions (station first, then the access point when that fails). Of the eight page facts four cannot change after boot (`mac`, `crashed_boots`, `saved_networks`, `heap_free`) while four move all the time (`uptime_ms`, `phase`, `address`, `station`). The first four are fixed by the "this boot" boundary and all four are known in `run` → built once, reused twice: `station_session` goes 8 → 4 parameters, `access_point` 5 → 2, `Report` 8 → 5 fields, and `facts()` reads `self.boot.*`.
- `heap_free` stays a `fn() -> usize` rather than a number: which heap this boot got is only known once `run` has it, and both sessions read it on every answer — fixed by the boundary, reused across it, not optional.

Candidates examined and rejected (recorded so the next round does not reopen them):

- `scan(ring, dropped)`, `hold(deadline, pause)`, `await_network(...)`, `(mac, aps)` and `run_loop`'s seven parameters: each reads a handful of locals once, none has a boundary owner, and the signature still reads plainly with any one field deleted → no bundle.
- `secured: bool` (shared by the driver trait, the bsp implementations and the probe images) and `Report.station: bool`: a single bool is not a clump, `fn_params_excessive_bools` fires at 3 and skips trait impls, and changing it would rewrite the driver contract for nothing.
- A MAC newtype: `embassy-net` / `smoltcp` already use `EthernetAddress([u8; 6])`; another one here would only add a conversion.
- "`NetCtx.heap_free` has one reader" is not a reason to remove it: `LinkWatch` wants the whole bundle (bus, network, readiness, heap), and ISP says "do not depend on what you do not use", not "split it when few use it".
- `mac_of` / `seed_from` existed in both `safe.rs` and `network/mod.rs`; the duplication is the derivation, not the call, so both now live in `network/mod.rs` as `pub(super)`.

Two new fences in `module_boundaries.rs`, both shown red first with a fake `Frame`:

- `context_types_live_only_at_boundaries`: the set of `struct` names ending in `Ctx` / `Context` must be exactly `NetCtx` and `PanelContext`. Adding one means editing this list, which is where its boundary has to be named.
- `a_bundle_is_built_where_it_is_owned`: the five bundles (`Wiring`, `Io`, `NetCtx`, `PanelContext`, `BootFacts`) may be constructed only in the owner file and at an entry. `Frame`'s shape was arguments repackaged mid-chain, and this is the alarm for its return.
- `[measured]` S3 product image (`nm` / `size -A`, the same credential-free build): main stack `_stack_start − _stack_end` = 86,344 B (unchanged), `.bss` 241,032 B (unchanged), `.data` 23,840 B (unchanged) and `.text` 918,977 B (919,457 B in round four, −480 B — the two copies of `mac_of` / `seed_from` in `safe.rs` are now one). App partition 1,135,984/6,291,456 B (18.06%, −480 B); C6 954,256/1,048,576 B (91.00%, +288 B). One boot log from each board after flashing: the S3 prints `[DISPLAY] stack peak 18932 B of 86344 B`, reports a healthy watchdog, joins its saved network and gets an address with its second core peaking at `2016 B of 24576 B`; the C6 prints `[HEAP] 65536 B`, takes the offline path (no saved network), breathes its lamp and reports the same healthy boot.

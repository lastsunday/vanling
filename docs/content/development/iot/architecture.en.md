+++
title = "Architecture"
weight = 10
[extra]
source_file_hash = "e6c76dc77c391c2b0267c070aff9b895507a8c0d"
translated_at = "2026-10-31T00:00:00Z"
+++

# Architecture

## Overview

A pipeline-logic system whose trigger points are intents.

### Flow

1. Chip init takes the peripherals (buses and pins), logging, the heaps (radio, other) and registers the heap-free hook
1. Wire the board from those buses and pins: the board object, the timer source, the software interrupt
1. Create and start the RTOS from the timer source and the software interrupt
1. Add the radio's hard init (`iot_chip_esp::ungate_radio()`)
1. Take the persistent store
1. Merge the product run and the network run into one task
   1. product run
      1. take the output-surface list from the board
      1. take the input-event list from the board
      1. take the motion events, their enabled state and their capabilities from the board
      1. take the audio events and their enabled state from the board
      1. record what each capability demands
      1. take the speaker object and its enabled state from the board
      1. take the camera object and its enabled state from the board
      1. create the renderer
      1. walk the surfaces, build a lamp or screen renderer per role, and hand the camera to the panel
      1. create the device manager, handing it the capabilities, demands, parameters and overrides
      1. create the configurator, the speaker handle and the demand handle
      1. start the main flow from the speaker handle and the demand handle
         1. speaker handle and software interrupt
            1. the clock-driven sound-playback engine
            1. the input-event engine
            1. the render engine
         1. speaker handle, no software interrupt
         1. no speaker handle
   1. network run

## Layers

`iot-core (pure logic) → iot-ui (page rendering) / iot-chip-esp (esp family runtime) → iot-bsp-esp (per-board wiring) → iot-app (single-task binary)`

- **Wiring lives only in the `bsp/` board modules**: fixed pins inside `Board::new(Peripherals)`; business code must not reference pin numbers.
- **iot-bsp-esp has two layers of components**: `components/` holds real components (chip drivers, parameterised by bus/pins, board-agnostic, e.g. `button`/`ft6336`/`pca9557`/`st7789`/`ws2812`), and `virtual_components/` holds abstract components (real components assembled into a virtual device, e.g. `Screen` assembling an ST7789 panel, its backlight and the frame buffer into a `PageSurface`). The board module only picks pins and assembles; the components themselves are shared across boards.
- **iot-ui is the page layer**: `Canvas` owns only the frame buffer, its dimensions and its ground; `Surface` adds a borrowed `&Diagnostics`, the light instance, FPS, the debug switch, the SPL offset and an optional platform stack probe, and pages draw in immediate mode. It names no chip, board or sensor. `bsp-esp` owns the frame buffer, DMA, panel and camera sampling, then ships the page bytes; pages are therefore host-testable without a panel.
- **Each component is its own feature**: one feature per component (`button`/`ft6336`/`pca9557`/`ws2812`/`st7789`/`screen`, gated once at the `mod` in `components/`, absent = file does not exist); a board feature is an aggregate list of components (see the module axis in `features.md`).
- **iot-chip-esp**: chip init / logging / RTOS / panic definitions; the app-level entry macro `#[esp_rtos::main]` stays in app `main.rs` (esp-hal/esp-rtos are feature-gated in the app purely so the entry point resolves).
- **iot-bsp-esp**: board differences converge in the `type Board` alias, the `HEAP_BYTES`/`FRONT` constants and the feed's landing (`Feed::Interrupt`/`Inline`) inside the per-part `start_executors`; one `#[cfg(feature)]` per board and per part respectively. The boot sequence itself is shared and carries none.
- **iot-app**: the application layer, product name `vanling`; zero chip dependencies in business code; flash/RAM budgets go through linking and partitions, not the type system.
- **Naming convention**: board layer uses board names, application layer uses the product name `vanling`; never duplicate.
- **Pure-logic modules used only inside bsp stay in bsp**: `hardware.rs` (the boot inventory) knows neither board nor chip and is host-testable. The sound catalogue (which `Sound` is which `.mid`, and how it becomes a frame) lives in `iot-app`: a board's `Speaker::play` takes a `Box<dyn SampleSource>`, frames rather than a name. Reading-to-glyph arithmetic, the font and page layout moved with the page layer into `iot-ui`; the peers left in `core` (`overlay`/`stamp`/`keyboard`) are there because both the pages and `intent` read them.
- **main entry**: dispatches per family feature, one entry block per family (esp family alias `esp32c6`).
- **`iot-tool` sits outside that chain**: the only crate that is not device code, reading and writing this repository's source files (currently just `midi`, which builds the sound catalogue's `.mid` files). **No dependencies** — a change on the device side must not be able to break the thing that builds the device side's assets. The only bare-metal `--workspace` left is the one in `lefthook.yml`, which excludes it; every `moon` task names `-p` and never reached it anyway. Its entry points are `moon run iot:test-tool` and `moon run iot:gen-midi-assets`.

## Inside `iot-app`

`app/src/` is split by domain; the composition root only wires:

| Path | Responsibility |
| --- | --- |
| `lib.rs` | composition root: take capabilities, build `DeviceManager`, start the four futures |
| `main.rs` | chip bring-up, executor choice, family entry |
| `boot.rs` | build-time configuration (`IOT_BOOT_PAGE` and overrides) |
| `workers.rs` | the whole coordinator ↔ worker vocabulary: `Workers { input, speaker, net }`, `Latest`/`PageWatch`, the capture window and command queues; the network command vocabulary lives in `core/state/network.rs` |
| `pipeline.rs` | the coordinator `run_loop`: apply intents, publish state, forward commands, wait for the next frame |
| `surface/mod.rs` | `Render` registry: one `ScreenRenderer` / `LampRenderer` per role |
| `frame.rs` | `FrameClock` (logical frames, including catch-up) |
| `input.rs` | input worker: polling, demand scheduling, gesture recognition |
| `audio/{mod,dispatch,capture,sounds}.rs` | playback worker, state→command, the capture window's driver side, sound catalogue |
| `net/{mod,link,setup,provision,survey,store}.rs` | `#[cfg(feature = "wifi")]`: the network worker — stack, link, panel-side setup, AP services, survey, credentials |
| `bench/replay.rs` | the product-reachable panel replay (`IOT_PANEL_BENCH`) |
| `bench/probe/`, `bin/` | build-time probes and thin entries; not on the product path |

Dependency directions (`iot-tool/tests/module_boundaries.rs` asserts them; the compiler does not
check module cycles inside a crate):

```
main → { lib, workers }
lib  → { boot, pipeline, workers, net }
pipeline → { workers, surface, input, audio, bench::replay }
net      → { workers, input }
audio    → { workers }
input    → { workers }
workers  → ∅
```

- `workers` carries no `wifi` gate: the host build still holds `Net`, nobody answers
- `workers` never reaches a worker: it knows only `core` types, so `audio`/`net`/`input` can depend on it one way
- `net` does not reach `surface`/`pipeline`: the network worker reads the page and commands from `workers`, not device state
- `bench::probe` stays off the product path: only `bin/*-probe.rs` reference it
- `core` does not know `iot_app`

## Composition (three orthogonal axes)

Hardware (board feature) ⊥ module (module feature) ⊥ capability (`iot-core` trait). Modules only see capabilities; boards only provide them.

- **Composition point = bin board manifest**: capabilities injected by move, generic over capabilities, shared by boards with the same capability set; enabling a module that lacks a capability is a compile error, never silent.
- **Product tier = feature alias**: the tier fixes the module set, the board fixes the supported set, and the intersection is trimmed and checked at compile time.
- **Module configurability** = feature switch + const parameters.
- **Runtime-pluggable render layer**: `iot-core`'s allocation-free central `RenderController` (reconcile closure callback) plus `iot-app`'s heap `Render` registry (`embedded-alloc` 32KB heap). Renderers are registered beside their surface at start-up and all live in the same render task; there is no cross-task bus.

## The demand model (what / how often / how sensitive)

The current firmware is a **board bring-up validation**, not a product; the structure has to accommodate a later application promoting any component to a primary interaction or computation, and that path is allowed to run that component at its maximum capability.

A demand is three independent sentences, and they must be expressed separately:

| Dimension       | The question it answers    | Type                                                  | What it decides                                                            |
| --------------- | -------------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------- |
| **Mode**        | Whether it produces at all | `Mode`: `Off` / `OnChange` / `Continuous`             | Whether polling happens, whether output is clocked                         |
| **Cadence**     | How often                  | `Claim::period_ms` (0 = the board's cadence)          | The floor on the poll period                                               |
| **Sensitivity** | How stale still counts     | `Sensitivity`: `Primary` / `Secondary` / `Background` | Nothing on this board (below); it only arbitrates and names itself in logs |

And "how to acquire" — the register set, the FIFO, the on-part engine, the transport — is the acquisition configuration (`MotionConfig` and its siblings), selected by the driver; a page never names it.

This separation is exactly why "low fidelity + high cost" is inexpressible: a page has no way to ask for a worse picture in order to save resources.

Fusing the two into a single ordinal "level" welds "what is wanted" and "how it is acquired" together. Stepping down then looks like both lowering fidelity and saving resources, when stepping down is not the same as spending less.

`Sensitivity` moves no cadence on this board, and that is the hardware's doing.

Android's `max_report_latency` presumes events are sampled on schedule and only their report waits in a FIFO, so "sample often, report late" are two independent statements.

This board's IMU has no FIFO, so reporting late can only become sampling less often — which is what `period_ms` already says out loud.

A "class to milliseconds" lookup therefore cannot land here: it would only scale `period_ms` by a coefficient and take a max.

Two shapes taken from Android's side, copied rather than reinvented:

- A sensor's `minDelay` must be 0 for on-change and −1 for one-shot. "How often" is not a question every reporting mode can answer.

  The motion device on this board is on-change in name only: it is continuous polling with the part's own No-Motion engine turning that into change events, so `MotionConfig::for_mode` maps both `Mode::OnChange` and `Mode::Continuous` to the same `PolledRegisters`.

  That is not merging two modes into one — what the consumer wants does differ in time (one wants changes, one wants a clock); the wiring that produces them is simply the same, and "how to acquire" is the part's answer rather than a field of the demand.

  `WakeDetector` is still in the configuration set, but there it is a rung the driver explicitly refuses on this silicon, not where `OnChange` lands. Other components need the reporting mode asked first.

- `power` is nearly always above the datasheet figure, and the assumption that several sensors together draw no more than the sum of each is an additivity claim. This board has no current meter, so it cannot be verified: only the executor column may be summed, `micro_amps` stays empty and is not summable.

### Arbitration takes the maximum, not a priority

When several consumers declare the same component, the largest requirement wins; they do not each decide for themselves.

That is Android's sensor-stack rule ("If one application requests one sensor with a maximum reporting latency of 0, all applications will receive the events from this sensor in continuous mode"), and the shape ESP-IDF uses for PM locks.

Priority arbitration is complexity you only add for exceptional cases, so there is none.

### Reconfiguring must not lose data, and flush is separate

Android's hard constraint: "A sensor must be able to be reconfigured at any time using `batch()` without the loss of sensor data."

Applied here: `ACCEL_LSB_PER_G` and `GYRO_LSB_PER_DPS` are `const`, so any change to full-scale or ODR silently invalidates the core-side conversion — they have to become runtime values.

At the switch instant the pairing of a new scale with a stale sample must be discarded for that one sample, not converted.

Switching also needs a `flush()` to drain what has already accumulated, or what gets lost is valid data.

Dropping samples is acceptable under non-wake-up semantics (Android: "The non wake-up sensors need to overwrite their FIFOs when they're full till the AP wakes up"), but it must be bounded and reported, never silent.

### A demand is a third thing next to the existing phases

`AudioPhase` (`Idle`/`Recording`/`Stopped`) and `PlaybackPhase` (`Idle`/`Playing`) are UI semantics. `AudioPhase`'s doc says it decides "which envelope the panel draws".

They are neither a mode nor a cadence: the same mode can carry different UI semantics and the same UI semantics different modes. All three cross-reference each other in comments; none are merged.

## Changing and transporting demands

### Changing: reuse the existing intent pipeline

`apply_business` is already the single place state changes; the only gesture-driven page change is `BusinessIntent::NextPage`, and `SplashOver` lands on the build's page when the splash ends.

A new scenario is a new `BusinessIntent` variant; switching cadence at runtime is another variant. `EffectiveDemand` is re-derived by `propose_demand()` right after `apply_business`. No new mechanism.

A capability that carries rules (multi-step transitions, invariants, edges across fields) gets its own module. `BusinessIntent` is grouped by domain into `Page`/`Network`/`Audio`/`Playback`/`Light`, and `apply_business` is only an index over them; each domain matches its own group exhaustively, so a new variant is one arm in the module that owns it and a missed one is a compile error. Cross-domain actions (a sound, a lamp notice) come back as `Effects` for the manager to apply; domains never call into each other.

`state/` holds one file per domain: `mod.rs` is only the barrel and `MAX_LIGHTS`, `wiring.rs` holds `Backed`/`Setup`/`SurfaceRole`, `device.rs` holds `DeviceState` and its queries, and `manager.rs` holds construction, routing and accessors. Domain files import explicitly and never use `use super::*` outside tests — a prelude lets a new symbol cross a module boundary unseen.

### Transport: coordinators and workers

`run_loop` is the **coordinator**: it only does non-blocking work — apply intents, publish state, hand over the windows and commands already ready, wait for the next frame. Anything that waits on a driver, an I2C bus or a network stack belongs to a **worker**:

| Worker                 | Owns                                | Executor                                   | Handle |
| ---------------------- | ----------------------------------- | ------------------------------------------ | -------- |
| Render layer / camera  | surface, camera                     | cooperative                                | `Render` / `Diagnostics` |
| `input_task`           | input devices and audio capture     | cooperative                                | `Input { to, from }` |
| `control_loop`         | codec, DMA                          | cooperative                                | `Speaker { to }` (completion returns on the intent bus) |
| `net::serve`           | stack, link, AP services            | cooperative                                | `Net { commands, page, scan }` |
| `feed_task`            | the microphone clock                | **Priority2 interrupt, separate executor** | ⚠️ **deliberately dependency-free; none may be added** |

The handles live together in the composition root's `Workers<'a>`: one field per worker, so a second protocol adds a field rather than a `TO_X`/`FROM_X` pair at the call site. Directions are named by role (`to` for the worker, `from` for the coordinator); the peer's name stays in the peer struct.

The shape follows the semantics, not convenience:

- **Commands** use a `Channel` (`Net.commands`, `Speaker.to`). Each pass the coordinator hands over only what the worker has room for; a command that found none waits for the next pass and is never dropped.
- **Levels** use `PageWatch` (`Net.page`): the page is idempotent current state, and consumers wait on the "back to / away from a page" edge.
- **Latest values** use `Latest`/a window (`Net.scan`, `Input.from`): a scan list or a capture window only has a newest copy worth keeping, so it overwrites instead of queueing.

The `NetworkCommand` vocabulary stays in `core/state/network.rs`; the handle only carries it. The coordinator forwards `DeviceManager::take_command()` directly instead of translating each variant, and setup commands carry the `Front` they were issued on so the worker can drop a previous session's leftovers.

**The render layer costs nothing**: `Diagnostics` in `core/src/diagnostics.rs` is already "one snapshot fusing every subsystem's state", carrying `page` / `motion` / `audio` / `playback`, and `DiagnosticsSink::consume` is already the "state into consumers" channel.

The camera's chain runs end to end today:

```
DeviceState.page → light_diagnostics() → SlotAppearance::Diagnostics
  → RenderController::reconcile → ScreenRenderer::on_appearance
  → Screen::consume → camera.pause() / released()
```

So `Demand` only has to grow on the same snapshot, and the display and camera parts add nothing.

**A demand is shared state, not a message.** A demand is idempotent aggregate state rather than an event, and only the latest value means anything.

Carrying one on a channel would need a one-deep queue, change detection on the writer, and a retry for a refused send — three mechanisms that exist only because a channel cannot hold a value.

The shared slot's shape follows from that constraint: embassy-sync's blocking mutex is shared-read-only (the closure `RawMutex::lock(f)` takes gets no `&mut`), so a `RefCell` sits inside the slot.

`Intent` is the opposite case — an event — so those still travel on a bus.

The capture window is a third shape: a **window**, not an event (only the newest one means anything), and not shared state either (nobody writes a common slot — the board hands over the frame the driver just folded, during its own poll). It therefore rides a single-slot channel: `HasAudio::take_audio(&'static dyn CaptureSink)` gives the board the destination, `input_task`'s poll publishes into it, and `run_loop` takes from it after the intents.

The second reason is width: every slot of a `Channel<T, N>` is allocated at `size_of::<T>()`, so the slot width is set by the widest event variant — a capture event carrying an envelope (1.76 KB) would price the eight-slot `INTENT_BUS` at 14,184 B of internal DRAM for the one frame the panel reads. Without that variant `INTENT_BUS` is 896 B and the window itself 1,824 B (recorded in the [Wi-Fi record](@/records/iot/wifi.md)).

The ordering falls out of it: `input_task` polls in wiring order with the capture last, so a tap on the record control in the same tick latches the frame the previous pass drew.

The handle is built at the composition point and injected, reusing the shape of `playback::share()`. That honours the convention in `input.rs`: the handle is passed in, and no consumer reaches a global by name.

### feed_task is a clock claimant, not a demand consumer

This is the easiest thing to get backwards.

The microphone's BCLK and WS are produced by the I2S TX unit and the RX slaves to them (the board module builds the RX without BCLK/WS). The feed's existence is what supplies the microphone's clock — silence is a voice too, and the ring is always kept fed.

So `feed_task` holds a clock claim it never releases, and demands do not flow to it.

Demands decide only the cadence of folding on the capture side, and that folding happens in `input_task`. The microphone's clock is therefore always present and the clock topology does not have to be touched.

Cost and coupling: the ring is `BYTES_PER_POLL * 3` (about 60 ms), so going about 60 ms without a drain starts dropping samples.

What matters more is that not draining does not only drop samples: once the ring is full the I2S engine halts on its own. So a rarer poll is not a staler reading but a stopped clock.

Measured with capture batched to 30 s, the ring filled inside 60 ms, `capture ring 10842 B deep` fired, and over 90 s `CaptureWatchdog` brought the chain back exactly once, at second 91.

So this repository's answer is that the capture path accepts no poll slower than the board's cadence: `Es7210Rx::accepts_slower_poll()` answers `false`.

`CaptureWatchdog` detecting stalls by "was there progress" is therefore not a problem — it never gets to see a deliberate long interval, because there is no such interval.

### Timing

`run_loop` writes the slot → `input_task` reads it on the next `INPUT_BASE_MS` tick. "Scene change → demand in force" is one tick plus however long `run_loop` is held.

#### The ceiling is the longest single hold, not a percentage

The utilisation bounds in Liu & Layland, CFS and RT groups all constrain the share of time a preemptive scheduler spends running tasks.

A cooperative executor is not in that model. It is fair per poll and starves nothing by volume. What it cannot do is preempt.

One non-yielding poll holds the executor for its whole worst case. So what has to be bounded is the single hold.

The object being bounded is the whole tick, not the largest contributor within it. Every source runs inside one non-awaiting loop, and the panel write is not in that loop at all.

Measured (S3, 8 windows of 5 s) a single tick runs 17–25 ms. `MAX_SINGLE_HOLD_MS = 30`, one poll period of margin above the measured bound. `[INPUT]` prints `worst single hold N ms (bound 30 ms)` every window, so a breach is visible without anyone comparing figures.

These 17–25 ms figures were measured at **80 MHz**. The product S3 now boots at 240 MHz through `chip_init_for_product()` (`chip_init_at(CpuClock::_240MHz)`); `chip_init()` remains the 80 MHz probe entry. This tick measurement has not been re-run at 240 MHz, so the numbers above are still 80 MHz figures and only get shorter there. For the 240 MHz evidence and its cost, see [Changing the CPU clock at runtime](@/records/iot/clock.en.md).

The total is not the risk: Ambient spends 94.8% and behaves. A single 25 ms block is. Playback feeding runs on its own higher-priority executor, driven by DMA, so such a block cannot reach it.

#### Two configurations are refused by implementation

They answer `UnsupportedConfig` rather than a plausible byte:

| Configuration  | What is missing                                                                                                                                                                                                                                                                             |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `WakeDetector` | The accelerometer alone changes both CTRL7's power-mode fields and which of the part's two ODR tables applies. The datasheet publishes both tables' frequencies but not the index each uses, and the engine windows convert by that index, so a guessed index is wrong by exactly that much |
| `Fifo`         | The FIFO registers start at `0x0B`, overlapping the addresses this driver writes motion calibration through — which is why the CTRL9 command protocol exists at all, so this is a sequencing problem rather than a register write                                                           |

Each is reachable by "opening the register table and adding one encoding". `Off` is not a configuration but a mode, met by the consumer not polling.

The driver's wiring and register-level detail is in [motion.md](@/development/iot/motion.en.md).

#### The configurator, arbitration and wiring

The configurator is the one translation between a mode and a register.

`Configurator::configuration_for` turns a demand into what each part must be left in; the mapping lives in core as `MotionConfig::for_mode`, and the part's capabilities are handed to `Configurator::new` by the board at the composition point.

The split is firm: which mode needs which configuration is the product's to say, what this silicon offers is the board's.

A refusal is part of the return type. A configuration that cannot be entered comes back as `Unreachable { key, mode, config }` — all three named, so the caller has no "quietly downgrade" option.

A refusal is written into the demand: the device it names leaves the demand the device is held to (`EffectiveDemand::without`), so nothing downstream reads a claim the part cannot keep.

`Off` is not in that set — it is met by not collecting and answers `None` rather than `Err`. The interaction path in a demand is never affected.

The diagram has `Logic` calling `Render` and `Configurator` separately.

But the parts live in `input_task`, which `run_loop` cannot reach, so the configurator sits with the parts too: it needs the board's declared capabilities to resolve a mode, and a refusal should be reported against the part that actually refused.

The configurator is realised alongside the demand, and `[DEMAND]` reports the resulting schedule.

The demand is keyed by device, not by enumerated fields. `EffectiveDemand` is a `BTreeMap<DemandKey, Claim>` where the key is a device type plus which of them is meant, so `claim`, `without` and `Override::apply` are all lookups and adding a sensor changes no line of the demand machinery.

Two devices of one kind can be demanded apart (`DemandKey::instance(Audio, 0)` and `…, 1`).

The device list is the board's. `DisplayPage::demand(&self, keys)` walks the devices the board declares and calls `claim_for` per key. That match is exhaustive, so a page cannot forget a device and inherit a demand nobody chose.

A device the board did not declare is absent from the demand and reads as `Off`, so four boolean gates are no longer needed on top.

The demand state is held, not re-derived. `DeviceManager` holds `demand` beside `state`. The one place it is derived is `propose_demand()`, and every write to `page`, the device list or `override_` is followed by it.

There is exactly one reason for holding it: the configurator needs something to change, and `commit_demand` writes the achievable demand back to the same place so everything downstream reads the amended version.

Arbitration: `resolve_speed` takes the highest requirement among collecting devices; a test holds it to being per device rather than one constant for the whole map.

Wiring:

| Consumer      | How it gets the demand                                                                                                                                                                                                                |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `input_task`  | the shared demand, borrowed once inside a critical section each tick, scheduling by `PollEntry.key()` (the cadence also asking the driver `accepts_slower_poll`)                                                                      |
| `run_loop` | the writer; the page is the product's declaration, so it is written here rather than each consumer deriving its own                                                                                                                   |
| camera        | `DiagnosticsSink` (an existing channel), gated on "the demand no longer asks for frames" rather than on "the page changed"                                                                                                            |
| `feed_task`   | deliberately not fed. Its existence supplies the microphone clock, so it is the source of the audio path's clock rather than a consumer of its demand; gating it would make the one thing nothing may switch off switchable by a page |

Cadence only ever moves outward: the board's cadence is the floor the debounce windows are measured against, and a demand has no standing to shorten it. The floor and the cadence in force are two fields (`board_cadence_ms` and `cadence_ms`) because merging them means a single stretch can never be given back.

Some sources may not even move outward: `accepts_slower_poll()` defaults to `true` and `Es7210Rx` answers `false`. The capture ring is drained only by `sample()`, so batching to 30 s fills the ring and the I2S engine halts. The judgement belongs to the driver rather than to core guessing which peripherals need it.

The codec half of the audio device's `Off` is still not wired: the consumer half is (`set_collecting` parks the I2S chain), but ES7210's register-level analogue power-down bit is not in the code.

The board has no pin-level microphone power control, and shutting only the ADC does not reduce folding cost (the DMA runs either way, and folding zeros costs what folding data costs). That would build exactly the "low fidelity at high cost" quadrant this section exists to avoid.

#### What the probe reports, and what it cannot

`aec-probe` is the only probe that wires no peripheral: echo cancellation is arithmetic on two streams, and a bring-up would only add noise to the measurement. What it reports is that the CPU cost depends on the tap count and the clock and on nothing else, which is what makes it safe to reserve time for it in the budget before wiring it in. It reports the first, mean and worst tick, the share of 20 ms, and the delay search, the filter and the meters timed separately — they differ in cost and, more to the point, in what they trade: the delay search is one constant against accuracy, and a tap is accuracy against both of the others.

Its ticks run back to back rather than on the 20 ms schedule, so what it reads is a **floor** and not the loaded figure. The room buffer is on the heap: a 10 KB local would sit in the main task's stack, which is the shared resource this document accounts for.

`echo-probe` is its exact counterpart: **the only AEC probe that touches a peripheral**, because it answers the one thing `aec-probe` structurally cannot — whether the wiring is right. The speaker plays, the microphone listens, the canceller is fed both, and it drives the product's own feed and capture poll. It reports ERLE, delay, the loudest tap, pair count, reference log depth and worst hold.

**Those figures have to be read together**: ERLE is flatly zero when the pair count is (a log that does not move is not measured), and it can look healthy while the reference log has lapped. Both print the same zero. `EchoReport` binds them together so a caller cannot read them apart.

It also reports something `aec-probe` cannot: **where the delay is locked**. The delay search on this board used to jump across a 64-sample window (`None → 16 → 50 → 6`), dragging the filter's window with it every 256 ms, so none of its 64 taps ever converged on one room. With the lock in place, delay moved exactly once across two measurements. As for how much actually gets cancelled, neither probe has produced a number on hardware yet — see the [echo cancellation record](@/records/iot/aec.en.md).

#### How the cancellation layer is wired

`Canceller` is two samples in and one out, and making those two samples be _about the same instant_ is not its problem. Two DMA transfers share one clock, each drained at its own cadence, and the reference is held back by a transmit ring's runway. So the pairing is its own layer:

| Type           | Lives in | What it is                                                                                                                                                       |
| -------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ReferenceLog` | core     | holds reference samples against the play cursor rather than the write pointer, and reports lapping — a pair read has to be a segment that has actually been sent |
| `Decimator`    | core     | carries a **phase** that each side reports for the other to match, rather than both guessing                                                                     |
| `EchoStage`    | core     | the pairing and cancellation of capture against reference, producing `EchoOutcome`                                                                               |

The phase is the one thing in here that can be wrong by one tap in silence: both sides take 0 today because the transmit arms before the capture, and **a board that arms the microphone first has to report the offset to both sides**. The delay search covers the difference; it is not automatic.

`demand-probe` walks the motion device's (configuration × cadence) grid, settling each cell before measuring it, and reports three numbers: **bus transactions per second** from the driver's own counter, the **executor occupancy** as the wall-clock share of the window spent polling, and the **number of polls measured**.

The third number is what lets the first two check each other rather than only be believed.

**It cannot report current** — that needs a meter across the board, and a datasheet figure for a part on someone else's board is not a measurement of this one, so the `micro_amps` column stays empty.

**Cells the driver has no register encoding for print as "unreachable" rather than being skipped**, so the log is as loud about what was not measured as about what was.

Two companions: `SUPPORTED_CONFIGS` lists only what the driver can leave the chip in (a test holds it to `config_for`), and `Board::new_motion_only()` provides an isolated bring-up, because something measured while another device is running is evidence about the wrong thing.

#### Executor occupancy figures cannot be summed

One cooperative executor gives its time to whichever task is awake: batching motion to 30 s drops the input side from 49.5% to 35.5% while the panel side simultaneously rises from 45.3% to 58.5%.

The panel row is a residual, not an increment. Adding it to a polled device's row counts the same microseconds twice, and the sum comes out over 100% of a core.

Likewise, a number measured with the device running alone on the board (the probe) and a number measured by differencing two whole-device builds are not the same unit. Added together they give a lower bound rather than a total — the three devices measured in isolation sum to 43.2% against a measured 51.0% for the same build.

Take the more expensive side: a device's cost rises with what runs beside it, and the cheap reading is the first to stop being true.

This yields one rule for measurement: to ask what throttling does to the worst block, the measurement must change the cadence and not the mode. Changing both leaves open that what was saved came from switching off a whole device rather than from throttling.

The per-device figures and their individual basis labels are in the [implementation records](@/records/iot/_index.en.md).

## The page cycle walks one ordered list

`DisplayPage::CYCLE` is the definition of the order (`Ambient → Attitude → Audio → Speaker → Camera → Network`).

`next_page` walks from just after this page, takes the first page the board's capabilities back, and wraps at the end.

The boot page (`Setup::boot_page`) is clamped by the same capability test, so one full turn from any page visits every available page once and returns to where it started.

The capability test is one column of each page's row (`PageSpec::needs`, with `is_backed` only looking it up), and every column asks the same `Backed` record: motion, audio, playback, camera and the link are five wiring facts written once into the `backed` field of `Setup` at the composition point. The first four come from the board's `take_*` calls; the link comes from the entry point, because the station only starts once the scheduler is up — by the time `run` has the board, the radio has already been taken. So `iot_app::Network` is shaped like `Feed`: **a wiring fact passed in as an argument**, not something `run` turns round and asks the board about. Surfaces work the same way: every `WiredSurface` handed out by `HasSurface::take_surfaces()` carries its own role (`Pages` or `Lamp`), and the entry point passes them to `run` alongside `clock`/`feed`/`network`; `surface::wired` builds the renderers and drops the `surface_roles` table in the same pass — no surface is ever asked back whether it draws pages, an empty slot never appears as a handle, and a renderer outnumbering the slots trips one assertion in that pass.

This rule exists because of a defect it fixed.

`next_page` was 18 hand-written jumps, and `Camera`'s leaving branch returned "a page the board can back" (`Attitude`) rather than the start, so `Ambient` was skipped for good: a full turn with every capability measured as `[Attitude, Attitude, Audio, Speaker, Camera]`, `Ambient` gone and `Attitude` twice.

Written as pairs, a cycle drops the page that closes it; a single list walked with a wrap cannot.

A test in `iot-core` walks capability combinations × start pages to hold that (`every_page_the_board_can_back_is_shown_on_the_way_round`) — restoring the old `Camera → Attitude` makes it fail and print the string above.

## Logging

Only the `log` facade (`log::info!` etc.); the output channel is initialized by the family crate `iot-chip-esp` (esp uses `esp_println::logger`); business code must not use `println!` / `esp_println::println!`.

A log line can only be believed if it reports the configuration actually in force, not the one that was asked for.

`[DISPLAY]`'s `asked` used to report the page's own demand rather than the one in force after the override, because the snapshot took the demand from state while the override sits on the manager.

A log reporting a configuration nothing is running is worse than no log.

## Adding a board

1. Add the board module to `bsp-esp/` (Board + HasXxx traits) with feature gating.
2. Add the board's `FRONT` constant in `app/src/main.rs` (and `HEAP_BYTES` where the part's memory differs); the feed's arm is already there per part.
3. Forward the app feature `iot-bsp-esp/<board>`.
4. Add the board to the `platforms` default JSON in `reusable-iot-build.yml` and the `platforms` input of `iot-dev-release.yml`. Xtensa boards need `use-xtensa-toolchain: true` and go through the `build-s3` task: the espup native `esp` 1.98.1.0 toolchain on CI/release, with a same-version Docker image locally on macOS Intel
5. Verify: `cargo build -p iot-app --bin vanling --target riscv32imac-unknown-none-elf --no-default-features --features <board>`

## Adding a chip family

The esp family is fixed as `iot-chip-esp`/`iot-bsp-esp`; adding a non-esp family:

1. Add the `iot-chip-<family>`/`iot-bsp-<family>` crates.
2. Gate that family's entry dependencies behind the app family-alias feature; add the family entry block to `main` (`#[cfg(feature)]`).
3. Toolchain target + `rust-toolchain.toml` + `platforms` in `reusable-iot-build.yml` (`target`/`use-xtensa-toolchain`) and the `platforms` input of `iot-dev-release.yml`.
4. Switch `moon`/`lefthook` `--workspace --target` to the firmware scope; the toolchain target flows into moon tasks via `CARGO_ESP_TARGET` (set in `.envrc`/CI env and workflow inputs; tasks take `- '$CARGO_ESP_TARGET'` in `inputs` to keep cross-chip build caches isolated).

+++
title = "Architecture"
weight = 10
[extra]
source_file_hash = "0dfa21deb67c18fcd9f591a1a8829707fecd4908"
translated_at = "2026-11-26T09:55:45Z"
+++

# Architecture

## Overview

A state machine whose trigger points are intents, advanced one frame at a time.

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
      1. walk the surfaces, build a lamp or screen renderer per role, and hand the camera to the screen
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

`iot-core (mechanisms, driver seams, pixels and glyphs) → iot-chip-esp (esp family runtime) → iot-bsp-esp (per-board wiring) → iot-app (product pages and state machine)`

- **Wiring lives only in the `bsp/` board modules**: fixed pins inside `Board::new(Peripherals)`; business code must not reference pin numbers.
- **iot-bsp-esp has two layers of components**: `components/` holds real components (chip drivers, parameterised by bus/pins, board-agnostic), and `virtual_components/` holds abstract components (real components assembled into a virtual device, e.g. `Screen` assembling an ST7789 panel, its backlight and the frame buffer into a `ScreenSurface`). The board module only picks pins and assembles; the components themselves are shared across boards.
- **`core::ui` knows pixels and glyphs only**: `Canvas` owns the frame buffer, its dimensions and its ground; `font` cuts glyphs, `typeset` breaks and prohibits lines, `layout` is a box model in Flutter's vocabulary (flex/wrap/stack/scroll) and `panel` derives the panel's measurements from the face. It names no page, snapshot, network, product or chip. The page shell (`shared::surface::Panel`) and the page furniture (`shared::page`), the page views and the network geometry live in `iot-app`, so they stay host-testable without a screen.
- **bsp draws but names no page**: `bsp-esp` owns the frame buffer, DMA, panel and camera sampling, and hands `ScreenTarget`/`ScreenContext` to the app through `core`'s `ScreenPainter` seam. Page state and the repaint gate live in the app; bsp knows neither `FrameSnapshot` nor any page type.
- **Each component is its own feature**: one feature per component, gated once at the `mod` in `components/` (absent = file does not exist); a board feature is an aggregate list of components (see the module axis in `features.md`).
- **iot-chip-esp**: chip init / logging / RTOS / panic definitions; the app-level entry macro `#[esp_rtos::main]` stays in the app's bin entries (`main.rs`, `bin/*.rs`).
- **iot-bsp-esp**: board differences converge in the `type Board` alias, the `HEAP_BYTES`/`SETUP_HOST` constants and the feed's landing (`Feed::Interrupt`/`Inline`) inside the per-part `start_executors`; one `#[cfg(feature)]` per board and per part respectively. The boot sequence itself is shared and carries none.
- **iot-app**: the application layer, product name `vanling`; zero chip dependencies in business code; flash/RAM budgets go through linking and partitions, not the type system.
- **Naming convention**: board layer uses board names, application layer uses the product name `vanling`; never duplicate.
- **Pure-logic modules used only inside bsp stay in bsp**: `hardware.rs` (the boot inventory) knows neither board nor chip and is host-testable. The sound catalogue (which `Sound` is which `.mid`, and how it becomes a frame) lives in `iot-app`: a board's `Speaker::play` takes a `Box<dyn SampleSource>`, frames rather than a name. The face, line breaking and layout live in `iot-core::ui`; the page shell, view layers and network words live in `iot-app`. The peers left in `core` (`geometry`/`light`, including RGB565 packing and inversion) are there because both the pages and `intent` read them; the key grid belongs to the Network page: its vocabulary is in `app::shared::intent` and its geometry in `app::services::network::shape`.
- **main entry**: the product binary entry (`bin/*-probe.rs` are their own probe entries); chip init, the heap, the executors and `Wiring` all live in `main.rs`, with the per-family branch in `start_executors`. The product and the panel bench share that path; the `panel-bench` feature only decides whether the entry finally calls `lib::run` or `bench::run`.
- **`iot-tool` sits outside that chain**: the only crate that is not device code, reading and writing this repository's source files (currently just `midi`, which builds the sound catalogue's `.mid` files). **No dependencies** — a change on the device side must not be able to break the thing that builds the device side's assets. The only bare-metal `--workspace` left is the one in `lefthook.yml`, which excludes it; every `moon` task names `-p` and never reached it anyway. Its entry points are `moon run iot:test-tool` and `moon run iot:gen-midi-assets`.

### Status vocabulary: four questions, one owner each

"How is it?" has four answers within one boot, and they do not share a lifetime. Mixing them turns a persisted fact into a runtime conclusion.

| Question | Type | Owner | Lifetime |
| --- | --- | --- | --- |
| Is this part built into the board? | `Backed` | `app::shared::model` | Decided once at composition, fixed at runtime |
| Did this part answer at boot? | `Presence` / `Evidence` | `bsp-esp::hardware` | Once per boot |
| Does this boot count? (across resets) | `ResetRecord` / `BootMode` | rule in `core::boot`, register in `chip-esp::boot` | RTC, survives a reset |
| Has this boot proved itself? | `Readiness` | `core::boot` | RAM, lost on reset |
| Does this part answer now? | `Health` | `app::shared::model` | Runtime |

`Readiness` is the RAM half: the first frame has been flushed to every wired surface and the network stack is up (or this build deliberately has no radio). The watchdog calls `mark_healthy()` once both arrive and clears the count — time alone is not evidence.

Names follow the work: a one-shot report is `*_reported` (`first_frame_reported`, `health_reported`, `chain_reported`), taking a fact in is `note_*`, sending it out is `report_*`, and setting a persistent marker is `mark_*`.

## Inside `iot-app`

`app/src/` is organised by vertical slice: each page is its own slice (a file for a small page, a directory for a large one), cross-page truth and vocabulary live in `shared/`, the composition root and the presentation layer stand apart, and wiring happens only in the composition root.

A page is where the device is, not a picture on a screen: `Route.needs` asks only what the board wired, and whether a panel exists is answered separately by `SurfaceRole::Screen`; a board with no panel still has a current page, and the lamp, demand and network setup are simply its other readers.

| Path | Responsibility |
| --- | --- |
| `lib.rs` | composition root: take the remaining capabilities, build `DeviceManager`, start every app worker, await the coordinator; `run` is the product entry, `run_with` takes one caller-boxed system and only `bench::run` uses it |
| `main.rs` | the product entry: chip init, heap, executors and `Wiring`, then `lib::run` or `bench::run` by feature |
| `services/` | the product's services (pages plus the device's own `nav`/`light`/`settings`), one vertical slice each: a small page is one file, a large one a directory (`model` / `controller` / `shape` / `view` as needed); `mod.rs` defines the `Service` every operating platform shares (`apply`) and the local platform's `LocalPage: Service` (`bind`/`publish`/`view`/`descriptor`/`tick`/`on_show`/`on_hide`), the `Services` holding every service and its one dispatch table (the private `ServiceId`: `Page(PageId)` or the device's own `Navigation`/`Light`/`Settings`), and the one `PageId → Descriptor` map; `Services::bind` is the input boundary: a transient page is asked directly; otherwise a button click or hold goes to navigation first (the click turns the page, the hold goes home), so no page can keep either; gestures are the page's alone; a board with no panel has no page to turn, so `Headless` keeps the click (the lamp); a page's input contract sits in its own `tests.rs` or at the end of its file, and the cross-page golden table lives in `services/tests.rs` |
| `services/{menu,input,attitude,camera,env,splash,text,widgets,lighting,headless}.rs` | single-file pages with one canvas: each page type's `view` declares the whole page; `text` also owns two independent scroll regions, and `menu`/`widgets`/`lighting` resolve hits through a `TagTable`; `headless` has no `view` and binds only the buttons and the lamp |
| `services/settings.rs` | the device's own record: the panel language `DeviceState.lang` and palette `DeviceState.theme`, written only when a page reports a `SettingsIntent` |
| `services/audio/{mod,tests}.rs`, `services/speaker/{mod,tests}.rs` | translation and drawing both in `mod.rs`; records more than one consumer reads, such as `AudioState`/`PlaybackState`, stay in `shared/model` |
| `services/network/` | gestures and controls in `controller/`: `mod.rs` is the gesture translation, the strip and the hit tests; `shape.rs` is the one geometry the list, key grid, strip and confirmation box share (drawing and hit tests both read it); `view/` is split by band (`header` / `content` / `overlay`, with the words in `text.rs`) and `shape::Band` decides the order they stack in; `NetworkView` lives in `shared/model/network_view/` because the snapshot has to name it |
| `shared/` | cross-page truth and vocabulary: only what more than one page reads, and the leaf of the module graph |
| `shared/model/` | records the workers write and the render loop reads, one file per domain; `device.rs` is the aggregate `DeviceState`, `shared/snapshot.rs` holds the `FrameSnapshot` and the `PageSnapshot` the pages publish, and `network_view/` is the Network page's view (owned by the page, copied by the snapshot, never read by the link) |
| `shared/route/{mod,nav}.rs` | the route table and `Navigator`: `PageId` is the destination, `route()` is the exhaustive match, `ALL` is the only order (the button walks it too) |
| `shared/intent/` | inbound messages: `InputIntent`/`BusinessIntent` and the questions each page asks, including the cross-page `Confirm`; the lamp's verb `LightAction` and the `BusinessIntent::Light { instance, action }` variant are defined here; `keyboard.rs` is the key vocabulary the Network page types with (`Key`/`Layer`/`Cycling`, the multitap window, the candidate character table per layer) |
| `shared/model/light.rs` | the light record: mode, time-based effect table and palette; `LightState::apply` lands a `LightAction` on the next mode, and `can` says whether the mode accepts that verb; `Effect` (`Steady` / `Breath` / `Cycle` / `Aurora` / `Candle` / `Pulse` / `Heartbeat` / `Lightning` / `Sparkle` / `Strobe` / `Police` / `Sos`) is drawn by one sampler in `iot-core::light`; `GlowScope` says which pages a breath may reach, and `Backlight` is the panel backlight's only source |
| `shared/snapshot.rs` | the product snapshot: one image fusing every subsystem's state, including the audio and playback snapshots and the `PageSnapshot` the pages publish |
| `shared/i18n/` | panel copy: `Lang`, `Msg` and the fixed words no language moves; a leaf that reads `core` only |
| `shared/ui/` | widget vocabulary: `State`/`Theme`/`Slot`, scroll state, keyboard, hit table and `widgets/`; reads no snapshot |
| `shared/page.rs` | page furniture: margins, line rhythm, `body`/`title`/`scroller` and the reading row |
| `shared/surface.rs` | the paint surface `Panel`: declaration → layout → draw; it reads a snapshot and no page |
| `state/manager/mod.rs` | the read side: ownership and construction of `DeviceState` (`Boot`, `DeviceManager::boot`), the owned `Services`, the per-page `DemandPolicy`, and the getters the coordinator and the render loop read |
| `state/manager/write.rs` | the write side: every write from the bus, a worker report or the clock's tick; records move only here |
| `state/env.rs` | build-time configuration (`IOT_BOOT_PAGE`, `IOT_LANG`, `IOT_THEME` and overrides) |
| `render/engine/` | surface-agnostic: `Differ` decides *what* to show (`differ.rs`), state→appearance lives in `projection.rs`, `appearance.rs` defines `Topic`/`Appearance`, `color.rs` steps the colour, and `set.rs` hands the appearance to the renderers that claimed it |
| `render/screen/` | the screen (`SurfaceRole::Screen`): `ScreenRenderer` (`mod.rs`), `painter.rs` calls `services::paint`, `repaint.rs` (frame drift and the repaint step) |
| `render/lamp/` | the light (`SurfaceRole::Lamp`): `LampRenderer`, one colour and nothing a screen would add |
| `render/wire.rs` | wiring: `wire()` turns the board's surfaces into `ScreenRenderer`/`LampRenderer`, and `RenderSet` is the handle this layer hands out |
| `runtime/` | the tasks beside the model: the coordinator, the feature workers under `feature/`, and the `workers/` handles they share |
| `runtime/system.rs` | the `System` contract: `tick` takes the manager and the clock, `intent` reads the state its intent already wrote; a worker handle is held by the system |
| `runtime/coordinator/{mod,pacing}.rs`, `coordinator/systems/*.rs` | the coordinator `run_loop`: four steps per frame (process intents, system sweep, present, wait); `pacing.rs` holds the frame clock and the next deadline |
| `runtime/workers/{mod,inbox,input,playback,network}.rs` | the whole coordinator ↔ worker vocabulary; `mod.rs` holds only the re-exports, and every handle's one home is `Runtime` in `runtime/mod.rs`; the network command vocabulary lives in `shared/model/network.rs` |
| `runtime/feature/{mod,input}.rs` | the feature workers; `mod.rs` is the one `wifi` gate. Input worker: polling, demand scheduling, gesture recognition |
| `runtime/feature/playback/{mod,sounds/}` | playback worker and sound catalogue; state→command lives in `coordinator/systems/playback.rs` |
| `runtime/feature/network/{mod,link,setup,provision,scan,store,watch}.rs` | `#[cfg(feature = "wifi")]`: the network worker — stack, link, panel-side setup, AP services, scan, credentials |
| `assets/` | the setup side's own bytes: the `vanling-` AP prefix and the setup portal HTML/SVG (the protocol stays in `core`) |
| `bench/{probe,replay}/`, `bin/` | bench-only drivers and entries: `probe/` is reached only by `bin/*-probe.rs`; `replay/` is the panel replay and the tap script that types text into the key grid (`point.rs` holds only the landing coordinates), reached only by `main.rs` through `bench::run` under the `panel-bench` feature, and compiled by the host tests. The product entry and the frame loop do not know `bench`; the `IOT_PANEL_BENCH` flag and the build-credential choice live in `state::env` |

The directions are held by a few honest rules in `iot-tool/tests/module_boundaries.rs` (the
compiler does not check module cycles inside a crate), not by an edge-by-edge map:

The layering is `lib → runtime → {render, state} → services → shared`; `assets` is a leaf shared by
`services` and `runtime`.

```
lib               → { render, state, runtime }
services/<page>   → { assets, shared::{i18n, intent, model, page, route, snapshot, surface, ui} }
services          → { services::<page>::controller, shared::{i18n, intent, model, page, route, snapshot, surface, ui} }
state             → { services::Services, shared::{intent, model, route} }
render            → { services::{descriptor, paint}, shared::{model, route, snapshot, surface} }
runtime::coordinator → { render, state, runtime::{feature::input, system, workers} }
runtime::system       → { state, runtime::feature::input }
runtime::feature::input    → { shared::{intent, model}, runtime::workers }
runtime::feature::network  → { assets, shared::intent, state, runtime::{feature::input, workers} }
runtime::feature::playback → { shared::model, runtime::{feature::input, workers} }
runtime::workers           → { shared::model }
bench             → { lib::run_with, services::network::{controller, shape}, shared::{intent, model}, state, runtime::{feature::{input, playback}, system} }
```

- **pages never import each other**: a page reads `shared` (including `DeviceState`); when a page needs something from another page, that thing belongs in `shared`
- **`services` is the only module that knows every service**: every page is a concrete type (`InputPage`…`NetworkPage`), and `Services::apply` is the one business dispatch table, one arm per `BusinessIntent` group; `state` asks only through `Services::bind`/`apply`/`snapshot`/`enter`/`leave`, and `render` only through `services::paint` and the reads on `services::descriptor(page)`; a page reads `shared` (including `DeviceState` and `FrameSnapshot`) and does not know `state`/`render`; vocabulary such as `PageId` comes from `shared::route`
- **`shared/ui` and `shared/page` name no page**: `surface::Panel` lays a page's declaration out into boxes and then paints them, `page` offers only the shared margins, line rhythm and reading row, and `ui` knows only widget state; which page declares what is `services::descriptor(page).view`, and what the page paints itself is `descriptor(page).custom`
- `shared` is a leaf: it does not depend on `state`/`services`/`render`/`runtime`. `DeviceState`, `PageSnapshot` and `NetworkView` live there so pages, the snapshot and rendering all look downward
- `shared` is eight buckets, one change reason each: `model` (records) / `route` (destinations) / `intent` (inbound messages) / `i18n` (words, the leaf) / `ui` (widgets and interaction) / `page` (page furniture) / `surface` (the paint surface) / `snapshot` (the frame and page views); `every_shared_bucket_has_two_product_readers` holds "one reader is not shared" and `the_shared_buckets_are_the_eight_the_taxonomy_names` holds the set
- **A test lives with the item it asserts**: in that module's own file, or in the module's own `tests.rs`; `state/tests/` is the only test tree split by topic, and the fixtures exist once, in `state/tests/support.rs` (`state::test_support` is only its re-export). Tests are not edges of the module graph, so `module_boundaries.rs` skips `tests/`, `tests.rs` and `test_support.rs`
- `runtime::workers` carries no `wifi` gate: the host build still holds `Network`, nobody answers
- `runtime::workers` never reaches a worker: it knows only `core` types, so `playback`/`network`/`input` can depend on it one way
- `runtime::feature::network` does not reach `services`/`render`/`runtime::coordinator`: the network worker reads the page and commands from `workers`, not device state

### The page model

Every page is a concrete type with two layers. `Service` is the half every operating platform shares: `apply(&mut self, &BusinessIntent, &mut DeviceState) -> (Option<Command>, Effects)` — the owner's own record settles first and the answer says what the device still owes and what has to leave the core, so one intent reaches exactly one owner. `LocalPage: Service` is the local platform's face: `bind` turns a local gesture into a business intent (`Option`, `None` when it means nothing), `publish` hands the page's private local UI state to the frame snapshot (the passphrase draft, keys, confirmation box; no remote platform sees it), `view` declares the whole page (`&FrameSnapshot, &mut Ui`; it takes no `self`), `descriptor` says how the page occupies the panel, `tick` advances the page's own clock, while `on_show`/`on_hide` default to doing nothing. `where Self: Sized` on `view` and `descriptor` keeps the `&dyn LocalPage` the local input table needs.

`Descriptor` is one page's panel answer: the declaration function pointer, the custom-slot painter, the ground, whether frames come from the camera, the repaint step and trigger, and the pure-scroll answer. The builder defaults to no custom slots, no ground, no camera frames, `REPAINT_FLOOR_MS`, `Never` and no scroll; each page writes only its own difference. A page's repaint rule is one of `Repaint::{Never, Always, Overlay, When(fn), WhenOrOverlay(fn)}`, not a function with unused parameters.

The render answers come in two layers: `LocalPage::view` declares the whole page with `Ui`'s `row`/`column`/`stack`/`wrap`/`scroll`/`text`/`custom`, and `services::paint` hands `descriptor(page).view` to `Panel` (lay out once, draw glyphs and boxes, then call `descriptor(page).custom` per slot), adding the badge; the other five answers are reads on `descriptor(page)` (`ground()`/`camera_frames()`/`repaint_step_ms()`/`repaints_on()`/`scroll_shift()`).

A long page keeps its scrolling in the page: `Style::scroll_area(id)` names a scroll box, layout stacks its children along the axis (gap included) and records the rectangle and travel in `Tree`, and `Ui::scroll_info(id)` reads them back; `shared::ui::scroll::ScrollState<N>` holds each region's offset, velocity, whether a finger holds it, and the two switches (bounce, bar). `measure` runs one headless layout of the same declaration the page will draw — the panel size and the content are static, so that pass is the run-time one. After a press, `GestureEvent::Drag` reports every 2 px of travel; the page moves the offset directly through `drag` (half damping past an edge) and the lift hands over to `fling`: the velocity is signed, and below the threshold the region only comes home. A held region does not spring in `tick` while the others keep gliding; layout never clamps an offset, so the limit lives in the page. A scrolling page uses `Repaint::WhenOrOverlay`, and the renderer stays `TimeDriven` while `FrameSnapshot::animating()` holds; `PageSystem` hands the wall-clock delta (0 on the first pass, clamped to `MAX_CATCH_UP_MS`) to `DeviceManager::tick_pages` in `schedule.tick`, so the offset reaches the same frame's snapshot and a slow rasterization frame does not shorten a fixed step.

The `text` page breaks its run once at construction: `typeset::wrap` stores each line's byte range and width in the snapshot, and the declaration turns each into a `nowrap` node with an explicit width; `layout_text` skips `max_content` for a nowrap node whose width is already fixed, so a frame no longer walks the face per glyph. `\n`, `\r` and `\r\n` all break the line.

Text has one face: a 12 px Fusion Pixel bitmap (`Face`) covering GB2312 plus Big5 (15,466 glyphs), magnified only by whole steps, so sizes are 12 / 24 / 36 and `13..=24` reads as 24. The `text` page's paragraph, page titles and button labels use 24 px; everything else uses 12 px, and a missing glyph draws □ (`MISSING`). `Style::font_size(Length)` picks the size: `Px` is exact, `Dp` is 5/4 px (the DPR of the 2.0" 320×240 glass at 200 dpi), and `Rem` is a hundredth of the root 12 px; weight, decorations and italics pack into one `u16`, `Style::text_flags`. Colours are ARGB32: `Style`/`RunStyle` hold a `ColorId` into a 16-slot palette interned per declaration, and `Canvas` blends coverage over RGB565; `Ui::span` allows 8 spans that recolour and redecorate a run without moving a line. `every_word_the_panel_prints_has_a_bitmap_glyph` pins every character of `Msg::ALL` × `Lang::ALL` to `Face::has`; a new string with a missing glyph goes back through `iot-tool font --merge <bdf> --chars <list>`.

A page may declare itself steady through `PageId::steady()`: `FrameSnapshot::steady` leaves the lamp at phase 0 of its declared colour without breathing; the panel backlight no longer follows it and takes only `Backlight`'s setting. The page's own scrolling still keeps the renderer `TimeDriven` through `animating()`. Today the steady set is `text` alone, and `only_the_reading_page_holds_the_light_still` pins that.

`Lighting` is the one entry point for the light: `GlowScope { Off, Page, Everywhere }` (default `Page`) lives in `DeviceState.glow` and projects to `FrameSnapshot.glow`; the effects `Effect { Steady, Breath, Cycle, Aurora, Candle, Pulse, Heartbeat, Lightning, Sparkle, Strobe, Police, Sos }` and their parameters come from the page's private `LightingView.breath`, and the glass and the lamp share the one sampling path through `Breath::sample`. `Off` lays no glow, `Page` lays it only on `Lighting`, and `Everywhere` lays it on every page. `Backlight { pct }` (default 100) is independent of the light, and `level() = pct` is the only source of the panel backlight; 0 is dark. A board with no panel lands on `PageId::Headless`: a click changes the effect (the twelve, then off), a hold steps brightness (30/50/70/90/100%, then off; a dark lamp answers by lighting at the first stop), a double-click the colour and a triple-click raises brightness (multi-click is off by default and switched on at runtime with `SettingsIntent::SetClicks`), and a hold through the splash requests provisioning; it is not in `PageId::ALL` or the menu, and only binds the buttons and the lamp; a request for `Headless` on a screen falls back to `Input`. The lamp and glass boot on `Aurora`. `PageId::from_code` still knows its wire code for boot and the host walk.

### The render pipeline: declare once, lay out once, paint once

A page keeps no widget tree and computes no coordinates: when `view` returns, a declaration is left behind, which `Ui` lays out into integer rectangles and then draws. A hit test runs the same declaration through `Ui::headless` without a canvas.

| Layer | What it does | Vocabulary |
| --- | --- | --- |
| `ui::layout` | 1/64 px fixed-point layout; measurement runs top-down and each container passes its children the constraints its own box leaves; final edges round to pixels so neighbouring boxes share an edge | `BoxConstraints`, `Length` (px / dp / rem / %), `Style`, `Flow`, `Flex` (grow/shrink/basis), `Wrap` runs, `Stack` positioning, `Scroll` clipping, `Align`/`Justify`/`TextAlign` |
| `ui::declare` | the `Ui` declaration builder and the paint driver: `view` builds a tree, then it is laid out once, slots are called back, and text and boxes are drawn | `Ui::{row, column, stack, wrap, scroll, text, span, custom, paint, headless, rects}` |
| `ui::paint` | each laid-out node's paint: a text run (recoloured and redecorated by its spans), a custom slot, its own paper and clip; a single non-wrapping run is drawn without the breaker | `NodeKind`, `RunStyle`, `draw_text` |
| `ui::typeset` | a subset of UAX #14 plus CLReq's prohibitions: no line starts on `。` or ends on `「`; measurement and drawing share one breaker, `…` truncates | `Paragraph { lines, count, truncated }`, `wrap`, `next_char` |
| `ui::font` | Fusion Pixel at 12 px, monospaced, GB2312 plus Big5 (15,466 glyphs), magnified only to 24 / 36 px. `Metrics` answers line height and advance per size; 6 px per ASCII cell and 12 px per Han glyph, 10 rows of ascent | `Face`, `Metrics` |
| `ui::panel` | the panel's measurements, derived from the face: 320×240, a 12-row footer, 13-row pitch, a 44 px touch target, the margins | `PANEL_COLUMNS`, `CONTENT_BOTTOM`, `LINE`, `ROW_PITCH`, `TOUCH_MIN` |
| `ui::canvas` | pixels, clipping and fill/stroke; ARGB32 src-over into RGB565 at coverage, the bitmap magnified by whole steps, a heavy mark merged with itself one column left and laid once; RGB565 packing and inversion as well | `Canvas::{fill_rect, stroke_rect, draw_char_at, draw_char_heavy_at, draw_text_at, push_clip}` |

Two rules hold a page still: readings go through `page::reading` (a four-cell label and a seven-cell right-aligned value, so a line keeps one width), and a column carries its width instead of `grow` — flex starts from content width, so a change of digit count redistributes the row and the whole block of text moves. A custom leaf sits in a `stack` and is pinned with `absolute().right/bottom`, declared after the text so it paints on top.

Custom leaves (a sweep, a dial, a network band) go through slot dispatch: `Ui::custom(style, slot)` records the slot, and `Ui::paint` calls the page's `Custom` for it after layout. Neither a page's `view` nor its custom painter knows anything beyond the frame buffer; `Panel` knows one `Canvas` and which screen it belongs to.

Layout is a pure function, but one frame asks a node for its size many times: flex measures then places, and a `grow` cascade asks again under other constraints. `Tree` therefore records `measured_at` (the four constraints after `enforce` packed into one `u64`) next to `measured`, returns the pair on a hit, and re-measures only when the constraints change (a height-only change counts); a fresh declaration carries `Node::default()`'s sentinel key, so last frame's size cannot pass for this one. The pixel layer is organised as "intersect once, write whole": a glyph reads its bitmap row by row and skips eight pixels for an empty byte, `fill_rect` intersects the clip once per row, a `Start` run that fits skips the breaker, and `Screen::fill` writes two pixels a store. Path only, never the picture — the whole-page golden hashes guard it.

A scroll frame does not repaint the page: the page answers `Descriptor::with_scroll` — only the two scroll regions' offsets may differ, every other field must match — and `AppPainter` keeps the previous snapshot, `memmove`s the pixels it already has up or down in place, and derives two `FrameDamage` sets (at most 7 rectangles each, including the badge band). The **repaint set** holds only the strip that entered the viewport, the header reading and the thumb, and `Canvas` clips every mark to it; the **ship set** holds the whole scroll box with its header and thumb, and `St7789` sends those through `CASET`/`RASET` — GRAM cannot move its own pixels, so writing only the strip would leave the rest of the box frozen at the content it held when the drag started. A changed ground, a page switch and a camera frame fall back to the whole frame; the overlay only adds the bottom 320×12 badge band to both sets instead of repainting every pixel. `ScreenPainter::reset` makes the canvas forget the frame it cannot reuse.

A frame the repaint floor turned away never comes again: the differ reports each change once, so `ScreenRenderer` also keeps the frame the glass actually holds (`shown`), compares against that, and paints the owed frame as soon as the floor allows; `owed()` keeps the loop `TimeDriven` instead of parking with a frame still undrawn.

`Services` owns the eighteen pages plus the device's own three things: `NavService` (the page stack), `LightService` (the lamp) and `SettingsService` (the panel language). It exposes `bind`/`get`/`apply`/`snapshot`/`enter`/`leave`; there is no per-page field accessor. A landed scan writes the list into the record first, then `LinkIntent::Scan` travels the same dispatch table so the Network page clamps its draft to the new list; there is no scan-specific hook. `Services::apply` is the one business dispatch table: one arm per `BusinessIntent` group: `Link` writes `NetworkState`, `Network` writes `NetworkView`, and the two records belong to `NetworkPage`, which is why they share one arm; the other groups have one each; the match is exhaustive, so a new group does not compile. Adding a page means a `services/<page>/` directory, one field on `Services`, one arm each in `get`/`get_mut`, `service_mut` and the `descriptor(page)` match, and the page's own `bind` and `descriptor` (plus `apply` when it claims intents); a missing arm does not compile, and `iot-tool/tests/page_table.rs` guards the template. Adding a service that is not a page means one field, one `ServiceId` variant, its own `apply`, and a `Services::apply` arm that points at it.

`state/manager/write.rs` drives the lifecycle: after one business intent has landed its state, it compares `state.page()`, and on a change calls `leave(old)` then `enter(new)`; the boot `Splash` counts as an `enter`. The hooks do not depend on `has_screen()` — a page on a board with no panel still hears them — but they touch only the page's own fields. The Network page puts its keyboard and confirmation box away in `on_hide` while keeping the picked row and the draft, so leaving and coming back does not reopen the keyboard.

`PageSnapshot` is the one view a page shows the outside world (`shared/snapshot.rs`); the render layer takes it and never names `crate::services`, which is why `NetworkView` lives in `shared/model/network_view/` too.

- `bench` is an entry/fixture, not a layer: `probe` is reached only by `bin/*-probe.rs` and `replay` only by `main.rs` through `bench::run` under the `panel-bench` feature (the host tests also compile `replay`); neither `lib` nor `runtime` knows `bench`, held by `the_bench_is_reached_only_by_its_own_entry` in `module_boundaries.rs`. The build-time flag `BENCH` and `build_credentials` live in `state::env`, so the network worker fetching a credential never points back at the harness
- `core` does not know `iot_app`

`main` is the product binary crate root (each `bin/*-probe.rs` is its own) and reaches `lib::run` /
`bench::run` and `runtime::feature::network` through `iot_app::`, so it is outside this graph; the
board bring-up lives in `main.rs`, and the chip family injects `second_core_peak` and the rest of
the capabilities into `Wiring`, and no `#[cfg(feature = "esp32*")]` appears inside the library.

### Remote surfaces are not pages

Three ways to put the interface somewhere else, told apart by **who computes the display**:

| Form | Who computes it | What goes on the wire | Reuses `services` |
| --- | --- | --- | --- |
| WebUI | the browser | semantic facts (link, config, levels, health) | only `Service` |
| Remote display | the device (`LocalPage` + `Panel`) | the picture | no (it ships after painting) |
| Remote control | the device | commands | only `Service::apply` |

Shipping `FrameSnapshot` is remote display, not a WebUI. `Descriptor` (ground, camera frames, repaint pacing) and `PageSnapshot` (a password draft being typed, say) are device-internal; no remote form should see them.

A WebUI's transport is an adapter: define the semantic API (resources and actions) once, then climb the cost ladder REST + JSON → long polling → SSE → WebSocket → BLE / NFC → ESP-NOW → MQTT, taking the first rung that meets the need. The device's existing HTTP server (`core/src/provision/`; keep-alive is implemented, bodies are fixed-length static text) already carries REST + JSON, and long polling needs no protocol work; SSE needs chunked bodies first. WebSocket is worth it only for bidirectional low latency (remote control), and its handshake plus one resident socket per client both count against stack and image size. BLE / NFC are provisioning and pairing channels (browser-side only in Chromium), ESP-NOW is a device-to-device link a browser never reaches, and MQTT suits upstream telemetry. A LAN WebUI also needs a token (the roadmap's `device_key`); the open setup AP is a provisioning design, not UI authorisation.

## Composition (three orthogonal axes)

Hardware (board feature) ⊥ module (module feature) ⊥ capability (`iot-core` trait). Modules only see capabilities; boards only provide them.

- **Composition point = bin board manifest**: capabilities injected by move, generic over capabilities, shared by boards with the same capability set; enabling a module that lacks a capability is a compile error, never silent.
- **Product tier = feature alias**: the tier fixes the module set, the board fixes the supported set, and the intersection is trimmed and checked at compile time.
- **Module configurability** = feature switch + const parameters.
- **Runtime-pluggable render layer**: `iot-app`'s allocation-free central `Differ` (diff closure callback; selections are computed per topic first, only the ones that moved are notified, and the snapshot is written once at the end) plus a heap `RenderSet` registry in the same crate (`embedded-alloc` heap: 512 KiB PSRAM on the S3, 64 KiB internal DRAM on the C6). Each renderer is registered beside its surface at start-up, and all live in the same render task; there is no cross-task bus. Each renderer declares only the `Topic`s it claims (`Surface(instance)` / `Frame` / `Demand`), so a lamp never touches the ~2 KB frame; `Appearance` borrows the frame and only the screen keeps a copy.
- **A steady page's light does not breathe**: `FrameSnapshot::steady` comes from `PageId::steady()` (above), so `LampRenderer` holds the declared colour at phase 0 and answers `Idle`; a page's own animation still drives `TimeDriven` through `animating()`

## The demand model (what / how often / how sensitive)

The current firmware is a **board bring-up validation**, not a product; the structure has to accommodate a later application promoting any component to a primary interaction or computation, and that path is allowed to run that component at its maximum capability.

A demand is three independent sentences, and they must be expressed separately:

| Dimension       | The question it answers    | Type                                                  | What it decides                                                            |
| --------------- | -------------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------- |
| **Mode**        | Whether it produces at all | `Mode`: `Off` / `OnChange` / `Continuous`             | Whether polling happens, whether output is clocked                         |
| **Cadence**     | How often                  | `Claim::period_ms` (0 = the board's cadence)          | The floor on the poll period                                               |
| **Priority**    | Who is asking              | `Priority`: `Primary` / `Secondary` / `Background`    | Nothing on this board (below); it only names itself in logs                |

And "how to acquire" — the register set, the FIFO, the on-part engine, the transport — is the acquisition configuration (`MotionConfig` and its siblings), selected by the driver; a page never names it.

This separation is exactly why "low fidelity + high cost" is inexpressible: a page has no way to ask for a worse picture in order to save resources.

Fusing the two into a single ordinal "level" welds "what is wanted" and "how it is acquired" together. Stepping down then looks like both lowering fidelity and saving resources, when stepping down is not the same as spending less.

`Priority` moves no cadence on this board, and that is the hardware's doing.

Android's `max_report_latency` presumes events are sampled on schedule and only their report waits in a FIFO, so "sample often, report late" are two independent statements.

This board's IMU has no FIFO, so reporting late can only become sampling less often — which is what `period_ms` already says out loud.

A "priority to milliseconds" lookup therefore cannot land here: it would only scale `period_ms` by a coefficient and take a max.

Two shapes taken from Android's side, copied rather than reinvented:

- A sensor's `minDelay` must be 0 for on-change and −1 for one-shot. "How often" is not a question every reporting mode can answer.

  The motion device on this board is on-change in name only: it is continuous polling with the part's own No-Motion engine turning that into change events, so `MotionConfig::for_mode` maps both `Mode::OnChange` and `Mode::Continuous` to the same `PolledRegisters`.

  That is not merging two modes into one — what the consumer wants does differ in time (one wants changes, one wants a clock); the wiring that produces them is simply the same, and "how to acquire" is the part's answer rather than a field of the demand.

  `WakeDetector` is still in the configuration set, but there it is a rung the driver explicitly refuses on this silicon, not where `OnChange` lands. Other components need the reporting mode asked first.

- `power` is nearly always above the datasheet figure, and the assumption that several sensors together draw no more than the sum of each is an additivity claim. This board has no current meter, so it cannot be verified: the firmware therefore carries no power figure at all and reports only executor occupancy.

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

### Changing: reuse the existing intent path

`apply_intent` is the only public entry point for state changes (the input half is written to the record before it is translated) and still lands everything through `apply_business`; worker reports take `apply_scan`/`apply_capture` and the clock takes `tick_notice`, each carrying a payload too large for an intent. `apply_input` and `apply_business` are the two halves of the pipeline and stay crate-visible, so no outside caller can write a record without translating it; the only gesture-driven page change is `BusinessIntent::Navigation(NavigationIntent::NextPage)`, and `SplashOver` lands on the build's page when the splash ends.

A new scenario is a new `BusinessIntent` variant; switching cadence at runtime is another variant. `EffectiveDemand` is re-derived by `settle_demand()` right after `apply_business`. No new mechanism.

A capability that carries rules (multi-step transitions, invariants, edges across fields) gets its own module. `BusinessIntent` is grouped by the record it writes into `Navigation`/`Link`/`Network`/`Audio`/`Playback`/`Light`, and `Services::apply` is the only index over them; each owner matches its own group exhaustively, so a new variant is one arm in the module that owns it and a missed one is a compile error. Cross-domain actions (a sound, a lamp notice) come back as `Effects` for the facade to apply; owners never call into each other.

One chain, five words: `Event → Intent → Action → State → Hardware` — an `Event` is what the device heard (a finger, a button, a worker's report, a boot decision), an `Intent` is what it was told, an `Action` is what it decided to do (a record's own verb table; where a domain has one verb, the intent is that table), `State` is what it looks like now (RAM), and `Hardware` is its body (panel, lamp, radio, speaker, storage).

**Hardware is driven by State alone, State is changed by Action alone, Action is triggered by Intent alone, and Intent is raised by Event alone.**

`shared/model/`'s file layout is in the table above; one hard rule to add: domain files import explicitly and never use `use super::*` outside tests, because a prelude lets a new symbol cross a module boundary unseen. `DeviceManager` and gesture→intent dispatch live in `state/`, and `DeviceManager::apply_intent` is defined in `state/manager/write.rs`: it binds through `Services::bind` for the current page and writes the record before translating.

Business state and page state sit apart, and **whoever writes a field owns it**: `NetworkState` holds only what the radio reported (`link` / `mac` / `aps` / `outage_announced`) and only `LinkIntent` writes it; `NetworkView` holds only what the page is in the middle of (the selected row, the draft, keys, the confirmation, the list's scroll position) and `NetworkPage::apply` writes it in answer to the finger's `NetworkIntent` and to the landing `LinkIntent::Scan`; it is not a `DeviceState` field. The page reads `&NetworkState`; the link never reads or writes a page field, so neither "a worker's report lands in a page field" nor "a page edit changes what the radio said" can happen. The other domains keep a `view` sub-record (`AudioView`, `PlaybackView`, `TouchView`) with the same meaning: only what the page is in the middle of, read by notifications and rendering. The link's own shape is a value too: `LinkStatus.mode` is a `NetworkMode` (`Link` / `Browsing` / `Setup { host, cancellable }`) instead of four parallel booleans describing what the page is about.

Where a command comes from follows the same boundary: the page domain's `apply` returns `Option<NetworkCommand>` directly, and `Effects` carries only in-device sounds and notices — there is no outbox, and one path from an intent to the worker.

Which form a crossing takes follows the meaning — two forms per direction:

- **Reports**: those that can raise a command (`Link(Status)`, `Playback(Finished)`, `SplashOver`) ride the intent bus; a pure latch (a scan list, a capture window) is written straight from the system's `tick`.
- **Requests**: those that cannot be derived from state (a repeated `ScanNow`, "forget" rather than "forgotten", `SplashOver{setup}`) go out as a `Command`; those that can be derived from a state edge (mute, playback phase) are diffed in `tick`.

### Transport: coordinators and workers

`run_loop` is the **coordinator**: it only does non-blocking work, in four steps per frame — take intents, let each system take one step, present, wait for the next frame. Anything that waits on a driver, an I2C bus or a network stack belongs to a **worker**:

| Worker                 | Owns                                | Executor                                   | Handle |
| ---------------------- | ----------------------------------- | ------------------------------------------ | -------- |
| Render layer / camera  | surface, camera                     | cooperative                                | `Render` / `FrameSnapshot` |
| `input_task`           | input devices and audio capture     | cooperative (S3: the second core)          | `Input { to, from }` |
| `control_loop`         | codec, DMA                          | cooperative (S3: the second core)          | `Speaker { to }` (completion returns on the intent bus) |
| `network::serve`           | stack, link, AP services            | cooperative (a task on core 0)             | `Network { commands, page, scan }` |
| `feed_task`            | the microphone clock                | **Priority2 interrupt, separate executor** | ⚠️ **deliberately dependency-free; none may be added** |
| `panel_service`        | the panel bus (frame transfers)     | **the second APP core**                    | no handle: `Screen` holds only the channel (`Panel::write`) |

`panel_service` and the driver half (`input_task`, `control_loop`) run on the second core: a frame transfer is a blocking DMA call, and the capture fold busy-waits 7 ms out of every 20, so each would hold its executor in one piece. The coordinator posts the finished frame as a ticket and parks for the answer; the driver half goes over as one bundle into the `IoPlacement::Elsewhere` slot, where `run_io_from` picks it up. That path exists only when a second executor was actually started — a single-core C6, or an S3 with no panel, gets `IoPlacement::Inline` and the very same task runs on the coordinator's executor, which is why the placement is decided in `start_executors`: that is the only place that knows about the second core. `esp_rtos::start_second_core` takes `FROM_CPU_INTR1` by signature, which is why `feed_task` uses `FROM_CPU_INTR2`.

**A worker is started by the composition point that owns its drivers.** The driver half is spawned by `run` through `Wiring.spawner` (`IoPlacement::Inline`) or handed to the second core (`Elsewhere`). The network worker's driver types are only known to the entry point, so the entry point takes the station and store off the board and builds `Wifi::Worker(NetworkWorker)` right there — a closure that takes only a `Spawner`; `run` runs it before `run_loop`, knowing neither what the radio is nor any worker list. Adding a worker is still one closure at the composition point, not another shape handed to `run`. `main` therefore joins no worker at all, and `run_loop` is the only future it awaits. `feed_task` and `panel_service` stay with `start_executors` — that is the only place that knows the interrupt executor and the second core. `network::serve` is spawned with the local `Spawner`, so its future need not be `Send`: it stays on core 0's executor, and the only thing that crosses a core is the `Io` bundle.

Each system holds the handle for its own boundary, and the composition root hands it over at `register`: one worker, one system, so a second protocol adds a file and one `register` line rather than a field on a shared struct. Directions are named by role (`to` for the worker, `from` for the coordinator); the peer's name stays in the peer struct. A parked frame waits on three ready lines (the intent ring, the capture window, the scan list), so whoever publishes wakes the loop and the loop names no worker.

**Handles flow down from the entry; nothing reaches back up a module path.** The composition root holds one `static RUNTIME: Runtime` (`runtime/mod.rs`) with every piece of storage more than one layer needs: intent ring, capture window, the network and playback queues, the feed wake and `Readiness`. What travels down is always a specific handle or one coherent companion value — never the whole `Runtime`: `Io` is the bundle the driver half carries across cores, and the network worker gets `NetCtx { bus, network, readiness, heap_free }` (only the composition point knows the readiness flag and the heap reading). The intent ring counts what it refused (`IntentBus::dropped_count()`), so that number has no second handle. A signature is therefore its own list of inputs, and a test can build its own instead of competing for a global singleton.

A companion value is produced only at a boundary: the composition root (`Wiring`, `NetCtx`), the board layer (`ScreenContext`), the start of a task or session (`BootFacts`). The coordinator, the renderers, the pages and the state never assemble one on the spot — that is the shape of the deleted `Frame`. The criterion and its two fences are in `records/iot/layering.md`.

`Playback`, `Network` and `CaptureWindow` own their channels (`const fn new()`), rather than being "a static channel plus a handle pointing at it": a cross-core lock appears only where it is genuinely needed, so `LinkWatch`'s last report and unsent MAC live on the task's own stack in a plain `Cell`.

The shape follows the semantics, not convenience:

- **Commands** use an `Inbox` (`Network.commands`): one arrival queue per worker. A publisher that cannot wait uses `publish`, which gives up the oldest entry when the queue is full — the coordinator is the only one that may go this way. A publisher that must not lose its message uses `send().await`, which waits for room (the phone's `Join` does, because it parks on its own page right after asking). The depth is one frame's largest command burst (8 intents), so the normal path loses nothing; an entry gives way only when the worker is more than a frame behind.
- **Retained values** use `PageWatch`/`Retained` (`Network.page`, the list inside `Scan`): a write replaces the old value and reading does not consume it, so the panel and the phone's page read the same copy. The splash's verdict is not a retained value but a **one-shot command**: it travels as `NetworkCommand::SplashOver { setup }` on the same queue as the panel's other requests, so the worker never asks how far the page has got.
- **The newest window** uses `Input.from`: a 1.7 KB capture sample only has a newest copy worth keeping, so it overwrites instead of queueing.

On the input side, each tick drains every event the source produced that round (`PollEntry::poll_all` → `EventAggregator::drain`) and posts at most `EVENT_BATCH = 5` per tick: two fingers moving in one sample produce two `Drag`s, and letting only one through would leave the other finger a frame behind forever. Discrete intents (`Press`/`Tap`/`Swipe`/`LongPress`) still take `send().await` and wait for a slot; `Drag`/`Touch` may be refused and the next sample replaces them.

The `NetworkCommand` vocabulary stays in `app/src/shared/model/network.rs`; the handle only carries it. `DeviceManager::apply_intent()` (defined in `state/manager/write.rs`) returns the command as a `Command`, `Schedule::process_intents` looks it up by `Command::type_id()` in the registry and hands it to the one system that declared `System::takes`, and `NetworkSystem` takes `Command::Network` and publishes it — no variant-by-variant translation, and no outbox in the state. A full queue gives up its oldest entry, so the coordinator never stops a frame for a command. Setup commands carry the `SetupHost` they were issued on so the worker can drop a previous session's leftovers.

**Per-frame logic is split by boundary.** One `System` per file under `coordinator/systems/`; it holds the worker handle for its own boundary and what must survive a frame (the last playback state, the last page, the governor), while `tick` hands it the shared state and the clock. Systems never call each other. Up to two shared collaborators travel as explicit parameters; a third, or a set that cannot be named at compile time, is the point to collect them into a `Context`. The bus is held by the loop and handed to `process_intents` explicitly each frame. Adding a feature is one file plus one `Schedule::register` at the composition root, where the handle is handed over; the registration order is the execution order, and reordering it is a behaviour change. The intent side goes through `System` too: after each intent is applied, the `Command` the model left behind is looked up in the registry by type and handed to the system that declared `System::takes`; two commands in one frame do not collapse, and a command nobody took is logged as an error under `[COORD]` in release too (debug builds have an assertion as well).

**The render layer costs nothing**: `FrameSnapshot` in `app/src/shared/snapshot.rs` is already "one snapshot fusing every subsystem's state", carrying `page` / `motion` / `audio` / `playback`; the screen renderer reads it through the painter seam.

The camera's chain runs end to end today:

```
DeviceState.page → frame_snapshot() → Appearance::Frame
  → Differ::diff → ScreenRenderer::on_appearance
  → surface.release_camera()
```

So `Demand` only has to grow on the same snapshot, and the display and camera parts add nothing.

**A demand is shared state, not a message.** A demand is idempotent aggregate state rather than an event, and only the latest value means anything.

Carrying one on a channel would need a one-deep queue, change detection on the writer, and a retry for a refused send — three mechanisms that exist only because a channel cannot hold a value.

The shared slot's shape follows from that constraint: embassy-sync's blocking mutex is shared-read-only (the closure `RawMutex::lock(f)` takes gets no `&mut`), so a `RefCell` sits inside the slot.

`Intent` is the opposite case — an event — so those still travel on a bus.

The capture window is a third shape: a **window**, not an event (only the newest one means anything), and not shared state either (nobody writes a common slot — the board hands over the frame the driver just folded, during its own poll). It therefore rides a single-slot channel: `HasAudio::take_audio(&'static dyn CaptureSink)` gives the board the destination, `input_task`'s poll publishes into it, and `run_loop` takes from it after the intents.

The second reason is width: every slot of a `Channel<T, N>` is allocated at `size_of::<T>()`, so the slot width is set by the widest event variant — a capture event carrying an envelope (1.76 KB) would price the eight-slot intent ring (`Runtime.bus`) at 14,184 B of internal DRAM for the one frame the panel reads. Without that variant the ring is 896 B and the window itself 1,824 B (recorded in the [Wi-Fi record](@/records/iot/wifi.md)).

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

The total is not the risk: Input spends 94.8% and behaves. A single 25 ms block is. Playback feeding runs on its own higher-priority executor, driven by DMA, so such a block cannot reach it.

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

The demand is keyed by device, not by enumerated fields. `EffectiveDemand` is a `BTreeMap<DemandKey, Claim>` where the key is a device type plus which of them is meant, so `claim` and `Override::apply` are all lookups and adding a sensor changes no line of the demand machinery.

Two devices of one kind can be demanded apart (`DemandKey::instance(Audio, 0)` and `…, 1`).

The device list is the board's. `PageId::demand(&self, keys)` walks the devices the board declares and calls `claim_for` per key. That match is exhaustive, so a page cannot forget a device and inherit a demand nobody chose.

A device the board did not declare is absent from the demand and reads as `Off`, so four boolean gates are no longer needed on top.

The demand state is held, not re-derived. `DeviceManager` keeps an `EffectiveDemand` beside the page it came from, together in `DemandPolicy`. The one place it is derived is `DemandPolicy::settle` (called from `apply_business` through `settle_demand()`): the page is the whole key — `keys` and `overrides` never change after boot — so an unchanged page means no recompute.

Downstream reads that proposal as it stands. When the input worker meets a part it cannot put into the asked-for configuration it logs the refusal and leaves that part where it was; the proposal is not written back. No part in the tree reaches that path today: `MotionConfig::for_mode` only returns `PolledRegisters`, and every wired motion driver supports it.

Arbitration: `resolve_speed` takes the highest requirement among collecting devices; a test holds it to being per device rather than one constant for the whole map.

Wiring:

| Consumer      | How it gets the demand                                                                                                                                                                                                                |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `input_task`  | the shared demand, borrowed once inside a critical section each tick, scheduling by `PollEntry.key()` (the cadence also asking the driver `accepts_slower_poll`)                                                                      |
| `run_loop` | the writer; the page is the product's declaration, so it is written here rather than each consumer deriving its own                                                                                                                   |
| camera        | `Appearance::Frame` (an existing channel), gated on "the demand no longer asks for frames" rather than on "the page changed"                                                                                                            |
| `feed_task`   | deliberately not fed. Its existence supplies the microphone clock, so it is the source of the audio path's clock rather than a consumer of its demand |

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

**It cannot report current** — that needs a meter across the board, and a datasheet figure for a part on someone else's board is not a measurement of this one, so the firmware reports no current figure at all.

**Cells the driver has no register encoding for print as "unreachable" rather than being skipped**, so the log is as loud about what was not measured as about what was.

Two companions: `SUPPORTED_CONFIGS` lists only what the driver can leave the chip in (a test holds it to `config_for`), and `Board::new_motion_only()` provides an isolated bring-up, because something measured while another device is running is evidence about the wrong thing.

The per-device figures and their individual basis labels are in the [implementation records](@/records/iot/_index.en.md).

#### Executor occupancy figures cannot be summed

One cooperative executor gives its time to whichever task is awake: batching motion to 30 s drops the input side from 49.5% to 35.5% while the panel side simultaneously rises from 45.3% to 58.5%.

The panel row is a residual, not an increment. Adding it to a polled device's row counts the same microseconds twice, and the sum comes out over 100% of a core.

Likewise, a number measured with the device running alone on the board (the probe) and a number measured by differencing two whole-device builds are not the same unit. Added together they give a lower bound rather than a total — the three devices measured in isolation sum to 43.2% against a measured 51.0% for the same build.

Take the more expensive side: a device's cost rises with what runs beside it, and the cheap reading is the first to stop being true.

This yields one rule for measurement: to ask what throttling does to the worst block, the measurement must change the cadence and not the mode. Changing both leaves open that what was saved came from switching off a whole device rather than from throttling.

## Pages: a route table plus a navigation stack

`app/src/shared/route/mod.rs` holds one route table: the `PageId` enum is the destination, one `static <PAGE>: Route` per destination (path, needs, claim, whether it is bootable, lifetime), and `route()` is an exhaustive `match` — **a new variant with no route does not compile**. `ALL` is the whole order (the wire code is its index) and the button walks it too: from here it takes the first page the board backs that is not a transient, and wraps at the end, so "listed" and "reachable by the button" cannot drift into two lists. The page's own answers live in one other place, and only one: the `Services::apply` business table and one `descriptor(page)` map in `app/src/services/mod.rs` (draw call, ground, frames, repaint pace), with business on `Service` and local gesture/view/picture/lifecycle on `LocalPage`. `crates/tool/tests/page_table.rs` guards a page missing from `ALL`, and each page's type, dispatcher arm and view module under `app/src/services/<path>` (the matches are checked exhaustively by the compiler).

`Navigator` in `app/src/shared/route/nav.rs` is the only source of the panel's position: a fixed-capacity stack (`MAX_DEPTH`) read through `current`/`root`/`entries`, with `go` replacing the top in place (the button's cycle is a tab, not a growing history), `push` placing a transient over the current page, and `pop` dropping the top while the root always survives. `DeviceState::page()` is `nav.current()`, so there is exactly one writable place for "the current page". At boot the root is `Boot::boot_page` after the capability clamp, and the splash is pushed over it; `SplashOver` pops, and `go(Network)` follows if the hold asked for setup. `next_page` walks from just after this page, takes the first page the board's capabilities back, and wraps at the end; a transient has no place in the order and stays until its `timeout_ms` runs out (the `input.rs` timer reads that route property directly).

The capability test is one column of each route (`Route::needs`, with `is_backed` only looking it up), and every column asks the same `Backed` record: motion, audio, playback, camera and the link are five wiring facts written once into the `backed` field of `Boot` at the composition point. The first four come from the board's `take_*` calls; the link comes from the entry point, because the station only starts once the scheduler is up — by the time `run` has the board, the radio has already been taken. So the entry point hands over a `Wifi` (`Absent` / `Unattended` / `Worker`), and `run` folds it into that `bool` with `wifi.wired()`: **a wiring fact given as an argument**, not something `run` turns round and asks the board about. Surfaces work the same way: every `WiredSurface` handed out by `HasSurface::take_surfaces()` carries its own role (`Pages` or `Lamp`), and the entry point passes them to `run` alongside `clock`/`feed`/`wifi`; `render::wire` builds the renderers and drops the `surface_roles` table in the same pass — no surface is ever asked back whether it draws pages, an empty slot never appears as a handle, and a renderer outnumbering the slots trips one assertion in that pass.

This rule exists because of a defect it fixed.

`next_page` was 18 hand-written jumps, and `Camera`'s leaving branch returned "a page the board can back" (`Attitude`) rather than the start, so `Input` was skipped for good: a full turn with every capability measured as `[Attitude, Attitude, Audio, Speaker, Camera]`, `Input` gone and `Attitude` twice.

Written as pairs, a cycle drops the page that closes it; a single list walked with a wrap cannot.

A test in `iot-app` walks capability combinations × start pages to hold that (`every_page_the_board_can_back_is_shown_on_the_way_round`) — restoring the old `Camera → Attitude` makes it fail and print the string above.

### A missing page means no wiring; a broken part keeps its page

`Backed` answers **whether the board soldered the part down**, decided once at the composition point and never changing at runtime: the S3's `take_*` calls always hand out a speaker, microphone and camera driver object (even when the probe failed), while the C6 has no such product path and hands out none. So a page disappearing has exactly one meaning: this board does not wire that part.

Whether the part answers is a separate, **runtime** fact: `Health::Down(reason)` for playback and capture, and `FrameSource::fault()` for the camera, land on the page as `NO OUT` / `NO IN` / `NO CAM` plus one line of reason. The page stays; its fingerprint and counters simply stop moving.

Recovery happens **where the part is used**, not in a boot loop: the speaker's `ensure_started()` runs on the first tap (tries both addresses and remembers the one that answers), the microphone retries at most once a second while polling and discards stale samples, and the camera retries `receive` and the sensor's bring-up every 5 s while its page is open. Retries are bounded and spaced out; a failure only writes the reason into health, never blocks the panel, and never resets the board.

## Logging

Only the `log` facade (`log::info!` etc.); the output channel is initialized by the family crate `iot-chip-esp` (esp uses `esp_println::logger`); business code must not use `println!` / `esp_println::println!`.

A log line can only be believed if it reports the configuration actually in force, not the one that was asked for.

`[DISPLAY]`'s `asked` used to report the page's own demand rather than the one in force after the override, because the snapshot took the demand from state while the override sits on the manager.

A log reporting a configuration nothing is running is worse than no log.

## Adding a board

1. Add the board module to `bsp-esp/` (Board + HasXxx traits) with feature gating. For a chip the board selects, first build the real component under `components/` (reuse an existing one, and give each new component its own feature); the board feature aggregates the component list, and the board module only assembles them.
2. Export the board's `SETUP_HOST` from the board module (and `HEAP_BYTES` where the part's memory differs); `app/src/main.rs` reads both, and the feed's arm is already there per part.
3. Forward the app feature `iot-bsp-esp/<board>`.
4. Add the board to the `platforms` default JSON in `reusable-iot-build.yml` and the `platforms` input of `iot-dev-release.yml`. Xtensa boards need `use-xtensa-toolchain: true` and go through the `build-s3` task: the espup native `esp` 1.98.1.0 toolchain on CI/release, with a same-version Docker image locally on macOS Intel
5. Verify: `cargo build -p iot-app --bin vanling --target riscv32imac-unknown-none-elf --no-default-features --features <board>`

## Adding a chip family

The esp family is fixed as `iot-chip-esp`/`iot-bsp-esp`; adding a non-esp family:

1. Add the `iot-chip-<family>`/`iot-bsp-<family>` crates.
2. Gate that family's entry dependencies behind the app family-alias feature; add the family's board bring-up arm in `app/src/main.rs` (each arm under its own `#[cfg(feature)]`).
3. Toolchain target + `rust-toolchain.toml` + `platforms` in `reusable-iot-build.yml` (`target`/`use-xtensa-toolchain`) and the `platforms` input of `iot-dev-release.yml`.
4. Switch `moon`/`lefthook` `--workspace --target` to the firmware scope; the toolchain target flows into moon tasks via `CARGO_ESP_TARGET` (set in `.envrc`/CI env and workflow inputs; tasks take `- '$CARGO_ESP_TARGET'` in `inputs` to keep cross-chip build caches isolated).

+++
title = "Advanced widgets and Feed"
description = "25 advanced widgets, one Feed and four galleries are in place: 568 host tests, 32 golden rows, an S3 main stack of 82,296 B, images of 2,065,824 / 1,866,448 B, both flashed and booting."
weight = 70
sort_by = "weight"

[extra]
source_file_hash = "6c5cfdae902c05c8a55b9a3cb1d0e479ce79e56e"
translated_at = "2026-11-25T14:49:09Z"
+++

<!-- doc-audience: ai -->

# Advanced widgets and Feed

> 2026-11-25: the vector face was removed and only the 12 / 24 px bitmap remains. The `vector.bin`, `vector-sample.txt` and `iot-tool outline` references below describe that time, now replaced by magnifying `ui12.bin`.

Every advanced widget that `docs/content/development/iot/widgets.md` used to list as a plan is implemented, plus `Feed`: one component that is a list, pull-to-refresh and load-more at once. Three demo pages replace the single basic gallery, and the menu grows from 9 entries to 12 — still four rows, with about 24 px of scroll left over.

## Scope

| Page | Contents |
| --- | --- |
| `Advanced` | `AppBar` / `NavigationBar` / `Tabs` / `PageIndicator`, `Accordion` / `Segmented` / `Stepper`, `Banner` / `Toast` / `Skeleton` / `LoadingOverlay`: 11 controls |
| `Extended` | `SearchField` / `FilterChips` / `FormField` / `NumberField` / `Keypad`, `Table` / `Chart` / `Gauge` / `SwipeAction` / `EmptyState`, `Dialog` / `PopupMenu` / `Sheet`: 13 controls |
| `List` | `Feed`: 34 rows over pages of 8, with 600 ms of fake work; pull resets, load appends, a left slide deletes, and an empty list shows a hint |

The new files live in `shared/ui/widgets`; `Feed` is `shared/ui/feed.rs`. `ListView` is no longer a separate entry: `Feed` is the long list.

## Feed's two edges

Both gestures share one `ScrollState<1>`; splitting them would duplicate the offset, the drag and the fling.

- Pull: only `ScrollConfig.pull > 0` allows top overscroll, and a release past `PULL_MIN = 28 px` enters `Refresh::Running`. The content follows the finger and springs home; the indicator floats at the top and takes no content height.
- Load: a release within `LOAD_AHEAD = 24 px` of the end, with a row count different from `asked_at`, enters `More::Running`. One row count is asked once; `loaded(rows, end)` or `failed()` reopens it under the new count.
- Footer: `FOOTER_H = 26` is always part of the content, so `max` does not jump when the state changes; after `More::End` nothing is requested.
- Delete and selection stay in the page: `FeedView` takes only `selected` and `reveal: Option<(row, px)>`; neither `SwipeAction` nor the row data enters the component.

## Two hard edges in the pages

`shared::intent` must not reach `shared::route` (`intents_name_what_is_asked_and_not_what_answers_it` scans shipping sources). The gallery intent therefore carries a route code:

```rust
BusinessIntent::Gallery { wire: u8, intent: GalleryIntent }
```

`Services::apply` turns it back into a page with `PageId::from_code`, and the `advanced` / `extended` `apply` methods compare `wire` too. That is one less reverse dependency than putting `PageId` in the intent, and it matches `NavigationIntent::Go(u8)`.

`Extended`'s dialog first sat inside `ui.stack(grow(1).basis(0))` and the scroll region came out 0 px tall: a stack is not a flex container, so the in-flow `page::scroller` row does not stretch. The fix keeps the scroller in the normal flow of `page::body` and adds the dialog as an `absolute` custom sibling above it. The scroller still takes the remaining height via `grow`, and the card does not scroll away. While `dialog` is true the page takes the tap first; the hit test uses the `Ui::paint_rect` rectangle and `dialog::action_at`.

Custom slots: `Extended` uses 6 (keyboard/keypad, table, chart, gauge, swipe row, dialog), `Advanced` uses 1 (the carousel dots), both below `PAINTS = 8`.

## Measured

| Item | Result |
| --- | --- |
| Host tests | `cargo test -p iot-app --lib --no-default-features --features host`: **523 passed** |
| Tool tests | `cargo test -p iot-tool`: **31 + 4 passed** |
| Format / lint | `cargo fmt --all`, `cargo clippy -p iot-app --lib --no-default-features --features host -- -D warnings`: no output |
| Target checks | `moon run iot:check-host` / `check-s3` / `check-c6` all pass |
| Goldens | The page table grows 22 → **28 rows** (`advanced` / `extended` / `list`, two debug states each); the menu was re-recorded for its 12 entries |
| Vector face | 306 glyphs, **157,460 B**, +16,634 B over the previous 140,826 B |
| S3 main stack | `nm`: `_stack_start_cpu0 − _stack_end_cpu0 = 0x3fce9710 − 0x3fcd5538 = 82,392 B`; the serial line agrees: `[DISPLAY] main stack 82392 B` |
| Images | S3 **2,014,432 / 6,291,456 B** (32.02%); C6 **1,812,672 / 6,291,456 B** (28.81%) |
| Flash | One write to each board, verification clean; S3 boots to `Menu`, C6 to `Input`, and both print `this boot is healthy` |

## Feel not measured here

Goldens pin the pixels; only a finger can confirm the `Feed` thresholds and spring-back: how the 28 px pull trigger feels, whether the release springs home naturally, and whether load-more fires too early or too late. The three constants sit at the top of `shared/ui/feed.rs` (`PULL_MIN`, `LOAD_AHEAD`, `FOOTER_H`), so tuning them never touches a page.

## Follow-up: four pages and one gesture arena

The previous delivery packed 24 widgets into two pages, `Advanced` and `Extended`, with interactive rows 22–30 px tall, so controls near the bottom were hard to hit; a vertical drag that started on a tagged control lost its first few pixels to the tap, and the scroll lost them too. This round splits the galleries into four pages and puts press, drag and tap into one arena.

| Page | Contents |
| --- | --- |
| `Advanced` | `AppBar` / `NavigationBar` / `Tabs` / `PageIndicator` / `Pager`, `Accordion` / `Segmented` / `Stepper` |
| `Feedback` | `Banner` / `Toast` / `Skeleton` / `LoadingOverlay` |
| `Extended` | `SearchField` / `FilterChips` / `FormField` / `NumberField` / `Keypad` |
| `Data` | `Table` / `Chart` / `Gauge` / `SwipeAction` / `EmptyState`, `Dialog` / `PopupMenu` / `Sheet` |

`Menu` grows from 12 entries to 14 over five rows; the last row needs a scroll. `PageId::ALL` has 16 items, and `COUNT` and `ROWS` derive from it rather than hard numbers.

## Pager and dots

`shared/ui/widgets/pager.rs` is a horizontal card strip: a card is two thirds of the region wide, the pitch is three quarters, and the neighbours peek. `turn(index, count, rect, dx)` picks a card on release at a quarter of the width; `page_at(rect, index, count, x, y)` uses the same geometry to name the card under a point and returns `None` in the gap. Each `PageIndicator` dot gets a `TOUCH_MIN`-wide target (`dot_at`), and `fit` drops dots that do not fit. Dragging, tapping a card and tapping a dot all land on one `pager` index.

## Gesture arena

`Scroller` in `services/gallery.rs` only remembers `{ id, tag, x, y }` on `Press`; it claims nothing yet. The first move past `DRAG_SLOP` picks an axis:

- Vertical: claim the scroll region under the press and replay the travel already made into the first `Drag`, so the opening pixels are not lost.
- Horizontal on a slide control: hand the gesture to the page as `Slide`, which owns the live preview.
- Below the threshold: wait for `Tap`, so the control still gets its click.

The first claimer owns the gesture; `Slide` and `Drag` cannot steal from each other. The `Menu`, `Widgets` and `Data` sliding rows share the same `Press` / `Slide` / `SlideEnd`. A translucent `LoadingOverlay` carries a tag only while it is up, so a tap takes it down.

## Sizes

Tappable elements all take `TOUCH_MIN = 44`: the `Tabs` / `Segmented` / `Stepper` / `NavigationBar` rows, the `Accordion` header, the `Banner` action, the `NumberField` steppers and a `Keypad` cell. Canvas widgets derive sizes from `rect` and text metrics: `Table` uses `LINE + 4` for the header and `TOUCH_MIN` per row; `Dialog` uses `TOUCH_MIN` for the action band and sums the band, the title/body lines and `MARGIN` for the card; `Chart` / `Gauge` / `Keypad` scale with `rect`; page custom slots are multiples of `page::TOUCH`. The sample heights 22/26/28/30 and `KEY_H = 30` are gone.

## Measured

| Item | Result |
| --- | --- |
| Host tests | `cargo test -p iot-app --lib --no-default-features --features host`: **552 passed** |
| Tool tests | `cargo test -p iot-tool`: **31 + 4 passed** |
| Format / lint | `cargo fmt --all -- --check`, host/riscv/xtensa clippy: no output |
| Target checks | `moon run iot:check-host` / `check-c6` / `check-s3` all pass |
| Goldens | 28 → **32 rows** (new `feedback` / `data` with two states each; `menu`, `advanced`, `feedback`, `extended` re-recorded for the new layout) |
| Custom slots | `Advanced` 2 (card strip, dots), `Feedback` 0, `Extended` 1 (keypad/keyboard), `Data` 5 (table, chart, gauge, sliding row, dialog) |
| Images | S3 **2,037,312 / 6,291,456 B** (32.38%); C6 **1,833,280 / 6,291,456 B** (29.14%) |
| S3 main stack | `nm`: `_stack_start_cpu0 − _stack_end = 0x3fce9710 − 0x3fcd55b8 = 82,264 B`; the same line on serial reads `[DISPLAY] … main stack 82264 B`, peak 20,696 B |
| Flash | S3 and C6 flashed once each, verified; S3 lands on `Menu`, C6 on `Input`, both print `this boot is healthy` |

## Feel: what only a finger can settle

The arena threshold is `DRAG_SLOP = GLYPH − 1 = 5 px` (`shared/ui/scroll.rs`), a card turns at a quarter of its width (`pager.rs::turn`), and a dot accepts `TOUCH_MIN / 2` either side of its centre. Tests pin the rule, not the feel: whether 5 px still reads as a tap and whether the card turn is too eager needs a finger, and both live in one constant each.

## The readability round: captions, scrims and gesture endings

The four galleries used to name only their sections, so a sample near the bottom did not say what it was or whether it could be tapped. Every sample now carries one caption (`page::caption`): its name plus the gesture, such as `TABLE / TAP A ROW`, with one `Msg` per language (24 new `Cap*` variants). `Advanced` and `Extended` therefore dropped their section words, while `Data` keeps `page::section` (a rule and a name) for the overlay band. A caption is a muted `nowrap` run with no tag.

Three fixes came with it:

- **Captions and tab labels measured zero wide.** The `page::scroller` body did not stretch, so a row written as `grow(1) + basis(0)` measured its natural width of zero and its labels were clipped to nothing. The body now stretches (Flutter's `ListView` hands its children the whole cross axis), and the `Tabs` / `Segmented` / `Stepper` / `NavigationBar` / `FilterChips` rows stretch too, so the cells really divide the width.
- **An `absolute` child on the root never got a size.** Flex, wrap and scroll placed only in-flow children; an out-of-flow child was never measured, so `paint_rect` returned `0×0`, and `stack` was the only flow that placed one. `place_absolute` now lays out the out-of-flow children of all four flows, with scroll adding its content shift; `an_absolute_child_of_a_row_or_a_scroll_is_placed_too` pins it.
- **The lift after a drag read as a tap.** On the wire that lift is still a `Tap`, so the `Data` table rows, the `Extended` keypad and the `Advanced` cards each selected something when a drag ended. `Scroller::dragging()` exposes the arena state and the three tap branches ask it first; the tests press, drag, lift, assert nothing was chosen, and assert the next drag still scrolls.

Interface changes: the `Dialog` scrim is now the opaque `theme.ground`, the card uses the new `theme.surface` (`rgb(0x22,0x22,0x22)`) with a muted outline, and the slot moved from the scroll region to the root at `320×228`, centring the card in the content area. `NavigationBar` became an inset accent pill on a `surface` strip, distinct from `Tabs` taking the accent paper whole; the `Accordion` body moved inside the header's paper; `Pager` clips each card; and `Table` rows dropped from `TOUCH_MIN = 44` to `LINE × 2 + 4 = 28` (`TABLE_H = 128`).

`Style::overflow` adds the half of Flutter's `TextOverflow` that a `nowrap` run can use: `Clip` (the default) and `Ellipsis`, with `.ellipsis()` as shorthand, used by `Banner` / `Button` / `EmptyState` / `FilterChips` / `Segmented` / `Stepper`. The vector table already carries `…` (`iot-tool outline` always appends `…` and `□`), so `vector-sample.txt` needed no change for it.

The Chinese captions needed 44 new hanzi and `-`, so `vector-sample.txt` grew and `vector.bin` was recut to 351 glyphs / **182,810 B** (from 306 glyphs / 157,460 B).

### Measured

| Item | Result |
| --- | --- |
| Host tests | `cargo test -p iot-app --lib --no-default-features --features host`: **557 passed** |
| Tool tests | `moon run iot:test-tool`: **31 + 4 passed** |
| Format / lint | `cargo fmt --all -- --check` clean; riscv `cargo clippy --workspace --exclude iot-tool --target riscv32imac-unknown-none-elf -- -D warnings`, the S3 `iot-xtensa.sh clippy …` and `cargo clippy -p iot-core --all-targets -- -D warnings` all pass |
| Target checks | `moon run iot:check-host` / `check-c6` / `check-s3` all pass |
| Golden | Still 32 rows; `advanced` / `feedback` / `extended` / `data` re-recorded for the stretch, the captions, the scrim and the row height, `menu` / `widgets` unchanged |
| Custom slots | `Advanced` 2, `Feedback` 0, `Extended` 1 (keyboard and keypad share it), `Data` 5 |
| Images | S3 **2,064,128 / 6,291,456 B** (32.81%); C6 **1,861,584 / 6,291,456 B** (29.59%) |
| S3 main stack | Serial `[DISPLAY] … main stack 82264 B`, peak 20,696 B; `[STACK] core 1 peak 2000~2176 B of 24576 B` (two boots) |
| Flashing | S3 at `/dev/cu.usbmodem1442301`, C6 at `/dev/cu.usbmodem14201`, one write each; both print `this boot is healthy` |

In a shell without a tty, `moon run iot:flash` fails its last step: espflash opens an interactive monitor after writing and exits 1 with `Failed to initialize input reader`. The image is already written (the log's `Flashing has completed!` comes first); capture the boot log with `espflash monitor --non-interactive --after hard-reset` rather than flashing again.

## Four feel defects: untagged canvas slots and finished overlays

Three of the four the user reported (the `Extended` keypad dead, the `Data` chart and dial ignoring taps, the menu and sheet examples too thin) shared one root cause.

A tap on the wire is `Press` then `Tap`. The canvas slots inside `page::scroller` (`TABLE_SLOT` / `CHART_SLOT` / `GAUGE_SLOT` / `KEY_SLOT`) carried **no tag**: the press point's `tag_at` resolved to 0, so `Scroller::apply(Press)` claimed the gesture as a drag (`tag == 0 → claim`) and `dragging()` was true on the lift; both pages gate their tap branch on `!self.scroll.dragging()`, skipped it entirely, and `scroll.bind` ate the `Tap` as a `Release`. Tagged buttons and chips worked, so only the rows, bars, dial and cells inside those slots went dead — the table was broken too, though unreported. The old tests sent only `Tap`, never `Press`, so they could not see it.

Fix: tag each of the four slots (`TAG_TABLE` / `TAG_CHART` / `TAG_GAUGE` / `TAG_KEYPAD`) and grow `TagTable` by 3 / 1; a press is only remembered, and only a move past `DRAG_SLOP` claims the scroll. The regression tests run the whole `Press → Tap` chain through `bind`, assert the row / bar / dial / cell really reach the intent, and pin "every canvas hit area's centre has a non-zero `tag_at`".

Keyboard: a new `Layer::More` holds the 22 punctuation marks `SYMBOLS` left out (`" ' ( ) [ ] { } < > / \ & % + = * $ ~ ^ | ` `` ` ``), so the four layers (letters / digits / symbols / more) type every printable ASCII byte `0x20..=0x7E`. Pure ASCII, all already in `ui12.bin`; `vector.bin` needed no recut.

Menu and sheet: converted from always-inline `Widget`s to canvas overlays shaped like `Dialog` (`card` + `item_at` / `row_at` + `draw`, opaque scrim). `Data` gains two always-declared root `absolute` slots `MENU_SLOT` / `SHEET_SLOT`, drawn only while open; the triggers are tagged buttons, the menu is anchored at its trigger's bottom edge (the snapshot publishes `menu_anchor`, the paint clamps it inside the content area) and the sheet rises from the content area's bottom edge; while an overlay is open `bind` takes the tap before the scroller — a row picks and closes, the scrim or outside closes, every other gesture is swallowed. The pick is printed under its trigger with `page::reading`.

### Measured

| Item | Result |
| --- | --- |
| Host tests | `cargo test -p iot-app --lib --no-default-features --features host`: **568 passed** |
| Tool tests | `moon run iot:test-tool`: **4 passed** |
| Format / lint | `cargo fmt --all -- --check` clean; `cargo clippy -p iot-app --lib --no-default-features --features host -- -D warnings` passes (under `--tests`, 4 pre-existing warnings in the untouched `i18n/lang.rs`, `render/engine/test_support.rs`, `render/screen/painter.rs`) |
| Target checks | `moon run iot:check-host` / `check-c6` / `check-s3` all pass |
| Golden | Still 32 rows; the `data` default frame is unchanged (the triggers sit below the first screen and the overlays start closed), so nothing was re-recorded |
| Custom slots | `Data` 7 (`TABLE` / `CHART` / `GAUGE` / `SWIPE` / `DIALOG` / `MENU` / `SHEET`) |
| Images | S3 **2,065,824 / 6,291,456 B** (32.84%); C6 **1,866,448 / 6,291,456 B** (29.67%) |
| S3 main stack | Serial `[DISPLAY] … main stack 82296 B`; `[STACK] core 1 peak 2000 B of 24576 B` |
| Flashing | One write each to S3 and C6; both print `this boot is healthy` |

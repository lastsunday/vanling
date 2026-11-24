+++
title = "Advanced widgets and Feed"
description = "25 advanced widgets, one Feed and four galleries are in place: 552 host tests, 32 golden rows, an S3 main stack of 82,264 B, images of 2,037,312 / 1,833,280 B, both flashed and booting."
weight = 70
sort_by = "weight"

[extra]
source_file_hash = "3504ce5aaab2107d1ba276d047ebb97a56ae6e97"
translated_at = "2026-11-24T02:41:25Z"
+++

<!-- doc-audience: ai -->

# Advanced widgets and Feed

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

+++
title = "UI Widgets"
weight = 80
[extra]
source_file_hash = "0459f99644bec79a24bd1ddc6bb049c5a081d366"
translated_at = "2026-11-22T00:00:00Z"
+++

# UI Widgets

The panel's widget vocabulary: two shapes, states and theme, the basic widgets, the pages built from them, cell drawing, i18n and budgets.

> Status: the basic widgets are in place (`core::ui::widget` + `shared::kit::widgets`); advanced and optional widgets are only a list.

## Two shapes

A Flutter widget tree means one Widget per row, which does not work here: `Ui` lives on the stack, a frame holds 60 nodes, and a ten-row `ListView` would eat half of them. Widgets therefore come in two shapes:

| Shape | What it is | Fits | Node cost |
| --- | --- | --- | --- |
| **Widget** | `Ui` declares nodes; layout and paint walk one tree | Page headers, forms, dialogs, a few rows | 1–5 |
| **Cell** | `Canvas` draws directly, taking one custom slot | Long lists, key grids, charts, gauges, scrollbars | 0 |

The test is whether the row count grows with data. A fixed few rows are Widgets; one to N rows are Cells. A Cell still follows Flutter's slot design (`ListTile`'s `leading`/`title`/`subtitle`/`trailing`); only the ink lands on `Canvas`.

## Hard limits

| Constant | Value | Source |
| --- | --- | --- |
| Panel | 320×240, content bottom 228 | `ui::panel` |
| Nodes | `MAX_NODES = 60` | `ui::layout` |
| Text | `TEXT_BYTES = 2048` | `ui::declare` |
| Colours | `MAX_COLORS = 16` per declaration | `ui::color` |
| Custom slots | `PAINTS = 8` | `ui::declare` |
| Scroll areas | `MAX_SCROLL_AREAS = 4` | `ui::layout` |
| Touch floor | `TOUCH_MIN = 44` | `ui::panel` |
| Line / pitch | `LINE = 12` / `ROW_PITCH = 13` | `ui::panel` |
| Margin | `MARGIN = 10` | `ui::panel` |

Two hard rules for widgets: one instance stays within five nodes; `Theme` keeps six to eight colours, and widgets never intern colours of their own.

## Layers

| Location | Content |
| --- | --- |
| `core::ui::widget` | The `Widget` trait and `Ui::add`; general vocabulary, knows no page |
| `app::shared::kit::state` | The `State` bitset |
| `app::shared::kit::theme` | `Theme::PANEL` and button/field style resolution |
| `app::shared::kit::slot` | The `Slot` enum, replacing `Widget?` |
| `app::shared::kit::widgets` | Concrete widgets, one file each |
| `services/<page>` | Composes widgets and binds intents |

A widget takes props only: it does not read `DeviceState`, take callbacks, or embed user-visible text. The page translates snapshot state into `State` and `&[u8]` before calling.

## States

`State` is a bitset derived by the page every frame:

| Bit | Meaning | Source |
| --- | --- | --- |
| `PRESSED` | A finger is down | Gesture |
| `SELECTED` | Selected / switch on | Page state |
| `DISABLED` | Unavailable | Page state |
| `ERROR` | Validation failed | Page state |
| `DRAGGED` | Slider is being dragged | Gesture |
| `FOCUSED` | Keyboard focus | Button / key navigation |
| `SCROLLED_UNDER` | Covered by scrolled content | Scroll offset |

This mirrors Flutter's `WidgetState` without `hovered` (there is no mouse). Widgets store no state; it is passed again every frame, while persistent flags such as `SELECTED` live in the page or the snapshot.

The state layer is a translucent overlay: `Color::rgba(..., 0x40)` goes through `Canvas` as source-over, with no theme engine.

## Theme and palette

`Theme` holds `Color` (ARGB32), never `ColorId`. A `ColorId` is an index into *this declaration's* palette, interned by `Ui::color`; saving it across frames would point at another frame's slot.

```rust
pub struct Theme {
    pub ground: Color,
    pub ink: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_pressed: Color,
    pub on_accent: Color,
    pub danger: Color,
    pub selection: Color,
    pub gap: i16,
}

impl Theme {
    pub const PANEL: Self = Self { /* ... */ };
}
```

Interning the same colour twice is idempotent; `MAX_COLORS = 16` is shared by the whole page. A new widget first asks whether an existing token fits; only then does it add a colour.

## API

```rust
pub trait Widget {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId;
}

impl Ui<'_, '_> {
    pub fn add<W: Widget>(&mut self, widget: W) -> NodeId { widget.build(self) }
}
```

Rules:

- A widget is a value type with `Copy` props; text is `&[u8]`, never an owned `String`.
- Builder methods are `const fn` and `#[must_use]`.
- Containers take closures, `ui.column(style, |ui| ...)`; leaves implement `Widget`.
- `leading` / `trailing` use `Slot`, not `Option<Box<dyn Widget>>`.
- A widget returns `NodeId` and takes no `onTap`; the page maps hits to intents through `Style::tag` or `shape`.

Flutter to here:

| Flutter | Here | Why |
| --- | --- | --- |
| `Widget` / `BuildContext` | `Widget` / `&mut Ui` | Same shape |
| `StatefulWidget` | State in the page / snapshot | Rebuilt every frame |
| `WidgetState` | `State(u8)` | No hover |
| `WidgetStateProperty<T>` | `fn(&Theme, State) -> T` | No property objects |
| `ButtonStyle` | `Theme::button(...)` | No runtime theme |
| `ThemeData` | `const Theme` | Same |
| `child` / `children` | Closures | No `List<Widget>` |
| `leading: Widget?` | `Slot` | No boxing |
| `onTap` | `NodeId` + page `bind` | Input is its own pipeline |
| `ListView.builder` | Cell + `list_window` | Node budget |
| `showDialog` | `stack` overlay | Declarative |
| `InheritedWidget` | Explicit `&Theme` / snapshot | No implicit tree |
| `MediaQuery` / `Semantics` | Not done | Fixed panel, no screen reader |

## Basic widgets

Built and living in `shared/kit/widgets`:

| Widget | Shape | Nodes | Notes |
| --- | --- | --- | --- |
| `Divider` | W | 1 | A 1 px line |
| `Badge` | W | 2 | Small block for state or counts |
| `Chip` | W | 2 | Rounded tag, optional selected state |
| `Button` | W | 2 | filled / outlined / text |
| `IconButton` | W | 2 | Uses symbols from the 12 px face; no icon set |
| `ProgressBar` | W | 3 | Track + progress |
| `Spinner` | W | 1 | `\|` `/` `-` `\` frames; the page supplies the phase |
| `Switch` | W | 2 | Track + knob |
| `Checkbox` | W | 2–3 | Box + tick |
| `Radio` | W | 2–3 | Circle + dot |
| `Slider` | W | 3 | Track + knob; the page supplies the value |
| `TextField` | W | 2 | Value + caret; the keyboard belongs to the page |
| `Select` | W | 2 | Value + arrow |
| `Tooltip` | W | 2 | Anchor + floating line |
| `Avatar` | W | 2 | Initial or short mark |
| `ListTile` | C | 0 | List row: four text runs + selected fill |
| `Scrollbar` | C | 0 | Existing `kit::scroll` |
| `Image` | C | 0 | Camera frames and RGB565 assets through a custom slot |

`Text` / `RichText` need no wrapper: `ui.text` and `ui.span` are the vocabulary.

`ListTile` follows Flutter's fields: `title`, `subtitle`, `leading`, `trailing`, `state`, `dense`. `leading` / `trailing` are `Slot`:

```rust
pub enum Slot<'a> {
    None,
    Text(&'a [u8]),
    Badge(&'a [u8]),
    Mark(&'a [u8]),
    Glyph(char),
}
```

## Pages

| Page | What it does |
| --- | --- |
| `Menu` | A three-column button grid of every page; pages the board cannot back are greyed and do not navigate. The boot page whenever a screen is present. |
| `Widgets` | The basic-widget gallery: five sections (actions / selection / input / display / rows), each under a heading, the whole page in one scroll box; controls that can hold a value are tappable. |

`Menu` puts its buttons in `chrome::scroller`: 98×44 with a 6 px gap, three columns exactly filling the content width. Hit testing shares those constants plus the scroll offset, so it never depends on a fresh layout. A tap on the wire is `Press` then `Tap`: the button area claims a drag only on `Drag` and `Press` merely stops a glide, otherwise the tap would be swallowed as a scroll. A page the board cannot back is still listed, just `State::DISABLED`, and a tap does not navigate. A board without a screen still lands on `Ambient`; `DeviceState::landed_on` reads `Screen` from `surface_roles`.

The `Widgets` page keeps its interactive values in the page itself (`WidgetsSnapshot`): buttons and chips latch selected, switch / checkbox / radio toggle, the slider takes a tap or a drag, the text field focuses, the select cycles, the icon button starts and stops the spinner, and the list tile selects; `Badge` shows the switch and `ProgressBar` follows the slider. A control's root node carries a `Style::tag`; the page measures them once with `Ui::tags()` at construction and resolves a tap against the scroll offset. The layout stays put across languages and values, pinned by `the_tags_stay_put_across_languages_and_values`. One declaration still has to fit the 60-node / 2048-byte budget in `the_whole_gallery_fits_one_declaration`.

## Advanced widgets

Composites, navigation, feedback and data display, built as the product needs them. Not yet in place:

| Widget | Shape | Reference |
| --- | --- | --- |
| `ListView` (windowed) | C | `ListView.builder` |
| `GridView` / `Table` | C | `GridView` / `DataTable` |
| `Accordion` | W | `ExpansionTile` |
| `Tabs` / `TabView` | W | `TabBar` / `TabBarView` |
| `SegmentedButton` | W | `SegmentedButton` |
| `Stepper` | W | `Stepper` |
| `PageIndicator` | C | Carousel dots |
| `SwipeAction` | C | `Dismissible` |
| `AppBar` | W | `AppBar` |
| `NavigationBar` | W | `NavigationBar` |
| `Drawer` / `BottomSheet` | W | `Drawer` / `showModalBottomSheet` |
| `Menu` | W | `PopupMenuButton` |
| `Dialog` | W | `AlertDialog` |
| `Toast` | W | `SnackBar` |
| `Banner` | W | `MaterialBanner` |
| `EmptyState` / `ErrorState` | W | No official; Ant `Empty` / `Result` |
| `LoadingOverlay` | W | No official; MUI `Backdrop` |
| `Skeleton` | W/C | Community `Skeleton` |
| `Form` / `FormField` | W | `Form` / `FormField` |
| `NumberField` | W | `TextField` + formatter |
| `Keypad` | C | No official |
| `SearchField` | W | `SearchBar` |
| `FilterChips` | W | `FilterChip` |
| `Chart` / `Gauge` / `Scale` | C | No official; `fl_chart` is reference only |

## Optional widgets

| Widget | Decision |
| --- | --- |
| `Calendar` / `TimePicker` | When dates or timers are set |
| `ColorPicker` / `Rating` | Light effects and ratings |
| `TreeView` / `Timeline` / `Heatmap` | Sensor history |
| `QRCode` / `Barcode` | Setup sharing, device labels |
| `PageView` / `Carousel` | Multi-page onboarding |
| `ReorderableList` / `PullToRefresh` | Ordering and refresh |
| `RichTextEditor` | Selection and undo are expensive |
| Implicit animation / `Hero` / `Focus` | The page `tick` can drive them when needed |
| `SplitView` / `Masonry` | Desktop or multi-column; not this panel |

Explicitly out: `Map`, `Video`, `Lottie`/`Rive`, `Hover`, `Mouse`, `Semantics`, `RTL`, `MediaQuery`. Not for lack of capability, but for lack of an input device, a screen reader or multiple sizes.

## Internationalisation

Two layers: this documentation is bilingual; panel strings come from `shared::kit::i18n`.

The vocabulary is an enum with an exhaustive match, so a missing translation fails to compile:

```rust
pub enum Lang { ZhHans, En }

pub enum Msg { CameraTitle, NoCam, /* ... */ }

impl Msg {
    pub const fn bytes(self, lang: Lang) -> &'static [u8] { /* ... */ }
}
```

| Layer | Entry | Notes |
| --- | --- | --- |
| Build | `IOT_LANG=en\|zh` | Same `option_env!` and parsing as `IOT_BOOT_PAGE` |
| Runtime | `DeviceState.lang` | Reserved for a settings page |
| Snapshot | `FrameSnapshot.lang` | Pages read the snapshot only |
| Widgets | Take `&[u8]` only | Translation happens in the page |

The default is `en`, matching the panel's current wording; `IOT_LANG=zh` builds a Chinese panel.

Layout constraints:

- The reading label slot is `chrome::LABEL = 24 px`: four ASCII cells or two hanzi. Longer labels are cut, so labels stay short.
- Long strings go through `chrome::title` (full row) or two lines in `EmptyState`.
- The `~` from `text::fit` is a fallback, not a design; every reading label needs a width test.
- The bitmap `ui12.bin` covers GB2312 + Big5; the vector table covers only `vector-sample.txt`, and a missing glyph drops the whole line to the bitmap face. New strings require rerunning `iot-tool outline`.
- SSIDs, MACs, IPs and logs are data, not copy.

Not done: RTL, ICU plurals/gender/interpolation, locale number formats.

## Cell drawing

`ListTile` is the smallest example:

```rust
pub fn draw(&self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
    // selected fill + four text runs, zero nodes
}
```

A list lays out visible rows only:

```rust
for (band, index) in list_window(top, rows, total).enumerate() {
    let row = Rect::new(area.x, area.y + (band * ROW_H) as i32, area.w, ROW_H as i32);
    tile(index).draw(canvas, row, theme);
}
```

`Chart`, `Gauge`, `Keypad` and `Scrollbar` likewise go through `Ui::custom(slot)`, one slot per band; `PAINTS = 8` is the page-wide ceiling. Drawing and hit testing share one geometry source: the page's `shape` module.

## Hit testing and events

- Widgets: `Ui::headless` runs the same declaration and `ui.rect(node)` returns the rectangle. Tappable widgets carry `Style::tag`, and `ui.tag_at(x, y)` yields the intent id.
- Cells: the page's `shape` constants compute rectangles shared with drawing.
- A `NodeId` is valid only within one declaration and never crosses frames; use `tag` for a stable id.
- Dialogs and toasts are declared in a `stack` overlay; the page decides visibility.
- Widgets know nothing of `InputEvent`; `bind` turns `InputIntent` into `BusinessIntent`.

## Budgets and tests

| Item | Tool |
| --- | --- |
| Nodes | `ui.node_count()` |
| Text | `ui.text_bytes()` |
| Colours | The page-wide `MAX_COLORS` |
| Golden | One fingerprint per widget; the page table is separate |
| Hit | Painted rectangle equals the headless rectangle |

After a change:

```sh
cargo test -p iot-app --lib --no-default-features --features host
moon run iot:check-s3
moon run iot:check-c6
moon run iot:check-host
```

## Adding a widget

1. Pick the shape: Cells when the row count grows with data, otherwise Widgets.
2. Props are `Copy`, text is `&[u8]`, optional slots are `Slot`.
3. Use existing `Theme` tokens only; change `Theme` before adding a colour.
4. Implement `Widget` or `Cell::draw`, within five nodes per instance.
5. Add a golden and a hit test; update this list.
6. Add no Cargo feature; unreferenced modules are dropped by the linker.

## References

Flutter's [widget catalog](https://api.flutter.dev/flutter/widgets/widgets-library.html), [`ListTile`](https://api.flutter.dev/flutter/material/ListTile-class.html), [`WidgetState`](https://api.flutter.dev/flutter/widgets/WidgetState.html) and [`ButtonStyle`](https://api.flutter.dev/flutter/material/ButtonStyle-class.html); MUI's component taxonomy; shadcn/ui's component list and `data-state`; LVGL's parts/states; embedded-gui and Guillotine from the Rust ecosystem are API comparisons only, not dependencies.

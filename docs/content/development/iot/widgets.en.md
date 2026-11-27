+++
title = "UI Widgets"
weight = 80
[extra]
source_file_hash = "5a77f4b1ebb718dae997d2025e3043163fcf1e44"
translated_at = "2026-11-27T02:00:00Z"
+++

# UI Widgets

The panel's widget vocabulary: two shapes, states and theme, rounded corners, the basic widgets, the pages built from them, cell drawing, i18n and budgets.

> Status: the basic widgets and 37 advanced widgets are in place, shown by the `Widgets` page and the `Advanced` / `Feedback` / `Extended` / `Data` / `Planner` / `Records` galleries; one `Feed` component provides both pull-to-refresh and load-more. The optional list is down to `QRCode` / `Barcode` and a few deliberate omissions.

## Two shapes

A Flutter widget tree means one Widget per row, which does not work here: `Ui` lives on the stack, a frame holds 64 nodes, and a ten-row `ListView` would eat half of them. Widgets therefore come in two shapes:

| Shape | What it is | Fits | Node cost |
| --- | --- | --- | --- |
| **Widget** | `Ui` declares nodes; layout and paint walk one tree | Page headers, forms, dialogs, a few rows | 1–5 |
| **Cell** | `Canvas` draws directly, taking one custom slot | Long lists, key grids, charts, gauges, scrollbars | 0 |

The test is whether the row count grows with data. A fixed few rows are Widgets; one to N rows are Cells. A Cell still follows Flutter's slot design (`ListTile`'s `leading`/`title`/`subtitle`/`trailing`); only the drawing lands on `Canvas`.

## Hard limits

| Constant | Value | Source |
| --- | --- | --- |
| Panel | 320×240, content bottom 228 | `ui::panel` |
| Nodes | `MAX_NODES = 64` | `ui::layout` |
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
| `app::shared::i18n` | `Lang`, `Msg` and the fixed words; words only, no pixels |
| `app::shared::ui::state` | The `State` bitset |
| `app::shared::ui::theme` | `Theme::PANEL` / `Theme::LIGHT` and button/field style resolution |
| `app::shared::ui::slot` | The `Slot` enum, replacing `Widget?` |
| `app::shared::ui::widgets` | Concrete widgets, one file each |
| `app::shared::page` | Page furniture: margins, line rhythm, title, scrolling body, reading row |
| `app::shared::surface` | `Panel`: lays a declaration out into boxes and paints them |
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

There is no two-way binding (Slint/QML's `<=>`). Flutter's `TextEditingController` is also a controller plus a rebuild, not a binding; here the snapshot is `Copy` and `view` is pure, so listeners would turn "repaint follows the snapshot diff" into "whoever notifies repaints". Editing state uses controllers instead: `ui::scroll::ScrollState` and `ui::keyboard::Text`.

## Theme and palette

`Theme` holds `Color` (ARGB32), never `ColorId`. A `ColorId` is an index into *this declaration's* palette, interned by `Ui::color`; saving it across frames would point at another frame's slot.

```rust
pub struct Theme {
    pub ground: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_pressed: Color,
    pub on_accent: Color,
    pub danger: Color,
    pub selection: Color,
    pub overlay: Color,
    pub surface: Color,
    pub gap: i16,
}

impl Theme {
    pub const PANEL: Self = Self { /* dark, the boot default */ };
    pub const LIGHT: Self = Self { /* Solarized Light */ };
    pub const fn of(id: ThemeId) -> &'static Self { /* ... */ }
}
```

`ThemeId { Dark, Light }` lives in `iot-core::ui::Screen` and travels from `DeviceState.theme` through `FrameSnapshot.theme` to the paint layer; widgets default to `Theme::of(ui.screen().theme)` and pages to `Theme::of(snapshot.theme)`. `.theme(&Theme)` still overrides a single widget. A switch swaps constant palettes and generates no colours: `ScreenRenderer::present` papers a light page with `ground` and takes the panel backlight only from `Backlight` (0 is dark), `Network` / `Camera` keep their own black ground, and `Lighting` plus an active glass breath force dark. Theme, like language, is runtime state and is not persisted; a build can set the boot value with `IOT_THEME=light`.

Interning the same colour twice is idempotent; `MAX_COLORS = 16` is shared by the whole page. A new widget first asks whether an existing token fits; only then does it add a colour.

## Rounded corners

`Style::radius(u8)` cuts the node's own paper and outline; `0` is square. The radius is clamped to half the short side and to `MAX_RADIUS = 16`, so a 24×24 `Avatar` written as `12` is a circle. Drawing uses the integer test `(2r-2c-1)² + (2r-2k-1)² <= 4r²` row by row: no table, no arcs, no allocation.

Only the node's own paper is cut — background and outline; children and glyphs are not clipped. A radius does not inherit across nodes: a container that should be round carries the radius itself.

| Widget | Radius |
| --- | --- |
| `Button` / `Chip` / `TextField` / `Select` | `theme::CORNER = 4` |
| `Badge` | 3 |
| `Avatar` | 12 (a 24 px box, i.e. half a side) |
| `Indicator`'s mark | dot 4 (`DOT = 8`) / number 7 (`PILL = 14`) |

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
| `ButtonStyle` | `Theme`'s `button(...)` | Constant palettes, no style objects |
| `ThemeData` | `const Theme` + `ThemeId` | A switch swaps constants; no theme tree |
| `child` / `children` | Closures | No `List<Widget>` |
| `leading: Widget?` | `Slot` | No boxing |
| `onTap` | `NodeId` + page `bind` | Input is its own pipeline |
| `ListView.builder` | Cell + `list_window` | Node budget |
| `showDialog` | `stack` overlay | Declarative |
| `InheritedWidget` | Explicit `&Theme` / snapshot | No implicit tree |
| `MediaQuery` / `Semantics` | Not done | Fixed panel, no screen reader |

## Basic widgets

Built and living in `shared/ui/widgets`:

| Widget | Shape | Nodes | Notes |
| --- | --- | --- | --- |
| `Divider` | W | 1 | A 1 px line |
| `Badge` | W | 1 | Small block; `Filled` / `Light` / `Outline` variants over a separate `Tone` colour axis |
| `Chip` | W | 2 | Rounded tag, optional selected state |
| `Button` | W | 2 | filled / outlined / text |
| `IconButton` | W | 2 | Uses symbols from the 12 px face; no icon set |
| `Indicator` | W | 2 | A corner mark over another element: a dot, or a number with `label` |
| `ProgressBar` | W | 3 | Track + progress |
| `Spinner` | W | 1 | `\|` `/` `-` `\` frames; the page supplies the phase |
| `Switch` | W | 2 | Track + knob |
| `Checkbox` | W | 2–3 | Box + tick |
| `Radio` | W | 2–3 | Circle + dot |
| `Slider` | W | 3 | Track + knob; the page supplies the value |
| `TextField` | W | 2 | Value + caret; once focused the page hands over to `ui::keyboard` |
| `Select` | W | 2 | Value + arrow |
| `Tooltip` | W | 2 | Anchor + floating line |
| `Avatar` | W | 1 | Initial or short mark on a round fill |
| `ListTile` | C | 0 | List row: four text runs + selected fill |
| `Scrollbar` | C | 0 | Existing `ui::scroll` |
| `Image` | C | 0 | Camera frames and RGB565 assets through a custom slot |

`Text` / `RichText` need no wrapper: `ui.text` and `ui.span` are the vocabulary.

Type has only the bitmap's whole steps: page titles, button labels and the `Text` page's paragraph at 24 px (`page::title`), other readings at 12 px. `Metrics::at(13..=24)` jumps to 24 px, and there is no 14 or 20 px; 24 px is twice the face, so an eight-letter label like `ATTITUDE` takes 96 px of a 98 px button. A missing glyph draws □ with no whole-run fallback; `every_word_the_panel_prints_has_a_bitmap_glyph` pins every character of `Msg::ALL` × `Lang::ALL` to `Face::has`, and a new string with a missing glyph goes through `iot-tool font --merge <bdf> --chars <list>`.

Text grows a layout, but only the nodes that did not fix their size: with no `width`, a text node takes the run's advance width and `line_height` (glyphs plus leading), so a column gets taller and a row gets wider. A `width` on a `nowrap` run clips it instead, and a `Button` stays 98×44 however large its label is; changing the type size means changing the layout constants with it. The panel is a fixed 320×240: growth comes out of the `grow` nodes and then runs off the panel, it never scales the layout down.

`Style::overflow(Overflow)` covers only a `nowrap` run wider than its box, the usable half of Flutter's `TextOverflow`: `Clip` (the default) cuts at the box edge, `Ellipsis` cuts and ends the visible run with `…`; the box never grows. `.ellipsis()` is shorthand for `nowrap + Ellipsis`, used by `Banner` / `Button` / `EmptyState` / `FilterChips` / `Segmented` / `Stepper` so a label that does not fit stays inside. Wrapping runs are unaffected and still stop at `max_lines`.

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
| `Menu` | A three-column button grid grouped into devices / UI / glow, listing every page; pages the board cannot back are greyed and do not navigate. The boot page whenever a screen is present. |
| `Widgets` | The basic-widget gallery: five sections (actions / selection / input / display / rows), each under a heading, the whole page in one scroll box; controls that can hold a value are tappable. |
| `Advanced` | The navigation / composed gallery: `AppBar` / `NavigationBar` / `Tabs` / `PageIndicator` / `Pager` / `Carousel` and `Accordion` / `Segmented` / `Stepper`, one scroll box. |
| `Planner` | The planning gallery: `MonthGrid` / `Wheel` / `Rating` / `ColorPicker` plus the `DatePicker` and `TimePicker` dialogs, one scroll box; wheels drag or take a row, and the date/time buttons raise a dialog. |
| `Records` | The records gallery: `TreeView` / `Timeline` / `Heatmap` / `ReorderableList`, one scroll box; drag a row by its right-hand handle. |
| `Feedback` | The feedback gallery: `Banner` / `Toast` / `Skeleton` / `LoadingOverlay`; the banner raises a toast and the placeholder swaps for content. |
| `Extended` | The input gallery: `SearchField` / `FilterChips` / `FormField` / `NumberField` / `Keypad`; focusing the search field hands the page to the keyboard. |
| `Data` | The data / overlay gallery: `Table` / `Chart` / `Gauge` / `SwipeAction` / `EmptyState` and `Dialog` / `PopupMenu` / `Sheet`; a row, a bar and the dial share the page state. |
| `List` | The `Feed` demo: pull to refresh, load more, swipe to delete, empty state. |
| `Lighting` | The lighting page: one of three scopes (off / this page / every page), three rows of four styles (steady / breath / cycle / aurora; candle / pulse / heartbeat / flash; sparkle / strobe / police / SOS), three sliders for period, hue and peak, and a separate backlight row (0 is dark); the caption at the foot follows the style and the period slider is disabled under steady. The page is always dark. |
| `Headless` | The landing page on a board with no panel (the C6): no canvas, only the lamp and the buttons; a click changes the effect (the twelve, then off), a hold steps brightness (30/50/70/90/100%, then off; a dark lamp answers by lighting at the first stop), a double-click the colour, a triple-click raises brightness (multi-click is off by default and needs a runtime switch), and a hold through the splash opens provisioning; it is not in the menu and not in `PageId::ALL`. |

Every sample in the six galleries carries one caption (`page::caption`): its name plus how it moves, such as `TABLE / TAP A ROW`, with one `Msg` per language. That is why `Advanced` and `Extended` lost their section words — the caption already names the sample; `Data` keeps `page::section` (a rule and a name) for the overlay band. A caption is a muted `nowrap` run with no tag, so it does nothing when tapped.

`Menu` puts its buttons in `page::scroller`: 98×44 with a 6 px gap, three columns exactly filling the content width. Each button carries the tag `PageId::code() + 1` and the page measures them once with `TagTable::measure`; a tap resolves the tag against the scroll offset and reports `MenuIntent::Choose { page }`, which `Services::apply` turns into navigation. Hit testing no longer recomputes the grid. A tap on the wire is `Press` then `Tap`: the button area claims a drag only on `Drag` and `Press` merely stops a glide, otherwise the tap would be swallowed as a scroll. A page the board cannot back is still listed, just `State::DISABLED`, and a tap does not navigate. A board without a screen lands on `Headless`; a request for `Headless` on a screen falls back to `Input`, and `DeviceState::landed_on` reads `Screen` from `surface_roles`. The title row is `TOUCH_MIN` tall with a `GAP` of air beneath it, and carries four 44×44 chips at its right, `EN` / `中文` / `DARK` / `LIGHT` (`the_settings_chips_are_touch_sized` pins them); the first two set `DeviceState.lang`, the last two set `DeviceState.theme`. The button grid therefore starts a whole row lower; the seventeen entries are grouped into devices / UI / glow, each under a rule and a muted caption, still three columns per group: seven rows in all, the last group needs a scroll to reach, with hits still resolved against the offset.

The `Widgets` page keeps its interactive values in the page itself (`WidgetsSnapshot`): buttons and chips latch selected, switch / checkbox / radio toggle, the slider takes a tap or a drag, the select cycles, the icon button starts and stops the spinner, and the list tile selects; `Badge` shows the switch, `ProgressBar` follows the slider, and `Divider` separates the five sections. Section captions name the action rather than the noun (`TAP TO LATCH`, `PICK OR TOGGLE`, `DRAG AND TYPE`, `LIVE STATE`, `ROW`). The `LIVE STATE` row is the spinner, the avatar with a numbered `Indicator`, the icon button and the three `Badge` variants, with `SPINNER AVATAR ICON BADGE` beneath it because the shapes do not identify themselves. The badges read `ON` / `OFF` with the switch, and `Filled` / `Light` / `Outline` are three papers for the same `Tone`. The field prints `WidgetsSnapshot::text` and only falls back to `VALUE` while empty; the row's subtitle is `SUBTITLE`, describing the row's parts rather than an action. A control's root node carries `WidgetsAction::tag()`; the same enum supplies `ALL` and `from_tag`, and an exhaustive `match` means a control without a handler does not compile. `every_action_is_drawn_once` keeps the view and the vocabulary in step. The page measures the tags once with `TagTable::measure` and resolves a tap against the scroll offset. The layout stays put across languages and values, pinned by `the_tags_stay_put_across_languages_and_values`. One declaration still has to fit the 64-node / 2048-byte budget in `the_whole_gallery_fits_one_declaration`.

A focused `TextField` replaces the page with a 4×4 keyboard (`ui::keyboard`, a Canvas with 0 nodes): `Text` holds 24 bytes, the multi-tap cycle and the shift/layer state, and `OK` puts the gallery back. The grid, its drawing and its hit test all read one `CELLS` table. Four layers rotate (letters / digits / symbols / more) and together type every printable ASCII byte `0x20..=0x7E`: the letter layer carries both cases and the space, the symbol layer holds `. , : ; ? ! - _ @ #`, and the more layer fills in the remaining 22 punctuation marks; each layer key's face names the next layer (`12` / `#` / `&` / `ab`).

## Advanced widgets

Navigation, composed, planning, records, feedback, input, data and overlay, all in `shared/ui/widgets`, one file each. Six pages show them: `Advanced` covers navigation and composed, `Planner` planning, `Records` records, `Feedback` feedback, `Extended` input, and `Data` data and overlay.

### Navigation

| Widget | Shape | Notes |
| --- | --- | --- |
| `AppBar` | W | Title plus an action at the right; the action carries a tag |
| `NavigationBar` | W | Three-cell bottom navigation: an inset accent pill on a `surface` strip |
| `Tabs` | W | Three tabs; the selected cell takes the accent paper whole |
| `PageIndicator` | C | Carousel dots; `dot_at` gives each dot a `TOUCH_MIN`-wide target, `fit` drops dots that do not fit |
| `Pager` | W | A horizontal card strip: `card_width` / `pitch` derive from `rect`, `turn` maps travel to an index, `page_at` serves taps |
| `Carousel` | C | `Pager`'s layout plus a dwell bar underneath: `progress` says how much of the next turn is left and `running` comes from the page's switch; this page does not autoplay by default |

### Composed

| Widget | Shape | Notes |
| --- | --- | --- |
| `Accordion` | W | Title plus a body inside the same paper; the page owns `open` |
| `Segmented` | W | Three mutually exclusive segments |
| `Stepper` | W | Three-step progress, `current` highlighted |

### Planning

| Widget | Shape | Notes |
| --- | --- | --- |
| `Wheel` | C | One generic vertical wheel: `Labels::Fixed` for any run of copy or `Labels::Numbers { min, max, step, pad }`; drag follows the finger, a lift projects `v²/2a`, then snaps, rebounds at the ends and optionally wraps; the selection band (two lines) stays on the centre row while the wheel scrolls; 3~7 visible rows at 28~44 px, all derived from `rect` |
| `MonthGrid` (`calendar`) | C | A 7×6 month grid whose cell width and height come from `rect`, capped at 48; `fits` tells the parent whether it has room, and `day_at` shares `Metrics` with the paint; previous/next month are the parent's own tags |
| `DatePicker` | C | A date dialog: a month grid with prev/next when the body is tall enough, otherwise year/month/day wheels; the year runs 1970–2100 without wrapping, month and day wrap, and a shorter month pulls the day back |
| `TimePicker` | C | Two or three wheel columns for hour/minute/second with `:` between them; `column_at` shares the column geometry with the paint, and the seconds column is optional |
| `Rating` | C | Five 44×44 slots with whole-star input from 0 to 5; a fraction is read-only fill, and the star is 12 horizontal spans |
| `ColorPicker` | C | A 132×132 saturation/value square (12×12 blocks), a 16 px hue strip and a 44 px swatch; tap and drag share `point_at`, and hue goes through `iot_core::light::hsv_to_rgb` |

### Records

| Widget | Shape | Notes |
| --- | --- | --- |
| `TreeView` | C | 24 px rows, 10 px indent and a chevron; `row_at` picks a row and the page owns the open state |
| `Timeline` | C | 40 px rows, a left rail with dots, and time and title columns |
| `Heatmap` | C | 53×7 cells (4 px cell, 1 px gap); levels come deterministically from a seed, and `cell_at` shares the paint's geometry |
| `ReorderableList` | C | 32 px rows with a 44×44 handle at the right owning the drag; a move is `remove(old)` → `insert(clamped)` on release |

#### Repaint extent

A page does not name repaint rectangles: after layout the framework derives the damage from per-node fingerprints (`Style`, the text bytes and a custom slot's `state`), and only a changed structure is a whole frame. A custom slot declares one `u32` covering its drawing through `Ui::custom_state(style, slot, state)`; `Ui::custom` says the page does not describe its pixels, so the slot is repainted whenever the snapshot moves. When the slot paints its whole rectangle itself, add `Style::cover()` so the framework may skip the nodes it hides. A partial frame lays the ground back down inside the damage, then redraws the intersecting nodes in tree order, so a card or scrim drawn later covers it again. Measurements are in `records/iot/panel-buffering.md`.

## Feedback

| Widget | Shape | Notes |
| --- | --- | --- |
| `Banner` | W | Message plus an action |
| `Toast` | W | One-line notice; the page gives the width |
| `Skeleton` | W | Grey placeholder bar |
| `LoadingOverlay` | W | Translucent scrim plus a caption, switched by `on` |

### Input

| Widget | Shape | Notes |
| --- | --- | --- |
| `SearchField` | W | Magnifier cell plus value/placeholder; a tap hands the page to `ui::keyboard` |
| `FilterChips` | W | Three multi-select chips; the selection is a bit set |
| `FormField` | W | Label, value and hint; `State::ERROR` draws the error edge |
| `NumberField` | W | A value with minus/plus 44 px hit areas |
| `Keypad` | C | 4×3 phone keypad; `key_at` and `draw` share `KEYS` |

### Data

| Widget | Shape | Notes |
| --- | --- | --- |
| `Table` | C | A header plus four fixed rows, evenly split columns; a row is `LINE × 2 + 4 = 28` |
| `Chart` | C | Bars for values 0–100 |
| `Gauge` | C | Half dial with a needle |
| `SwipeAction` | C | A left slide uncovers an action band, capped at `REVEAL_MAX` |
| `EmptyState` | W | Title and hint in `Tone::Empty` or `Tone::Error` |

### Overlay

| Widget | Shape | Notes |
| --- | --- | --- |
| `Dialog` | C | Opaque scrim and a centred card; `action_at` splits OK from cancel |
| `PopupMenu` | C | Opaque scrim and a three-row menu hung under its trigger; `item_at` splits rows, the selected one filled |
| `Sheet` | C | Opaque scrim and an edge drawer (`Edge::Bottom` / `Edge::Left`); `row_at` splits rows, the demo uses the bottom edge |
| `Picker` (dialog) | C | A scrim, a centred card, a title, a body and Cancel/OK; the card is `min(rect − 2×8, 360×480)` and the action band 32~44; `action_at` splits OK, cancel and a scrim tap; the scrim covers only the bands around the card and its four corners, so repainting the card's body costs no blend |

Every tappable control reports a tag: widgets through `Style::tag`, canvas slots (Dialog, Keypad, Table, Chart, Gauge, SwipeAction) on the slot itself, all measured once at construction with `TagTable::measure`. A gallery's `bind` returns only `Gallery { wire, intent }`, where `wire` is the route code, so `shared::intent` does not depend on `shared::route`. A canvas slot's hit test uses the rectangle from `Ui::paint_rect`, the same geometry as the paint, and Table rows, Chart bars and Keypad cells reuse the equal-split arithmetic their painters use. Overlay items (three menu rows, two sheet rows) are not declared nodes; `bind` splits them with `item_at` / `row_at`.

The `Data` dialog, popup menu and sheet all sit on root nodes after `page::body`: `absolute`, `320 × 228`, one slot each, always declared and drawn only while open, covering the title and the whole body. They therefore neither scroll with the list nor show the controls underneath through a translucent scrim. The menu is anchored at its trigger's bottom edge (the snapshot publishes `menu_anchor` and the paint clamps it inside the content area) and the sheet rises from the content area's bottom edge; while an overlay is open `bind` takes the tap first — a row picks and closes, the scrim or outside closes.

`Pager` and `PageIndicator` share one index on `Advanced`: cards drag, cards tap and dots tap, and all three land on the same card. Card width and pitch derive from the strip, so no pixel count is pinned.

### Tap matrix

| Page | Tappable | Decoration only |
| --- | --- | --- |
| `Advanced` | AppBar action, three nav cells, three tabs, every dot, every card of both strips, the `Auto` switch, the accordion header, the segmented cells, the stepper | The selected tab's and nav pill's highlights and the dwell bar itself |
| `Planner` | Previous/next month, every day, the hour wheel (drag or tap a row), the `DATE` / `TIME` buttons, the dialog's year/month/day wheels, the month-grid days and prev/next, Cancel/OK, the five stars, the hue strip and the colour square | Blank month cells, the weekday header, the `:` between wheel columns |
| `Records` | Six tree rows, four timeline entries, every heat cell, each row's 44 px handle | The timeline rail, the heatmap month labels |
| `Feedback` | Banner action, toast, placeholder toggle, scrim caption | `Skeleton` bars, `Divider` |
| `Extended` | Search field, three chips, the form, the number stepper, twelve keys | — |
| `Data` | Four table rows, six bars, the dial, the sliding row, the empty state, two dialog answers, the menu/sheet triggers and the rows they open | The chart baseline |

The test is whether there is an action: decorative widgets carry no tag and do nothing when tapped, while a tagged control's root node is the hit area, so the page never guesses at inner layout. Press and drag are arbitrated by `Scroller` in `services/gallery.rs`: it records the press point and tag, a move within `DRAG_SLOP` is a Tap, a longer vertical move claims the scroll region, and a horizontal move on a slide control becomes `Slide`; the first claimer owns the gesture. Canvas slots (table, chart, dial, keypad) must carry a tag too — untagged, the press point resolves to tag 0, the scroller claims the gesture as a drag, and the lift's Tap is eaten as a `Release`, so the rows, bars, dial and cells inside the slot stop answering. The lift after a drag still arrives on the wire as a `Tap`, so a page's tap branch asks `Scroller::dragging()` first: the `Advanced` cards, the `Extended` keypad and the `Data` rows and bars select nothing when a drag lifts, and the next drag scrolls as usual.

### Adaptive sizing

- Tappable elements bottom out at `TOUCH_MIN = 44`: the `Tabs` / `Segmented` / `Stepper` / `NavigationBar` rows, the `Accordion` header, the `Banner` action, the `NumberField` steppers and a `Keypad` cell all take it.
- `page::scroller` stretches its body: a row takes the content width, so cells written as `grow(1) + basis(0)` actually get a share of it. Without the stretch such a row measures its children's natural widths, and `basis(0)` collapses the whole row to zero.
- Canvas widgets derive their size from `rect` and text metrics: `Table` uses `LINE + 4` for the header and `LINE × 2 + 4 = 28` per row (`TABLE_H = 128`); `Dialog` uses `TOUCH_MIN` for the action band and sums the band, the title/body lines and `MARGIN` for the card; `Chart` / `Gauge` / `Keypad` scale with `rect`; `SwipeAction` still caps the uncovered band at `REVEAL_MAX`.
- A page's custom-slot heights are multiples of `page::TOUCH` (two rows for the dial and chart, four for the keypad), not sample magic numbers.
- The date and time widgets read no panel constant and hard-code no 306 / 42 / 32: `Wheel` takes `rect.h / 28`, clamps to an odd 3~7 visible rows and caps the row height at 44; `MonthGrid` takes `(rect.w − 6×1) / 7` and `(rect.h − 12) / 6`, both capped at 48; the `Picker` card is `min(rect − 2×8, 360×480)` and centred. Four sizes — 320×228, 480×320, 800×600 and 240×180 — each have a test.
- The parent chooses the shape by `fits`: `MonthGrid::fits` needs 28 px cells, and the 320×228 dialog body is only 140 px, so `DatePicker::kind` picks the three wheels; only 480×320 and up get the grid. A small screen never squeezes the grid to 21 px.

## Feed

`shared/ui/feed.rs` is one component for three jobs: a list, pull-to-refresh and load-more. Both edge gestures act on one scroll region, and two components would duplicate the offset and the drag state.

| Item | Contract |
| --- | --- |
| Rows | `ROW_H` starts at `TOUCH_MIN`; a `Source` supplies rows, and painting declares no nodes |
| Content | `content(rows) = rows * ROW_H + FOOTER_H`; the footer always stays inside, so `max` does not jump |
| Pull | The top edge overscrolls only when `ScrollConfig.pull > 0`; a release past `PULL_MIN = 28 px` refreshes |
| Load | A release within `LOAD_AHEAD = 24 px` of the end asks for the next page, once per row count |
| Footer | `More::{Idle, Running, End, Failed}`; a failed page may be asked again |
| Answer | The page calls `loaded(rows, end)` when its data returns, or `failed()` |
| Delete | The page owns the band a left slide uncovers; `Feed` only takes `reveal: Option<(row, px)>` |

`Feed` owns no data: `FeedView` takes a `Source`, `Words`, the offset, the pull and the two edge states, and `draw` paints the visible rows and the footer in one pass. `asked_at` records the row count of the last request, so the same count is never asked twice; `reset()` clears the drag, the edges and that record.

The `List` page is the demo: 34 rows over pages of 8, with 600 ms of fake work per edge. A pull resets to the first page, loading appends pages until `End`, a left slide then a tap on the band deletes the row, and a tap on the row itself selects it. With no rows the list shows `EMPTY` centred, and the pull indicator floats at the top.

## Optional widgets

| Widget | Decision |
| --- | --- |
| `QRCode` / `Barcode` | Setup sharing, device labels; not this round |
| `RichTextEditor` | Selection and undo are expensive |
| Implicit animation / `Hero` / `Focus` | The page `tick` can drive them when needed |
| `SplitView` / `Masonry` | Desktop or multi-column; not this panel |

Landed: `Wheel` / `MonthGrid` / `DatePicker` / `TimePicker` / `Rating` / `ColorPicker` on `Planner`, `TreeView` / `Timeline` / `Heatmap` / `ReorderableList` on `Records`, and `Carousel` on `Advanced`. `PageView` is not a widget of its own: `Pager` plus `PageIndicator` already covers turning and locating a page.

Explicitly out: `Map`, `Video`, `Lottie`/`Rive`, `Hover`, `Mouse`, `Semantics`, `RTL`, `MediaQuery`. Not for lack of capability, but for lack of an input device, a screen reader or multiple sizes.

## Internationalisation

Two layers: this documentation is bilingual; panel strings come from `shared::i18n`.

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
| Runtime | `SettingsIntent::SetLang` | The `EN` / `中文` chips on `Menu`; `services::settings` writes `DeviceState.lang` |
| Snapshot | `FrameSnapshot.lang` | Pages read the snapshot only |
| Widgets | Take `&[u8]` only | Translation happens in the page |

A `lang` change repaints on its own in `render::screen::repaint_due`, so every speaking page changes words together; pages do not each carry a condition, and a new page cannot forget one.

The default is `en`, matching the panel's current wording; `IOT_LANG=zh` builds a Chinese panel.

Layout constraints:

- The reading label slot is `page::LABEL = 24 px`: four ASCII cells or two hanzi. Longer labels are cut, so labels stay short.
- Long strings go through `page::title` (full row) or two lines in `EmptyState`.
- The `~` from `text::fit` is a fallback, not a design; every reading label needs a width test.
- The bitmap `ui12.bin` covers GB2312 + Big5; a missing glyph draws □. New strings require `iot-tool font --merge <bdf> --chars <list>`, with `every_word_the_panel_prints_has_a_bitmap_glyph` passing.
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

- Widgets: `Ui::headless` runs the same declaration and `Ui::tags()` measures every tag once into `ui::tag::TagTable`. Tappable widgets carry `Style::tag`, and `TagTable::at(x, y, offset)` yields the intent id.
- Cells: the page's `shape` constants compute rectangles shared with drawing.
- A `NodeId` is valid only within one declaration and never crosses frames; use `tag` for a stable id.
- An overlay is an `absolute` child; flex, wrap and scroll boxes all place their out-of-flow children, and an `absolute` child on the root measures correctly. Toasts still live in a `stack`, with the page deciding visibility.
- Widgets know nothing of `InputEvent`; `bind` turns `InputIntent` into `BusinessIntent`.
- A hold latch belongs to the page (the slider's is `values.pressed`) and every gesture that ends must clear it: with the latch still set, a 2 px drift binds as a drag and the next tap never becomes a `Tap`.

## Budgets and tests

| Item | Tool |
| --- | --- |
| Nodes | `ui.node_count()` |
| Text | `ui.text_bytes()` |
| Colours | The page-wide `MAX_COLORS` |
| Golden | One fingerprint per page and debug state; `Advanced` / `Planner` / `Records` / `Feedback` / `Extended` / `Data` / `List` add two each |
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

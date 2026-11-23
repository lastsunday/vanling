+++
title = "UI 组件"
weight = 80
+++

# UI 组件

面板的组件词汇：两种形态、状态与主题、圆角、基础组件、Cell 绘制、多语言与预算。

> 状态：基础组件、`Menu` 页与基础组件演示页已落地；高级与可选组件只是清单。

## 两种形态

Flutter 的组件树每行一个 Widget，这里不行：`Ui` 在栈上，一帧只有 60 个节点，`ListView` 十行就吃掉一半。组件因此分两种：

| 形态 | 是什么 | 适合 | 节点成本 |
| --- | --- | --- | --- |
| **Widget** | `Ui` 声明节点，布局与绘制走同一棵树 | 页头、表单、对话框、少量行 | 1~5 |
| **Cell** | `Canvas` 直接画，占一个 custom 槽 | 长列表、键格、图表、仪表盘、滚动条 | 0 |

判据是“行数是否随数据增长”。固定几行用 Widget；一到 N 行用 Cell。Cell 的 API 仍照 Flutter 的槽位设计（`ListTile` 的 `leading`/`title`/`subtitle`/`trailing`），只是落墨在 `Canvas`。

## 硬约束

| 常量 | 值 | 出处 |
| --- | --- | --- |
| 面板 | 320×240，内容底 228 | `ui::panel` |
| 节点 | `MAX_NODES = 60` | `ui::layout` |
| 文本 | `TEXT_BYTES = 2048` | `ui::declare` |
| 颜色 | `MAX_COLORS = 16` / 声明 | `ui::color` |
| custom 槽 | `PAINTS = 8` | `ui::declare` |
| 滚动区 | `MAX_SCROLL_AREAS = 4` | `ui::layout` |
| 触控下限 | `TOUCH_MIN = 44` | `ui::panel` |
| 行高 / 行距 | `LINE = 12` / `ROW_PITCH = 13` | `ui::panel` |
| 边距 | `MARGIN = 10` | `ui::panel` |

组件规范里两条硬线：单实例不超过 5 个节点；`Theme` 的颜色控制在 6~8 个，组件不各自 intern 新色。

## 分层

| 位置 | 内容 |
| --- | --- |
| `core::ui::widget` | `Widget` trait 与 `Ui::add`，通用词汇，不认页面 |
| `app::shared::kit::state` | `State` 位集 |
| `app::shared::kit::theme` | `Theme::PANEL`，按钮/输入框的样式解析 |
| `app::shared::kit::slot` | `Slot` 枚举，替代 `Widget?` |
| `app::shared::kit::widgets` | 具体组件，一文件一个 |
| `app::shared::kit::i18n` | `Lang` / `Msg`，面板文案 |
| `services/<page>` | 组合组件，绑定意图 |

组件只收 props，不读 `DeviceState`，不拿回调，不内置用户可见文案。页面把快照里的状态翻译成 `State` 和 `&[u8]` 再传进来。

## 状态

`State` 是一位集，取值来自页面每帧派生的快照：

| 位 | 含义 | 来源 |
| --- | --- | --- |
| `PRESSED` | 手指正按着 | 手势 |
| `SELECTED` | 选中 / 开关打开 | 页面状态 |
| `DISABLED` | 不可用 | 页面状态 |
| `ERROR` | 校验失败 | 页面状态 |
| `DRAGGED` | 滑杆拖动中 | 手势 |
| `FOCUSED` | 键盘焦点 | 按钮 / 键盘导航 |
| `SCROLLED_UNDER` | 被滚动内容压住 | 滚动偏移 |

对应 Flutter 的 `WidgetState`，去掉 `hovered`（没有鼠标）。组件不存状态，状态每帧重新传；`SELECTED` 之类的持续态放在页面或快照里。

状态层用半透明覆盖：`Color::rgba(..., 0x40)` 交给 `Canvas` 做 source-over，不引入主题引擎。

不做双向绑定（Slint/QML 的 `<=>`）。Flutter 的 `TextEditingController` 也是 controller 加重建，不是绑定；这里快照是 `Copy`、`view` 是纯函数，加监听器会把“重绘由快照 diff 决定”变成“谁 notify 谁重绘”。需要编辑态的地方用 controller：`kit::scroll::ScrollState`、`kit::keyboard::Text`。

## 主题与调色板

`Theme` 存 `Color`（ARGB32），不存 `ColorId`。`ColorId` 是“这一次声明”的调色板索引，由 `Ui::color` 现场 intern；跨帧保存会指向另一帧的槽。

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

同一颜色重复 intern 是幂等的；`MAX_COLORS = 16` 是整页共享的上限。新增组件先问“能不能用现有 token”，不能才加色。

## 圆角

`Style::radius(u8)` 给节点自己的底与描边切角，`0` 是方角；半径按短边折半、并以 `MAX_RADIUS = 16` 封顶，所以 24×24 的 `Avatar` 直接写 `12` 就是个圆。绘制走整数判据 `(2r-2c-1)² + (2r-2k-1)² <= 4r²`，按行填格，不建表、不画弧、不分配。

只切节点自己的纸：背景、描边；子节点与文字不被裁。圆角半径不跨节点继承，容器要圆就把半径写在容器上。

| 组件 | 半径 |
| --- | --- |
| `Button` / `Chip` / `TextField` / `Select` | `theme::CORNER = 4` |
| `Badge` | 3 |
| `Avatar` | 12（24 px 方框，即半边长） |
| `Indicator` 的标记 | 点 4（`DOT = 8`）/ 数字 7（`PILL = 14`） |

## API

```rust
pub trait Widget {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId;
}

impl Ui<'_, '_> {
    pub fn add<W: Widget>(&mut self, widget: W) -> NodeId { widget.build(self) }
}
```

约定：

- 组件是值类型，props 用 `Copy` 字段；文本用 `&[u8]`，不持有 `String`。
- builder 方法 `const fn`，`#[must_use]`。
- 容器用闭包 `ui.column(style, |ui| ...)`；叶子实现 `Widget`。
- `leading` / `trailing` 用 `Slot`，不用 `Option<Box<dyn Widget>>`。
- 返回 `NodeId`，不接 `onTap`；页面用 `Style::tag` 或 `shape` 把命中翻成意图。

Flutter 到这里的映射：

| Flutter | 这里 | 原因 |
| --- | --- | --- |
| `Widget` / `BuildContext` | `Widget` / `&mut Ui` | 同构 |
| `StatefulWidget` | 状态留在页面 / 快照 | 每帧重建 |
| `WidgetState` | `State(u8)` | 去掉 hover |
| `WidgetStateProperty<T>` | `fn(&Theme, State) -> T` | 不做属性对象 |
| `ButtonStyle` | `Theme` 的 `button(...)` | 无运行时主题 |
| `ThemeData` | `const Theme` | 同上 |
| `child` / `children` | 闭包 | 没有 `List<Widget>` |
| `leading: Widget?` | `Slot` | 避免装箱 |
| `onTap` | `NodeId` + 页面 `bind` | 输入独立管线 |
| `ListView.builder` | Cell + `list_window` | 节点预算 |
| `showDialog` | `stack` 覆盖层 | 声明式 |
| `InheritedWidget` | 显式 `&Theme` / 快照 | 无隐式树 |
| `MediaQuery` / `Semantics` | 不做 | 固定面板、无读屏 |

## 基础组件

已落地，全部在 `shared/kit/widgets`：

| 组件 | 形态 | 节点 | 说明 |
| --- | --- | --- | --- |
| `Divider` | W | 1 | 一条 1 px 线 |
| `Badge` | W | 1 | 小字块；`Filled` / `Light` / `Outline` 三变体，`Tone` 另管颜色 |
| `Chip` | W | 2 | 圆角标签，可带选中态 |
| `Button` | W | 2 | filled / outlined / text |
| `IconButton` | W | 2 | 暂用 12 px 字模里的符号，无图标集 |
| `Indicator` | W | 2 | 叠在子元素右上角的角标：默认圆点，`label` 时带数字 |
| `ProgressBar` | W | 3 | 轨道 + 进度 |
| `Spinner` | W | 1 | `\|` `/` `-` `\` 四帧，由页面给相位 |
| `Switch` | W | 2 | 轨道 + 滑块 |
| `Checkbox` | W | 2~3 | 方框 + 勾 |
| `Radio` | W | 2~3 | 圆圈 + 点 |
| `Slider` | W | 3 | 轨道 + 滑块，值由页面给 |
| `TextField` | W | 2 | 值 + 光标；聚焦后整页交给 `kit::keyboard` |
| `Select` | W | 2 | 值 + 箭头 |
| `Tooltip` | W | 2 | 锚点 + 浮层 |
| `Avatar` | W | 1 | 首字母或短标记，圆形底 |
| `ListTile` | C | 0 | 列表行，四段文字 + 选中底 |
| `Scrollbar` | C | 0 | 已有 `kit::scroll` |
| `Image` | C | 0 | 相机帧、RGB565 资源，走 custom 槽 |

`Text` / `RichText` 不需要包装：`ui.text` 与 `ui.span` 就是词汇本身。

字号三档：页标题 20 px 矢量（`chrome::title`）、按钮与 chip 标签 14 px 矢量、正文与读数 12 px 点阵。点阵表只有 12 px 一档，`Metrics::at(13..=24)` 整档跳到 24 px，`ATTITUDE` 这类 8 字标签会超出 98 px 按钮，所以大字号只能走矢量。绘制按整段回退：段里缺一个字，整段退回点阵。`every_word_the_panel_prints_has_a_vector_glyph` 把 `Msg::ALL` 的每个词在 14 / 20 px 下钉死；新文案先重跑 `iot-tool outline`。

文本能撑大布局，但只撑得开没写死尺寸的节点：不写 `width` 时文本节点取整段前进宽度、高度取 `line_height`（字高 + 行距），于是列跟着变高、行跟着变宽；写了 `width` 的 `nowrap` 段按给定宽度裁切，`Button` 的 98×44 不会因为标签变大而长个，换字号就得同时改布局常量。面板固定 320×240，撑出来的空间先从 `grow` 节点里扣，扣完就溢出面板，不会整体缩放。

`ListTile` 的字段照 Flutter：`title`、`subtitle`、`leading`、`trailing`、`state`、`dense`。`leading` / `trailing` 是 `Slot`：

```rust
pub enum Slot<'a> {
    None,
    Text(&'a [u8]),
    Badge(&'a [u8]),
    Mark(&'a [u8]),
    Glyph(char),
}
```

## 页面

| 页 | 作用 |
| --- | --- |
| `Menu` | 3 列按钮网格，列出全部页面；板子撑不起的页灰掉、点击无效；有屏时是默认启动页 |
| `Widgets` | 基础组件画廊：五个分区（操作 / 选择 / 输入 / 显示 / 行）各一行标题，整页滚动；能切换的控件可点 |

`Menu` 的按钮区放进 `chrome::scroller`：按钮 98×44、间距 6，三列正好占满内容宽。每个按钮带 `PageId::code() + 1` 的 tag，构造期用 `TagTable::measure` 量一次；点击按滚动 offset 反查 tag，再上报 `MenuIntent::Choose { page }`，由 `Services::apply` 转成导航。命中不再重算网格常量。一次点击在线上是 `Press` 后跟 `Tap`：按钮区只在 `Drag` 才认领拖动，`Press` 只停住滑行，否则点击会被当成拖动、导航丢掉。板子不支持的页仍列出，只是 `State::DISABLED`，点击不导航。无屏的板子启动仍落在 `Input`；`DeviceState::landed_on` 用 `surface_roles` 里的 `Screen` 判。标题行高 `TOUCH_MIN`、下面留一个 `GAP` 的空白，右侧是 `EN` / `中文` 两个 44×44 chip（`the_language_chips_are_touch_sized` 钉住），点一下改 `DeviceState.lang`；按钮区因此下移一整行，仍不需要滚动。

`Widgets` 页的交互值存在页面自己（`WidgetsSnapshot`）：按钮与 chip 锁定选中、switch / checkbox / radio 切换、slider 点击与拖动、select 循环、icon button 开关 spinner、list tile 选中；`Badge` 显示 switch 状态，`ProgressBar` 跟随 slider，`Divider` 分隔五个分区。分区标题写的是动作而不是名词（`TAP TO LATCH`、`PICK OR TOGGLE`、`DRAG AND TYPE`、`LIVE STATE`、`ROW`），`LIVE STATE` 一行按顺序是 spinner、带 `Indicator` 数字角标的头像、icon button 和三个 `Badge` 变体，下面一行 `SPINNER AVATAR ICON BADGE` 点出这四个名字——光靠图形认不出哪个是哪个。徽章的字随 switch 在 `ON` / `OFF` 之间换，`Filled` / `Light` / `Outline` 是同一 `Tone` 的三种纸。输入框印 `WidgetsSnapshot::text`，只有空着时才显示 `VALUE` 当占位；列表行的副标题是 `SUBTITLE`，讲的是行的结构，不是动作。控件根节点带 `WidgetsAction::tag()`，同一个枚举给出 `ALL` / `from_tag`，`match` 穷尽所以漏一个 handler 编译不过；`every_action_is_drawn_once` 保证每个控件恰好画一次。构造期用 `TagTable::measure` 量一次，点击按滚动 offset 反查；布局跨语言与取值不变，由 `the_tags_stay_put_across_languages_and_values` 钉住。整页一次声明仍受 `the_whole_gallery_fits_one_declaration` 的 60 节点 / 2048 字节预算约束。

`TextField` 聚焦后页面换成 4×4 键盘（`kit::keyboard`，Canvas，0 节点）：`Text` 持有 24 字节、多击循环与 shift/layer 状态，`OK` 键收起键盘。键位、绘制与命中都读同一张 `CELLS` 表。

## 高级组件

组合件、导航、反馈、数据展示。按产品需要实现，尚未落地：

| 组件 | 形态 | 参照 |
| --- | --- | --- |
| `ListView`（窗口化） | C | `ListView.builder` |
| `GridView` / `Table` | C | `GridView` / `DataTable` |
| `Accordion` | W | `ExpansionTile` |
| `Tabs` / `TabView` | W | `TabBar` / `TabBarView` |
| `SegmentedButton` | W | `SegmentedButton` |
| `Stepper` | W | `Stepper` |
| `PageIndicator` | C | 轮播点 |
| `SwipeAction` | C | `Dismissible` |
| `AppBar` | W | `AppBar` |
| `NavigationBar` | W | `NavigationBar` |
| `Drawer` / `BottomSheet` | W | `Drawer` / `showModalBottomSheet` |
| `Menu` | W | `PopupMenuButton` |
| `Dialog` | W | `AlertDialog` |
| `Toast` | W | `SnackBar` |
| `Banner` | W | `MaterialBanner` |
| `EmptyState` / `ErrorState` | W | 无官方；Ant `Empty` / `Result` |
| `LoadingOverlay` | W | 无官方；MUI `Backdrop` |
| `Skeleton` | W/C | 社区 `Skeleton` |
| `Form` / `FormField` | W | `Form` / `FormField` |
| `NumberField` | W | `TextField` + formatter |
| `Keypad` | C | 无官方 |
| `SearchField` | W | `SearchBar` |
| `FilterChips` | W | `FilterChip` |
| `Chart` / `Gauge` / `Scale` | C | 无官方；`fl_chart` 只作参照 |

## 可选组件

| 组件 | 判断 |
| --- | --- |
| `Calendar` / `TimePicker` | 有日期、定时设置再做 |
| `ColorPicker` / `Rating` | 灯效、评分场景 |
| `TreeView` / `Timeline` / `Heatmap` | 传感器历史 |
| `QRCode` / `Barcode` | 配网分享、设备标签 |
| `PageView` / `Carousel` | 多页引导 |
| `ReorderableList` / `PullToRefresh` | 排序与刷新 |
| `RichTextEditor` | 选区与撤销成本高 |
| 隐式动画 / `Hero` / `Focus` | 页面 `tick` 已能驱动，按需 |
| `SplitView` / `Masonry` | 桌面或多列，不适合本面板 |

明确不做：`Map`、`Video`、`Lottie`/`Rive`、`Hover`、`Mouse`、`Semantics`、`RTL`、`MediaQuery`。不是能力不够，是没有输入设备、读屏或多尺寸。

## 多语言

两层：文档本身中英双语；面板文案由 `shared::kit::i18n` 提供。

词汇是枚举 + 穷尽匹配，漏翻即编译错：

```rust
pub enum Lang { ZhHans, En }

pub enum Msg { CameraTitle, NoCam, /* ... */ }

impl Msg {
    pub const fn bytes(self, lang: Lang) -> &'static [u8] { /* ... */ }
}
```

| 层 | 入口 | 说明 |
| --- | --- | --- |
| 构建期 | `IOT_LANG=en\|zh` | 与 `IOT_BOOT_PAGE` 同一套 `option_env!` + 解析 |
| 运行期 | `SettingsIntent::SetLang` | `Menu` 的 `EN` / `中文` chip；`services::settings` 写 `DeviceState.lang` |
| 快照 | `FrameSnapshot.lang` | 页面只读快照 |
| 组件 | 只收 `&[u8]` | 翻译在页面完成 |

`lang` 变化在 `render::screen::repaint_due` 里单独触发重绘，所有说话页一起换字；不逐页补条件，新页不会漏。

默认 `en`，与现有面板文案一致；`IOT_LANG=zh` 构建中文面板。

版面约束：

- 读数标签槽 `chrome::LABEL = 24 px`，等于 4 个 ASCII 格或 2 个汉字；超长截断，所以标签用短键。
- 长文案走 `chrome::title`（整行）或 `EmptyState` 的两行。
- `text::fit` 的 `~` 是兜底，不是设计；每条读数标签都要有宽度测试。
- 点阵 `ui12.bin` 覆盖 GB2312 + Big5；矢量表只覆盖 `vector-sample.txt`，缺字时整行退回点阵。新文案要重跑 `iot-tool outline`。
- SSID、MAC、IP、日志是数据，不翻。

不做：RTL、ICU 复数/性别/插值、locale 数字格式。

## Cell 绘制

`ListTile` 是最小样板：

```rust
pub fn draw(&self, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
    // 选中底 + 四段文字，0 个节点
}
```

列表只排可见行：

```rust
for (band, index) in list_window(top, rows, total).enumerate() {
    let row = Rect::new(area.x, area.y + (band * ROW_H) as i32, area.w, ROW_H as i32);
    tile(index).draw(canvas, row, theme);
}
```

`Chart`、`Gauge`、`Keypad`、`Scrollbar` 同样走 `Ui::custom(slot)`，一个带一个槽；`PAINTS = 8` 是整页上限。绘制与命中共用同一份几何：页面的 `shape` 模块仍是唯一来源。

## 命中与事件

- Widget：`Ui::headless` 跑同一份声明，`Ui::tags()` 一次量完，存进 `kit::tag::TagTable`；可点的组件带 `Style::tag`，`TagTable::at(x, y, offset)` 得意图编号。
- Cell：页面 `shape` 常量算矩形，和绘制共用。
- `NodeId` 只在一次声明内有效，不跨帧保存；要稳定标识用 `tag`。
- Dialog / Toast 声明在 `stack` 覆盖层，可见性由页面决定。
- 组件不认 `InputEvent`；`bind` 把 `InputIntent` 翻成 `BusinessIntent`。
- 按住状态是页面自己的 latch（滑块用 `values.pressed`），每次手势收尾（`Release`、抬手、按住没变成滑动）都要清零：latch 留着时 2 px 的位移就会被判成拖拽，下一次点按永远变不成 `Tap`。

## 预算与测试

| 项 | 手段 |
| --- | --- |
| 节点 | `ui.node_count()` |
| 文本 | `ui.text_bytes()` |
| 颜色 | 整页 `MAX_COLORS` |
| 黄金图 | 每组件一条指纹，页面表另算 |
| 命中 | 绘制矩形与 headless 矩形一致 |

改完跑：

```sh
cargo test -p iot-app --lib --no-default-features --features host
moon run iot:check-s3
moon run iot:check-c6
moon run iot:check-host
```

## 新增组件

1. 定形态：行数随数据增长就 Cell，否则 Widget。
2. props 全 `Copy`，文本 `&[u8]`，可选槽用 `Slot`。
3. 只用 `Theme` 现有 token；要新色先改 `Theme`。
4. 实现 `Widget` 或 `Cell::draw`，单实例 ≤5 节点。
5. 写黄金图 + 命中测试；更新本文清单。
6. 不新增 Cargo feature；未引用的模块由链接器丢弃。

## 参考

Flutter 的 [Widget 目录](https://api.flutter.dev/flutter/widgets/widgets-library.html)、[`ListTile`](https://api.flutter.dev/flutter/material/ListTile-class.html)、[`WidgetState`](https://api.flutter.dev/flutter/widgets/WidgetState.html)、[`ButtonStyle`](https://api.flutter.dev/flutter/material/ButtonStyle-class.html)；MUI 的组件分类；shadcn/ui 的组件表与 `data-state`；LVGL 的 parts/states；Rust 生态的 embedded-gui 与 Guillotine 只作 API 对照，不引入依赖。

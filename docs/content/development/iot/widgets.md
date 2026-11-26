+++
title = "UI 组件"
weight = 80
+++

# UI 组件

面板的组件词汇：两种形态、状态与主题、圆角、基础组件、Cell 绘制、多语言与预算。

> 状态：基础组件与 37 个高级组件已落地，由 `Widgets` 基础页与 `Advanced` / `Feedback` / `Extended` / `Data` / `Planner` / `Records` 六个画廊演示；`Feed` 一个组件同时提供下拉刷新与上拉加载。可选组件只剩 `QRCode` / `Barcode` 与几类刻意不做的。

## 两种形态

Flutter 的组件树每行一个 Widget，这里不行：`Ui` 在栈上，一帧只有 64 个节点，`ListView` 十行就吃掉一半。组件因此分两种：

| 形态 | 是什么 | 适合 | 节点成本 |
| --- | --- | --- | --- |
| **Widget** | `Ui` 声明节点，布局与绘制走同一棵树 | 页头、表单、对话框、少量行 | 1~5 |
| **Cell** | `Canvas` 直接画，占一个 custom 槽 | 长列表、键格、图表、仪表盘、滚动条 | 0 |

判据是“行数是否随数据增长”。固定几行用 Widget；一到 N 行用 Cell。Cell 的 API 仍照 Flutter 的槽位设计（`ListTile` 的 `leading`/`title`/`subtitle`/`trailing`），只是落墨在 `Canvas`。

## 硬约束

| 常量 | 值 | 出处 |
| --- | --- | --- |
| 面板 | 320×240，内容底 228 | `ui::panel` |
| 节点 | `MAX_NODES = 64` | `ui::layout` |
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
| `app::shared::i18n` | `Lang` / `Msg` 与固定词；只出字，不认像素 |
| `app::shared::ui::state` | `State` 位集 |
| `app::shared::ui::theme` | `Theme::PANEL` / `Theme::LIGHT` 与按钮/输入框的样式解析 |
| `app::shared::ui::slot` | `Slot` 枚举，替代 `Widget?` |
| `app::shared::ui::widgets` | 具体组件，一文件一个 |
| `app::shared::page` | 页面版式：边距、行距、标题、滚动体与读数行 |
| `app::shared::surface` | `Panel`：把声明布局成盒子并落墨 |
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

不做双向绑定（Slint/QML 的 `<=>`）。Flutter 的 `TextEditingController` 也是 controller 加重建，不是绑定；这里快照是 `Copy`、`view` 是纯函数，加监听器会把“重绘由快照 diff 决定”变成“谁 notify 谁重绘”。需要编辑态的地方用 controller：`ui::scroll::ScrollState`、`ui::keyboard::Text`。

## 主题与调色板

`Theme` 存 `Color`（ARGB32），不存 `ColorId`。`ColorId` 是“这一次声明”的调色板索引，由 `Ui::color` 现场 intern；跨帧保存会指向另一帧的槽。

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
    pub const PANEL: Self = Self { /* 深色，开机默认 */ };
    pub const LIGHT: Self = Self { /* Solarized Light */ };
    pub const fn of(id: ThemeId) -> &'static Self { /* ... */ }
}
```

`ThemeId { Dark, Light }` 放在 `iot-core::ui::Screen`，由 `DeviceState.theme` 经 `FrameSnapshot.theme` 投影到绘制层；组件默认 `Theme::of(ui.screen().theme)`，页面用 `Theme::of(snapshot.theme)`。`.theme(&Theme)` 仍可给单个组件覆盖。切换只换常量调色板，不生成颜色：浅色页由 `ScreenRenderer::present` 用 `ground` 铺纸，屏幕背光只取 `Backlight` 的设置（0 即全黑），`Network` / `Camera` 仍用各自的黑底，`Lighting` 与玻璃呼吸期间强制深色。主题和语言一样是运行时状态，不落盘；构建期可用 `IOT_THEME=light` 改开机值。

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
| `ButtonStyle` | `Theme` 的 `button(...)` | 常量调色板，无样式对象 |
| `ThemeData` | `const Theme` + `ThemeId` | 切换换常量，不建主题树 |
| `child` / `children` | 闭包 | 没有 `List<Widget>` |
| `leading: Widget?` | `Slot` | 避免装箱 |
| `onTap` | `NodeId` + 页面 `bind` | 输入独立管线 |
| `ListView.builder` | Cell + `list_window` | 节点预算 |
| `showDialog` | `stack` 覆盖层 | 声明式 |
| `InheritedWidget` | 显式 `&Theme` / 快照 | 无隐式树 |
| `MediaQuery` / `Semantics` | 不做 | 固定面板、无读屏 |

## 基础组件

已落地，全部在 `shared/ui/widgets`：

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
| `TextField` | W | 2 | 值 + 光标；聚焦后整页交给 `ui::keyboard` |
| `Select` | W | 2 | 值 + 箭头 |
| `Tooltip` | W | 2 | 锚点 + 浮层 |
| `Avatar` | W | 1 | 首字母或短标记，圆形底 |
| `ListTile` | C | 0 | 列表行，四段文字 + 选中底 |
| `Scrollbar` | C | 0 | 已有 `ui::scroll` |
| `Image` | C | 0 | 相机帧、RGB565 资源，走 custom 槽 |

`Text` / `RichText` 不需要包装：`ui.text` 与 `ui.span` 就是词汇本身。

字号只有点阵的整倍档：页标题、按钮标签与 `Text` 页正文 24 px（`page::title`），其余读数 12 px。`Metrics::at(13..=24)` 整档跳到 24 px，没有 14 / 20 px；24 px 是两倍字面，`ATTITUDE` 这类 8 字标签占 96 px，98 px 的按钮内宽装得下。缺字画 □，没有整段回退；`every_word_the_panel_prints_has_a_bitmap_glyph` 把 `Msg::ALL` × `Lang::ALL` 的每个字钉在 `Face::has` 上，新文案缺字先重跑 `iot-tool font --merge <bdf> --chars <list>`。

文本能撑大布局，但只撑得开没写死尺寸的节点：不写 `width` 时文本节点取整段前进宽度、高度取 `line_height`（字高 + 行距），于是列跟着变高、行跟着变宽；写了 `width` 的 `nowrap` 段按给定宽度裁切，`Button` 的 98×44 不会因为标签变大而长个，换字号就得同时改布局常量。面板固定 320×240，撑出来的空间先从 `grow` 节点里扣，扣完就溢出面板，不会整体缩放。

`Style::overflow(Overflow)` 只管 `nowrap` 段超出盒子时的处理，对应 Flutter `TextOverflow` 里这一档能用的部分：`Clip`（默认）在盒边切断，`Ellipsis` 切断并以 `…` 收尾；盒子本身不因此变大。`.ellipsis()` 是 `nowrap + Ellipsis` 的简写，`Banner` / `Button` / `EmptyState` / `FilterChips` / `Segmented` / `Stepper` 用它保证一行放不下的标签不出格。多行段不参与，仍按 `max_lines` 截。

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
| `Menu` | 按设备 / 界面 / 灯光分组的 3 列按钮网格，列出全部页面；板子撑不起的页灰掉、点击无效；有屏时是默认启动页 |
| `Widgets` | 基础组件画廊：五个分区（操作 / 选择 / 输入 / 显示 / 行）各一行标题，整页滚动；能切换的控件可点 |
| `Advanced` | 导航 / 组合画廊：`AppBar` / `NavigationBar` / `Tabs` / `PageIndicator` / `Pager` / `Carousel`、`Accordion` / `Segmented` / `Stepper`，整页滚动 |
| `Planner` | 规划画廊：`MonthGrid` / `Wheel` / `Rating` / `ColorPicker` 与 `DatePicker` / `TimePicker` 两个弹窗，整页滚动；滚轮可拖可点，日期/时间按钮拉起弹窗 |
| `Records` | 记录画廊：`TreeView` / `Timeline` / `Heatmap` / `ReorderableList`，整页滚动；拖右侧手柄换行 |
| `Feedback` | 反馈画廊：`Banner` / `Toast` / `Skeleton` / `LoadingOverlay`，从横幅拉起 toast，占位条换成内容 |
| `Extended` | 输入画廊：`SearchField` / `FilterChips` / `FormField` / `NumberField` / `Keypad`；搜索框聚焦后整页换成键盘 |
| `Data` | 数据 / 浮层画廊：`Table` / `Chart` / `Gauge` / `SwipeAction` / `EmptyState`、`Dialog` / `PopupMenu` / `Sheet`；点行、点柱、点表盘共用页面状态 |
| `List` | `Feed` 演示：下拉刷新、上拉加载、左滑删除、空态 |
| `Lighting` | 灯效页：范围三选一（关闭 / 本页 / 所有页）、样式三行四格（常亮 / 呼吸 / 变色 / 极光；烛光 / 脉冲 / 心跳 / 闪电；星光 / 频闪 / 警灯 / 求救）、周期 / 色相 / 最亮三个滑块，另有一行独立背光（0 即全黑）；页底说明随样式换，常亮时周期禁用。整页强制深色 |
| `Headless` | 无屏板（C6）的落地页：没有画布，只有灯珠与按钮；单击换灯效（12 档后接关）、长按调亮度（30/50/70/90/100% 后接关，暗灯长按回到首档）、双击换颜色、三击加亮度（多击默认关闭，运行时开启才生效），开机时按住越过 splash 进配网；不进菜单，也不占 `PageId::ALL` |

六个画廊的每个样例都带一行说明（`page::caption`）：名字加这个样例怎么动，例如 `TABLE / TAP A ROW`，中英各一份 `Msg`。`Advanced` / `Extended` 原先的分区词因此删掉——说明行已经报了名字；`Data` 的浮层一带仍用 `page::section`（一条线加名字）隔开。说明行是 muted 的 `nowrap`，不带 tag，点了不动。

`Menu` 的按钮区放进 `page::scroller`：按钮 98×44、间距 6，三列正好占满内容宽。每个按钮带 `PageId::code() + 1` 的 tag，构造期用 `TagTable::measure` 量一次；点击按滚动 offset 反查 tag，再上报 `MenuIntent::Choose { page }`，由 `Services::apply` 转成导航。命中不再重算网格常量。一次点击在线上是 `Press` 后跟 `Tap`：按钮区只在 `Drag` 才认领拖动，`Press` 只停住滑行，否则点击会被当成拖动、导航丢掉。板子不支持的页仍列出，只是 `State::DISABLED`，点击不导航。无屏的板子启动落在 `Headless`；有屏时请求 `Headless` 退回 `Input`，`DeviceState::landed_on` 用 `surface_roles` 里的 `Screen` 判。标题行高 `TOUCH_MIN`、下面留一个 `GAP` 的空白，右侧是 `EN` / `中文` / `DARK` / `LIGHT` 四个 44×44 chip（`the_settings_chips_are_touch_sized` 钉住），前两个改 `DeviceState.lang`，后两个改 `DeviceState.theme`；按钮区因此下移一整行。17 个入口按 设备 / 界面 / 灯光 分三组，每组一条分隔线加 muted 标题；组内仍是三列，合计七行，最后一组要滚动才见得到，命中仍按 offset 反查。

`Widgets` 页的交互值存在页面自己（`WidgetsSnapshot`）：按钮与 chip 锁定选中、switch / checkbox / radio 切换、slider 点击与拖动、select 循环、icon button 开关 spinner、list tile 选中；`Badge` 显示 switch 状态，`ProgressBar` 跟随 slider，`Divider` 分隔五个分区。分区标题写的是动作而不是名词（`TAP TO LATCH`、`PICK OR TOGGLE`、`DRAG AND TYPE`、`LIVE STATE`、`ROW`），`LIVE STATE` 一行按顺序是 spinner、带 `Indicator` 数字角标的头像、icon button 和三个 `Badge` 变体，下面一行 `SPINNER AVATAR ICON BADGE` 点出这四个名字——光靠图形认不出哪个是哪个。徽章的字随 switch 在 `ON` / `OFF` 之间换，`Filled` / `Light` / `Outline` 是同一 `Tone` 的三种纸。输入框印 `WidgetsSnapshot::text`，只有空着时才显示 `VALUE` 当占位；列表行的副标题是 `SUBTITLE`，讲的是行的结构，不是动作。控件根节点带 `WidgetsAction::tag()`，同一个枚举给出 `ALL` / `from_tag`，`match` 穷尽所以漏一个 handler 编译不过；`every_action_is_drawn_once` 保证每个控件恰好画一次。构造期用 `TagTable::measure` 量一次，点击按滚动 offset 反查；布局跨语言与取值不变，由 `the_tags_stay_put_across_languages_and_values` 钉住。整页一次声明仍受 `the_whole_gallery_fits_one_declaration` 的 64 节点 / 2048 字节预算约束。

`TextField` 聚焦后页面换成 4×4 键盘（`ui::keyboard`，Canvas，0 节点）：`Text` 持有 24 字节、多击循环与 shift/layer 状态，`OK` 键收起键盘。键位、绘制与命中都读同一张 `CELLS` 表。四层轮转（字母 / 数字 / 符号 / 更多）合起来能打出 `0x20..=0x7E` 全部可打印 ASCII：字母层带大小写与空格，符号层放 `. , : ; ? ! - _ @ #`，更多层补齐剩下 22 个标点；层键面按下一层提示（`12` / `#` / `&` / `ab`）。

## 高级组件

导航、组合、规划、记录、反馈、输入、数据、浮层八类，都在 `shared/ui/widgets`，一文件一个。演示分六页：`Advanced` 放导航与组合，`Planner` 放规划，`Records` 放记录，`Feedback` 放反馈，`Extended` 放输入，`Data` 放数据与浮层。

### 导航

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `AppBar` | W | 标题 + 右端动作，动作带 tag |
| `NavigationBar` | W | 三格底部导航：`surface` 条上内嵌选中胶囊 |
| `Tabs` | W | 三页签，选中格整格换成 accent 纸 |
| `PageIndicator` | C | 轮播点，`dot_at` 在点中心给出 `TOUCH_MIN` 宽的命中区，`fit` 收起放不下的点 |
| `Pager` | W | 横向卡片条：`card_width` / `pitch` 从 `rect` 推，`turn` 把拖动距离换成索引，`page_at` 给点按 |
| `Carousel` | C | `Pager` 的布局 + 底部驻留条：`progress` 说下一张还剩多少，`running` 由页面的开关给；本页默认不自动播放 |

### 组合

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `Accordion` | W | 标题 + 展开体（同一张纸内），`open` 由页面给 |
| `Segmented` | W | 三段互斥选择 |
| `Stepper` | W | 三步行进器，`current` 高亮 |

### 规划

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `Wheel` | C | 通用单列滚轮：`Labels::Fixed` 任意文案或 `Labels::Numbers { min, max, step, pad }`；拖动跟手、松手动量（投影 `v²/2a`）、吸附、端点回弹、可选循环；选中带（上下两条线）固定在中间行，不随滚动移动；可见行 3~7、行高 28~44，全从 `rect` 推 |
| `MonthGrid`（`calendar`） | C | 7×6 月格，格宽/格高从 `rect` 推、上限 48；`fits` 判断父层是否够大，`day_at` 与绘制共用 `Metrics`；上一月 / 下一月是父层自己的 tag |
| `DatePicker` | C | 日期弹窗：body 够大时月格 + 上/下月，否则年/月/日三滚轮；年 1970–2100 不循环，月/日循环，月长变化把日夹回当月 |
| `TimePicker` | C | 2~3 列时/分/秒滚轮，列间画 `:`；`column_at` 与绘制共用列几何，秒列可选 |
| `Rating` | C | 五个 44×44 槽，整星输入 0–5；半星只做只读填充，星形用 12 行横向 span 表画 |
| `ColorPicker` | C | 132×132 饱和/明度方（12×12 块）+ 16 px 色相条 + 44 px 色块；点与拖共用 `point_at`，色相走 `iot_core::light::hsv_to_rgb` |

### 记录

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `TreeView` | C | 24 px 行，10 px 缩进，展开箭头；`row_at` 给行，展开态由页面给 |
| `Timeline` | C | 40 px 行，左轨 + 圆点，时间与标题两列 |
| `Heatmap` | C | 53×7 格（4 px 格 + 1 px 缝），档位由种子确定性推出，`cell_at` 与绘制同源 |
| `ReorderableList` | C | 32 px 行，右侧 44×44 手柄独占拖动；换位是 `remove(old)` → `insert(clamped)`，抬手落位 |

### 反馈

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `Banner` | W | 消息 + 动作 |
| `Toast` | W | 单行提示，宽度由页面给 |
| `Skeleton` | W | 灰条占位 |
| `LoadingOverlay` | W | 半透明遮罩 + 文案，`on` 控制显隐 |

### 输入

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `SearchField` | W | 放大镜槽 + 值/占位；点一下把整页交给 `ui::keyboard` |
| `FilterChips` | W | 三枚可多选 chip，选中项是位集 |
| `FormField` | W | 标签 + 值 + 提示；`State::ERROR` 画错误边 |
| `NumberField` | W | 数值 + 减/加两个 44 px 热区 |
| `Keypad` | C | 4×3 电话键盘，`key_at` 与 `draw` 共用 `KEYS` |

### 数据

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `Table` | C | 表头 + 四行固定格，列宽均分；行高 `LINE × 2 + 4 = 28` |
| `Chart` | C | 柱状条，值 0~100 |
| `Gauge` | C | 半圆表盘 + 指针 |
| `SwipeAction` | C | 行左滑露出动作带，`REVEAL_MAX` 封顶 |
| `EmptyState` | W | `Tone::Empty` / `Tone::Error` 两态的标题 + 提示 |

### 浮层

| 组件 | 形态 | 说明 |
| --- | --- | --- |
| `Dialog` | C | 不透明遮罩 + 居中卡片，`action_at` 分确定/取消 |
| `PopupMenu` | C | 不透明遮罩 + 挂在触发按钮下的三行菜单，`item_at` 分行，选中行高亮 |
| `Sheet` | C | 不透明遮罩 + 贴边抽屉（`Edge::Bottom` / `Edge::Left`），`row_at` 分行，演示用底部 |
| `Picker`（弹窗框） | C | scrim + 居中卡片 + 标题 + body + Cancel/OK；卡片 `min(rect − 2×8, 360×480)`，动作带 32~44，`action_at` 分确定 / 取消 / 点遮罩；scrim 只铺卡片四周的条带与四个角，卡片 body 重绘不产生混合 |

所有可点控件都报 tag：Widget 用 `Style::tag`，Canvas 槽（Dialog、Keypad、Table、Chart、Gauge、SwipeAction）在声明时给槽本身带 tag，构造期 `TagTable::measure` 量一次；画廊的 `bind` 只回 `Gallery { wire, intent }`，`wire` 是路由码，`shared::intent` 不因此引用 `shared::route`。Canvas 槽的命中用 `Ui::paint_rect` 量到的矩形，和绘制同一份几何；Table 行、Chart 柱、Keypad 格再按绘制时的同一套等分算子格。浮层项（菜单三行、抽屉两行）不是声明节点，`bind` 用 `item_at` / `row_at` 直接分。

`Data` 的 Dialog、PopupMenu、Sheet 都是声明在 `page::body` 之后的根节点：`absolute`、`320 × 228`，各占一个槽、常驻声明、按开合绘制，盖住标题与整页内容，所以既不随列表滚走，也不靠半透明遮罩透出底下的控件。菜单锚在触发按钮下边缘（快照里发布 `menu_anchor`，绘制时夹在内容区内），抽屉从内容区底边升起；浮层打开时 `bind` 先收 Tap，点项选中并关闭，点遮罩或外部关闭。

`Pager` 与 `PageIndicator` 在 `Advanced` 里共用同一个索引：卡片能拖、能点，圆点能点，三种输入都落到同一张卡片上。卡宽与步距从滚动条带宽度推导，不写死像素。

### 点击矩阵

| 页 | 可点 | 纯装饰 |
| --- | --- | --- |
| `Advanced` | `AppBar` 动作、三个导航格、三个页签、每个圆点、两张卡片条各自的每张卡片、`Auto` 开关、`Accordion` 头、三段选择、`Stepper` | 当前页签、导航胶囊与驻留条的高亮本身 |
| `Planner` | 上一月 / 下一月、每一天、小时滚轮（拖动或点行）、`DATE` / `TIME` 两个按钮、弹窗里的年/月/日滚轮、月格日与上/下月、Cancel/OK、五颗星、色相条与色方 | 月格里的空格、星期头、滚轮两侧的 `:` |
| `Records` | 六行树、四条时间线、每个热力格、每行的 44 px 手柄 | 时间线左轨、热力图月份标签 |
| `Feedback` | 横幅动作、toast、占位切换按钮、遮罩文案 | `Skeleton` 灰条、`Divider` |
| `Extended` | 搜索框、三枚 chip、表单、数值加减、12 个键格 | — |
| `Data` | 四行表格、六根柱、表盘、滑动行、空态、对话框两项、菜单/抽屉的触发按钮与打开后的项 | 图表底轴 |

判据是“有没有动作”：装饰组件不带 tag，点了不动；带 tag 的控件根节点本身就是热区，页面不需要再猜内部布局。按下与拖动由 `services/gallery.rs` 的 `Scroller` 仲裁：先记按下点与 tag，位移不超过 `DRAG_SLOP` 就是 Tap，超过就由纵向拖动认领滚动区，横向拖动在滑动控件上转成 `Slide`；首个认领者独占本次手势。Canvas 槽（表格、图表、表盘、键格）也必须带 tag——不带时按下点的 tag 为 0，滚动区直接把这次手势认领成拖动，抬手的 Tap 被当成 `Release` 吃掉，槽里的行、柱、盘、格就全点不动。拖动后的抬手在线上仍是一枚 `Tap`，所以页面的 Tap 分支要先问 `Scroller::dragging()`：`Advanced` 的卡片、`Extended` 的键格、`Data` 的行与柱都在拖动抬手时什么都不选，下一次拖动照常滚。

### 尺寸自适应

- 可点元素的下限是 `TOUCH_MIN = 44`：`Tabs` / `Segmented` / `Stepper` / `NavigationBar` 行高、`Accordion` 头、`Banner` 动作、`NumberField` 加减、`Keypad` 单格都按它取。
- `page::scroller` 的滚动体是 `stretch` 的：一行拿到内容宽，所以 `grow(1) + basis(0)` 的分格能真分到宽度；不拉伸时这类行只按子节点的自然宽度算，`basis(0)` 让整行塌成 0。
- Canvas 组件从 `rect` 与文字度量推尺寸：`Table` 表头 `LINE + 4`、行高 `LINE × 2 + 4 = 28`（`TABLE_H = 128`）；`Dialog` 动作带 `TOUCH_MIN`，卡片高由动作带、标题/正文行与 `MARGIN` 相加；`Chart` / `Gauge` / `Keypad` 随 `rect` 缩放；`SwipeAction` 的露出宽度仍以 `REVEAL_MAX` 封顶。
- 页面 custom 槽高度用 `page::TOUCH` 的整数倍，例如表盘/图表两行、键格四行，不写示例魔法数。
- 日期/时间组件不读面板常量，也不写死 306 / 42 / 32：`Wheel` 的可见行由 `rect.h / 28` 取 3~7 的奇数、行高 `rect.h / 可见行` 封顶 44；`MonthGrid` 的格宽 `(rect.w − 6×1) / 7`、格高 `(rect.h − 12) / 6`，都封顶 48；`Picker` 卡片 `min(rect − 2×8, 360×480)` 居中。320×228、480×320、800×600、240×180 四组尺寸各有一条测试。
- 父层按 `fits` 选形态：`MonthGrid::fits` 要求 28 px 格，320×228 的弹窗 body 只有 140 px，所以 `DatePicker::kind` 自动选三滚轮；480×320 以上才出月格。小屏不把月格压到 21 px。

### 重绘范围

滚轮页只重绘动的那几块像素：`Planner` 的弹窗滚轮报动的那一列，内联小时轮报「可见滚轮矩形 + 打印小时的读数行」（快照发布 `inline_box` / `hour_line`，已和滚动视口求交），经 `services::Descriptor::with_partial` 交给 painter；页面报不出就退回整帧。partial 帧先把 damage 铺回底色——页面文字靠反色地面绘制，同一块画两遍会来回翻转——所以 `Picker` 的 scrim 拆成条带，卡片 body 重绘不碰混合。实测与数字见 `records/iot/panel-buffering.md`。

## Feed

`shared/ui/feed.rs` 一个组件做三件事：列表、下拉刷新、上拉加载。两种边缘手势共用一个滚动区，拆成两个组件只会重复 offset 与拖动状态。

| 项 | 约定 |
| --- | --- |
| 行 | `ROW_H = TOUCH_MIN` 起，行由 `Source` 提供，绘制不声明节点 |
| 内容高 | `content(rows) = rows * ROW_H + FOOTER_H`，页脚始终在内容里，`max` 不跳 |
| 下拉 | `ScrollConfig.pull > 0` 才允许顶端过拉；松手时过 `PULL_MIN = 28 px` 就刷新 |
| 上拉 | 距底 `LOAD_AHEAD = 24 px` 内松手、且行数与上次请求不同，才请求下一页 |
| 页脚 | `More::{Idle, Running, End, Failed}` 四态，失败后允许再拉 |
| 回答 | 页面处理完数据后调 `loaded(rows, end)`；失败调 `failed()` |
| 删除 | 行左滑露出的动作带由页面自己管，`Feed` 只收 `reveal: Option<(row, px)>` |

`Feed` 不碰数据：`FeedView` 收 `Source`、`Words`、offset、pull 和两个边缘状态，`draw` 一次画完可见行与页脚。`asked_at` 记住上次请求时的行数，同一行数不会连发；`reset()` 清掉拖动、边缘和请求记录。

`List` 页是演示：34 行、每页 8 行、每次 600 ms 假工作；下拉把列表重置回第一页，上拉续页直到 `End`，左滑再点动作带删行，点行本身选中。没有行时居中显示 `EMPTY`，下拉指示器浮在顶端。

## 可选组件

| 组件 | 判断 |
| --- | --- |
| `QRCode` / `Barcode` | 配网分享、设备标签；这一轮不做 |
| `RichTextEditor` | 选区与撤销成本高 |
| 隐式动画 / `Hero` / `Focus` | 页面 `tick` 已能驱动，按需 |
| `SplitView` / `Masonry` | 桌面或多列，不适合本面板 |

已落地：`Wheel` / `MonthGrid` / `DatePicker` / `TimePicker` / `Rating` / `ColorPicker` 在 `Planner`，`TreeView` / `Timeline` / `Heatmap` / `ReorderableList` 在 `Records`，`Carousel` 在 `Advanced`。`PageView` 不单独做：`Pager` + `PageIndicator` 已经覆盖翻页与定位。

明确不做：`Map`、`Video`、`Lottie`/`Rive`、`Hover`、`Mouse`、`Semantics`、`RTL`、`MediaQuery`。不是能力不够，是没有输入设备、读屏或多尺寸。

## 多语言

两层：文档本身中英双语；面板文案由 `shared::i18n` 提供。

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

- 读数标签槽 `page::LABEL = 24 px`，等于 4 个 ASCII 格或 2 个汉字；超长截断，所以标签用短键。
- 长文案走 `page::title`（整行）或 `EmptyState` 的两行。
- `text::fit` 的 `~` 是兜底，不是设计；每条读数标签都要有宽度测试。
- 点阵 `ui12.bin` 覆盖 GB2312 + Big5；缺字画 □。新文案要重跑 `iot-tool font --merge <bdf> --chars <list>`，并让 `every_word_the_panel_prints_has_a_bitmap_glyph` 通过。
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

`Chart`、`Gauge`、`Keypad`、`Scrollbar` 与 `Feed` 同样走 `Ui::custom(slot)`，一个带一个槽；`PAINTS = 8` 是整页上限。`Feed` 只排可见行，行数不换成节点。绘制与命中共用同一份几何：页面的 `shape` 模块仍是唯一来源。

## 命中与事件

- Widget：`Ui::headless` 跑同一份声明，`Ui::tags()` 一次量完，存进 `ui::tag::TagTable`；可点的组件带 `Style::tag`，`TagTable::at(x, y, offset)` 得意图编号。
- 画廊：`GalleryIntent` 带上路由码而不是 `PageId`，`shared::intent` 保持不依赖 `shared::route`；`Services::apply` 用 `PageId::from_code` 把它换回页面。
- Cell：页面 `shape` 常量算矩形，和绘制共用。
- `NodeId` 只在一次声明内有效，不跨帧保存；要稳定标识用 `tag`。
- 浮层用 `absolute` 子节点声明；flex / wrap / scroll 的盒子都会摆它的出流子节点，根节点上只挂一个 `absolute` 也能量到尺寸。Toast 之类仍放在 `stack` 里，可见性由页面决定。
- 组件不认 `InputEvent`；`bind` 把 `InputIntent` 翻成 `BusinessIntent`。
- 按住状态是页面自己的 latch（滑块用 `values.pressed`），每次手势收尾（`Release`、抬手、按住没变成滑动）都要清零：latch 留着时 2 px 的位移就会被判成拖拽，下一次点按永远变不成 `Tap`。

## 预算与测试

| 项 | 手段 |
| --- | --- |
| 节点 | `ui.node_count()` |
| 文本 | `ui.text_bytes()` |
| 颜色 | 整页 `MAX_COLORS` |
| 黄金图 | 页面表一条一页一态；`Advanced` / `Planner` / `Records` / `Feedback` / `Extended` / `Data` / `List` 各两条 |
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

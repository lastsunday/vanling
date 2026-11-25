+++
title = "高级组件与 Feed"
description = "25 个高级组件、一条 Feed 与四个画廊已落地：host 557 条测试、32 行黄金图，S3 主栈 82,264 B，镜像 2,064,128 / 1,861,584 B 并已烧进两块板。"
weight = 70
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 高级组件与 Feed

> 2026-11-25：矢量字模已整体移除，字号只剩 12 / 24 px 点阵。本文中的 `vector.bin`、`vector-sample.txt` 与 `iot-tool outline` 是当时状态，现由 `ui12.bin` 整倍放大取代。

`docs/content/development/iot/widgets.md` 的清单里，原先只是计划的高级组件全部实现，并加了 `Feed`：一个组件同时做列表、下拉刷新和上拉加载。演示分三页，菜单从 9 个入口变成 12 个，仍是四行、约 24 px 的滚动余量。

## 落地范围

| 页 | 内容 |
| --- | --- |
| `Advanced` | `AppBar` / `NavigationBar` / `Tabs` / `PageIndicator`、`Accordion` / `Segmented` / `Stepper`、`Banner` / `Toast` / `Skeleton` / `LoadingOverlay`，11 个控件 |
| `Extended` | `SearchField` / `FilterChips` / `FormField` / `NumberField` / `Keypad`、`Table` / `Chart` / `Gauge` / `SwipeAction` / `EmptyState`、`Dialog` / `PopupMenu` / `Sheet`，13 个控件 |
| `List` | `Feed`：34 行、每页 8 行、600 ms 假工作；下拉重置、上拉续页、左滑删除、空态 |

新文件都在 `shared/ui/widgets`，`Feed` 在 `shared/ui/feed.rs`。`ListView` 不再单列，长列表由 `Feed` 承担。

## Feed 的两个边缘

两种手势共用一个 `ScrollState<1>`：拆开只会重复 offset、拖动与 fling 状态。

- 下拉：`ScrollConfig.pull > 0` 才允许顶端过拉，松手过 `PULL_MIN = 28 px` 进入 `Refresh::Running`。下拉时内容跟着走，松手后弹回；指示器浮在顶端，不占内容高度。
- 上拉：距底 `LOAD_AHEAD = 24 px` 内松手，且当前行数与 `asked_at` 不同，才进入 `More::Running`。同一次行数只问一次，`loaded(rows, end)` 或 `failed()` 之后按新行数重开。
- 页脚：`FOOTER_H = 26` 始终算在内容里，`max` 不因加载状态跳变；`More::End` 之后不再请求。
- 删除与选中留在页面：`FeedView` 只收 `selected` 与 `reveal: Option<(row, px)>`，`SwipeAction` 与行数据都不进组件。

## 页面的两处硬约束

`shared::intent` 不能引用 `shared::route`（`intents_name_what_is_asked_and_not_what_answers_it` 扫描 shipping 源码）。画廊意图因此带路由码：

```rust
BusinessIntent::Gallery { wire: u8, intent: GalleryIntent }
```

`Services::apply` 用 `PageId::from_code` 换回页面；`advanced` / `extended` 的 `apply` 也只比较 `wire`。这比把 `PageId` 塞进意图少一条反向依赖，且和 `NavigationIntent::Go(u8)` 同一套说法。

`Extended` 的 Dialog 一开始包在 `ui.stack(grow(1).basis(0))` 里，结果滚动区拿到 0 高：stack 不是 flex，in-flow 的 `page::scroller` 行不会拉伸。改成让 scroller 留在 `page::body` 的正常流里，Dialog 作为同级的 `absolute` custom 节点盖上去。这样滚动区仍按 `grow` 拿满剩余高度，卡片不随列表滚走；页面在 `dialog` 为真时先收 Tap，命中用 `Ui::paint_rect` 的矩形与 `dialog::action_at`。

custom 槽用量：`Extended` 6 个（键盘/键格、表格、图表、仪表、滑动行、对话框），`Advanced` 1 个（轮播点），都低于 `PAINTS = 8`。

## 实测

| 项 | 结果 |
| --- | --- |
| host 测试 | `cargo test -p iot-app --lib --no-default-features --features host`：**523 passed** |
| 工具测试 | `cargo test -p iot-tool`：**31 + 4 passed** |
| 格式 / lint | `cargo fmt --all`、`cargo clippy -p iot-app --lib --no-default-features --features host -- -D warnings` 无输出 |
| 目标检查 | `moon run iot:check-host` / `check-s3` / `check-c6` 全部通过 |
| 黄金图 | 页面表 22 → **28 行**（`advanced` / `extended` / `list` 各 debug 两态）；菜单因 12 个入口重录 |
| 矢量字模 | 306 字形、**157,460 B**，比上一版 140,826 B 多 16,634 B |
| S3 主栈 | `nm`：`_stack_start_cpu0 − _stack_end_cpu0 = 0x3fce9710 − 0x3fcd5538 = 82,392 B`；串口同一行 `[DISPLAY] main stack 82392 B` |
| 镜像 | S3 **2,014,432 / 6,291,456 B**（32.02%）；C6 **1,812,672 / 6,291,456 B**（28.81%） |
| 烧录 | S3 与 C6 各烧一次，校验通过；S3 启动落 `Menu`、C6 落 `Input`，两边都印 `this boot is healthy` |

## 手感：只能用手确认的那几项

仲裁阈值是 `DRAG_SLOP = GLYPH − 1 = 5 px`（`shared/ui/scroll.rs`），翻卡阈值是卡宽的四分之一（`pager.rs::turn`），圆点命中区是中心两侧各 `TOUCH_MIN / 2`。测试钉住的是「按哪条规则走」，起手 5 px 内算点、翻卡是否太灵敏只有手指能判；调这两处不动页面代码。

## 手感未代测

黄金图钉住的是像素，`Feed` 的阈值与回弹只有手指能确认：下拉到 28 px 的触发感、松手回弹、上拉触底是否太早/太晚。代码里这三个常量集中在 `shared/ui/feed.rs` 顶部（`PULL_MIN`、`LOAD_AHEAD`、`FOOTER_H`），改动不需要碰页面。

## 后续：四页拆分与手势仲裁

上一版把 24 个组件塞进 `Advanced` / `Extended` 两页，交互项高度多为 22~30 px，页面底部控件很难点中；从带 tag 的控件起手纵向拖动，前几像素被当成点击，滚动还会丢。这一轮把画廊拆成四页，并把按下、拖动、点击放进同一个仲裁器。

| 页 | 内容 |
| --- | --- |
| `Advanced` | `AppBar` / `NavigationBar` / `Tabs` / `PageIndicator` / `Pager`、`Accordion` / `Segmented` / `Stepper` |
| `Feedback` | `Banner` / `Toast` / `Skeleton` / `LoadingOverlay` |
| `Extended` | `SearchField` / `FilterChips` / `FormField` / `NumberField` / `Keypad` |
| `Data` | `Table` / `Chart` / `Gauge` / `SwipeAction` / `EmptyState`、`Dialog` / `PopupMenu` / `Sheet` |

`Menu` 从 12 个入口变成 14 个，五行；最后一行要滚动。`PageId::ALL` 16 项，`COUNT` 与 `ROWS` 都由它推出来，不再写死数字。

## Pager 与圆点

`shared/ui/widgets/pager.rs` 是横向卡片条：卡宽取滚动区宽的三分之二，步距四分之三，相邻卡片露一条边。`turn(index, count, rect, dx)` 在松手时按四分之一宽判一张；`page_at(rect, index, count, x, y)` 用同一份几何判点落在哪张卡上，卡片之间的缝返回 `None`。`PageIndicator` 的每个点在中心有 `TOUCH_MIN` 宽的命中区（`dot_at`），`fit` 收起放不下的点。`Advanced` 页的拖动、点卡、点圆点三条路都落到同一个 `pager` 索引。

## 手势仲裁

`services/gallery.rs` 的 `Scroller` 在 `Press` 时只记 `{ id, tag, x, y }`，不立即认领。第一次位移超过 `DRAG_SLOP` 才分方向：

- 纵向：认领按下点所在的滚动区，并把已经走过的位移补进第一次 `Drag`，开头几像素不再丢。
- 横向且 tag 是滑动控件：交给页面进入 `Slide`，页面自己持有预览位移。
- 未过阈值：等 `Tap`，控件仍然收到点击。

第一个认领者独占本次手势，`Slide` / `Drag` 不会互抢。`Menu`、`Widgets`、`Data` 的滑动行都接同一套 `Press` / `Slide` / `SlideEnd`。透明层（`LoadingOverlay`）只在显示时带 tag，点击能把它收掉。

## 尺寸

可点元素统一到 `TOUCH_MIN = 44`：`Tabs` / `Segmented` / `Stepper` / `NavigationBar` 行高、`Accordion` 头、`Banner` 动作、`NumberField` 加减、`Keypad` 单格。Canvas 组件的尺寸从 `rect` 与文字度量推：`Table` 表头 `LINE + 4`、行高 `TOUCH_MIN`；`Dialog` 动作带 `TOUCH_MIN`，卡片高由动作带、标题/正文行与 `MARGIN` 相加；`Chart` / `Gauge` / `Keypad` 随 `rect`；页面 custom 槽用 `page::TOUCH` 的整数倍。少掉的示例高度是 22/26/28/30 与 `KEY_H = 30`。

## 实测

| 项 | 结果 |
| --- | --- |
| host 测试 | `cargo test -p iot-app --lib --no-default-features --features host`：**552 passed** |
| 工具测试 | `cargo test -p iot-tool`：**31 + 4 passed** |
| 格式 / lint | `cargo fmt --all -- --check`、host/riscv/xtensa clippy 均无输出 |
| 目标检查 | `moon run iot:check-host` / `check-c6` / `check-s3` 全部通过 |
| 黄金图 | 28 → **32 行**（新增 `feedback` / `data` 各两态；`menu`、`advanced`、`feedback`、`extended` 因布局重录） |
| custom 槽 | `Advanced` 2（卡片条、圆点）、`Feedback` 0、`Extended` 1（键格/键盘）、`Data` 5（表、图、表盘、滑动行、对话框） |
| 镜像 | S3 **2,037,312 / 6,291,456 B**（32.38%）；C6 **1,833,280 / 6,291,456 B**（29.14%） |
| S3 主栈 | `nm`：`_stack_start_cpu0 − _stack_end = 0x3fce9710 − 0x3fcd55b8 = 82,264 B`；串口同一行 `[DISPLAY] … main stack 82264 B`，峰值 20,696 B |
| 烧录 | S3 与 C6 各烧一次，校验通过；S3 启动落 `Menu`、C6 落 `Input`，两边都印 `this boot is healthy` |

## 手感：只能用手确认的那几项

仲裁阈值是 `DRAG_SLOP = GLYPH − 1 = 5 px`（`shared/ui/scroll.rs`），翻卡阈值是卡宽的四分之一（`pager.rs::turn`），圆点命中区是中心两侧各 `TOUCH_MIN / 2`。测试钉住的是「按哪条规则走」，起手 5 px 内算点、翻卡是否太灵敏只有手指能判；调这两处不动页面代码。

## 可读性一轮：说明行、遮罩与手势收尾

四个画廊原先只有分区词，底部样例看不出叫什么、能不能点。这一轮给每个样例加一行说明（`page::caption`）：名字加手势，例如 `TABLE / TAP A ROW`，中英各一条 `Msg`（新增 24 条 `Cap*`）。`Advanced` / `Extended` 的分区词因此删掉，`Data` 的浮层一带保留 `page::section`（一条线加名字）。说明行是 muted 的 `nowrap`，不带 tag。

顺带修掉的三处：

- **说明行与页签文字宽度为 0**。`page::scroller` 的滚动体没有拉伸，`grow(1) + basis(0)` 的行量出自然宽度 0，标签被裁成空。滚动体改为 `stretch`（Flutter `ListView` 给子节点整条交叉轴的语义），`Tabs` / `Segmented` / `Stepper` / `NavigationBar` / `FilterChips` 的行也各自 `stretch`，分格才真分到宽度。
- **根节点上的 `absolute` 子节点量不到尺寸**。flex / wrap / scroll 只排 in-flow 子节点，出流子节点从来没被 `layout_node` 量过，`paint_rect` 得到 `0×0`；`stack` 是唯一会摆它们的地方。抽出 `place_absolute` 让四种流都摆出流子节点，scroll 额外带上内容位移；回归测试 `an_absolute_child_of_a_row_or_a_scroll_is_placed_too`。
- **拖动后的抬手被当成点击**。线上抬手仍是一枚 `Tap`，`Data` 的表格行、`Extended` 的键格、`Advanced` 的卡片会在拖动结束时各选一个。`Scroller::dragging()` 暴露仲裁状态，三页的 Tap 分支先问它；测试从按下、拖动到抬手，断言什么都没选、下一次拖动照常滚。

界面调整：`Dialog` 的遮罩改成不透明的 `theme.ground`，卡片用新的 `theme.surface`（`rgb(0x22,0x22,0x22)`）加 muted 描边，槽从滚动区挪到根节点 `320×228`，卡片在内容区正中；`NavigationBar` 换成 `surface` 条上内嵌选中胶囊，与 `Tabs` 的整格 accent 区分；`Accordion` 的展开体挪进标题那张纸；`Pager` 每张卡各自裁切；`Table` 行高从 `TOUCH_MIN = 44` 降到 `LINE × 2 + 4 = 28`，`TABLE_H = 128`。

`Style::overflow` 补上 Flutter `TextOverflow` 里 `nowrap` 段能用的那一档：`Clip`（默认）/ `Ellipsis`，`.ellipsis()` 是简写，落在 `Banner` / `Button` / `EmptyState` / `FilterChips` / `Segmented` / `Stepper` 上。矢量表本就有 `…`（`iot-tool outline` 固定补 `…` 与 `□`），不必改 `vector-sample.txt`。

中文说明行需要 44 个新汉字与 `-`，`vector-sample.txt` 随之扩行，`vector.bin` 重切为 351 字形 / **182,810 B**（上一版 306 字形 / 157,460 B）。

### 实测

| 项 | 结果 |
| --- | --- |
| host 测试 | `cargo test -p iot-app --lib --no-default-features --features host`：**557 passed** |
| 工具测试 | `moon run iot:test-tool`：**31 + 4 passed** |
| 格式 / lint | `cargo fmt --all -- --check` 无输出；riscv `cargo clippy --workspace --exclude iot-tool --target riscv32imac-unknown-none-elf -- -D warnings`、S3 `iot-xtensa.sh clippy …`、`cargo clippy -p iot-core --all-targets -- -D warnings` 均通过 |
| 目标检查 | `moon run iot:check-host` / `check-c6` / `check-s3` 全部通过 |
| 黄金图 | 仍 32 行；`advanced` / `feedback` / `extended` / `data` 因拉伸、说明行、遮罩与行高重录，`menu` / `widgets` 未变 |
| custom 槽 | `Advanced` 2、`Feedback` 0、`Extended` 1（键盘与键格共用一个槽）、`Data` 5 |
| 镜像 | S3 **2,064,128 / 6,291,456 B**（32.81%）；C6 **1,861,584 / 6,291,456 B**（29.59%） |
| S3 主栈 | 串口 `[DISPLAY] … main stack 82264 B`，峰值 20,696 B；`[STACK] core 1 peak 2000~2176 B of 24576 B`（两次启动） |
| 烧录 | S3 `/dev/cu.usbmodem1442301`、C6 `/dev/cu.usbmodem14201` 各烧一次，两边都印 `this boot is healthy` |

`moon run iot:flash` 在无 tty 的 shell 里刷完最后一步会失败：espflash 烧录后要开交互式 monitor，`Failed to initialize input reader` 退出码 1。镜像已经写完（日志里的 `Flashing has completed!` 在前），抓启动日志用 `espflash monitor --non-interactive --after hard-reset`，不必重烧。

## 四个手感缺陷：未带 tag 的 canvas 槽与浮层补全

用户报的四处（`Extended` 键格无反应、键盘标点不全、`Data` 菜单/抽屉太单薄、图表/表盘点击无响应）里，三处是同一个根因。

线上一次点击是 `Press` 后跟 `Tap`。`page::scroller` 里的 canvas 槽（`TABLE_SLOT` / `CHART_SLOT` / `GAUGE_SLOT` / `KEY_SLOT`）原先**没有 tag**：按下点 `tag_at` 得 0，`Scroller::apply(Press)` 于是把这次手势认领成拖动（`tag == 0 → claim`），抬手时 `dragging()` 为真；两页的 Tap 分支都以 `!self.scroll.dragging()` 为前置，整段跳过，`Tap` 被 `scroll.bind` 当 `Release` 吃掉。带 tag 的按钮 / chip 正常，所以只有这些槽里的行、柱、盘、格全点不动——用户没报表格，其实表格也一起坏了。旧测试只发 `Tap`、不发 `Press`，测不出这条路径。

修法：给四个槽各带 tag（`TAG_TABLE` / `TAG_CHART` / `TAG_GAUGE` / `TAG_KEYPAD`），`TagTable` 容量各加 3 / 1；按下只记住，超过 `DRAG_SLOP` 才认领滚动。回归测试走 `bind` 的 `Press → Tap` 整链，断言行 / 柱 / 盘 / 格真的落到意图，并另钉一条“每个 canvas 热区中心 `tag_at` 非 0”。

键盘补全：新增 `Layer::More`，放 `SYMBOLS` 没覆盖的 22 个标点（`" ' ( ) [ ] { } < > / \ & % + = * $ ~ ^ | ` `` ` ``），四层（字母 / 数字 / 符号 / 更多）合起来正好打出 `0x20..=0x7E` 全部可打印 ASCII。纯 ASCII，`ui12.bin` 已含全部字符，不必重切 `vector.bin`。

菜单 / 抽屉：从常驻内联 `Widget` 改成和 `Dialog` 同构的 canvas 浮层（`card` + `item_at` / `row_at` + `draw`，不透明遮罩）。`Data` 在根节点新增两个常驻 `absolute` 槽 `MENU_SLOT` / `SHEET_SLOT`，按开合绘制；触发按钮带 tag，菜单锚在触发按钮下边缘（快照发布 `menu_anchor`，绘制时夹在内容区内），抽屉从内容区底边升起；浮层打开时 `bind` 在滚动区之前先收 Tap，点项选中并关闭、点遮罩或外部关闭，其余手势一律吞掉。选中结果用 `page::reading` 印在触发按钮下。

### 实测

| 项 | 结果 |
| --- | --- |
| host 测试 | `cargo test -p iot-app --lib --no-default-features --features host`：**568 passed** |
| 工具测试 | `moon run iot:test-tool`：**4 passed** |
| 格式 / lint | `cargo fmt --all -- --check` 无输出；`cargo clippy -p iot-app --lib --no-default-features --features host -- -D warnings` 通过（`--tests` 下 4 条既有告警在未改动的 `i18n/lang.rs`、`render/engine/test_support.rs`、`render/screen/painter.rs`） |
| 目标检查 | `moon run iot:check-host` / `check-c6` / `check-s3` 全部通过 |
| 黄金图 | 仍 32 行，`data` 默认态未变（触发按钮在首屏之下、浮层默认关闭），未重录 |
| custom 槽 | `Data` 7（`TABLE` / `CHART` / `GAUGE` / `SWIPE` / `DIALOG` / `MENU` / `SHEET`） |
| 镜像 | S3 **2,065,824 / 6,291,456 B**（32.84%）；C6 **1,866,448 / 6,291,456 B**（29.67%） |
| S3 主栈 | 串口 `[DISPLAY] … main stack 82296 B`；`[STACK] core 1 peak 2000 B of 24576 B` |
| 烧录 | S3、C6 各烧一次，两边都印 `this boot is healthy` |

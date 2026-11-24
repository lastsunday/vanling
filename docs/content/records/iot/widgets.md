+++
title = "高级组件与 Feed"
description = "25 个高级组件、一条 Feed 与四个画廊已落地：host 552 条测试、32 行黄金图，S3 主栈 82,264 B，镜像 2,037,312 / 1,833,280 B 并已烧进两块板。"
weight = 70
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 高级组件与 Feed

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

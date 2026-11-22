+++
title = "架构"
weight = 10
+++

# 架构

## 概览

以意图(Intent)为触发点、按帧推进的状态机

### 流程

1. 初始化芯片获取外围设备(总线和引脚),日志,堆(射频，其他),堆剩余容量调用钩子
1. 根据总线和引脚进行接线创建板子,获取板子对象,定时源,软中断
   1. 获取持久化仓库
1. 根据定时源，软中断创建和启动RTOS
1. 合并产品运行和网络运行到同一任务里
   1. 产品运行
      1. 从板子获取输出表面列表
      1. 从板子获取输入事件列表
      1. 从板子获取姿态传感事件,开启状态,能力
      1. 从板子获取声音事件,开启状态
      1. 记录各个特性的需求
      1. 从板子获取喇叭对象,开启状态
      1. 从板子获取摄像头对象,开启状态
      1. 创建渲染器
      1. 遍历表面,按角色建灯渲染器或屏幕渲染器,把摄像头交给屏幕
      1. 创建设备管理器,将各个特性和需求,参数,覆盖参数传入设备管理器
      1. 创建配置器,喇叭共享对象,需求共享对象
      1. 根据喇叭共享对象，需求共享对象来启动主流程
         1. 有喇叭共享对象和软中断
            1. 定时运转的声音播放引擎
            1. 输入事件引擎
            1. 渲染引擎
         1. 有喇叭共享对象,没有软中断
         1. 没有喇叭共享对象
   1. 网络运行

## 分层

`iot-core(纯逻辑、驱动接缝与像素/字形) → iot-chip-esp(esp 家族运行时) → iot-bsp-esp(每板接线) → iot-app(产品页面与状态机)`

- **接线只出现在 `bsp/` 板模块**：`Board::new(Peripherals)` 内固定引脚；业务代码禁止出现引脚号
- **bsp-esp 内部分两层元件**：`components/` 是实际元件（芯片驱动，参数化总线与引脚，不认板）；`virtual_components/` 是抽象元件（由实际元件组装而成的虚拟器件，如 `Screen` 把 ST7789 面板、背光与帧缓冲组装成 `ScreenSurface`）。板模块只做选引脚与装配，元件本身跨板通用
- **`core::ui` 只认像素与字形**：`Canvas` 只持有帧缓冲、尺寸与 ground；`font` 只做字模，`typeset` 做禁则与换行，`layout` 是 Flutter 词汇的盒模型（flex/wrap/stack/scroll），`panel` 是从字面推出的面板度量。它不认页面、快照、网络、产品或芯片；页面壳（`shared::kit::{panel::Panel, chrome}`）、页面视图与网络几何都在 `iot-app`，因此仍可在宿主机测试而不依赖屏幕
- **bsp 只送去画，不认页面**：`bsp-esp` 持有帧缓冲、DMA、面板与相机采样，经 `core` 的 `ScreenPainter` 接缝把 `ScreenTarget`/`ScreenContext` 交给 app；页面状态与重绘门控在 app，bsp 不知道 `FrameSnapshot` 或任何页面类型
- **元件独立 feature**：每个元件一个 feature，在 `components/` 的 `mod` 处门控一次，off 即文件不存在；板 feature = 元件聚合清单（见 [features.md](@/development/iot/features.md) 模块轴）
- **iot-chip-esp**：芯片初始化、日志、RTOS、panic 定义；应用层入口宏 `#[esp_rtos::main]` 留在 app 的 bin 入口（`main.rs`、`bin/*.rs`）
- **iot-bsp-esp**：板差异收敛在 `type Board` 别名、`HEAP_BYTES`/`SETUP_HOST` 常量与每芯片的 `start_executors`（feed 落点 `Feed::Interrupt`/`Inline`），前者每板一个 `#[cfg(feature)]`、后者每芯片一个；开机流程本身共用且不含 `#[cfg]`
- **iot-app**：应用层，产品名 `vanling`；业务零芯片依赖；flash 与 RAM 容量走链接与分区，不进类型系统
- **命名约定**：板层用板名、应用层用产品名，不重复
- **纯逻辑但只有 bsp 内部用的模块留在 bsp**：`hardware.rs`（启动清单）不认板也不认芯片，宿主机可测。声音目录（哪个 `Sound` 是哪个 `.mid`、以及它怎么变成一帧）在 `iot-app`：板子的 `Speaker::play` 收的是 `Box<dyn SampleSource>`，帧而不是名字。字模、排版与布局在 `iot-core::ui`；页面壳、视图层与网络词在 `iot-app`。`core` 里留下 `geometry`/`light`（含 RGB565 打包与反色），因为页面与 `intent` 两边都要读；键盘属于 Network 页，术语在 `app::shared::intent`、几何在 `app::services::network::shape`
- **main 入口**：产品二进制入口（`bin/*-probe.rs` 是各自的探针入口）；芯片 init、堆、执行器与 `Wiring` 都在 `main.rs`，芯片族分支留在 `start_executors`。产品与面板 bench 共用这条启动路径，`panel-bench` feature 只决定最后调 `lib::run` 还是 `bench::run`
- **iot-tool 在这条链之外**：唯一不属于设备的 crate，读写本仓的源文件（目前只有 `midi`，生成声音目录的 `.mid`）。**零依赖**——固件一改不该把生成固件资产的那个东西改坏。裸金属的 `--workspace` 只剩 `lefthook.yml` 里那一条，已 `--exclude iot-tool`；`moon` 各任务都显式带 `-p`，本来就不碰它。入口是 `moon run iot:test-tool` 与 `moon run iot:gen-midi-assets`

### 状态词汇：四道问题，各归一处

一次启动里「还好吗」有四种问法，存活期不同；混用会把持久事实当成运行期结论。

| 问题 | 类型 | 归属 | 存活期 |
| --- | --- | --- | --- |
| 这块板焊出来了吗 | `Backed` | `app::shared::model` | 组合点一次决定，运行期不变 |
| 启动时这个件答了吗 | `Presence` / `Evidence` | `bsp-esp::hardware` | 启动一次 |
| 这次启动算数吗（跨复位） | `ResetRecord` / `BootMode` | 规则 `core::boot`，寄存器 `chip-esp::boot` | RTC，跨复位 |
| 这次启动证明过自己吗 | `Readiness` | `core::boot` | RAM，复位即失 |
| 现在这个件答不答 | `Health` | `app::shared::model` | 运行期 |

`Readiness` 是 RAM 里那半边：首帧在每块有线屏上 `flush` 完、网络栈就绪（或这个构建本来没有 radio）两半都到，喂狗任务才调一次 `mark_healthy()` 清掉计数——时间不是证据。

命名跟着做的事走：一次性上报用 `*_reported`（`first_frame_reported`、`health_reported`、`chain_reported`），收下一个事实用 `note_*`，发出去用 `report_*`，盖持久标记用 `mark_*`。

## iot-app 内部分区

`app/src/` 按垂直切片组织：页面各自成片（小页一个文件，大页一个目录），跨页真相与词汇收在 `shared/`，组合根与表示层各自独立，接线只发生在组合根。

页面是设备当前的目的地，不是屏幕上的画面：`Route.needs` 只看板子接了什么能力，屏的存在由 `SurfaceRole::Screen` 单独回答；无面板的板子照样有当前页，灯带、demand 与网络 setup 都只是这个目的地的其他消费者。

| 路径 | 职责 |
| --- | --- |
| `lib.rs` | 组合根：取剩余能力、建 `DeviceManager`、启动全部应用工作者、等待协调者；`run` 是产品入口，`run_with` 多收一个由调用方装箱的系统，只有 `bench::run` 用它 |
| `main.rs` | 产品入口：芯片初始化、堆、执行器与 `Wiring`，最后按 `panel-bench` 选 `lib::run` / `bench::run` |
| `services/` | 产品服务（页面与设备自己的 `nav`/`light`），一个服务一个垂直切片：小页一个文件，大页一个目录（`model` / `controller` / `shape` / `view` 按需拆）；`mod.rs` 定义各平台共用的 `Service`（`apply`）与本地平台面的 `LocalPage: Service`（`bind`/`publish`/`view`/`descriptor`/`tick`/`on_show`/`on_hide`）、持有全部服务的 `Services` 与它那一张分派表（私有 `ServiceId`：`Page(PageId)` 或设备自己的 `Navigation`/`Light`），以及 `PageId → Descriptor` 的唯一映射；`Services::bind` 是输入边界：瞬态页直接问页，否则先答跨页绑定（setup 长按、三击）再问页；页自己的输入契约在它的 `tests.rs` 或文件尾，跨页的黄金像素表在 `services/tests.rs` |
| `services/{ambient,attitude,camera,env,splash,text}.rs` | 只有一块画布的单文件页：页面类型上的 `view` 直接声明整页；`text` 另持两段独立滚动区 |
| `services/audio/{mod,tests}.rs`、`services/speaker/{mod,tests}.rs` | 翻译与绘制都在 `mod.rs`；`AudioState`/`PlaybackState` 这类被多个消费者读的记录留在 `shared/model` |
| `services/network/` | 手势与控件都在 `controller/`：`mod.rs` 是手势翻译、动作条与命中；`shape.rs` 是列表、键格、动作条与对话框共用的唯一几何（绘制与命中同读它）；`view/` 按带拆（`header` / `content` / `overlay`，词在 `text.rs`），带之间的层序由 `shape::Band` 决定；`NetworkView` 在 `shared/model/network_view/`（快照要能命名它） |
| `shared/` | 跨页真相与词汇：只放一个以上页面会读的东西，是模块图里的叶子 |
| `shared/model/` | 工作者写入、渲染循环读取的记录，一域一文件；`device.rs` 是聚合体 `DeviceState`，`shared/snapshot.rs` 持有 `FrameSnapshot` 与页面发布的 `PageSnapshot`，`network_view/` 是 Network 页的视图（页面拥有、快照复制、链路不读） |
| `shared/route/{mod,nav}.rs` | 路由表与 `Navigator`：`PageId` 是目的地，`route()` 是穷尽 `match`，`ALL` 是唯一次序（按钮也照它走） |
| `shared/intent/` | 入站消息：`InputIntent`/`BusinessIntent` 与各页问句，含跨页的 `Confirm`；灯光动词 `LightAction` 与 `BusinessIntent::Light { instance, action }` 在这里定义；`keyboard.rs` 是 Network 页敲字用的键位词汇（`Key`/`Layer`/`Cycling`、多击窗口、每层候选字符表） |
| `shared/model/light.rs` | 灯光记录：模式、呼吸表与调色板；`LightState::apply` 把 `LightAction` 落到新模式，`can` 说明该模式认不认这个动词 |
| `shared/snapshot.rs` | 产品快照：一张图融合每个子系统的状态，含音频/播放快照与页面发布的 `PageSnapshot` |
| `shared/kit/` | 页面壳 `Panel`（声明 → 布局 → 绘制的调度）、跨页版面 `chrome`（页边距、行距、标签槽）、文字词汇 `text` 与滚动状态 `scroll`（偏移、滑行、回弹、两个开关）；只认像素与快照，不认页面 |
| `state/manager/mod.rs` | 读侧：`DeviceState` 的所有权与构造（`Boot`、`DeviceManager::boot`）、持有 `Services`、按页派生的 `DemandPolicy`，以及协调者与渲染循环读的口 |
| `state/manager/write.rs` | 写侧：总线意图、worker 上报与时钟 tick 的每一次写入；记录只在这里移动 |
| `state/env.rs` | 构建期配置（`IOT_BOOT_PAGE`、`IOT_LANG` 与覆盖项） |
| `render/engine/` | 与 surface 无关：`Differ` 决定*显示什么*（`differ.rs`），状态→外观在 `projection.rs`，`appearance.rs` 定义 `Topic`/`Appearance`，`color.rs` 步进颜色，`set.rs` 把外观交给认领它的 renderer |
| `render/screen/` | 屏幕（`SurfaceRole::Screen`）：`ScreenRenderer`（`mod.rs`）、`painter.rs` 调 `services::paint`、`repaint.rs`（帧漂移与重绘步长） |
| `render/lamp/` | 灯带（`SurfaceRole::Lamp`）：`LampRenderer`，只有一个颜色，没有屏幕会叠上去的东西 |
| `render/wire.rs` | 接线：`wire()` 把板子的 surface 变成 `ScreenRenderer`/`LampRenderer`，`RenderSet` 就是这一层交出去的句柄 |
| `runtime/` | 模型旁边的任务：协调者、`feature/` 里的功能工作者，以及它们共享的 `workers/` 句柄 |
| `runtime/system.rs` | `System` 契约：`tick` 收 `&mut DeviceManager` 与时钟，`intent` 只读意图已写入的状态；worker 句柄由系统自己持有 |
| `runtime/coordinator/{mod,pacing}.rs`、`coordinator/systems/*.rs` | 协调者 `run_loop`：一帧四段（取意图 → 走系统 → 呈现 → 等待）；`pacing.rs` 持帧时间与下一帧截止点 |
| `runtime/workers/{mod,inbox,input,playback,network}.rs` | 协调者 ↔ 工作者的全部句柄与协议；`mod.rs` 只有再导出，句柄唯一的家是 `runtime/mod.rs` 的 `Runtime`；网络命令词汇表在 `shared/model/network.rs` |
| `runtime/feature/{mod,input}.rs` | 功能工作者；`mod.rs` 是唯一的 `wifi` 门控点。输入工作者：轮询、demand 调度、手势识别 |
| `runtime/feature/playback/{mod,sounds/}` | 播放工作者与声音目录；状态→命令在 `coordinator/systems/playback.rs` |
| `runtime/feature/network/{mod,link,setup,provision,scan,store,watch}.rs` | `#[cfg(feature = "wifi")]`：网络工作者，协议栈、链路、面板侧 setup、AP 服务、扫描、凭据 |
| `assets/` | 设置侧自己的字节：`vanling-` 热点前缀与 setup portal 的 HTML/SVG（协议仍在 `core`） |
| `bench/{probe,replay}/`、`bin/` | bench 专用驱动与入口：`probe/` 只被 `bin/*-probe.rs` 引用；`replay/` 是面板回放与把文本敲进键盘的 tap 脚本（`point.rs` 只放落点坐标），只被 `main.rs` 在 `panel-bench` feature 下经 `bench::run` 引用，宿主测试也编译它。产品入口与帧循环都不认识 `bench`；`IOT_PANEL_BENCH` 与构建凭据的取舍在 `state::env` |

方向由 `iot-tool/tests/module_boundaries.rs` 的几条诚实规则守着（编译器不查 crate 内模块环），不是逐条边的地图：

分层是 `lib → runtime → {render, state} → services → shared`；`assets` 是 `services`/`runtime` 共用的叶子。

```
lib               → { render, state, runtime }
services/<page>   → { assets, shared::{intent, kit, model, route, snapshot} }
services          → { services::<page>::controller, shared::{intent, kit, model, route, snapshot} }
state             → { services::Services, shared::{intent, model, route} }
render            → { services::{descriptor, paint}, shared::{kit, model, route, snapshot} }
runtime::coordinator → { render, state, runtime::{feature::input, system, workers} }
runtime::system       → { state, runtime::feature::input }
runtime::feature::input    → { shared::{intent, model}, runtime::workers }
runtime::feature::network  → { assets, shared::intent, state, runtime::{feature::input, workers} }
runtime::feature::playback → { shared::model, runtime::{feature::input, workers} }
runtime::workers           → { shared::model }
bench             → { lib::run_with, services::network::{controller, shape}, shared::{intent, model}, state, runtime::{feature::{input, playback}, system} }
```

- **页面之间不互相 import**：页面只读 `shared`（含 `DeviceState`）；页面需要另一页的东西时，那份东西就属于 `shared`
- **`services` 是唯一认识所有服务的模块**：每页一个具体类型（`AmbientPage`…`NetworkPage`），`Services::apply` 是唯一的业务分派表，一条 `BusinessIntent` 组一条臂；`state` 只经 `Services::bind`/`apply`/`snapshot`/`enter`/`leave` 提问，`render` 只经 `services::paint` 与 `services::descriptor(page)` 上的读取取答案；页面只读 `shared`（含 `DeviceState`、`FrameSnapshot`），不认识 `state`/`render`，`PageId` 等词汇来自 `shared::route`
- **`shared/kit` 不认页面**：`Panel` 把页面的声明布局成盒子再落墨，`chrome` 只提供跨页共用的边距、行距与标签槽；哪个页面声明什么由 `services::descriptor(page).view` 决定，页面自己画的槽由 `descriptor(page).custom` 决定
- `shared` 是叶子：不依赖 `state`/`services`/`render`/`runtime`。`DeviceState`、`PageSnapshot` 与 `NetworkView` 放在这里，就是为了让页面、快照和渲染都只朝下看
- `shared` 五个桶各有一个变更理由：`model`（记录）/`route`（目的地）/`intent`（入站消息）/`kit`（像素原语）/`snapshot`（帧与页面视图）；`every_shared_bucket_has_two_product_readers` 守着“只有一个读者就不是共享”，`the_shared_buckets_are_the_five_the_taxonomy_names` 守着桶的集合
- **测试跟着断言对象走**：一个测试放在它断言的模块的文件里，或该模块自己的 `tests.rs`；`state/tests/` 是唯一按主题分文件的测试树，夹具只有一份，在 `state/tests/support.rs`（`state::test_support` 只是它的再导出）。测试不算模块图的一条边，`module_boundaries.rs` 因此不扫 `tests/`、`tests.rs` 与 `test_support.rs`
- `runtime::workers` 不带 `wifi` 门控：host 构建照样持有 `Network`，只是没人应答
- `runtime::workers` 不反向依赖任何工作者：它只认识 `core` 的类型，这样 `playback`/`network`/`input` 才能单向引用它
- `runtime::feature::network` 不依赖 `services`/`render`/`runtime::coordinator`：网络工作者读 `workers` 的页面与命令，不读设备状态

### 页面模型

每个页面是一个具体类型，分两层回答。`Service` 是各操作平台共用的一面：`apply(&mut self, &BusinessIntent, &mut DeviceState) -> (Option<Command>, Effects)`——自己那条记录先落地，返回设备还欠的事与要出核心的命令，一条意图恰好调到它的一个所有者。`LocalPage: Service` 是本地平台面：`bind` 把一个本地手势译成业务意图（`Option`，含义什么都没有时是 `None`），`publish` 把页面私有的本地 UI 状态交给帧快照（口令草稿、键位、确认框，远程平台看不到），`view` 声明整页（`&FrameSnapshot, &mut Ui`，不取 `self`），`descriptor` 描述它怎么占屏，`tick` 推进页面自己的时钟，`on_show`/`on_hide` 默认空实现。`view` 与 `descriptor` 的 `where Self: Sized` 让 `&dyn LocalPage` 仍是本地手势表要的 trait 对象。

`Descriptor` 是一页的面板答案：声明函数指针、自绘槽函数指针、底色、是否相机帧、重绘步长、触发条件与纯滚动判据。构建器默认「无自绘槽、无底色、非相机帧、`REPAINT_FLOOR_MS`、`Never`、不认滚动」，各页只写自己那条差异。重绘判据是 `Repaint::{Never, Always, Overlay, When(fn), WhenOrOverlay(fn)}` 之一，而不是一段带空形参的函数。

渲染答案分两层：`LocalPage::view` 用 `Ui` 的 `row`/`column`/`stack`/`wrap`/`scroll`/`text`/`custom` 声明整页，`services::paint` 取 `descriptor(page).view` 交给 `Panel`（布局一次、画字与盒子、再按槽调 `descriptor(page).custom`），最后盖角标；其余五个答案都是 `descriptor(page)` 上的读取（`ground()`/`camera_frames()`/`repaint_step_ms()`/`repaints_on()`/`scroll_shift()`）。

长内容页把滚动留在页面：`Style::scroll_area(id)` 给滚动盒命名，布局沿轴累加子节点（含 `gap`）并把矩形与可滚动距离记进 `Tree`，`Ui::scroll_info(id)` 读回；`shared::kit::scroll::ScrollState<N>` 持每个区的偏移、速度、是否被手指按住与两个开关（回弹、滚动条）。`measure` 用页面将要画的那份声明跑一次 headless 布局——面板尺寸与内容都是静态的，所以这一次就是运行时那次。手指按下后 `GestureEvent::Drag` 每移动 2 px 报一次，页面用 `drag` 直接改偏移（越界半程阻尼），抬手用 `fling` 接手：速度带符号、阈值以下只回弹。被按住的区 `tick` 不弹回，其他区照常滑行；布局从不夹偏移，边界只在页面里。滚动页的重绘判据是 `Repaint::WhenOrOverlay`，`FrameSnapshot::animating()` 为真时渲染器保持 `TimeDriven`；`PageSystem` 在 `schedule.tick` 里把墙钟增量（首帧 0，夹到 `MAX_CATCH_UP_MS`）交给 `DeviceManager::tick_pages`，偏移因此与快照同帧，也不会因某帧光栅化慢而按固定步长少走。

`text` 页在构造时断行一次：`typeset::wrap` 的每行按字节范围与宽度存进快照，声明时各是一个 `nowrap` + 显式宽度的文本节点；`layout_text` 对 nowrap 且宽度已定的节点跳过 `max_content`，一帧不再逐字查字模表。`\n`、`\r`、`\r\n` 都按硬换行处理。

文字有两套面：12 px 点阵（`Face`，只有整倍放大）与矢量轮廓（`VectorFace`，按给定尺寸精确缩放）。`text` 页正文用矢量面 24 px，字模行与覆盖行仍用点阵；矢量表由 `cargo run -p iot-tool -- outline` 从 Noto Sans Mono CJK SC Regular（OFL-1.1）裁出，码点集合就是正文样张 `core/assets/font/vector-sample.txt`，表只存折线与 1/64 em 坐标；行盒取 typographic 升降与全表墨迹包络的较大者，再加字号 1/8 的 leading（24 px → 30）。`Style::font_size(Length)` 定尺寸：`Px` 精确、`Dp` 是 5/4 px（2.0" 320×240 = 200 dpi 的 DPR）、`Rem` 是根字号 12 px 的百分之一；字重、字体族、装饰线与斜体压在 `Style::text_flags` 的一个 `u16` 里。颜色是 ARGB32：`Style`/`RunStyle` 存 16 槽调色板的 `ColorId`（声明期去重），`Canvas` 按覆盖率把字墨 src-over 混合到 RGB565；`Ui::span` 最多 8 条，只改颜色、装饰线与底色，不改行度量。矢量面缺任一字形时整行退回点阵（`Metrics::covering`），一行不会半矢量半点阵。矢量字形落在整像素上，因此覆盖率只取决于码点、字号与字重：`Canvas` 把 ≤24 px 的字形按 `(char, size, weight)` 收进 `GlyphCache`（堆上，约 69 KiB），首次栅格化后每帧只 `blit`；重墨先按 `over(a,b)=a+b-ab/16` 与左移一列合并成一张掩码再落一次，所以反色地面上不会互相抵消。更大的字号与未挂缓存的画布直接栅格化，超过 64×256 px 会触发断言而不是静默不画。

页面可以用 `PageId::steady()` 声明稳态：`FrameSnapshot::steady` 让屏幕背光常 100（只有关灯才降 0），灯带停在声明颜色的相位 0 不再呼吸；页面自己的滚动仍按 `animating()` 保持 `TimeDriven`。今天的稳态集只有 `text`，`only_the_reading_page_holds_the_light_still` 钉住这一点。

### 渲染管线：声明一次，布局一次，落墨一次

页面不持有控件树，也不自己算坐标：`view` 跑完就留下一份声明，`Ui` 把它布局成整数矩形，然后画字与盒子。命中测试用同一份声明跑 `Ui::headless`，不建画布。

| 层 | 做什么 | 词汇 |
| --- | --- | --- |
| `ui::layout` | 1/64 px 定点布局；测量自上而下，容器把去掉内边距后的约束交给子节点；最终边按像素取整，相邻盒子的共享边落在同一像素 | `BoxConstraints`、`Length`（px / dp / rem / %）、`Style`、`Flow`、`Flex`（grow/shrink/basis）、`Wrap` 多 run、`Stack` 绝对定位、`Scroll` 裁剪、`Align`/`Justify`/`TextAlign` |
| `ui::declare` | `Ui` 的声明构建器与落墨驱动：`view` 期间攒树，结束后布局一次、按槽回调，再画字与盒子 | `Ui::{row, column, stack, wrap, scroll, text, span, custom, paint, headless, rects}` |
| `ui::paint` | 每个已布局节点的墨：文字 run（按 span 换色与装饰线）、自绘槽、底色与裁剪；单行不换行的 run 直接画 | `NodeKind`、`RunStyle`、`draw_text` |
| `ui::typeset` | UAX #14 子集 + CLReq 禁则：行首不落 `。`、行尾不留 `「`；测量与绘制共用同一个断行器，`…` 截断 | `Paragraph { lines, count, truncated }`、`wrap`、`next_char` |
| `ui::font` | 两套面：12 px 点阵 Fusion Pixel，GB2312 + Big5（15,466 字形）；矢量轮廓按 1/64 em 存折线。`Metrics` 按 `(family, size)` 给行高与步进；ASCII 步进 6 px、汉字 12 px（点阵），升部 10 行 | `Face`、`VectorFace`、`Metrics` |
| `ui::panel` | 从字面推出的面板度量：320×240、页脚 12 行、行距 13、触控最小 44 px、左右边距 | `PANEL_COLUMNS`、`CONTENT_BOTTOM`、`LINE`、`ROW_PITCH`、`TOUCH_MIN` |
| `ui::canvas` | 像素、裁剪与填充/描边；ARGB32 按覆盖率 src-over 到 RGB565，矢量轮廓按 4 条子扫描线取覆盖，不超过 24 px 的字形按 `(char, size, weight)` 缓存进 `GlyphCache`；RGB565 打包与反色也在这里 | `Canvas::{fill_rect, stroke_rect, draw_char_at, draw_char_heavy_at, draw_text_at, push_clip, with_glyphs}`、`GlyphCache` |

页面组合有两条硬规矩：读数行一律走 `chrome::reading`（标签 4 格 + 值右对齐 7 格，行长恒定），列的宽度写死而不靠 `grow`——flex 的起点是内容宽，位数一变就重新分配，整块文本跟着跳；自绘叶子放进 `stack`，用 `absolute().right/bottom` 贴边，声明在文字之后（z 序在上）。

自绘叶子（波形、刻度盘、网络带）走槽派发：`Ui::custom(style, slot)` 只记槽号，`Ui::paint` 布局后按槽回调页面的 `Custom`。页面的 `view` 与自绘函数都不认识帧缓冲以外的世界，`Panel` 只认识一块 `Canvas` 与它属于哪块屏幕。

布局是纯函数，但一帧里同一节点会被问很多次：flex 先测量再摆放，`grow` 级联还会换一组约束再问。`Tree` 因此在每个节点上记 `measured_at`（`enforce` 之后的四条约束压进一个 `u64`）与对应的 `measured`，同约束第二次问直接返回，约束变了才重测（高度单独变化也算）；新声明拿 `Node::default()` 的哨兵键，上一帧的尺寸不会冒充这一帧。像素层按「一次求边界、整段写」组织：glyph 按行取位图、空字节跳过八个像素，`fill_rect` 每行只求一次裁剪交集，`Start` 且单行的 `draw_text` 不跑断行器，`Screen::fill` 一次写两个像素。这些只改路径不改画面，整页黄金哈希守着。

滚动帧不重画整页：页面用 `Descriptor::with_scroll` 交出纯滚动判据——只比较两个滚动区的偏移（其余字段必须相同），`AppPainter` 留上一帧快照，命中后把已有像素原地 `memmove` 上移或下移，然后给出两份 `FrameDamage`（各最多 7 块，含底部徽标带）。**落墨集**只含进入视口的窄条、头部读数与 thumb，`Canvas` 按它裁剪落墨；**上屏集**含整个滚动框与 header、thumb，`St7789` 用 `CASET`/`RASET` 把这些交给面板——GRAM 不会自己搬像素，只写窄条会让框里其余行停在拖动开始时的内容上。底色改变、换页与相机帧退回整帧；overlay 只把底部 320×12 徽标带加进两份集合，不再让整页重绘。`ScreenPainter::reset` 负责让画布忘掉上一帧。

`Services` 持有九个页面，外加设备自己的两件事：`NavService`（页栈）与 `LightService`（灯）。公开 `bind`/`get`/`apply`/`snapshot`/`enter`/`leave`，没有按页取字段的访问器。扫描落地后，列表先写进记录，再由 `LinkIntent::Scan` 走同一条分派表让 Network 页把草稿夹进新列表——没有专门的扫描钩子。`Services::apply` 是唯一的业务分派表：一个 `BusinessIntent` 组一条臂：`Link` 写 `NetworkState`、`Network` 写 `NetworkView`，两条记录同属 `NetworkPage`，所以合成一条，其余四组各归一个；表是穷尽 `match`，加一组不编译。新增页面＝建 `services/<page>/`、在 `Services` 加一个字段、在 `get`/`get_mut`、`service_mut` 与 `descriptor(page)` 的 `match` 里各加一条臂，再写 `bind` 与 `descriptor`（认领意图的页再写 `apply`）；漏一条编译不过，`iot-tool/tests/page_table.rs` 守着这条模板。新增一个不是页面的服务＝加字段、加一条 `ServiceId` 变体、写 `apply`，再让 `Services::apply` 指向它。

生命周期由 `state/manager/write.rs` 在一次业务意图落完状态后比较 `state.page()`：变了就先 `leave(旧页)` 再 `enter(新页)`，启动时 `Splash` 也算一次 `enter`。钩子与 `has_screen()` 无关（无屏板子的页面照样收到），但只能读写页面自己的字段。Network 页在 `on_hide` 里收起键盘与确认框，保留用户选中的行与草稿，所以离开再回来不会重新弹出键盘。

`PageSnapshot` 是页面对外唯一的视图（`shared/snapshot.rs`），渲染层只拿它，不认 `crate::services`；`NetworkView` 因此也放在 `shared/model/network_view/`。

- `bench` 是入口/夹具，不是层：`probe` 只被 `bin/*-probe.rs` 引用，`replay` 只被 `main.rs` 在 `panel-bench` feature 下经 `bench::run` 引用（宿主测试另编译 `replay`）；`lib` 与 `runtime` 都不认 `bench`，`module_boundaries.rs` 的 `the_bench_is_reached_only_by_its_own_entry` 守着。构建期开关 `BENCH`/`build_credentials` 在 `state::env`，网络工作者取凭据因此不会反向指向 harness
- `core` 不认识 `iot_app`

`main` 是产品二进制 crate 根（`bin/*-probe.rs` 各有自己的根），经 `iot_app::` 取 `lib::run` / `bench::run` 与 `runtime::feature::network` 的入口，不在这张模块图内；板级 bring-up 就在 `main.rs`，芯片族入口在这里把 `second_core_peak` 这类能力注入 `Wiring`，库内不再出现 `#[cfg(feature = "esp32*")]`。

### 远程界面不是页面

三种「把界面放到别处」的形态，判据是**谁算显示**：

| 形态 | 谁算显示 | 线上传什么 | 复用 `services` |
| --- | --- | --- | --- |
| WebUI | 浏览器 | 语义事实（链路、配置、电平、健康） | 只复用 `Service` |
| 远程显示 | 设备（`LocalPage` + `Panel`） | 画面 | 不（画完才发） |
| 远程控制 | 设备 | 命令 | 只复用 `Service::apply` |

把 `FrameSnapshot` 发出去就是远程显示，不是 WebUI。`Descriptor`（底图、相机帧、重绘节流）与 `PageSnapshot`（例如正在输入的口令草稿）是设备内概念，任何远程形态都不该看到。

WebUI 的传输是适配层：语义 API（资源 + 动作）定一次，传输按代价爬阶梯 REST + JSON → 长轮询 → SSE → WebSocket → BLE / NFC → ESP-NOW → MQTT，够用就停。设备现有的 HTTP 服务（`core/src/provision/`，keep-alive 已实现，响应体只有定长静态内容）直接承载 REST + JSON，长轮询不用动协议；SSE 要先补 chunked 响应体。WebSocket 只在双向低延迟（远程控制）时才值，握手与每连接常驻 socket 都要算栈和镜像。BLE / NFC 是配网与配对通道（浏览器侧只有 Chromium 可达），ESP-NOW 是设备间链路、浏览器永远到不了，MQTT 适合上行遥测。局域网内的 WebUI 还要令牌（roadmap 的 `device_key`）；setup AP 的开放是配网设计，不是 UI 授权。

## 组合（三轴正交）

硬件（板 feature）⊥ 模块（模块 feature）⊥ 能力（`iot-core` trait）。模块只见能力、板只供能力。

- **组合点 = bin 板清单**：能力 move 注入、对能力泛型、同能力板共享一份；启用缺能力的模块 = 编译错，不静默
- **产品档 = feature 别名**：档定模块集、板定支持集，交点编译期裁剪校验
- **模块可配置** = feature 开关 + const 参数
- **渲染层运行时可插拔**：app 里一个零分配的中央 `Differ` 做 diff（闭包回调；先按 topic 选出各自该显示什么，只对选中项变化的 topic 通知，最后一次性写回快照），以及一个堆上的 `RenderSet` 注册表（`embedded-alloc` 堆：S3 512 KiB PSRAM，C6 64 KiB 内部 DRAM）。renderer 在启动时随各自 surface 注册，之后一起进入 render task；没有跨任务总线。每个 renderer 只声明它认领的 `Topic`（`Surface(instance)` / `Frame` / `Demand`），灯带因此从不接触约 2 KB 的帧；`Appearance` 借用帧，只有屏幕留副本
- **稳态页的光不参与呼吸**：`FrameSnapshot::steady` 来自 `PageId::steady()`（见上文），`LampRenderer` 因此在相位 0 答 `Idle`，页面自己的动画仍由 `animating()` 驱动 `TimeDriven`

## 需求模型（要什么 / 多频繁 / 多敏感）

当前固件是**板级接入验证**，不是产品；结构要能容纳后续应用把任一元件提升为主要交互或运算，且那条路径允许开到该元件的最大能力。

一个需求是三句独立的话，必须分开表达：

| 维度       | 问的问题   | 类型                                                  | 决定什么                               |
| ---------- | ---------- | ----------------------------------------------------- | -------------------------------------- |
| **模式**   | 产不产     | `Mode`：`Off` / `OnChange` / `Continuous`             | 轮询是否发生、是否按时钟产             |
| **节奏**   | 多频繁     | `Claim::period_ms`（0 = 板级节奏）                    | 轮询周期的下限                         |
| **优先级** | 谁在问     | `Priority`：`Primary` / `Secondary` / `Background`      | 本板不据此改节奏（见下），只在快照与日志里标明 |

而「怎么取」——寄存器组、FIFO、片上引擎、传输——是采集配置（`MotionConfig` 等），由驱动选定，页面永远不点名。

这条分离让「低保真 + 高成本」不可表达：页面根本没有办法要求一个更差的画面来省资源。

把两者合成一个「档位」序数，等于同时编码「要什么」和「怎么取」。降档既像降保真、又像省资源，而降档并不等于省资源。

`Priority` 在本板改不动任何节奏，这是硬件决定的。Android 的 `max_report_latency` 前提是事件按时钟采集、只在上报前压在 FIFO 里，所以「采得勤、报得晚」是两句独立的话。本板 IMU 没有 FIFO，报得晚只能换成采得稀，而那正是 `period_ms` 已经说出口的话。

所以「priority → 毫秒」那层查表在本板不可兑现：它只会把 `period_ms` 乘一个系数再取 max。

Android 那一侧给的两条形状照抄过来：

- 每个传感器的 `minDelay` 对 on-change 必须是 0、one-shot 必须是 −1。「多频繁」不是每种上报模式都问得到的。

  运动设备在本板虽然名字像 on-change，实际是持续轮询加器件内的 No-Motion 引擎产出变化事件：`Mode::OnChange` 与 `Mode::Continuous` 由 `MotionConfig::for_mode` 映射到同一个 `PolledRegisters`。

  这不是把两个模式并成一个——消费者要的东西在时间上确实不同（一个只要变化、一个按时钟），只是产出它们的接线相同。「怎么取」是器件的答案，不是需求的字段。

  `WakeDetector` 仍在配置集合里，但那是驱动按器件能力明确拒绝的一档，不是 `OnChange` 的落点。别的元件要先问一句上报模式。

- `power` 几乎总高于器件手册，且「多传感器同时开启的功耗增量不超过各自之和」是可加性假设。本板没有电流表，这条无法验证——所以固件里不带任何功耗数字，只报执行器占用。

### 仲裁取最大值，不取优先级

多个消费者声明同一个元件时，最大需求胜出，不是各自决定。

这是 Android sensor stack 的规则（"If one application requests one sensor with a maximum reporting latency of 0, all applications will receive the events from this sensor in continuous mode"），也是 ESP-IDF 的 PM lock 形状。

优先级仲裁是例外场景才需要的复杂度，不做。

### 换配置必须无损，且有独立 flush

Android 的硬约束："A sensor must be able to be reconfigured at any time using `batch()` without the loss of sensor data"。

落在本仓库：`ACCEL_LSB_PER_G` 与 `GYRO_LSB_PER_DPS` 是 `const`，全量程或 ODR 一变，core 侧换算就错，所以它们必须转运行期值。

切换瞬间「新 scale + 旧样本」的那一拍必须作废，而不是折算。

换配置前还需 `flush()` 把已积累的数据排空，否则丢掉的是有效样本。

non-wake-up 语义下允许丢样本（Android："The non wake-up sensors need to overwrite their FIFOs when they're full till the AP wakes up"），但必须有界且报出来，不能静默。

### 需求与已有的 phase 是三件事

`AudioPhase`（`Idle`/`Recording`/`Stopped`）与 `PlaybackPhase`（`Idle`/`Playing`）是 UI 语义。`AudioPhase` 的注释说它决定 "which envelope the panel draws"。

它们不是模式也不是节奏：同一模式可以有不同 UI 语义，同一 UI 语义可以有不同模式。三者用注释互相指认，不合并。

## 需求的更改与传输

### 更改：复用现有意图路径

`apply_intent` 是状态变化的唯一公开入口（输入先写进记录再翻译），内部仍只经 `apply_business` 落状态；worker 上报另走 `apply_scan`/`apply_capture`，时钟走 `tick_notice`，三个入口各自带着无法装进意图的大载荷。`apply_input` 与 `apply_business` 是流水线两半，crate 内可见——外部不能只写记录不翻译；手势改页只有 `BusinessIntent::Navigation(NavigationIntent::NextPage)`，`SplashOver` 只在启动页结束时落到 build 的页。

新增场景 = 新增一个 `BusinessIntent` 变体；运行期换节奏 = 再一个变体。`apply_business` 之后由 `settle_demand()` 重新求 `EffectiveDemand`。不新造机制。

需要规则的能力（多步转移、不变量、跨字段边沿）放自己的模块。`BusinessIntent` 按它写的记录分组为 `Navigation`/`Link`/`Network`/`Audio`/`Playback`/`Light`，`Services::apply` 是唯一的索引表；每个所有者在自己的模块里穷尽 `match` 本组变体，新增变体只在那里加一条臂，漏接是编译错。跨域动作（响一声、灯上报一句）由所有者返回 `Effects`、门面执行，所有者之间不互相调用。

一条链，五个词：`Event → Intent → Action → State → Hardware` —— `Event` 是设备听到的事（手指、按钮、worker 的回报、启动的决定），`Intent` 是设备被告知了什么，`Action` 是它决定做什么（记录自己的动作表；动词只有一个的域，意图本身就是那张表），`State` 是它现在是什么样（RAM），`Hardware` 是它的身体（屏、灯、射频、喇叭、存储）。

**硬件只由 State 驱动，State 只由 Action 改，Action 只由 Intent 触发，Intent 只由 Event 产生。**

`shared/model/` 的文件布局见上表；这里只补一条硬规矩：领域文件显式 import，禁止 `use super::*`（测试除外），预导入会让新增符号静默穿过模块边界。`DeviceManager` 与手势→意图的分派在 `state/`，`DeviceManager::apply_intent` 定义在 `state/manager/write.rs`，按当前页走 `Services::bind`，先写记录再翻译。

业务状态与页面状态分开放，且**谁写的就归谁**：`NetworkState` 只装射频报上来的东西（`link`/`mac`/`aps`/`outage_announced`），只有 `LinkIntent` 写它；`NetworkView` 只装页面正在做的事（选中的行、草稿、按键、确认框、列表滚动位置），由 `NetworkPage::apply` 响应手指的 `NetworkIntent` 与扫描落地的 `LinkIntent::Scan` 写它，不在 `DeviceState` 里。页面读 `&NetworkState`，链路从不读写页面字段，于是「工作者的报告落进页面字段」和「页面编辑改掉射频读数」都不再可能。其余领域保留 `view` 子记录（`AudioView`、`PlaybackView`、`TouchView`），语义相同：只装页面正在做的事，通知与渲染只读它。链路自身的形状也收成值：`LinkStatus.mode` 是 `NetworkMode`（`Link`/`Browsing`/`Setup { host, cancellable }`），不再用四个并列 bool 表示「页面在做什么」。

命令的出处也随这条边界走：页面域的 `apply` 直接返回 `Option<NetworkCommand>`，`Effects` 只装设备内的回声与提示，不留发件箱——从意图到工作者只有一条路径。

跨边界的写法按语义选，两个方向各两种：

- **回报**：可能引出命令的（`Link(Status)`、`Playback(Finished)`、`SplashOver`）走总线意图；纯闩锁（扫描列表、采集窗口）在系统的 `tick` 里直接写。
- **请求**：在状态里推不出来的（重复 `ScanNow`、「要删」而非「已删」、`SplashOver{setup}`）走 `Command`；能由状态边沿推出来的（静音、播放相位）在 `tick` 里 diff。

### 传输：协调者与工作者

`run_loop` 是**协调者**：它只做不阻塞的事，一帧四段——取意图、让系统各走一步、呈现、等下一帧。任何会等驱动、等 I2C、等协议栈的工作都属于**工作者**：

| 工作者         | 拥有的东西                        | executor                          | 句柄 |
| -------------- | --------------------------------- | --------------------------------- | ---- |
| 渲染层 / 摄像头 | surface、camera                   | 协作                              | `Render` / `FrameSnapshot` |
| `input_task`   | 输入器件与音频采集                | 协作（S3：第二个核）              | `Input { to, from }` |
| `control_loop` | 编解码器、DMA                     | 协作（S3：第二个核）              | `Speaker { to }`（完成回报走 intent 总线） |
| `network::serve`   | 协议栈、链路、AP 服务             | 协作（core 0 上的任务）           | `Network { commands, page, scan }` |
| `feed_task`    | 麦克风时钟                        | **Priority2 中断，独立 executor** | ⚠️ **故意无依赖，不能加** |
| `panel_service` | 面板总线（帧传输）                | **第二个 APP core**                | 无句柄：`Screen` 只持通道（`Panel::write`） |

`panel_service` 与驱动半边（`input_task`、`control_loop`）跑在第二个核上：帧传输是阻塞 DMA，采集折叠每 20 ms 忙等 7 ms，两者都会整段占住所在执行器。协调者把画完的帧投成票据等回执；驱动半边整体交给 `IoPlacement::Elsewhere` 的槽位，由核上的 `run_io_from` 取走。只有核上确实起了第二个执行器时才走这条路——没起（单核 C6、S3 无面板）时是 `IoPlacement::Inline`，同一份任务照旧跑在协调者的执行器上，所以 placement 由 `start_executors` 决定，那是有没有第二个核这件事唯一的知情处。`esp_rtos::start_second_core` 固定用 `FROM_CPU_INTR1`，因此 `feed_task` 用 `FROM_CPU_INTR2`。

**工作者由拥有其驱动的组合点启动。** 驱动半边由 `run` 用 `Wiring.spawner` 起（`IoPlacement::Inline`）或交给第二核（`Elsewhere`）。网络工作者的驱动类型只有入口认识，所以入口把 station 与 store 取走后就地建出 `Wifi::Worker(NetworkWorker)`——一个只接收 `Spawner` 的闭包；`run` 在 `run_loop` 之前执行它，既不认识射频，也不持有 worker 清单。加一个工作者仍是组合点的一行闭包，而不是多一种交给 `run` 的形状。`main` 因此不 `join` 任何工作者，`run_loop` 是它唯一 `await` 的协调者。`feed_task` 与 `panel_service` 仍由 `start_executors` 起——只有那里知道中断执行器与第二个核。`network::serve` 用本地 `Spawner` 起成任务，future 不必 `Send`：它留在 core 0 的执行器上，跨核的只有 `Io` 束。

句柄由各自的系统持有，组合根在 `register` 时交给它：一个工作者一个系统，加第二个协议只加一个文件与一行 `register`，不改共享结构体。方向名只用角色（`to` 给工作者、`from` 给协调者），peer 名留在 peer 结构体里。停帧时等三条就绪线（意图环、采集窗口、扫描列表），谁发布了谁自然叫醒循环，主循环因此不点名任何 worker。

**句柄只从入口流下去，不从模块路径摸上来。** 组合根有一份 `static RUNTIME: Runtime`（`runtime/mod.rs`），里面是跨层要用的全部存储：意图环、采集窗口、网络与播放的队列、喂数唤醒、`Readiness`。往下传的永远是具体句柄或一个内聚的旁值，不传整个 `Runtime`：`Io` 是驱动半边跨核要带走的那一束，网络工作者拿到的是 `NetCtx { bus, network, readiness, heap_free }`（`readiness` 与堆读数只有组合点知道）。意图环自己数它拒掉的意图（`IntentBus::dropped_count()`），所以那个计数没有第二根句柄。函数签名因此就是它的输入清单，测试可以自己造一份而不用抢全局单例。

旁值只在边界上产生：组合根（`Wiring`、`NetCtx`）、板层（`ScreenContext`）、任务/会话起点（`BootFacts`）。协调者、渲染、页面、状态不得就地拼包——那是被删掉的 `Frame` 的形状。判据与两条栅栏见 `records/iot/layering.md`。

`Playback`、`Network`、`CaptureWindow` 都自己拥有通道（`const fn new()`），不再是「static 通道 + 指向它的句柄」：跨核锁只在确有必要时出现，`LinkWatch` 的「上次报告」与「未送出的 MAC」因此在任务自己的栈上，用普通 `Cell`。

三种形状按语义选，不按方便选：

- **命令**走 `Inbox`（`Network.commands`）：每个工作者一条入场队列。不等的发布者用 `publish`，满时让最旧的一条让位——协调者每帧只能走这条；不能丢的发布者用 `send().await`，等到有空位才入队（手机侧那条 `Join` 就是这样，它发完就停在自己的页面上）。队列深度取一帧最大的命令数（8 条意图），所以正常路径一条不丢，丢只发生在工作者落后一整帧以上。
- **保留值**走 `PageWatch`/`Retained`（`Network.page`、`Scan` 的列表）：写入替换旧值，读走之后仍然在，所以面板与手机页面读的是同一份。启动页的结论不是保留值而是**一次性命令**：它走 `NetworkCommand::SplashOver { setup }`，与面板的其余请求同一条队列，工作者不必再看页面演进到哪一步。
- **最新窗口**走 `Input.from`：1.7 KB 的采集样本只留最新一份，覆盖而不是排队。

输入侧每个 tick 把该源本轮产生的事件全部取走（`PollEntry::poll_all` → `EventAggregator::drain`），每拍最多投递 `EVENT_BATCH = 5` 条：一次采样里两根手指同时移动会产出两个 `Drag`，只放行一个会让另一根手指永远落后一帧。`Press`/`Tap`/`Swipe`/`LongPress` 这类离散意图仍走 `send().await` 等空位，`Drag`/`Touch` 可被拒、下一次采样补上。

`NetworkCommand` 的词汇表留在 `app/src/shared/model/network.rs`，句柄只负责搬运。命令由 `DeviceManager::apply_intent()`（定义在 `state/manager/write.rs`）作为 `Command` 返回，`Schedule::process_intents` 按 `Command::type_id()` 查注册表，交给声明了 `System::takes` 的那个系统，`NetworkSystem` 取走 `Command::Network` 那一支后 `publish`，不逐个变体翻译、也不在状态里留发件箱；队列满时让最旧的一条让位，所以协调者不会为一条命令停一帧。setup 类命令带上发出时的 `SetupHost`，工作者据此丢弃上一轮留下的请求。

**每帧逻辑按边界拆成系统。** `coordinator/systems/*.rs` 一个文件一个 `System`，它持有自己那条边界的 worker 句柄与跨帧记忆（上一条播报状态、上一页、governor），共享状态与时钟由 `tick` 的参数交给它；系统之间不互相调用。共享协作者不超过两个时用显式参数，达到三个或集合在编译期说不清时才收成一个 `Context`；bus 由循环持有，每帧显式传给 `process_intents`。加一个特性 = 加一个文件 + 在组合根 `Schedule::register` 一行（句柄在那里交给系统），注册顺序就是执行顺序，改顺序就是改语义。意图那一侧也走 `System`：每条意图应用后，模型留下的 `Command` 按类型查注册表，交给声明了 `System::takes` 的那个系统；同一帧的两条命令不会合并，没人取走则在 release 里打一条 `[COORD]` 错误日志（调试构建另有断言）。

**渲染层零成本**：`FrameSnapshot` 已经是「融合所有子系统状态的单快照」，含 `page`/`motion`/`audio`/`playback`；`Appearance::Frame` 就是「状态进消费者」的通道。摄像头那条链已端到端跑通：

```
DeviceState.page → frame_snapshot() → Appearance::Frame
  → Differ::diff → ScreenRenderer::on_appearance
  → surface.release_camera()
```

所以 `Demand` 只需长在同一快照上，display 与 camera 部分零新增。

**需求是共享状态，不是消息。** 需求是幂等的聚合状态而非事件，只有最新那一份有意义。用通道装它，就得补上一格深队列、写方变化检测与失败重试——三套机制都只为「通道存不住一个值」而存在。

共享槽的形状跟着这个约束来：embassy 的 blocking mutex 是共享只读的（`RawMutex::lock(f)` 的闭包拿不到 `&mut`），所以槽内还要一层 `RefCell`。

`Intent` 是相反的情形——事件——所以仍走总线。

采集窗口是第三种形状：它是**窗口**不是事件（只有最新一份有意义），但也不是共享状态（不是谁去改公共槽，而是板子在 poll 里把驱动刚折好的一帧交出来）。于是它走一条单槽通道：`HasAudio::take_audio(&'static dyn CaptureSink)` 把目的地交给板子，`input_task` 的 poll 往里发布，`run_loop` 在 intent 之后取走。

第二个理由是宽度：`Channel<T, N>` 的每个槽按 `size_of::<T>()` 分配，槽宽由最宽的事件变体决定 —— 带 envelope 的采集事件（1.76 KB）会把 8 槽的意图环（`Runtime.bus`）定成 14,184 B 内部 DRAM，而面板每次只读最新那一帧。变体删掉之后意图环是 896 B，窗口自己 1,824 B（记在 [Wi-Fi 实现记录](@/records/iot/wifi.md)）。

顺序是顺带的：`input_task` 按接线序 poll，采集排最后，所以同一个 tick 里按下录制键闩住的是上一帧画过的那一张。

句柄由组合点造并注入（复用 `playback::share()` 的形状），满足 `input.rs` 的约定：句柄传进去，没有消费者按名字摸全局。

### feed_task 是时钟声明方，不是需求消费者

这条最容易搞反。

麦克风的 BCLK 与 WS 由 I2S TX 单元产生，RX 从属它（板模块里 RX 不接 BCLK 与 WS）。feed 的存在本身就是麦克风时钟的来源——silence 也是一种 voice，ring 永远被喂满。

所以 `feed_task` 持有一个永不释放的时钟 claim，需求不流向它。

需求只决定采集侧折叠的节奏，而折叠在 `input_task` 里。麦克风时钟因此永远在，折叠可按需降频，不需要动时钟拓扑。

代价与耦合：ring 是 `BYTES_PER_POLL * 3`（约 60 ms），最多约 60 ms 不 drain 就开始丢样本。

更要紧的是 drain 不只影响丢样本：环满之后 I2S 引擎自行停机。于是 poll 变稀不是「读数变旧」而是「时钟停摆」。

实测把 capture 批到 30 s 之后，环在 60 ms 内撞满、`capture ring 10842 B deep` 报警，90 s 里 `CaptureWatchdog` 只在第 91 秒才通过 `restart` 把链接回来一次。

所以采集路径不接受任何慢于板级 cadence 的 poll：`Es7210Rx::accepts_slower_poll()` 答 `false`。

`CaptureWatchdog` 靠「有没有进度」判停滞这件事因此不是问题——它根本没有机会看到一次故意的长间隔，间隔不存在。

### 时序

`run_loop` 写槽 → `input_task` 下一个 `INPUT_BASE_MS` tick 读到。「场景切换 → 需求生效」是一个 tick 加上 `run_loop` 被持有的时间。

#### 上限是单次最长持有，不是百分比

Liu & Layland、CFS、RT group 的利用率界约束的都是抢占式调度器跑任务的时间份额。

协作式执行器不在这个模型里：每次 poll 公平，没有任务被份额饿死。它不能做的是打断。

一次不让步的 poll 会占住整个最坏执行时间。所以要界定的是单次持有。

界定对象是整个 tick，不是其中最大的那个源。input tick 里所有源跑在同一个不让步的循环里，面板写不在其中。

实测（S3，8 个 5 秒窗口）单次 tick 17–25 ms。`MAX_SINGLE_HOLD_MS = 30`，留一个 poll 周期的余量。`[INPUT]` 每窗口打印 `worst single hold N ms (bound 30 ms)`，越界直接可见。

这组 17–25 ms 是在 **80 MHz** 下测的。产品 S3 现在由 `chip_init_for_product()` 以 240 MHz 启动（`chip_init_at(CpuClock::_240MHz)`），`chip_init()` 仍是 80 MHz 的探针入口；这组 tick 没有在 240 MHz 下重跑，240 下只会更短，但上面的数字仍应按 80 MHz 的量级读。240 MHz 的验证与代价见 [运行期改 CPU 频率](@/records/iot/clock.md)。

总量不是风险项：Ambient 花掉 94.8% 且一切正常。一次 25 ms 的阻塞才是。播放喂数跑在独立的高优先级 executor 上、由 DMA 驱动，碰不到这种阻塞。

#### 两种配置拒绝实现，返回 `UnsupportedConfig` 而不是写一个像样的字节

| 配置           | 缺什么                                                                                                                                 |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `WakeDetector` | 单加速度计同时改变 CTRL7 功耗域和「用哪张 ODR 表」；datasheet 给了两张表的频率但没给各自索引，引擎窗口按该索引换算，猜错就正好错这么多 |
| `Fifo`         | FIFO 寄存器从 `0x0B` 起，与本驱动写运动标定的地址重叠——重叠正是 CTRL9 命令协议存在的原因，所以这不是一次寄存器写而是排序问题           |

每一条都是「打开寄存器表补一个编码」的可达路径。

`Off` 不是配置而是模式，由消费者不轮询兑现。

驱动的具体接线与寄存器细节在 [motion.md](@/development/iot/motion.md)。

#### 配置器、仲裁与接线

配置器是模式与寄存器之间唯一的一道翻译。`Configurator::configuration_for` 把需求变成各器件「必须被配置成什么」，映射在 core 的 `MotionConfig::for_mode`，器件能力由板子在装配点交给 `Configurator::new`。

分工是硬的：哪个模式要哪个配置是产品的话，这块硅片上有什么是板子的话。

拒绝是返回类型的一部分。走不进去时返回 `Unreachable { key, mode, config }`，三者齐全，调用方拿不到「悄悄降级」的选项。

拒绝会被写进需求：被点名的设备从「设备承诺」里去掉（`EffectiveDemand::without`），下游读到的需求不再声称一件器件做不到的事。

`Off` 不在此列，它由「不采集」兑现，返回 `None` 而非 `Err`。交互通道永不受影响。

图上 Logic 分别调 Render 与 Configurator。但器件在 `input_task` 里，`run_loop` 够不着，所以配置器也放在持有器件的一侧：它要板子声明的能力来翻译模式，拒绝也该对着真正拒绝的那个器件报告。

配置器随需求一起兑现，`[DEMAND]` 报的是这次排程的结果。

需求按设备索引，不按字段枚举。`EffectiveDemand` 是 `BTreeMap<DemandKey, Claim>`，键是「器件类型 + 第几个」，所以 `claim` 与 `Override::apply` 全是查表，新增一个传感器不需要改动需求机械的任何一行。

两个同型号器件可以分开提需求（`DemandKey::instance(Audio, 0)` 与 `…, 1`）。

设备清单由板子给。`PageId::demand(&self, keys)` 遍历板子声明的设备表调 `claim_for`。这个 match 是穷尽的，所以页面不可能漏掉一个设备然后继承一份没人选过的需求。

板子没声明的设备不出现在需求里，读作 `Off`，不需要再用四个布尔开关夹一次。

需求状态是被持有的，不是现算的。`DeviceManager` 持有一份 `EffectiveDemand`，与它来自的页一起放在 `DemandPolicy` 里。派生的唯一出处是 `DemandPolicy::settle`（`apply_business` 末尾经 `settle_demand()` 调用）：缓存键只有页，`keys` 与 `overrides` 自 boot 起不再变，所以页没换就不重算。

下游读的始终是这份提案。输入任务遇到某个器件无法兑现时只记一条告警，并让该器件停在原来的配置；提案本身不回写。当前树里 `MotionConfig::for_mode` 只返回 `PolledRegisters`，所有接线动件的驱动都支持它，所以这条告警路径不可达。

仲裁由 `resolve_speed` 在采集设备上取最高要求，测试断言它按设备取最高速，而不是整表一个常量。

接线：

| 消费者        | 怎么拿到需求                                                                                                  |
| ------------- | ------------------------------------------------------------------------------------------------------------- |
| `input_task`  | 共享需求，每个 tick 在临界区内借用一次，按 `PollEntry.key()` 排程（cadence 还要问驱动 `accepts_slower_poll`） |
| `run_loop` | 写入方。页面就是产品的声明，所以写在这里而不是让每个消费者各自推导                                            |
| 摄像头        | `Appearance::Frame`（已有通道），门控条件从「页面变了」改成「需求不再要求画面」                                 |
| `feed_task`   | 刻意不注入。它的存在本身供给麦克风时钟，所以它是音频路径时钟的来源而不是它需求的消费者                        |

轮询节奏只向外走：板级 cadence 是抖动窗口的量度下限，需求没有资格改短。地板与当前节奏分两个字段（`board_cadence_ms` 与 `cadence_ms`），否则一次拉长就再也收不回来。

有些源连向外都不许。`accepts_slower_poll()` 默认 `true`，`Es7210Rx` 答 `false`。采集环只有 `sample()` 会排空，被批到 30 s 就是环撞满、I2S 引擎停机。把这个判断交给驱动，而不是让 core 猜哪个外设需要它。

audio 的 codec 那一半仍未接。`Off` 在消费端已接（`set_collecting` 把 I2S 链 park 掉），但 ES7210 的寄存器级模拟电源关闭位没有落进代码。板上没有引脚级 mic 电源控制，而只关 ADC 不降低折叠成本（DMA 照跑，折叠零串与真数据同价），那只会造出「低保真 + 高成本」这个象限。

#### 探针报什么、不报什么

`aec-probe` 是唯一不接外设的探针：回声消除是两路数据的算术，bring-up 只会往测量里加噪声。它报的是「CPU 代价只与抽头数和时钟有关」，因而可以先把时间预留进预算再接线。报首/均/最坏 tick、占 20 ms 的比例，以及延迟搜索、滤波、计量器分开计时——三者的代价不同，能换的东西也不同：延迟搜索是一个常数换精度，抽头是精度同时换另外两者。

它的 tick 是连续跑的而不是按 20 ms 调度，所以读到的是**下限**而不是带载值。房间缓冲区放在堆上：10 KB 的局部量会占掉 main 任务的栈，而栈正是本文记账的那份共享资源。

`echo-probe` 正好是它的对照：**唯一接外设的 AEC 探针**，因为它要回答的恰恰是 `aec-probe` 答不了的那件事——接线对不对。扬声器在放、麦克风在听、取消器喂两路，走产品自己的喂入与采集轮询。报 ERLE、delay、抽头峰值、配对数、参考日志深度与最坏持有时间。

**这些数字必须捆在一起读**：配对数为零时 ERLE 恒为 0（日志不动就不测量），而参考日志 lap 过时 ERLE 可以看着正常。两者输出的是同一个零。`EchoReport` 把它们捆在一起就是为了不让调用方拆开读。

它还报一条 `aec-probe` 结构上不可能报的东西：**延迟锁在哪**。板上的延迟搜索曾在一个 64 样本窗口里乱跳（`None → 16 → 50 → 6`），而滤波器被拖着每 256 ms 换一次窗，64 个抽头没有一次朝同一个房间收敛。加上锁之后两次实测 delay 全程只动一次。至于消除量本身，`aec-probe` 和 `echo-probe` 都还没有在板上出过数——见[回声消除实现记录](@/records/iot/aec.md)。

#### 回声消除这一层的接线

`Canceller` 是两路进一路出，而把这两路配成「同一个瞬间」不是它的问题。两个 DMA 共用一个时钟、各自按自己的节奏排空，参考信号还要被发射环的跑道拖住。所以配对这一层是独立的：

| 类型           | 归哪 | 是什么                                                                               |
| -------------- | ---- | ------------------------------------------------------------------------------------ |
| `ReferenceLog` | core | 按放音游标而不是写入指针保存参考样本，并报 lap —— 配对读到的必须是已经播出去的那一段 |
| `Decimator`    | core | 抽取器带**相位**，两路各自报自己的，供配对方对齐而不是各猜各的                       |
| `EchoStage`    | core | 采集 × 参考的配对与抵消，产出 `EchoOutcome`                                          |

相位是这套里唯一会静默错一抽的东西：发端比收端先 arm 时两边取 0，**换一块先 arm 麦克风的板子必须同时把偏移报给两边**，差的部分由延迟搜索兜，那不是自动的。

`demand-probe` 遍历运动设备的（配置 × 节奏）矩阵，每个格子先 settle 再测量，报三个数：驱动自己计数器给出的总线事务/秒、窗口内 poll 的执行器占用墙钟占比、以及测量的 poll 数（好让前两个互相校验）。

它报不了电流——那需要板子上接表，别人的数据手册数字不是对这块板的测量。所以固件里不报电流。

驱动没有寄存器编码的格子按「不可达」打印而不是跳过，日志对「没测到的」和「测到的」一样响。

配套两件事：`SUPPORTED_CONFIGS` 只列驱动能把芯片配成的样子（有测试断言它与 `config_for` 一致）；`Board::new_motion_only()` 提供隔离 bring-up——一个在别的设备还在跑的格子里测出来的东西是关于错的东西的证据。

各设备的实测数字与它们的口径限制，在[实现记录索引](@/records/iot/_index.md)。

#### 执行器占用的数字不可相加

同一批协作执行器，谁醒着谁拿时间：把 motion 批到 30 s 后 input 侧从 49.5% 降到 35.5%，面板侧同时从 45.3% 升到 58.5%。

面板那一侧是残差不是增量。把它和被轮询设备的数字相加，等于把同一批微秒数数两遍，加出来会超过一个核的 100%。

同理，设备独自在板上跑测到的数（探针）与两个整机构建相减测到的数不是同一个口径。混在一起相加得到的和是下界不是总量（三个设备隔离相加读作 43.2%，同一构建实测 51.0%）。

要贵的那一侧：设备的代价随旁侧运行的东西上升，便宜的读数正是最先失效的那个。

由此有一条测量方法上的硬规矩：**要问节流对最坏阻塞的作用，测法是只改节奏不动模式。** 两者同时改时，省下的部分可能来自关掉整个设备而不是来自节流。

## 页面：路由表 + 导航栈

`app/src/shared/route/mod.rs` 只写一份路由表：枚举 `PageId` 是目的地，每个目的地一个 `static <PAGE>: Route`（名字、能力、claim、是否可启动、生存期），`route()` 是穷尽 `match`——新增变体没有路由就编译不过。`ALL` 是全部的次序（wire code 就是它的下标），按钮也照它走：从本页之后取第一个本板支持、又不是瞬态的页，走到末尾回绕，于是「表里有」与「按得出来」不会成为两份会漂移的清单。页面自己的答案在另一处、也只有一处：`app/src/services/mod.rs` 里 `Services::apply` 的业务分派表与一张 `descriptor(page)` 表（画法、底色、是否出帧、重绘节奏），业务在 `Service`，本地手势/视图/画/生命周期在 `LocalPage`。`crates/tool/tests/page_table.rs` 守着 `ALL` 漏页，以及每个页面的类型、分派分支与 `app/src/services/<path>` 下的视图模块（`match` 本身由编译器穷尽检查）。

`app/src/shared/route/nav.rs` 的 `Navigator` 是页面位置的唯一来源：固定容量（`MAX_DEPTH`）的栈，`current`/`root`/`entries` 读它，`go` 原地换页（按钮的循环是 tab，不堆历史），`push` 把瞬态压在当前页之上，`pop` 弹掉栈顶且根永远保留。`DeviceState::page()` 就是 `nav.current()`，所以「当前页」只有一处可写。启动时根是 `Boot::boot_page` 判过能力的落地页，splash 以 `push` 压上去；`SplashOver` 弹栈，按住请求过 setup 则 `go(Network)`。`next_page` 从本页之后按 `ALL` 走一圈、跳过瞬态、取第一个本板能力支持的页、走到末尾回绕；瞬态停在原页直到 `timeout_ms` 到期（`input.rs` 的定时器直接读这条路由属性）。

能力判定是每条路由自己的一列（`Route::needs`，`is_backed` 只做查表），问的是同一个 `Backed` 记录——运动、音频、回放、相机、链路五项接线事实在组合点一次写进 `Boot` 的 `backed` 字段。前四个来自板子的 `take_*`，链路来自入口点（station 要在调度器起来之后才启动，`run` 拿到板子时射频已经被取走）——所以入口交进来的是 `Wifi`（`Absent` / `Unattended` / `Worker`），`run` 在组合 `Backed` 时用 `wifi.wired()` 落成那个 `bool`：是**接线事实的参数**，不是 `run` 里回头去问板子。表面同理：`HasSurface::take_surfaces()` 交出的每个 `WiredSurface` 自带角色（`Pages` 或 `Lamp`），入口点取出后连同 `clock`/`feed`/`wifi` 一起交给 `run`，`render::wire` 在同一趟里既建 renderer 又落下 `surface_roles`——没有哪个表面会被回头问「你画不画页面」，空槽位也不在句柄里出现，渲染器数超过槽位数只有那一处断言。

这条规约是修一个缺陷写下来的。

`next_page` 曾是 18 个手写跳转分支，`Camera` 的离开分支返回「第一个本板能显示的页」（`Attitude`）而非起点，于是 `Ambient` 被永久跳过：实测全能力下走一圈是 `[Attitude, Attitude, Audio, Speaker, Camera]`，`Ambient` 消失、`Attitude` 出现两次。

跳着写会漏掉闭环的那一页，走一张表再回绕不会。

`iot-app` 里有一条测试遍历能力组合 × 起点断言这一点（`every_page_the_board_can_back_is_shown_on_the_way_round`），把旧的 `Camera → Attitude` 放回去它会失败并打出上面那串。

### 页面缺席 = 没接线；件坏了 = 页面在

`Backed` 回答的是**板子有没有把这件东西焊出来**，在组合点一次决定、运行期不变：S3 的 `take_*` 总是交出扬声器、麦克风和相机的驱动对象（探针失败也给），C6 没有这条产品路径就不交。于是页面消失只有一种解释：这块板没接这件东西。

器件答不答是另一回事，归**运行期健康**：回放/采集的 `Health::Down(reason)`、相机的 `FrameSource::fault()` 都落在页面上，写成 `NO OUT` / `NO IN` / `NO CAM` 加一行原因；页面不消失，指纹与计数只是停更。

恢复发生在**用它的地方**，不在启动循环里：扬声器在第一次轻点时 `ensure_started()`（两个地址都试、记住应答的那个），麦克风在轮询里最多每 1 s 重试并丢弃旧样本，相机在页面打开期间每 5 s 重试 `receive` 与传感器 bring-up。重试有界、有间隔，失败只把原因写进健康，不阻塞面板，也不复位。

## 日志

只用 `log` façade（`log::info!` 等）；输出通道由家族 `iot-chip-esp` 初始化（esp 为 `esp_println::logger`）；业务代码禁止 `println!` / `esp_println::println!`

一个日志行要能被相信的前提是它报的是**正在运行的那个配置**，不是被请求的那一个。`[DISPLAY]` 的 `asked` 曾报页面自己的需求而非 override 生效后的，因为快照从 state 取需求而 override 挂在 manager 上。一个报出无人运行之配置的日志比没有日志更糟。

## 新增板

1. `bsp-esp/` 增板模块（Board + HasXxx traits）+ feature 门控；选框上的芯片先在 `components/` 建实际元件（已有则直接复用，并为新元件声明独立 feature），板 feature 聚合元件清单，板模块内装配
2. 板模块导出该板的 `SETUP_HOST`（芯片内存不同再加 `HEAP_BYTES`）；`app/src/main.rs` 按芯片分支读取，feed 落点已按芯片分好
3. app feature 透传 `iot-bsp-esp/<board>`
4. `reusable-iot-build.yml` 的 `platforms` 默认 JSON 与 `iot-dev-release.yml` 的 `platforms` 输入里加该板（xtensa 板需标 `use-xtensa-toolchain: true`，经 `build-s3` 任务构建：CI/发布用 espup 原生 `esp` 1.98.1.0 工具链，macOS Intel 本地用同版本 Docker 镜像）
5. 验证：`cargo build -p iot-app --bin vanling --target riscv32imac-unknown-none-elf --no-default-features --features <board>`

## 新增芯片家族

esp 家族仓固定为 `iot-chip-esp`/`iot-bsp-esp`；新增非 esp 家族：

1. 新增 `iot-chip-<family>`/`iot-bsp-<family>` crate
2. app 家族别名 feature 门控该家族 entry deps；`app/src/main.rs` 的 `start_executors` 加该家族的板级 bring-up 分支（`#[cfg(feature)]`）
3. 工具链目标 + `rust-toolchain.toml` + `reusable-iot-build.yml` 的 `platforms`（`target`/`use-xtensa-toolchain`）与 `iot-dev-release.yml` 的 `platforms` 输入
4. `moon`/`lefthook` 的 `--workspace --target` 改按固件作用域；工具链目标经 `CARGO_ESP_TARGET` 传入 moon 任务（`.envrc`/CI env 与 workflow inputs 提供，moon 任务以 `- '$CARGO_ESP_TARGET'` 收入 inputs 保证跨芯片缓存隔离）

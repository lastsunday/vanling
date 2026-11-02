+++
title = "应用层分层评审的落地记录"
weight = 90
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 应用层分层评审的落地记录

`apps/iot/crates/app` 的一次分层评审：通用机制回到 `iot-core`，页面与功能知识回到各自的模块。这里只记判断与阻碍，不重复代码。

## 已归位

- 呼吸灯的取色与背光包络是 `Breath` 的属性，落在 `core::state::light`；渲染器只问 `rgb()` / `backlight()`，不再各自复算一遍色相与亮度。
- `LampRenderer` / `ScreenRenderer` 与 renderer 注册表落在 `core::render`：`Renderer` 接口本来就在 core，`iot-ui` 才是页面层，所以渲染器不放进 UI。
- 播放命令（`Play` / `SetMuted`）与边沿判定（`mute_command` / `play_command`）归 `PlaybackState`；app 的 dispatch 只负责排队与满队重试。
- `InputSource::name`、`PollEntry::name`、`DemandKind::name` 由驱动与需求层回答，app 不再维护一张按位置索引的名字表。
- 页面整屏铺底与「画面来自摄像头」是页面属性（`DisplayPage::ground` / `draws_frames`），渲染器直接问页面，板级与设备都不再各写一份 `matches!`。
- 在线判据 `AddressWatch` 与 `ADDRESS_LOST_WINDOWS` 落在 `core::drivers::network`，链路循环只喂观察值。
- 扫频参数属于会议室探针，留在 `bench/probe/echo_room`；产品音色表不再生成它，探针用 `audio::start_source` 播放自己构造的激励。
- 构建期网络名的读取只有一处（`boot::build_wifi_credentials`），bench 例外由产品侧的 `boot::wifi_credentials` 决定；探针要的永远是构建时给的名字，所以直接读前者。
- 链路状态投递与去重归 `network::publish_link`（`LinkWatch`）；页面观察 `PageWatch` 按页参数化并注入 `network`；扫描列表只由 `NETWORK.scan` 持有，`scan` 的每轮与 `store::remark_and_publish` 都往这一处发布，所以面板与手机看到的是同一份标记。
- 产品堆大小归芯片层（`iot-chip-esp::PRODUCT_HEAP_BYTES`），setup 起始 front 归板层（`iot-bsp-esp::FRONT`），`main.rs` 只读取。
- 链路业务状态与网络页状态拆成两个记录：`NetworkState` 只由 `LinkIntent` 写，`NetworkPage` 只由手指的 `NetworkPageIntent` 写，页面读 `&NetworkState` 而链路从不碰页面字段；`Effects` 不再留发件箱，页面域直接返回 `NetworkCommand`，从意图到工作者只有一条路径。
- 网络工作者不再经 `WorkerStart` 交给 `run`：驱动类型只有入口认识，就在入口与驱动半边并列 `spawn`，`run` 不再持 worker 清单。启动页结论改走 `NetworkCommand::SplashOver { setup }`，`PageWatch` 只剩页面级别，不再携带启动页边沿。

## 阻碍：probe 的 board `#[cfg]` 没有下移

`bench/probe` 仍以 `#[cfg(feature = "...")]` 选择探针入口。把这段门控移进 bsp 不可行：`echo_room` 依赖 `crate::audio` 的产品播放与采集实现，bsp 不能反向依赖 app。`bench/probe/mod.rs` 作为模块 owner 单点门控，本身符合软 feature 的规则，故保持现状。

## 灯与屏幕的接口拆开（第二轮）

第一轮把 `page_ground` / `page_draws_frames` 从板级收进 `Screen`，但接口仍是「灯」：屏幕能力长在 `RgbLight` 上，没有屏幕的板子只能写空实现。这一轮按能力拆开设备接口。

- `RgbLight` 只剩 `set_rgb(Rgb)`；`PageSurface: RgbLight + DiagnosticsSink + CameraTarget` 才有背光、repaint 节流与相机，只有面板实现。
- `page_ground` / `page_draws_frames` / `CameraTarget::takes_frames` 删除。铺底与「画面来自相机」早已是 `DisplayPage` 的属性，角色也已由 `WiredSurface::Pages/Lamp` 在接线时给出；渲染层再问一遍设备，等于把同一个答案抄两遍。
- `WiredSurface` 改为 `Pages(Box<dyn PageSurface>) | Lamp(Box<dyn RgbLight>)`，`HasSurface` 不再带关联类型；WS2812 随之删掉 `DiagnosticsSink`、`CameraTarget`、背光与 repaint 四个空实现。相机由 `surface::wired()` 交给第一个 `Pages`，不再问 `takes_frames`。
- renderer 拆 `LampRenderer` / `ScreenRenderer`，共享 `render::colour::ColourDriver`（呼吸、常亮、关、显示需求门控）。注册表仍是 `Box<dyn Renderer>`，晚注册逻辑不变。
- `Rgb` 与色彩数学迁到 `core::paint`；`MAX_LIGHTS` 归 `state`（它量的是状态槽位，不是驱动事实）。`ui` 与 `stamp` 不再从 `drivers` 取颜色。
- `Fill` 与 `Fill::VerticalGradient` 删除：生产路径只发 `Uniform`，`Fill` 的唯一作用是在 `FrameOwner` 里多记一位。现在 `FrameOwner` 只记 `Rgb`，`Canvas::ground()` 返回 `Option<Rgb>`，`stamp::fill_surface` 去掉 fill 参数。
- 行为保真：相机 owner 的释放顺序、`Activity::TimeDriven` 的返回、`Mode::Off` 分支、`Diagnostics` 整份 clone 的位置、先 `accepts_color` 再 `paint` 的顺序都逐字保留。
- 镜像：C6 845,440/1,048,576 B（80.63%，比拆分前 +2,448 B），余量约 198 KiB。

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
- `LampRenderer` / `ScreenRenderer` 与 renderer 注册表落在 `app::render`：`core::ui` 才是像素与字形层，所以渲染器不放进 UI；`Renderer` 接口随后也随实现一起收进 app。
- 播放命令（`Play` / `SetMuted`）与边沿判定（`mute_command` / `play_command`）归 `PlaybackState`；app 的 dispatch 只负责排队与满队重试。
- `InputSource::name`、`PollEntry::name`、`DemandKind::name` 由驱动与需求层回答，app 不再维护一张按位置索引的名字表。
- 页面整屏铺底、是否出帧与重绘步长是渲染策略（`render::screen::policy`），渲染器按页问策略，板级与设备都不再各写一份 `matches!`；页面表（`shared::route`）只留名字、能力、claim、可启动与生存期。
- 在线判据 `AddressWatch` 与 `ADDRESS_LOST_WINDOWS` 落在 `core::drivers::network`，链路循环只喂观察值。
- 扫频参数属于会议室探针，留在 `bench/probe/echo_room`；产品音色表不再生成它，探针用 `audio::start_source` 播放自己构造的激励。
- 构建期网络名的读取只有一处（`boot::build_wifi_credentials`），bench 例外由产品侧的 `boot::wifi_credentials` 决定；探针要的永远是构建时给的名字，所以直接读前者。
- 链路状态投递与去重归 `LinkWatch::deliver`；页面观察 `PageWatch` 按页参数化并注入 `network`；扫描列表只由 `Network.scan` 持有，`scan` 的每轮与 `store::remark_and_publish` 都往这一处发布，所以面板与手机看到的是同一份标记。
- 产品堆大小归芯片层（`iot-chip-esp::PRODUCT_HEAP_BYTES`），setup 起始 front 归板层（`iot-bsp-esp::FRONT`），`main.rs` 只读取。
- 链路业务状态与网络页状态拆成两个记录：`NetworkState` 只由 `LinkIntent` 写，`NetworkView` 只由手指的 `NetworkPageIntent` 写，页面读 `&NetworkState` 而链路从不碰页面字段；`Effects` 不再留发件箱，页面域直接返回 `NetworkCommand`，从意图到工作者只有一条路径。
- 网络工作者不再经 `WorkerStart` 交给 `run`：驱动类型只有入口认识，就在入口与驱动半边并列 `spawn`，`run` 不再持 worker 清单。启动页结论改走 `NetworkCommand::SplashOver { setup }`，`PageWatch` 只剩页面级别，不再携带启动页边沿。
- `state` 与 `intent` 是**有意双向**：`state` 定义并持有意图的载荷类型（`Confirm`、`PageId`、`AudioPhase` 等），而把手指翻成 `BusinessIntent` 的 `translate` 现在在 `controller`。两者都不是渲染/快照层，结构测试只禁 `state → {render, diagnostics}` 与 `intent → {page, render, diagnostics}`；审计时不要把这条当成新破口。
- 协调者的网络机制收进 `coordinator/systems/network.rs`：`NetworkOutbox` 持有没塞进队列的那条命令、`NetworkMirror` 发布页面级别并记录 front/lamp 边沿、回收扫描列表；`run_loop` 只留意图编排。`core` 侧则把 splash 判决并回 `apply_navigation` 的返回值，`take_splash_end` 侧信道删除，从意图到工作者确实只剩一条路径。
- 协调者按边界拆成系统：`coordinator/systems/*.rs` 一个文件一个 `System`（`coordinator::systems::Schedule` 的声明顺序就是执行顺序），只持跨帧记忆；状态由帧函数的参数交给它，工作者句柄在注册时由组合根交给它。意图 → 状态 → 命令的配方收进 `DeviceManager::apply_intent`，测试不再各抄一份；需求槽的锁回到 `workers::Input::publish`。同日实测（S3 产品镜像）：`_stack_end` 比 HEAD 低 96 B，`.text` 少 2,188 B，`.data`/`.bss` 不变。

## 阻碍：probe 的 board `#[cfg]` 没有下移

`bench/probe` 仍以 `#[cfg(feature = "...")]` 选择探针入口。把这段门控移进 bsp 不可行：`echo_room` 依赖 `crate::audio` 的产品播放与采集实现，bsp 不能反向依赖 app。`bench/probe/mod.rs` 作为模块 owner 单点门控，本身符合软 feature 的规则，故保持现状。

## 灯与屏幕的接口拆开（第二轮）

第一轮把 `page_ground` / `page_draws_frames` 从板级收进 `Screen`，但接口仍是「灯」：屏幕能力长在 `RgbLight` 上，没有屏幕的板子只能写空实现。这一轮按能力拆开设备接口。

- `RgbLight` 只剩 `set_rgb(Rgb)`；`PageSurface: RgbLight + DiagnosticsSink + CameraTarget` 才有背光、repaint 节流与相机，只有面板实现。
- `page_ground` / `page_draws_frames` / `CameraTarget::takes_frames` 删除。铺底与「画面来自相机」早已由页面回答（现在在 `render::screen::policy`），角色也已由 `WiredSurface::Pages/Lamp` 在接线时给出；渲染层再问一遍设备，等于把同一个答案抄两遍。
- `WiredSurface` 改为 `Pages(Box<dyn PageSurface>) | Lamp(Box<dyn RgbLight>)`，`HasSurface` 不再带关联类型；WS2812 随之删掉 `DiagnosticsSink`、`CameraTarget`、背光与 repaint 四个空实现。相机由 `surface::wired()` 交给第一个 `Pages`，不再问 `takes_frames`。
- renderer 拆 `LampRenderer` / `ScreenRenderer`，共享 `render::colour::ColourDriver`（呼吸、常亮、关、显示需求门控）。注册表仍是 `Box<dyn Renderer>`，晚注册逻辑不变。
- `Rgb` 与色彩数学迁到 `core::paint`；`MAX_LIGHTS` 归 `state`（它量的是状态槽位，不是驱动事实）。`ui` 与 `stamp` 不再从 `drivers` 取颜色。
- `Fill` 与 `Fill::VerticalGradient` 删除：生产路径只发 `Uniform`，`Fill` 的唯一作用是在 `FrameOwner` 里多记一位。现在 `FrameOwner` 只记 `Rgb`，`Canvas::ground()` 返回 `Option<Rgb>`，`stamp::fill_surface` 去掉 fill 参数。
- 行为保真：相机 owner 的释放顺序、`Activity::TimeDriven` 的返回、`Mode::Off` 分支、`Diagnostics` 整份 clone 的位置、先 `accepts_color` 再 `paint` 的顺序都逐字保留。
- 镜像：C6 845,440/1,048,576 B（80.63%，比拆分前 +2,448 B），余量约 198 KiB。

## 隐式句柄清零（第三轮）

要求是「每个函数的输入输出都在签名里」。这一轮把 `INTENT_BUS`、`WAKE`、`NETWORK`、`PLAYBACK`、`READINESS`、`DROPPED_INTENTS`、`HEAP_FREE`、`PAGE_READ`、`LINK` 这些模块路径上的单例全部删掉，改成入口造、参数传。

- 组合根各有一份 `static RUNTIME: Runtime`（产品入口与 `host-smoke` 各一），`Wiring` 拿 `&'static Runtime`，`Io` 拿跨核那一束。往下只传具体句柄或一个内聚旁值：网络工作者收 `NetCtx { bus, network, readiness, heap_free }`，一帧收到 `bus`、`capture`、`network`、`dropped`、`readiness` 五个句柄（当时收成 `Frame`，第四轮又拆回具名参数，见下）。**没有** `runtime()` 取用函数——那是换了名字的服务定位器。
- `Playback`、`Network`、`CaptureWindow` 自己拥有通道（`const fn new()`），不再「static 通道 + 指向它的句柄」。跨核锁因此只在 `Shared`（喇叭）一处留下。
- `LinkWatch` 的「上次报告」与「未送出的 MAC」搬到 `serve` 的栈上，用普通 `Cell`；`PageRead` 改成每次挂上 setup AP 时新建的信号，上一轮遗留的读不再让新 AP 的空闲超时立刻触发。
- 纯判定改成返回值：`resolve_join` 返回 `#[must_use] enum Join::{Found, Missing}`，由调用方决定发什么，函数自己不再从内部往总线上塞。
- 满环策略收在一处：`input::hand_over` 用 `send().await` 等空位，只有它能保证「问一次就一定要问出去」的那一份不丢；状态投递仍是 `try_send` + 保留值重试（丢了下一次还能补）。新增宿主测 `a_full_ring_delays_a_hand_over_instead_of_losing_it`（第四轮改为 `IntentBus::send`，测试名同步）。
- 该测试顺带逼出一个真 bug：`DroppedIntents::note` 用 `then_some(count + 1)`，`count == u32::MAX` 时先溢出再判断，已改回 `then(|| count + 1)`（第四轮这段逻辑随计数并入 `IntentBus::try_send`）。
- 行为保真：帧内顺序、三个停帧就绪线、wifi 软 feature 门控、C6 无产品路径都不变；宿主场景改为 `send().await` 逐条入环，`dropped.count() == 0` 仍是断言。
- `[measured]` S3 产品镜像（`nm`/`size -A`，同一份无凭据构建）：主栈 `_stack_start − _stack_end` = 86,536 B（HEAD 87,024 B，−488 B），`.bss` 240,840 B（−3,128 B），`.data` 23,840 B（+3,496 B），`.text` 919,113 B（+1,100 B），三项合计 RAM 净 −120 B。镜像 1,136,128/6,291,456 B，C6 954,048/1,048,576 B（90.99%）。两块板实测启动：S3 连上已存网络并拿到地址，看门狗报 healthy，栈峰值 18,948 B / 86,536 B。

## 拒绝计数回到环（第四轮）

第三轮把所有单例收到签名里，代价是一帧收了个 `Frame { bus, capture, network, dropped, readiness }`：五个字段没有共同概念，只因为 `run_loop` 一起用就被打包，于是 `run_loop` 的签名不再说明它到底读什么。查到的惯例一致——clippy 的 `too_many_arguments` 门槛是 7（第 8 个才报，trait impl 被豁免），参数对象只在值来自不同对象且确实构成一个概念时才划算，一包无关句柄会**掩盖**依赖。这一轮按「减少真实依赖」而不是「重新打包」来做。

- `Frame` 删除，`run_loop` 收七个具名参数：`bus`、`capture`、`scan`、`readiness`、渲染器、模型、系统表。停帧只等 `scan.ready()`，所以传 `&'static Scan` 而不是整个 `Network`。七个正好卡在 clippy 的默认门槛内，没有新增 `#[allow]`：再多一个依赖，编译就会拒绝。
- 计数并入 `IntentBus`：拒绝本来就发生在环上，`try_send` 满时自己记，环不可能默默拒掉一条；`dropped_count()` 在 `Schedule::process_intents` 的 drain 之前折进快照，一帧读一次。`DroppedIntents` 类型、`input::forward`、`input::hand_over` 随之删除——等空位就是 `IntentBus::send`，`input_task` 从六个参数降到五个，`[INPUT]` 窗口日志里的掉包数改成总量差值，只剩一份真相。
- 新栅栏 `the_refusal_count_is_read_where_the_ring_is_drained`：`dropped_count` 的读者只允许是环本身、drain 那次折叠与宿主场景的断言；多一个读者，这个数就有两个含义。
- 行为保真：帧内顺序、三条停帧就绪线、wifi 软 feature 门控、C6 无产品路径都不变，宿主场景的 `dropped_count() == 0` 仍是断言。
- `[measured]` S3 产品镜像（`nm`/`size -A`，同一份无凭据构建）：主栈 `_stack_start − _stack_end` = 86,344 B（上一轮 86,536 B，−192 B；HEAD 87,024 B，−680 B），`.bss` 241,032 B（+192 B），`.data` 23,840 B（不变），`.text` 919,457 B（+344 B）——栈与 `.bss` 在同一处让位，RAM 合计 351,216 B 与上一轮相同。应用分区 1,136,464/6,291,456 B（18.06%），C6 953,968/1,048,576 B（90.98%，−80 B）。两块板烧录后实测：S3 启动日志打 `[DISPLAY] … main stack 86344 B`，看门狗报 healthy，连上已存网络并拿到地址，栈峰值 18,932 B / 86,344 B；C6 走离线路径（无已存网络），灯呼吸，`0 crashed boot(s)`，看门狗同样 healthy。


## 旁值何时合法（第五轮）

第三轮引入 `NetCtx`，第四轮删掉 `Frame`：两轮都在处理「一束值该不该打包」，却都没写下判据，于是每处都要重新争一遍。这一轮先把判据定死，再拿它扫全仓，最后把重复的管道收掉。

七条判据，全中才算旁值，缺一条就是「参数们」换了名字：

1. 有唯一所有者，且只有一处构造；
2. 字段由它所跨的那条边界先验确定，不是某次调用凑出来的；
3. 生存期长于一次签名——要跨任务、跨帧，或过界之后还得在；
4. 内容来自接收方或所有者，不是调用方当场才有的东西；
5. 名字是领域名词，不是 `Args` / `Params` / `Frame` 这类容器名；
6. 真的减少依赖，或跨过签名本身跨不过的边界；
7. 不含「可选、很少用」的字段——这类字段本身就说明这不是一个概念。

出处彼此一致：Fowler 的 *Data Clump*（删掉一个字段再读一遍签名）与 *Flag Argument*，J2EE 的 Context Object，Go `context` 的三条用法（入口创建、只向下传、必需的值走参数而不是 context），ISP 的「客户端不该依赖它不用的东西」，以及 connascence 从位置降到名字。`Frame` 恰恰相反：它用名字把位置关系藏起来，反而多出一层「字段含义」的隐含耦合。clippy 的 `too_many_arguments` 门槛是 7，所以「参数太多」本身不构成理由。

按这七条新拆出来的是 `BootFacts`：

- 安全模式的 `run` 同时喂两条会话（先 station，失败再开 AP）。八项页面事实里四项开机后不再变（`mac`、`crashed_boots`、`saved_networks`、`heap_free`），四项一直在变（`uptime_ms`、`phase`、`address`、`station`）。前四项由「这次开机」这条边界先验确定，且 `run` 里一次全知道 → 单点构造、两处复用；于是 `station_session` 8 → 4 参，`access_point` 5 → 2 参，`Report` 8 → 5 字段，`facts()` 读 `self.boot.*`。
- `heap_free` 保持 `fn() -> usize` 而不是数值：这个开机拿到哪个堆要等 `run` 才知道，两条会话都要在每次应答里读它——由边界确定、跨边界复用，不是可选字段。

查过而否掉的候选（写下来，下一轮别再议）：

- `scan(ring, dropped)`、`hold(deadline, pause)`、`await_network(...)`、`(mac, aps)`、`run_loop` 的七参：都是「一次读一组局部量」，没有跨边界的所有者，删掉任一字段签名照样说得清 → 不打包。
- `secured: bool`（驱动 trait、各 bsp 实现与探针镜像共用）与 `Report.station: bool`：单个布尔不是 clump，`fn_params_excessive_bools` 门槛是 3 且 trait 实现豁免；改它要动驱动契约，收益为零。
- MAC newtype：`embassy-net` / `smoltcp` 已经是 `EthernetAddress([u8; 6])`，本仓再造一个只多一层转换。
- 「`NetCtx.heap_free` 只有一个读者」不是删除理由：`LinkWatch` 要的是整束（总线、网络、就绪线、堆），ISP 说的是「不要依赖不用的」，不是「用的人少就拆散」。
- `mac_of` / `seed_from` 在 `safe.rs` 与 `network/mod.rs` 各有一份；重复的是推导而不是调用，合并到 `network/mod.rs`，以 `pub(super)` 共享。

两条新栅栏（`module_boundaries.rs`，都先用一个假 `Frame` 证明会红）：

- `context_types_live_only_at_boundaries`：`struct` 名以 `Ctx` / `Context` 结尾的集合必须恰好是 `NetCtx` 与 `PanelContext`。新增一个就得改这份清单，等于先回答「哪条边界定了它的字段」。
- `a_bundle_is_built_where_it_is_owned`：五个旁值（`Wiring`、`Io`、`NetCtx`、`PanelContext`、`BootFacts`）的构造点只允许在所有者文件与入口。`Frame` 的形状就是「签名参数在中途重新打包」，这是它复发时的报警器。
- `[measured]` S3 产品镜像（`nm` / `size -A`，同一份无凭据构建）：主栈 `_stack_start − _stack_end` = 86,344 B（不变），`.bss` 241,032 B（不变），`.data` 23,840 B（不变），`.text` 918,977 B（上一轮 919,457 B，−480 B——`safe.rs` 里那两份重复的 `mac_of` / `seed_from` 只剩一份）。应用分区 1,135,984/6,291,456 B（18.06%，−480 B）；C6 954,256/1,048,576 B（91.00%，+288 B）。两块板烧录后各抓一条启动日志：S3 打印 `[DISPLAY] stack peak 18932 B of 86344 B`，看门狗报 healthy，连上已存网络并拿到地址，`[STACK] core 1 peak 2016 B of 24576 B`；C6 打印 `[HEAP] 65536 B`，走离线路径（无已存网络），灯呼吸，看门狗同样 healthy。

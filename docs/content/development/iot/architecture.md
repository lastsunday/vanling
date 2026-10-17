+++
title = "分层与组合"
weight = 20
+++

# 分层与组合

## 分层

`iot-core(纯逻辑) → iot-chip-esp(esp 家族运行时) → iot-bsp-esp(每板接线) → iot-app(单任务二进制)`

- **接线只出现在 `bsp/` 板模块**：`Board::new(Peripherals)` 内固定引脚；业务代码禁止出现引脚号
- **bsp-esp 内部分两层元件**：`components/` 是实际元件（芯片驱动，参数化总线与引脚，不认板）；`virtual_components/` 是抽象元件（由实际元件组装而成的虚拟器件，如 `DisplayLight` 把 ST7789 面板适配成 `RgbLight`）。板模块只做选引脚与装配，元件本身跨板通用
- **元件独立 feature**：每个元件一个 feature，在 `components/` 的 `mod` 处门控一次，off 即文件不存在；板 feature = 元件聚合清单（见 [features.md](@/development/iot/features.md) 模块轴）
- **iot-chip-esp**：芯片初始化、日志、RTOS、panic 定义；应用层入口宏 `#[esp_rtos::main]` 留在 app `main.rs`
- **iot-bsp-esp**：板差异收敛在 `type Board` 别名与 run 分发，每板一个 `#[cfg(feature)]` 分支
- **iot-app**：应用层，产品名 `vanling`；业务零芯片依赖；flash 与 RAM 容量走链接与分区，不进类型系统
- **命名约定**：板层用板名、应用层用产品名，不重复
- **main 入口**：按家族 feature 分支，每家族一份
- **iot-tool 在这条链之外**：唯一不属于设备的 crate，读写本仓的源文件（目前只有 `midi`，生成声音目录的 `.mid`）。**零依赖**——固件一改不该把生成固件资产的那个东西改坏。裸金属的 `--workspace` 只剩 `lefthook.yml` 里那一条，已 `--exclude iot-tool`；`moon` 各任务都显式带 `-p`，本来就不碰它。入口是 `moon run iot:test-tool` 与 `moon run iot:gen-midi-assets`

## 组合（三轴正交）

硬件（板 feature）⊥ 模块（模块 feature）⊥ 能力（`iot-core` trait）。模块只见能力、板只供能力。

- **组合点 = bin 板清单**：能力 move 注入、对能力泛型、同能力板共享一份；启用缺能力的模块 = 编译错，不静默
- **产品档 = feature 别名**：档定模块集、板定支持集，交点编译期裁剪校验
- **模块可配置** = feature 开关 + const 参数
- **渲染层运行时可插拔**：core 里一个零分配的中央 `RenderController` 做 reconcile（闭包回调），app 里一个堆上的注册表（`embedded-alloc` 32 KB 堆）。boot LED 在任务内注册，跨任务的 renderer 走 `RENDER_BUS` 并要求 `Send`

## 需求模型（要什么 / 多频繁 / 多敏感）

当前固件是**板级接入验证**，不是产品；结构要能容纳后续应用把任一元件提升为主要交互或运算，且那条路径允许开到该元件的最大能力。

一个需求是三句独立的话，必须分开表达：

| 维度 | 问的问题 | 类型 | 决定什么 |
|---|---|---|---|
| **模式** | 产不产 | `Mode`：`Off` / `OnChange` / `Continuous` | 轮询是否发生、是否按时钟产 |
| **节奏** | 多频繁 | `PlaneDemand::period_ms`（0 = 板级节奏） | 轮询周期的下限 |
| **敏感度** | 多旧还算对 | `Sensitivity`：`Primary` / `Secondary` / `Background` | 本板无处落地（见下），只参与仲裁与日志 |

而「怎么取」——寄存器组、FIFO、片上引擎、传输——是采集配置（`MotionConfig` 等），由驱动选定，页面永远不点名。

这条分离让「低保真 + 高成本」不可表达：页面根本没有办法要求一个更差的画面来省资源。

把两者合成一个「档位」序数，等于同时编码「要什么」和「怎么取」。降档既像降保真、又像省资源，而降档并不等于省资源。

`Sensitivity` 在本板改不动任何节奏，这是硬件决定的。Android 的 `max_report_latency` 前提是事件按时钟采集、只在上报前压在 FIFO 里，所以「采得勤、报得晚」是两句独立的话。本板 IMU 没有 FIFO，报得晚只能换成采得稀，而那正是 `period_ms` 已经说出口的话。

所以「class → 毫秒」那层查表在本板不可兑现：它只会把 `period_ms` 乘一个系数再取 max。

Android 那一侧给的两条形状照抄过来：

- 每个传感器的 `minDelay` 对 on-change 必须是 0、one-shot 必须是 −1。「多频繁」不是每种上报模式都问得到的。

  运动平面在本板虽然名字像 on-change，实际是持续轮询加器件内的 No-Motion 引擎产出变化事件：`Mode::OnChange` 与 `Mode::Continuous` 由 `MotionConfig::for_mode` 映射到同一个 `PolledRegisters`。

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

### 更改：复用现有 intent 管道

`apply_business` 已经是唯一改状态的入口，`BusinessIntent::TogglePage` 是唯一改 `page` 的地方。

新增场景 = 新增一个 `BusinessIntent` 变体；运行期换节奏 = 再一个变体。`apply_business` 之后求 `EffectiveDemand`（取最大值聚合）。不新造机制。

### 传输：三条下行路径，性质不同

| 消费者 | executor | 现状 |
|---|---|---|
| 渲染层 / 摄像头 | 协作 | ✅ **已有通道** |
| `input_task` | 协作 | ❌ `input_task(intent_bus, sources)` 拿不到 state |
| `control_loop` | 协作 | ❌ 同上 |
| `feed_task` | **Priority2 中断，独立 executor** | ⚠️ **故意无依赖，不能加** |

**渲染层零成本**：`Diagnostics` 已经是「融合所有子系统状态的单快照」，含 `page`/`motion_enabled`/`audio`/`playback`；`DiagnosticsSink::consume` 就是「状态进消费者」的通道。摄像头那条链已端到端跑通：

```
DeviceState.page → light_diagnostics() → SlotAppearance::Diagnostics
  → RenderController::reconcile → LightRenderer::on_appearance
  → DisplayLight::consume → camera.pause() / released()
```

所以 `Demand` 只需长在同一快照上，display 与 camera 部分零新增。

**需求是共享状态，不是消息。** 需求是幂等的聚合状态而非事件，只有最新那一份有意义。用通道装它，就得补上一格深队列、写方变化检测与失败重试——三套机制都只为「通道存不住一个值」而存在。

共享槽的形状跟着这个约束来：embassy 的 blocking mutex 是共享只读的（`RawMutex::lock(f)` 的闭包拿不到 `&mut`），所以槽内还要一层 `RefCell`。

`Intent` 是相反的情形——事件——所以仍走总线。

采集窗口是第三种形状：它是**窗口**不是事件（只有最新一份有意义），但也不是共享状态（不是谁去改公共槽，而是板子在 poll 里把驱动刚折好的一帧交出来）。于是它走一条单槽平面：`HasAudio::take_audio(&'static dyn CaptureSink)` 把目的地交给板子，`input_task` 的 poll 往里发布，`render_loop` 在 intent 之后取走。

第二个理由是宽度：`Channel<T, N>` 的每个槽按 `size_of::<T>()` 分配，槽宽由最宽的事件变体决定 —— 带 envelope 的采集事件（1.76 KB）会把 8 槽的 `INTENT_BUS` 定成 14,184 B 内部 DRAM，而面板每次只读最新那一帧。变体删掉之后 `INTENT_BUS` 是 896 B，平面自己 1,824 B（记在 [Wi-Fi 实现记录](@/records/iot/wifi.md)）。

顺序是顺带的：`input_task` 按接线序 poll，采集排最后，所以同一个 tick 里按下录制键闩住的是上一次 pass 画过的那一帧。

句柄由组合点造并注入（复用 `playback::share()` 的形状），满足 `input.rs` 的约定：句柄传进去，没有消费者按名字摸全局。

### feed_task 是时钟声明方，不是需求消费者

这条最容易搞反。

麦克风的 BCLK 与 WS 由 I2S TX 单元产生，RX 从属它（板模块里 RX 不接 BCLK 与 WS）。feed 的存在本身就是麦克风时钟的来源——silence 也是一种 voice，ring 永远被喂满。

所以 `feed_task` 持有一个永不释放的时钟 claim，需求不流向它。

需求只决定采集侧折叠的节奏，而折叠在 `input_task` 里。麦克风时钟因此永远在，折叠可按需降频，不需要动时钟拓扑。

代价与耦合：ring 是 `BYTES_PER_POLL * 3`（约 60 ms），最多约 60 ms 不 drain 就开始丢样本。

更要紧的是 drain 不只影响丢样本：环满之后 I2S 引擎自行停机。于是 poll 变稀不是「读数变旧」而是「时钟停摆」。

实测把 capture 批到 30 s 之后，环在 60 ms 内撞满、`capture ring 10842 B deep` 报警，90 s 里 `CaptureWatchdog` 只在第 91 秒才通过 `restart` 把链接回来一次。

所以采集平面不接受任何慢于板级 cadence 的 poll：`Es7210Rx::accepts_slower_poll()` 答 `false`。

`CaptureWatchdog` 靠「有没有进度」判停滞这件事因此不是问题——它根本没有机会看到一次故意的长间隔，间隔不存在。

### 时序

`render_loop` 写槽 → `input_task` 下一个 `INPUT_BASE_MS` tick 读到。「场景切换 → 需求生效」是一个 tick 加上 `render_loop` 被持有的时间。

#### 上限是单次最长持有，不是百分比

Liu & Layland、CFS、RT group 的利用率界约束的都是抢占式调度器跑任务的时间份额。

协作式执行器不在这个模型里：每次 poll 公平，没有任务被份额饿死。它不能做的是打断。

一次不让步的 poll 会占住整个最坏执行时间。所以要界定的是单次持有。

界定对象是整个 tick，不是其中最大的那个源。input tick 里所有源跑在同一个不让步的循环里，面板写不在其中。

实测（S3，8 个 5 秒窗口）单次 tick 17–25 ms。`MAX_SINGLE_HOLD_MS = 30`，留一个 poll 周期的余量。`[INPUT]` 每窗口打印 `worst single hold N ms (bound 30 ms)`，越界直接可见。

这个数字是在 **80 MHz** 下测的，不是数据手册的 240：`CpuClock` 的默认臂是 `_80MHz`，而 `chip_init` 走的是 `esp_hal::init(Config::default())`，仓库里没有任何地方显式设过时钟。`[实测]` `aec-probe` 的 `esp_hal::clock::cpu_clock()` 读回 80,000,000 Hz。esp-hal 对 160/240 预设有一条注释说换 PLL 会影响 USB Serial/JTAG，而本仓库控制台正是它，所以这档位是否可用未测。**读上面任何一个毫秒数都要带上这个 3 倍的余量。**

总量不是风险项：Ambient 花掉 94.8% 且一切正常。一次 25 ms 的阻塞才是。播放喂数跑在独立的高优先级 executor 上、由 DMA 驱动，碰不到这种阻塞。

#### 两种配置拒绝实现，返回 `UnsupportedConfig` 而不是写一个像样的字节

| 配置 | 缺什么 |
|---|---|
| `WakeDetector` | 单加速度计同时改变 CTRL7 功耗域和「用哪张 ODR 表」；datasheet 给了两张表的频率但没给各自索引，引擎窗口按该索引换算，猜错就正好错这么多 |
| `Fifo` | FIFO 寄存器从 `0x0B` 起，与本驱动写运动标定的地址重叠——重叠正是 CTRL9 命令协议存在的原因，所以这不是一次寄存器写而是排序问题 |

每一条都是「打开寄存器表补一个编码」的可达路径。

`Off` 不是配置而是模式，由消费者不轮询兑现。

驱动的具体接线与寄存器细节在 [motion.md](@/development/iot/motion.md)。

#### 配置器、仲裁与接线

配置器是模式与寄存器之间唯一的一道翻译。`Configurator::configuration_for` 把需求变成各器件「必须被配置成什么」，映射在 core 的 `MotionConfig::for_mode`，器件能力由板子在装配点交给 `Configurator::new`。

分工是硬的：哪个模式要哪个配置是产品的话，这块硅片上有什么是板子的话。

拒绝是返回类型的一部分。走不进去时返回 `Unreachable { key, mode, config }`，三者齐全，调用方拿不到「悄悄降级」的选项。

拒绝会被写进需求：被点名的平面从「设备承诺」里去掉（`EffectiveDemand::without`），下游读到的需求不再声称一件器件做不到的事。

`Off` 不在此列，它由「不采集」兑现，返回 `None` 而非 `Err`。交互通道永不受影响。

图上 Logic 分别调 Render 与 Configurator。但器件在 `input_task` 里，`render_loop` 够不着，所以配置器也放在持有器件的一侧：它要板子声明的能力来翻译模式，拒绝也该对着真正拒绝的那个器件报告。

配置器随需求一起兑现，`[DEMAND]` 报的是这次排程的结果。

需求按平面索引，不按字段枚举。`EffectiveDemand` 是 `BTreeMap<PlaneKey, PlaneDemand>`，键是「器件类型 + 第几个」，所以 `plane`、`without`、`aggregate` 与 `Override::apply` 全是查表，新增一个传感器不需要改动需求机械的任何一行。

两个同型号器件可以分开提需求（`PlaneKey::instance(Audio, 0)` 与 `…, 1`）。

平面清单由板子给。`DisplayPage::demand(&self, planes)` 遍历板子声明的平面表调 `claim_for`。这个 match 是穷尽的，所以页面不可能漏掉一个平面然后继承一份没人选过的需求。

板子没声明的平面不出现在需求里，读作 `Off`，不需要再用四个布尔开关夹一次。

需求状态是被持有的，不是现算的。`DeviceManager` 持有 `demand`，与 `state` 并列。派生的唯一出处是 `propose_demand()`，而 `page`、平面清单、`override_` 的任何一次写入都要跟着它走。

理由只有一个：配置器要有东西可以改。`commit_demand` 把可达的那份写回同一处，下游读的都是被改过的那份。

仲裁由 `EffectiveDemand::aggregate` 逐键取最大值，测试断言它是逐平面取胜而非整表取胜。

接线：

| 消费者 | 怎么拿到需求 |
|---|---|
| `input_task` | 共享需求，每个 tick 在临界区内借用一次，按 `PollEntry.key()` 排程（cadence 还要问驱动 `accepts_slower_poll`） |
| `render_loop` | 写入方。页面就是产品的声明，所以写在这里而不是让每个消费者各自推导 |
| 摄像头 | `DiagnosticsSink`（已有通道），门控条件从「页面变了」改成「需求不再要求画面」 |
| `feed_task` | 刻意不注入。它的存在本身供给麦克风时钟，所以它是音频平面时钟的来源而不是它需求的消费者 |

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

| 类型 | 归哪 | 是什么 |
|---|---|---|
| `ReferenceLog` | core | 按放音游标而不是写入指针保存参考样本，并报 lap —— 配对读到的必须是已经播出去的那一段 |
| `Decimator` | core | 抽取器带**相位**，两路各自报自己的，供配对方对齐而不是各猜各的 |
| `EchoStage` | core | 采集 × 参考的配对与抵消，产出 `EchoOutcome` |

相位是这套里唯一会静默错一抽的东西：发端比收端先 arm 时两边取 0，**换一块先 arm 麦克风的板子必须同时把偏移报给两边**，差的部分由延迟搜索兜，那不是自动的。

`demand-probe` 遍历运动平面的（配置 × 节奏）矩阵，每个格子先 settle 再测量，报三个数：驱动自己计数器给出的总线事务/秒、窗口内 poll 的执行器占用墙钟占比、以及测量的 poll 数（好让前两个互相校验）。

它报不了电流——那需要板子上接表，别人的数据手册数字不是对这块板的测量。所以 `micro_amps` 列保持空白。

驱动没有寄存器编码的格子按「不可达」打印而不是跳过，日志对「没测到的」和「测到的」一样响。

配套两件事：`SUPPORTED_CONFIGS` 只列驱动能把芯片配成的样子（有测试断言它与 `config_for` 一致）；`Board::new_motion_only()` 提供隔离 bring-up——一个在别的平面还在跑的格子里测出来的东西是关于错的东西的证据。

各平面的实测数字与它们的口径限制，在[实现记录索引](@/records/iot/_index.md)。

#### 执行器占用的数字不可相加

同一批协作执行器，谁醒着谁拿时间：把 motion 批到 30 s 后 input 侧从 49.5% 降到 35.5%，面板侧同时从 45.3% 升到 58.5%。

面板那一侧是残差不是增量。把它和被轮询平面的数字相加，等于把同一批微秒数数两遍，加出来会超过一个核的 100%。

同理，平面独自在板上跑测到的数（探针）与两个整机构建相减测到的数不是同一个口径。混在一起相加得到的和是下界不是总量（三个平面隔离相加读作 43.2%，同一构建实测 51.0%）。

要贵的那一侧：平面的代价随旁侧运行的东西上升，便宜的读数正是最先失效的那个。

由此有一条测量方法上的硬规矩：**要问节流对最坏阻塞的作用，测法是只改节奏不动模式。** 两者同时改时，省下的部分可能来自关掉整个平面而不是来自节流。

## 页面循环按次序表走一圈

`DisplayPage::CYCLE` 是次序的定义（`Ambient → Attitude → Audio → Speaker → Camera → Network`），`next_page` 从本页之后走一圈、取第一个本板能力支持的页、走到末尾回绕。启动页（`with_boot_page`）用同一份能力判定夹取，所以一次「走一圈」从任何一页出发都是全部可用页各一次并回到起点。

能力判定是每条页自己的一个 bool（`is_backed`），由组合点注入：运动、音频、回放、相机、链路。前四个来自板子的 `take_*`，链路来自入口点（station 要在调度器起来之后才启动，`run` 拿到板子时射频已经被取走）——所以 `iot_app::Link` 与 `SpeakerRunner` 同形，是**接线事实的参数**，不是 `run` 里回头去问板子。

这条规约是修一个缺陷写下来的。

`next_page` 曾是 18 个手写跳转分支，`Camera` 的离开分支返回「第一个本板能显示的页」（`Attitude`）而非起点，于是 `Ambient` 被永久跳过：实测 `CYCLE` 全能力下走一圈是 `[Attitude, Attitude, Audio, Speaker, Camera]`，`Ambient` 消失、`Attitude` 出现两次。

跳着写会漏掉闭环的那一页，走一张表再回绕不会。

`iot-core` 里有一条测试遍历能力组合 × 起点断言这一点（`every_page_the_board_can_back_is_shown_on_the_way_round`），把旧的 `Camera → Attitude` 放回去它会失败并打出上面那串。


## 日志

只用 `log` façade（`log::info!` 等）；输出通道由家族 `iot-chip-esp` 初始化（esp 为 `esp_println::logger`）；业务代码禁止 `println!` / `esp_println::println!`

一个日志行要能被相信的前提是它报的是**正在运行的那个配置**，不是被请求的那一个。`[DISPLAY]` 的 `asked` 曾报页面自己的需求而非 override 生效后的，因为快照从 state 取需求而 override 挂在 manager 上。一个报出无人运行之配置的日志比没有日志更糟。

## 新增板

1. `bsp-esp/` 增板模块（Board + HasXxx traits）+ feature 门控；选框上的芯片先在 `components/` 建实际元件（已有则直接复用，并为新元件声明独立 feature），板 feature 聚合元件清单，板模块内装配
2. `app/src/main.rs` 加 `type Board`/run 分发 cfg 分支
3. app feature 透传 `iot-bsp-esp/<board>`
4. `reusable-iot-build.yml` 的 `platforms` 默认 JSON 与 `iot-dev-release.yml` 的 `platforms` 输入里加该板（xtensa 板需标 `use-xtensa-toolchain: true`，经 `build-s3` 任务构建：CI/发布用 espup 原生 `esp` 1.95.0.0 工具链，macOS Intel 本地自动回退同版本 Docker 容器）
5. 验证：`cargo build -p iot-app --bin vanling --target riscv32imac-unknown-none-elf --no-default-features --features <board>`

## 新增芯片家族

esp 家族仓固定为 `iot-chip-esp`/`iot-bsp-esp`；新增非 esp 家族：

1. 新增 `iot-chip-<family>`/`iot-bsp-<family>` crate
2. app 家族别名 feature 门控该家族 entry deps；`main` 加家族入口块（`#[cfg(feature)]`）
3. 工具链目标 + `rust-toolchain.toml` + `reusable-iot-build.yml` 的 `platforms`（`target`/`use-xtensa-toolchain`）与 `iot-dev-release.yml` 的 `platforms` 输入
4. `moon`/`lefthook` 的 `--workspace --target` 改按固件作用域；工具链目标经 `CARGO_ESP_TARGET` 传入 moon 任务（`.envrc`/CI env 与 workflow inputs 提供，moon 任务以 `- '$CARGO_ESP_TARGET'` 收入 inputs 保证跨芯片缓存隔离）

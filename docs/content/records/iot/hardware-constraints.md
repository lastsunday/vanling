+++
title = "硬件约束记录"
weight = 20
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 硬件约束记录

`apps/iot` 在真实硬件上摸出来的约束。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

---

## 还没有电源管理，所以还没有时钟源锁——但顺序不能反

`[据称]`（未实现，仅留痕）ESP-IDF 的 `esp_pm` 用三种锁表达组件对功耗的需求：`ESP_PM_CPU_FREQ_MAX`、`ESP_PM_APB_FREQ_MAX`、`ESP_PM_NO_LIGHT_SLEEP`。锁是**递归**的，持有几次就得释放几次。其文档给出两条本板将来会撞上的事实：

- **Light-sleep 期间外设被时钟门控，且不会产生中断。** 因此任何依赖外设中断的驱动必须持 `NO_LIGHT_SLEEP`。本板的 DVP 采集链与 I2S 采集/放音链都靠 DMA 完成传输，落在这一条里。
- `REF_TICK` / `XTAL` / `RC_FAST`（即 RTC_8M）等外设时钟源**不受 APB 降频影响**，所以降频不等于全停，某些时钟能活过 APB 变化。

当前固件**没有任何 PM 配置**——没有 light-sleep、没有 `esp_pm_configure`、没有 `WFI`。喂数任务已在独立的 `InterruptExecutor` 上（`lib.rs` 的 `Feed::Interrupt`），所以执行器争用这条路已经被结构解决，不需要锁。

因此**没有实现锁是对的**：加两把没有持有者的锁就是死代码，而死锁比死代码更难查。

但顺序不能反。要加 light-sleep，必须先有引用计数的锁集合，并让至少这些持有：

| 持有者 | 需要的原因 |
|---|---|
| 播放喂数 | 它的存在本身就是时钟来源；停它等于停掉 codec 的主时钟 |
| 采集链 | DMA 完成即释放环，DMA 被门控则环永远不排空 |
| DVP 采集链 | 同上，且其链的修复轮询住在 render 侧 |

`InputSource::set_demand_collecting` 已经是这个形状（设备关闭时不能靠下一次 poll 得知，必须有人显式说），所以接线位置已经对了，缺的只是锁本身。

Zephyr 的做法也印证这个分工：它的 device runtime PM 明确**不管理设备依赖**，「devices can manually get or put their dependencies」——依赖关系由消费者自己管，不是自动的。

---

## 共用一根 MCLK

`[实测]` 采集与放音两个 codec 挂同一组 MCLK / BCLK / LRCK。若各自用独立时钟声明，可以相差几 Hz 而**没有任何东西会报告**：面板会把采集计量对，喇叭却略微偏高——听起来像硬件故障，而它不是。

`[实测]` 两个故障各自都很安静、且像对方：喇叭吃到错位时钟是爆音，麦克风被错位时钟采样则返回恒定满刻度，看起来像"房间很吵"。

`[实测]` **哪个单元驱动那对引脚，两个方向都试过，两个都坏**：

| 谁驱动引脚 | 发送侧听到的 | 采集侧看到什么 |
| --- | --- | --- |
| 采集单元 | 持续爆音（发送单元在一个 ES8311 并未被其时钟的除数上移出采样） | 正常 |
| 发送单元 | 正常 | 恒定满刻度读数 + 一条死平的波形（在采集一个并非给 ES7210 定时的那只除数） |

两个单元跑在各自的时钟域里，谁都不能独自拥有时钟。所以**发送单元驱动引脚、采集被强制为从属**，方向不可交换。

代码里的对应约束（`shared_tdm_config`）：`with_signal_loopback` 让接收单元从属跟随，**其名是唯一误导之处**——它不跨单元传样本，只共享时钟。

`audio::tdm_config` 与 `audio_out::shared_tdm_config` 是**有意保留的两份拷贝**，因为两个 feature 相互独立（只有喇叭的板、只有麦克风的板都是真实存在的），不能让任何一半拥有那个共同答案。采集侧那份必须关掉从属——没有发送单元就没有主。

---

## 麦克风前端的 DC 阻断写序

`[测试]` ES7210 每对输入的 DC 阻断滤波器占两个只差 bit 5 的寄存器（`0x0A ^ 0x2A == 0x20`），因此拐角是两者唯一共享的字段。有 MockI2c 的写序 trace 测试守着这次写序。

寄存器按**它们携带的位**命名，而非按级次顺序：没有公开文档支持级次命名——datasheet 是 "Everest Semiconductor Confidential" 并把位定义指向一份不公开的 guide，两个驱动家族连名字都不一致（`esp-bsp` 称 `0x22` 为 HPF2，`esp-audio-dev` 称 HPF1）。

`[实测]` 交换这一对**不是外观问题**：会抬高静音底噪。所以写序跟着参考驱动走，而不是跟着一个无文档支持的级次顺序。

---

## 0 dBFS = 102 dB SPL

`[据称]` 2026-09-28 以 30 cm 对着语音标定：底噪 66 dB SPL，峰值到 80。本次未复现此标定。

推导链（代码注释里有完整版）：ES7210 满量程为 AVDD/3.3 Vrms，故在 `ANALOG_POWER_RUN` 下是 1.0 Vrms 而非"headroom"暗示的 2 Vrms；ZTS6216 的 −38 dBV/Pa 即 12.59 mV/Pa，经 `GAIN_30DB` 到转换器是 397.8 mV/Pa——于是满量程 2.51 Pa，对 20 µPa 参考压力即 101.98 dB。

**重新标定的方法**（换胶囊或换增益档时）：设 `CAL_SPL_LOG`，在同一距离说话。

> 这些数字留在代码注释里是因为它们决定读数；要改数值必须同时改这里和代码。

核心侧如何消费这个偏移（为什么 `spl()` 把它当参数收）见[音频实现记录](@/records/iot/audio.md)。

---

## QMI8658A 的 tap engine 不可用

`[据称]` 该器件的轻点引擎在使能瞬间锁住一个不释放的 tap 位和冻结的 `TAP_NUM`，无法分辨真实敲击，因此只 arm No-Motion，敲击判定改由 core 侧的识别器承担（`recognizer.rs`，`[测试]` 7 个用例覆盖敲击/转身/一次敲击计数）。本次未复现该现象。

完整的判定规则、门限来源与被否方案见[运动实现记录](@/records/iot/motion.md)。

---

## 主任务栈容量

> `[实测]` **栈的三个数字都已随摄像头进产品而变，以下是当前值**（`vanling`，`moon run iot:build-s3` 后读 ELF 符号）：
> ```
> _stack_end        = 0x3fcd529c   (was 0x3fcd154c)
> _stack_start_cpu0 = 0x3fce9710   →  83 060 B = 81.1 KB   (was 98 756 B)
> ```
> 面板帧缓冲从堆上那份 `Vec` 变成了板层的静态 arena（与相机 ring 共用，见 `camera.md`），`HEAP_MEM` 因此从 204,800 降到 65,536；净效果是 `.bss` **增加** 14,336 B、主栈减少约 15 KB。`FRAME_ARENA` 实测落在 `0x3fcaf900`，`% 64 == 0`。

`[实测]` `esp-hal` 自带的 `stack.x` 把主任务栈顶设在 `dram_seg` 末尾，`dram2_seg` 那块无人认领，`crates/app/linker/esp32s3-main-stack.x` 把它接上，栈因此跨两块内存连续。

`[实测]` 从 ELF 符号读出的真实几何：

```
_stack_end        = 0x3fcd154c
_stack_start_cpu0 = 0x3fce9710   →  98 756 B = 96.4 KB
```

> `[实测]` **这两个值是摄像头进产品之前测的**，见本节开头。`_stack_start_cpu0` 此前还记为 `0x3fced710` / 115 140 B，少算了 ROM 的两个 16 KB 引导栈：`esp32s3-main-stack.x` 把栈顶设在 `dram2_seg` 上沿**减** `ROM_BOOT_STACKS = 0x4000`（`0x3fced710 - 0x4000 = 0x3fce9710`）；ROM 的 PRO/APP 引导栈不能被主栈顶到。同一个漏算让 `camera.md` 一度得出"ring 进 `vanling` 必然 region overflow"的错误结论，那一节已注明。

**为什么需要**：绘制一屏渲染时 `Diagnostics` 快照与两个 `[u16; ENVELOPE_COLUMNS]` 列数组按值在栈上，Audio 页曾在这个栈上崩溃（异常的 `A1` 落在 `_stack_end` 之下）。

`[实测]` 2026-10-28 在 Ambient 页读到 **42,392 B of 65,976 B**（64.3%），余 23,584 B（info 镜像开机日志，`[DISPLAY] stack peak 42392 B of 65976 B`，2.7 s 处的第一个窗口）；此前记的 36,453 B（43.9%）是这几轮 render/state 改动之前的值。固件自带水位测量（`[DISPLAY] stack peak`），每重绘窗口采样一次，取最深值——不是估算。Camera 页的水位此前一直读 `0 B`：那条路径不经过任何采样点，是没测而不是水位低，现已补上。

**一个未解的疑问**：这个峰值从开机起几乎不变，多页切换也没有推高它。而历史崩溃点是 Audio 页的整帧 stamp。峰值到底属于哪条路径**尚未定位**；若它其实属于渲染，说明渲染比代码注释描述的还重，值得继续查。

### 操作面按页拆开之后：墙没动，峰值 42,392 B（`[实测]`，2026-10-28）

`[实测]` 同一把尺子（`nm apps/iot/target/xtensa-esp32s3-none-elf/release/vanling`），读数取在 `state/` 按页拆分与声音目录搬去 `app` 之后：

| 量 | 读数 |
| --- | --- |
| `_stack_start_cpu0 − _stack_end_cpu0` | `0x3fce9710 − 0x3fcd9558` = **65,976 B**（下限 65,536，余量 **440 B**） |
| `__embassy_main::POOL` | **42,776 B**（`0xa718`） |
| `moon run iot:image` | S3 `1,059,904 / 6,291,456 B`（16.85%）、C6 `842,400 / 1,048,576 B`（80.34%） |

这一组地址与拆分之前逐字节相同，所以这两轮对 S3 静态量的净增量是 **0**：搬的是代码的归属，不是布局。镜像是 S3 `1,059,888 → 1,059,904 B`（+16 B）；C6 上一版记的是 `826,976 B`，但与 HEAD 用同一条命令重编比，C6 是 **826,656 → 842,400 B（+15,744 B）**。

**这 15.7 KB 全在 C6 一侧，出处已经量到**：`app/src/playback/sounds.rs` 的 `play` 现在先解析再交给喇叭，而 HEAD 上解析发生在 S3 的驱动里——C6 的 `UnwiredSpeaker::play(Sound)` 什么都不做，LTO 就把解析摘掉了。证据是符号：HEAD 的 C6 镜像里 `from_midi`、`Score::into_notes` 的排序、`Tone::fill` **0 处**，本轮 7 处；`control_loop::poll` 因此长了 5,466 B，`iot_core::drivers::playback` 的符号在本轮合计 10,608 B。C6 的 `take_playback()` 返回 `None`，这条路径运行时不执行，付的是槽位不是周期。C6 还剩 206,176 B，所以这条只记录：要不要把解析重新关在编译期（软 feature，或只在有喇叭的构建里保留 `play`）是一个待定项。

该次构建没有设 `IOT_WIFI_*`，凭证字符串会挪 `_stack_end`，所以这组墙只与同样不带凭证的构建比。

`[实测]` **峰值：`42,392 B of 65,976 B`**（64.3%，余 23,584 B）。这行印在产品 info 镜像的开机日志里（`moon run iot:flash-s3`，复位后 2.7 s 的第一个窗口，Ambient 页），`main stack` 与 `nm` 一致——`[DISPLAY] stack peak` 并不需要 debug 镜像：水位每涨 1,024 B 就在 info 印一次，同样的读数重播才降到 debug。这个数含启动页与落地后那一页的绘制。

`[实测]` **跨页最坏值：42,392 B `of` 65,976 B**（Speaker 页 42,376 B，是六页里唯一更低的一个）。六页各用 `IOT_BOOT_PAGE=<页名>` 重编、烧录、录一次开机；同一块板、同一时段、全部拿到 `192.168.1.136`：

| 页 | `App/part` B | `main stack` B | 峰值 B |
| --- | --- | --- | --- |
| ambient | 1,059,904 | 65,976 | 42,392 |
| attitude | 1,059,952 | 65,976 | 42,392 |
| audio | 1,059,984 | 65,976 | 42,392 |
| speaker | 1,060,192 | 65,976 | 42,376 |
| camera | 1,059,920 | 65,976 | 42,392 |
| network | 1,059,936 | 65,976 | 42,392 |

`main stack` 六页一字不差：启动页只挑入口，不动静态布局。峰值最小的是 Speaker 页（42,376 B，比其余页浅 16 B），差的是那一页的绘制链深度，不是测量粒度——打印按「每涨 1,024 B 报一次」触发，报出来的值是精确水位。按上限记账时仍取 42,392 B。**440 B 是墙到下限的距离；42,392 B 是已经踩到的水位。**

### B/C 拆分之后：墙宽了 160 B（`[实测]`，2026-10-29）

`[实测]` 同一把尺子，读数取在 `core/src/page/` 一页一模块、`state/tests.rs` 按模块搬家、`ui/src/pages/live.rs` 拆成三页、帧节拍抽成 `app/src/frame.rs` 之后（Docker 引擎恢复后补测，构建未设 `IOT_WIFI_*`）：

| 量 | 读数 | 上一组（A 收尾） | 差 |
| --- | --- | --- | --- |
| `_stack_start_cpu0 − _stack_end_cpu0` | `0x3fce9710 − 0x3fcd94b8` = **66,136 B** | 65,976 B | **+160 B** |
| 余量（减 65,536） | **600 B** | 440 B | +160 B |
| `__embassy_main::POOL` | **42,784 B**（`0xa720`） | 42,776 B | +8 B |
| `moon run iot:image` S3 | `1,059,216 / 6,291,456 B`（16.84%） | 1,059,904 | **−688 B** |
| `moon run iot:image` C6 | `843,728 / 1,048,576 B`（80.46%） | 842,400 | **+1,328 B** |

`[实测]` POOL 长了 8 B，墙却宽了 160 B，说明别的静态量少了 168 B：B/C 是代码搬家，LTO 在新模块边界上重排了结果，不是某一处被刻意缩小。C6 的 +1,328 B 没有逐符号归因——C6 还剩 204,848 B，只记录这个数。`check-s3`、`build-s3`、`iot:image`、`check-features`（六 subset host + 九 subset 裸金属/C6）本轮全部通过。

`[实测]` 第一次尝试因本机 Docker Desktop 引擎掉线而未测：`moon run iot:check-s3` 停在 `image vanling/xtensa:esp32s3_1.98.1.0 is missing`，`docker info` 报 `Cannot connect to the Docker daemon at unix:///Users/yu/.docker/run/docker.sock`；引擎恢复（Server 28.5.1）后同一棵树补测得上表。

### 板层两个大件装箱：墙宽了 35,616 B（`[实测]`，2026-10-30）

`[实测]` `Board` 按值传进 `main` 的 future，future 又整个驻在 `__embassy_main::POOL` 这个静态池里，所以**字段内联多少，静态量就多重多少**。本轮把 `lckfb_szpi_esp32s3` 板上两个按值携带的器件装箱（字段 `light` 同时更名为 `screen`；C6 的 `light` 是真 WS2812，不动）：

```rust
screen: Option<Box<Screen>>,                    // 原 light: Option<Screen>
audio: Option<Box<Es7210Rx<…, REFERENCE_SAMPLES>>>,   // 原 Option<Es7210Rx<…>>
```

`size_of`（`check-s3` 里拿 `let _: [u8; 0] = [0u8; size_of::<T>()]` 探出来的，探针未留在树上）：`Screen` **2,712 B**（其中 `Diagnostics` 2,568 B）、`Option<Es7210Rx<…>>` **9,168 B**（AEC 的 `AdaptiveFilter` 是大头）、`Board` 两者都装箱 **240 B**（都内联时 **12,112 B**，由上两次读数推算）。

| 量 | 都装箱（本轮） | 只装箱屏幕 | 都不装箱 | 差（都不装箱 → 都装箱） |
| --- | --- | --- | --- | --- |
| `_stack_start_cpu0 − _stack_end_cpu0` | `0x3fce9710 − 0x3fcd09b8` = **101,720 B** | 74,232 B | 66,104 B | **+35,616 B** |
| 余量（减 65,536） | **36,184 B** | 8,696 B | 568 B | +35,616 B |
| `__embassy_main::POOL` | **7,192 B**（`0x1c18`） | 34,696 B（`0x8788`） | 42,808 B（`0xa738`） | **−35,616 B** |

三段读数取自同一棵树、同一把尺子（`nm -S apps/iot/target/xtensa-esp32s3-none-elf/release/vanling`），只改这两处的装箱与字段名。POOL 的减少量各是字段大小的约三倍（屏幕 **−8,112 B** ≈ 3 × 2,712；麦克风前端 **−27,504 B** = 3 × 9,168），即 future 布局里每个值被搬了三份（板层构造、`take_*` 的临时、`WiredSurface`/`PollEntry` 的落地各一）；具体符号没有逐处归因。

**堆不变**：两个 `Box` 在 `take_surfaces` / `take_audio` 处释放，值随即搬进本来就有的 `Vec<WiredSurface<Screen>>` 与 `PollEntry` 的 `Box`，S3 那块 96 KB PSRAM 堆的峰值不变，只是这些字节在开机那一刻先落堆、再原样挪走。

`check-s3`、`check-c6`、`check-host`、`build-s3` 与 S3 目标的 `cargo clippy -- -D warnings` 本轮全部通过。

### 面板 SPI 拷贝缓冲 8 KiB → 128 B，主栈墙宽 8,088 B（`[实测]`，2026-11-01）

`[实测]` 面板 SPI 那两个 4 KiB `with_buffers` 不参与帧传输。`SpiDma::write` 的 `DmaOperationKind::for_write` 对**内部 DRAM 切片**判 `InPlace`（`memory_range!("DRAM")` = `0x3FC88000..0x3FD00000`，`FRAME_ARENA` 在 `0x3fc9b420`），所以 153,600 B 的帧、以及栈上的命令与参数都是直传；只有 **rustc 提升到 `.rodata` 的常量切片**（`&[0x01]` 这种）落在 flash、DMA 读不到，才走拷贝缓冲。

`[实测]` 干脆删掉 `with_buffers` **起不来**：`board init failed, resetting: Spi(DmaError(BufferTooSmall))` —— `panel_spi` 里的软件复位 `&[0x01]` 正是那种常量。所以缓冲必须留，尺寸按「最长的一条命令参数」定：两个 64 B，合计 128 B。

| 量 | 改前 | 改后 | 差 |
| --- | --- | --- | --- |
| `.bss` | 231,064 B | 222,976 B | −8,088 B |
| `.stack`（dram_seg 一侧） | 43,880 B | 52,000 B | +8,120 B |
| `main stack`（开机日志） | 101,240 B | **109,328 B** | +8,088 B |
| `[HEAP]` | 96 KiB | **512 KiB** | 堆在 PSRAM，不占内部 DRAM |

`dram_seg` 总量仍是 341,760 B、余量仍是 0：省下来的字节被链接脚本直接并进主栈，所以收益是**墙更宽**，不是「多出可用字节」。

`[实测]` 帧传输没变慢：64 B 缓冲下同一窗口 9 次重绘里 8 次落在 15–19 ms（153,600 B / 80 MHz = 15.36 ms）。另一次读数 610 ms，同一时段 `[INPUT]` 报 `worst single hold 597 ms … worst source: touch 597 ms` —— 是触摸轮询占住执行器，与缓冲无关（帧在 `.bss`，本来就不经拷贝缓冲）。

`[实测]` 同一轮把射频驱动堆的 `dram_seg` 份额从 16 KiB 收到 8 KiB（`radio/esp32s3.rs` 的 `INTERNAL_BYTES`）：开机 `[WIFI] driver heap: 8192 B in dram_seg + 32768 B above it`，总量 49,152 → **40,960 B**，主栈 109,328 → **117,520 B**（+8,192，与收掉的份额一一对应）。余量按已量的驱动峰值 33,332 B 算约 **7.6 KB**（32.2% → 23.1%）；产品在该尺寸下关联、拿到地址并稳定持有，没有 `OutOfMemory`。S3 与 C6 的驱动堆就此不再同尺寸（C6 仍是单块 48 KiB）。

`[实测]` 堆 96 KiB → 512 KiB：开机 `[HEAP] 524288 B at 0x3c100000 in PSRAM, of a 8388608-byte window`。改前开机只剩约 1.6 KB，下一次 `Box` 就是分配失败；PSRAM 窗口 8 MiB，抬到 512 KiB 不动内部 DRAM。

### 面板帧传输按 32 行分块 + yield：最长连续持有 15 ms → 1–2 ms（`[实测]`，2026-11-01）

`[实测]` 一帧 153,600 B / 80 MHz = **15.36 ms**，原先一次 `SpiBus::write` 把这一整段连续占住 core 0 的协作执行器 —— 喂数、触摸/运动轮询、capture 轮询与渲染共用它。修法不是换驱动，而是让这段传输中间让出执行器。

**async DMA 这条路走不通**：`SpiDma<Async>::write_async` 把它的 `[DmaDescriptor; LINK_DESCRIPTOR_COUNT]`（`MAX_DMA_SIZE.div_ceil(CHUNK_SIZE) + 3`）放在协程帧里，而本仓的 future 一律 `Box::pin` 到 PSRAM 堆上，于是真机一写就 panic：

```
panicked at ... unwrap of DescriptorSet::new(descriptors) failed: UnsupportedMemoryRegion
```

描述符必须落内部 DRAM，堆上的 boxed future 给不了它。所以本板**不能**把 esp-hal 的 async DMA 与堆上 future 一起用。

采用的做法：驱动仍是 `SpiDma<'static, Blocking>`，`write_frame` 变 `async`，帧按 **32 个控制器行 = 15,360 B** 分块发送，块间 `embassy_futures::yield_now().await`；描述符继续留在 blocking 调用自己的栈帧（内部 DRAM）。`FRAME_CHUNK_BYTES ≤ 32,736`（esp-hal 私有的 `MAX_DMA_SIZE`）由 `const _: () = assert!(...)` 在编译期钉住，越界会在运行时报 `MaxDmaTransferSizeExceeded`、表现为整帧不上屏。

**线上安全**：`panel_spi` 不接 CS（只有 `.with_sck().with_mosi()`），分块只是插入 SCK 空闲间隙，DC 全程为高，字节流与一次发完完全一致；渲染结果与页面行为不变。

接口面只多一条：`PageSurface::flush` / `Renderer::flush` 返回 `Option<Pin<Box<dyn Future>>>`，`Registry::flush()` 在 `tick` 之后 await 一次；绘制仍同步（只写 RAM），传输落在 flush 里。`camera-probe` 也 await 同一个 `write_frame`。

计量口径：`hold` = render + 线上总时间（CPU 负载没变，只是不再连成一整段），新增 `worst chunk` = 最长单次传输，也就是**真正的最长连续持有**。本节读数用的是旧口径；下一节把传输搬到第二个核之后，`hold` 只剩 render，`worst wait` 记的是被 park 的时间。

| 读数（Attitude 页，`IOT_BOOT_PAGE=Attitude IOT_LOG_LEVEL=debug`） | 改前 | 改后 |
| --- | --- | --- |
| 首窗 | `6 repaints in 2046 ms, held 110 ms (5%), worst render 3 ms, worst write 15 ms` | `6 repaints in 2724 ms, held 110 ms (4%), worst render 2 ms, worst write 17 ms (longest chunk 2 ms)` |
| 稳态窗 | `10 repaints in 2097 ms, held 193 ms (9%), worst render 4 ms, worst write 16 ms` | `12 repaints in 2202 ms, held 229 ms (10%), worst render 3 ms, worst write 16 ms (longest chunk 2 ms)` |

`worst write` 仍是 15–17 ms（线上时间不会因为分块变短），变的是它不再是一整段：`longest chunk` 1–2 ms。开机窗偶见的 6xx ms 是 WiFi 关联落在单块传输里，改前同样存在（同一时段 `[INPUT]` 报 `worst single hold 597 ms … worst source: touch 597 ms`），与分块无关。

`[实测]` 资源不变：开机 `[DISPLAY] … main stack 117520 B` 与 HEAD 相同（`nm` 读数 `_stack_start_cpu0 − _stack_end` = `0x3fce9710 − 0x3fcccc00`），`.data` 18,904 B、`.data.wifi` 496 B、`.bss` 214,784 B，App 1,055,264 B。回退 async 驱动顺带丢掉了它带来的 +88 B `.data`。

### Audio 重绘步长翻倍 + 面板传输搬到第二个核：core 0 持有 40% → 3%（`[实测]`，2026-11-01）

`[实测]` 上一节把一帧的**连续**持有从 15 ms 压到 1–2 ms，但 16 ms 的线上时间仍是 core 0 的账：`SpiDma<Blocking>` 的忙等只是被切成小段，整段仍归协作执行器。Audio 页把这件事放大到看得见 —— `[DISPLAY] Audio 44 repaints in 2059 ms, held the executor 819 ms (40%)`，而 `[INPUT]` 在同一构建的稳态窗里是 39%（`1970 ms of 5003 ms`，见下文），也就是说 core 0 的大头一直是采集折叠，不是面板。两处一起改，先改便宜的。

**一、Audio 页的重绘步长 40 → 80 ms。** 门槛从 `render::DIAG_REPAINT_STEP_MS` 挪到每页的渲染策略（`render::screen::policy::repaint_step_ms(page)`），只有 Audio 是默认值的两倍，其余页面仍是 40。理由是这一页的漂移来得比谁读得都快：capture 每 20 ms 折两列包络，40 ms 的步长已经把 200 列的扫线每 ~45 ms 重画一次；80 ms 下一帧推进八列，仍是扫线，代价减半。`the_audio_page_waits_twice_the_default_step` 钉住这条。

| 读数（`IOT_BOOT_PAGE=Audio IOT_LOG_LEVEL=debug`，~2 s 窗） | 改前 | 只改步长 | 再搬第二个核 |
| --- | --- | --- | --- |
| 重绘次数 | 44–45 | 27–30 | 26–30 |
| `held the executor` | 819 ms (40%) | 505–561 ms (24–27%) | **65–76 ms (3%)** |
| `worst render` | 3 ms | 2–3 ms | 2–3 ms |
| `worst wait` | — | — | 17 ms |
| `worst write`（`longest chunk`） | 16 ms (1–2 ms) | 16 ms (1–2 ms) | 15–16 ms (1 ms) |

**二、面板的传输搬到第二个 APP core。** 控制器和总线成为 `PanelBus`，`Screen` 只留一条通道；协调者把画完的帧当 `Ticket`（裸指针 + 长度，`unsafe impl Send`）投进 `REQUESTS: Channel<_, Ticket, 1>`，然后 park 在 `SHIPPED.wait()` 上等结果，`panel_service` 在另一个核上 `write_frame` 并回一个 `Result<Wire, Error>`。安全性靠握手本身：帧是 `'static` 的 `static mut` 竞技场，`Panel::write` 在票据整个生命周期内都停在 `wait()` 里不碰它，总线上只有一次请求在飞。

`[实测]` 触发点必须换：`esp_rtos::start_second_core` 硬编码 `FROM_CPU_INTR1`，而产品里 `Priority2` 的喇叭 feed executor 已经占着它，所以 feed 改到 `FROM_CPU_INTR2`（`InterruptExecutor<2>`），核间唤醒留在 INTR1。第二个核的栈是 16 KiB 静态量（`chip-esp` 的 `PANEL_CORE_STACK`）—— `.stack` 填满区域剩余部分，所以这是从主栈墙上切走的一块，不是额外拿到的一块。

`[实测]` 计量口径随之改：`hold` 现在只算 render（协调者自己的 CPU），被 park 的时间单列 `worst wait`。所以「40% → 3%」是**持有归属的转移**，不是线上时间变短：那 16 ms 仍在花，只是花在另一个核上；`worst write` 仍 15–16 ms，`longest chunk` 1 ms —— 但从这一节起，`longest chunk` 量的是 **core 1 的持有**，不再进 core 0 的账；它是面板服务自己那条 lane 的抖动指标，不能和 `[INPUT]`/`[DISPLAY]` 的持有相加。

| 资源（`size -A` / `nm`，同一棵树） | 改前 | 改后 | 差 |
| --- | --- | --- | --- |
| `.text` | 850,713 B | 857,729 B | +7,016 B |
| `.bss` | 214,784 B | 231,544 B | +16,760 B（16 KiB 栈 + 通道/信号量） |
| `.data` | 18,904 B | 18,928 B | +24 B |
| 主栈（开机日志 / `_stack_start_cpu0 − _stack_end`） | 117,520 B（`0x3fcccc00`） | **100,728 B**（`0x3fcd0d98`） | −16,792 B |

`[实测]` 行为没有回退：Attitude 页 `[DISPLAY] Attitude 5–9 repaints in 2041–2560 ms, held the executor 16–29 ms (0–1%), worst render 3 ms, worst wait 16 ms, worst write 15 ms (longest chunk 1 ms)`；两页 `stack peak 29848 B of 100728 B`，无 panic、无 `[DISPLAY]` 告警。core 0 剩下最大的持有者是 capture 折叠（`[INPUT] held the executor 1970 ms of 5003 ms`，~39%），本次不动。

`[实测]` 检查面：`check-s3`、`check-c6`、`check-host`、`check-features`（全部子集）、`build-s3`（只剩既有的 RWX 链接告警）、S3 目标的 `cargo clippy -- -D warnings`、`lint`、`lint-host`、`test`、`test-bsp` 本轮全部通过。

### 驱动半边整体搬到 core 1：core 0 只剩 render 的 4%（`[实测]`，2026-11-01）

上一节末尾把 core 0 剩下的 39% 记成「本次不动」的采集折叠。这一节动它：`input_task` 与 `control_loop` 连同它们持有的驱动成为一个 `Io` 束，由 `IoPlacement` 决定落在哪——`Elsewhere(&IoQueue)` 交给第二个核上的 `run_io_from`，`Inline(spawner)` 留在协调者自己的执行器上。单核 C6 和没有面板的 S3 走 `Inline`，任务与代码路径完全相同，只是位置不同。

| 读数（`IOT_BOOT_PAGE=Audio IOT_LOG_LEVEL=debug`，~2 s 窗，稳态） | 改前（面板在 core 1） | 改后（驱动半边也在 core 1） |
| --- | --- | --- |
| `[DISPLAY] Audio` 重绘 | 26–30 | 26–29 |
| `held the executor` | 65–76 ms (3%) | 83–95 ms (4%) |
| `worst render` | 2–3 ms | 3–4 ms |
| `worst wait` | 17 ms | 26–47 ms |
| `worst write`（`longest chunk`） | 15–16 ms (1 ms) | 15–16 ms (1 ms) |
| `[INPUT] held the executor` | 1970 ms of 5003 ms（39%），记在 core 0 | 1677–2026 ms of 5001–5007 ms（33–40%），记在 core 1 |
| `[INPUT] worst single hold` | — | 9–19 ms（bound 30 ms），capture 7–16 ms |
| `[PLAY] cadence` | — | 400 feeds in 2000 ms（100% of nominal），0 gaps over 20 ms，worst gap 5 ms，worst work 428–707 µs |
| `stack peak` | 29848 B of 100728 B | 24008 B of 91824 B |

`[实测]` 采集折叠的账几乎原样搬了家：`[INPUT] held the executor … of 5001 ms` 这条读数的分子分母都没变，只是从 core 0 换到 core 1；core 0 的执行器现在只剩 render 的 4%。代价单一且可指认——Audio 页的 `worst wait` 从 17 ms 涨到 26–47 ms：面板服务在 core 1 上与采集轮询（最长 7 ms，开机首个窗口 16 ms）和喇叭喂数排同一个协作执行器，一次完整传输 ~15 ms 加上这段，落在 30 ms 上下，偶发一次 47 ms。重绘数没变（26–29），多出来的等待没有丢帧。

`[实测]` 播放、运动与摄像头都没有因此变差：`[PLAY]` 满速 400 feeds/2 s、0 gaps；Attitude 页 `[DISPLAY] Attitude 5–9 repaints in 2022–2779 ms, held the executor 15–30 ms (0–1%), worst render 3–4 ms, worst wait 17–19 ms, worst write 15 ms (longest chunk 1 ms)`，同一窗口 `[INPUT] held the executor 632–637 ms of 5003–5005 ms`（12–13%，这页不开采集）、core 1 栈 1856 B；Camera 页稳态 `[DISPLAY] Camera 11 repaints in 2118–2120 ms, held the executor 3–4 ms (0%), worst render 0 ms, worst wait 17 ms, worst write 15 ms (longest chunk 1 ms)`，与改前同量级。Camera 页每次开机都会在第一个窗口报一次 `[CAM] no descriptor completed in 420 ms, stalled at 0 of 40, peripheral running (re-arm 1/2)`（同一窗口 `worst wait 639 ms`），此后不再出现；那条路径在 core 0 的渲染里（`advance(frame)` 在绘制里写面板帧），本次不动。摄像头仍走 core 0 的渲染链路（`advance(frame)` 在绘制里写面板帧），本次不动。

`[实测]` 资源：`.text` 857,729 → 857,321 B（−408），`.bss` 231,544 → 240,448 B（+8,904：第二个核的栈 16 → 24 KiB，其余是 `IoQueue` 与 `run_io_from` 的任务槽），`.data` 18,928 B 不变，App 1,062,400 B；主栈 100,728 → 91,824 B。

`[实测]` 那 24 KiB 现在有读数：每 5 s 一条 `[STACK] core 1 peak {high} B of {total} B`（涨 ≥1 KiB 才升到 info，否则 debug），三页各跑一轮：

| 页（`IOT_BOOT_PAGE=…`） | 常开的东西 | core 1 栈水位 |
| --- | --- | --- |
| Audio | 采集 + 面板 + 播放 | **4556 → 4780 B** of 24,576 B（19%） |
| Speaker | 面板 + 播放 | 2336 B |
| Attitude | 面板 + 运动轮询 | 1856 B |
| Camera | 面板 | 1920 B |

`[实测]` 最深的一条路是音频采集的轮询，24 KiB 里还剩 19,796 B 给后续的驱动任务。量法是把栈刷成 0xA5（字模式，单字节会撞上普通数据）再找最下面一个仍然完整的字，水位只增不减；扫描从栈底 512 B 之上开始——esp-hal 在第二核启动前会把自己的入口闭包停在栈底那几个字节里，不跳过就永远读成满栈（第一次量正是读成 24,512 B）。从 core 1 内部补刷一遍漆这条路试过，板子起不来，已回退：跨核写自己正在用的栈不可行。

`[实测]` 检查面：`check-s3`、`check-c6`、`check-host`、`check-features`、`lint`、`lint-host`、`test`（726 + 10 + 29 + 42）、`test-bsp`（101）、S3 目标的 `clippy -- -D warnings`、`build-s3`（只剩既有的 RWX 链接告警）全绿；C6 实机烧写后 Ambient 页正常起、WS2812 亮、无 panic。三个 trait object（`InputSource`/`EventAggregator`/`FrameSource`）与 `CaptureSink` 上的 `Send` 是编译期约束：束里放不进一个不能跨核的东西，不靠逐处检查。

### 网络工作者从 `join` 改成任务：主栈区 −64 B（`[实测]`，2026-11-02）

`main` 不再 `join(run, net::serve)`：网络是一个 `#[embassy_executor::task]`（那一版由 `run` 起），和驱动半边同一种生命周期。静态量从 `main` 的池挪到网络自己的池，总量只差一个任务头。同一棵树、同一条命令（`bash ../../scripts/iot-xtensa.sh build --release -p iot-app --bin vanling --target xtensa-esp32s3-none-elf -Z build-std=core,alloc --no-default-features --features lckfb-szpi-esp32s3,console-auto`）前后各量一次：

| 读数（`nm -S`，S3 `vanling`） | 改前 | 改后 |
| --- | --- | --- |
| `__embassy_main::POOL` | 2,528 B（`0x9e0`） | 1,432 B（`0x598`） |
| `network::task::POOL` | — | 1,168 B（`0x490`） |
| `_stack_start_cpu0 − _stack_end_cpu0` | 96,432 B | 96,368 B |

`[实测]` 两个池相加从 2,528 涨到 2,600 B（+72 B），主栈区只少了 64 B；`network::task` 的 future 只有 1,168 B，因为 `link` 里的大件（凭据库、扫描历史、`browsing` 的 future）本来就是堆上的。`_stack_end_cpu0` 落在 `0x3fcd1ea0`，距 65,536 B 的下限还很远。`check-s3` 只 type-check，这个差值只有链接后的 `nm` 看得见。

同一份镜像的开机日志把「任务真的起来了」对上号（那一版 `run` 才起工作者，所以顺序是）：`[IOT] starting the net worker` 出现在 `[IOT] demand …` 之后、`[DISPLAY] stack peak` 之前，紧接着 `[WIFI] network stack up, DHCP requested`；`[DISPLAY] backlight dark until the first frame; main stack 96368 B` 与 `[DISPLAY] stack peak 14917 B of 96368 B` 和 `nm` 的分区尺寸一致，说明池搬家没有改变主栈区的大小。

### 网络工作者改在组合点 `spawn`：池 −16 B，主栈区 +32 B（`[实测]`，2026-11-02）

`WorkerStart { name, start }` 删除：网络任务的驱动类型只有入口认识，于是 `main` 在交给 `run` 之前直接 `spawn`，`run` 不再持 worker 清单，也不再拥有「这次要起哪些工作者」这件事。同一棵树、同一条命令：

| 读数（`nm -S`，S3 `vanling`） | 上一版 | 本版 |
| --- | --- | --- |
| `__embassy_main::POOL` | 1,432 B（`0x598`） | 1,416 B（`0x588`） |
| `network::task::POOL` | 1,168 B（`0x490`） | 1,152 B（`0x480`） |
| `_stack_start_cpu0 − _stack_end_cpu0` | 96,368 B | 96,400 B |

`[实测]` 池合计少 32 B、主栈区多 32 B，两者互为镜像，净额为零；没有逐符号归因。`_stack_end_cpu0` = `0x3fcd1e80`。App 1,062,128 B（S3）/ 845,392 B（C6，分区 80.62%）。

`[实测]` 开机日志把新的启动顺序和启动页结论都钉住了：S3 先打 `[IOT] starting the net worker`、再打 `[IOT] demand …`（因为 `spawn` 在 `run` 之前），`[DISPLAY] … main stack 96400 B` 与 `nm` 读数一致；`[INPUT] the splash is over; setup mode was not asked for` 之后 8 ms 就出现 `[WIFI] this device saved a network; joining …`，说明工作者确实在等 `NetworkCommand::SplashOver` 而不是别的信号。C6 同样 `[IOT] starting the net worker` → `[WIFI] network stack up`，启动页结束后立刻 `[WIFI] no page surface and no saved network; waiting offline`，WS2812 走完离线提示回到页面自己的灯。两块板都无 panic。

`[未重测]` 之后的 Wi-Fi 重写把启动点收回 `run`：入口只交出 `Wifi::Worker(NetworkWorker)`（一个接收 `Spawner` 的闭包），`run` 在 `run_loop` 之前执行它。上表的两个池读数是「入口直接 `spawn`」那一版的；这次重写只跑了 `check-s3` / `check-c6` / `check-host`（均只 type-check），没有重新链接，因此没有新的 `nm` 读数。

### IRAM 的 DRAM 镜像 47,396 B：一个字节都不属于本仓（`[实测]`，2026-11-01）

`[实测]` S3 的 IRAM 与 DRAM 是同一块 SRAM 的两个视图，所以每一段放进 IRAM 的代码都在 `dram_seg` 里占一份同样大的镜像 —— `.rwdata_dummy` 就是这个占位，**47,396 B**，由三段相加而来（`objdump -h`）：

| 节 | 字节 | 是什么 |
| --- | --- | --- |
| `.vectors` | 1,024 | 中断向量表 |
| `.rwtext` | 14,392 | esp-hal / esp-rtos / esp-storage / xtensa 运行时放进 IRAM 的代码 |
| `.rwtext.wifi` | 31,980 | 射频 blob 的 IRAM 副本（ESP-IDF 对 blob 的硬要求） |

`[实测]` 把 `.rwtext` 按符号归类（`nm -S` 取 `0x40378400..0x4037bc38`）：

| 归属 | 字节 | 为什么必须在 IRAM |
| --- | --- | --- |
| esp-hal | 4,843 | PSRAM 初始化与映射（改 cache/SPI0 时不能从 flash 取指）、GPIO 与 I2C 的 ISR、cache 配置 |
| esp-rtos | 3,547 | 调度器：`trigger_task_switch`、`RunQueue`、`timer_tick_handler`，全在中断里跑 |
| xtensa 运行时与异常/中断入口 | 1,789 | `Reset`、`save_context`/`restore_context`、`__level_{1..7}_interrupt`、`__INTERRUPTS` |
| esp-radio / phy 胶水 | 1,048 | `semphr_take/give`、`interrupt_dispatch`、`tx_done_cb`、`__esp_phy_*`、`__pender` |
| esp-storage | 765 | `spiflash_read/write/erase_*`：写 flash 期间 cache 是关的 |
| 链接符号表与函数间对齐 | 2,400 | `sym_*` 那张地址表（984 B）加 127 个符号间的对齐缝隙 |

`[实测]` **本仓在这 14,392 B 里占 0**：`rg '#\[ram'` 全仓只有一处，是 `radio/esp32s3.rs` 的 `RECLAIMED_MEM`（一块数据，不是代码）。所以「把不需要 IRAM 的函数移回 flash」这条在本仓没有对象 —— 能动的只有 esp-hal 自己的取舍（PSRAM 初始化的 ~1.7 KB、`gpio::wakeup::record_wakeup` 的 410 B），而那要 patch 依赖，不做。

`[实测]` **D-cache 32 → 16 KB 不是选项**：S3 只有 32 KB 与 64 KB 两档。把 `ESP_HAL_CONFIG_DATA_CACHE_SIZE` 改成 `16KB` 时 esp-hal 的构建脚本直接拒绝：

```
Validation error for crate esp-hal, option data-cache-size: Expected one of ["32KB", "64KB"], found '16KB'
```

`[实测]` **第二个核不是白拿的**：`Stack<N>` 是 DRAM 里的静态量（探针用 8 KiB / 16 KiB），也就是直接从主栈墙里切同样大的一块；而且 `esp_rtos::start_second_core` 的签名固定要 `FROM_CPU_INTR1`，产品里那个 `Priority2` 的喇叭 feed executor 原本占着它（`start_feed_executor(feed_intr)`）。用第二个核得先把 feed 挪到 `FROM_CPU_INTR2`，再算上跨核同步的代码量 —— 这是一次架构改动，不是一次调参；面板传输这一次的代价与收益见上文「Audio 重绘步长翻倍 + 面板传输搬到第二个核」（`.text` +7,016 B、`.bss` +16,760 B、主栈 −16,792 B）。

---

## `IOT_BOOT_PAGE` 必须在构建期解析（`[实测]`，2026-10-29）

`[实测]` 症状：任何**设置了** `IOT_BOOT_PAGE` 的构建都在连网时死——日志停在 `[WIFI] this device saved a network; joining …` 之后，异常是 `StoreProhibited`（有时 `LoadProhibited`），PC 落在 esp-radio 的 `os_adapter::free` / `event_post` 里，`EXCVADDR` 是 `0x3`。同一份源码不设变量构建、烧进同一块板，每次都连上并拿到地址；HEAD（`a13e96f5`）设 `IOT_BOOT_PAGE=ambient` 也能连（峰值 41,880 B of 65,912 B）。

`[实测]` 崩的不在主栈上：异常的 `THREADPTR = 0x3fcd4908` 是电台任务的 TCB，`A1 = 0x3fcd4570` 比主栈底 `_stack_end = 0x3fcd9568` 低 20,472 B——那是电台任务自己的栈。写坏的是电台 blob 的静态量，`sta_con_timer`（`0x3fcd8974`）起那一整片就紧贴在主栈底下方。`.cargo/config.toml` 的 `ESP_HAL_CONFIG_STACK_GUARD_MONITORING = "false"` 让越界不出任何告警，只以「别人的代码崩了」的形式出现（见下一节）。

`[实测]` 差异锁在一处：`boot_page()` 当时在运行期解析 `option_env!`——`trim().to_ascii_lowercase()` 成 `String` 再查表。同一份代码，不设变量时连接处用到 23,409 B、峰值 42,392 B；设 `IOT_BOOT_PAGE=ambient` 时 27,745 B / 46,728 B，**+4,336 B**，然后必崩。多出变量那一版的主任务 poll 帧是 `entry a1, 0x6710`（26,384 B），HEAD 是 `entry a1, 0x5540`（21,824 B）——后者含本轮其它改动，所以帧的绝对差不能全记到启动页账上，能逐项对齐的是上面那对同源读数。

`[未分离]` 那 4,336 B 具体压坏了哪一块没有分开：可能是主栈越界、直接写进 blob 的 `.bss`；也可能只是帧变大改变了 internal heap 的布局，让电台任务的栈落到了 blob 的静态量上。两种都能解释这组读数，也都不影响修法。

`[实测]` 修法：启动页在**编译期**落成常量——`const BOOT_PAGE: Page = boot_page_from_env(option_env!("IOT_BOOT_PAGE"))`，`const fn` 做去空白与大小写比较；非法名字另存 `const UNKNOWN_BOOT_PAGE: Option<&str>`，由 `boot_page()` 打那行 `[IOT] IOT_BOOT_PAGE=… is not a page`（名字合法时它是 `None`，`if let` 整块被折掉）。修后 `IOT_BOOT_PAGE=attitude` 与产品构建逐项同值（`main stack 65,976 B`、峰值 `42,392 B`、连接处 23,409 B），并正常关联、拿到 `192.168.1.136`。桌面测试 `only_a_name_no_page_answers_to_is_reported` 钉住「非法名才报警、合法名在各种拼写下都不报」。

**约束**：启动页留在编译期。bench 镜像与产品镜像共用同一张栈预算，这就是它的前提——上表六页的 `main stack` 能一字不差，靠的是启动页不参与运行期布局。

---

## esp-hal 的栈溢出 watchpoint 会误触发

`[实测]` esp-hal / esp-rtos 在硬件 STORE watchpoint 上做栈溢出保护，而被 arm 的窗口会压到合法写入，抛出 level-6 Debug 异常把 boot 干掉（曾在 esp32s3 的渲染 `on_appearance` 上观察到）。

因此 `.cargo/config.toml` 关闭硬件检测、开启软件 canary：

```toml
ESP_RTOS_CONFIG_HW_TASK_OVERFLOW_DETECTION = "false"
ESP_HAL_CONFIG_STACK_GUARD_MONITORING = "false"
ESP_RTOS_CONFIG_SW_TASK_OVERFLOW_DETECTION = "true"
```

软件 canary 在每次上下文切换时检查栈底以下的哨兵，并以出错的任务名 panic——比静默破坏内存好。
---

## 面板的字模：12 px 点阵 + 24 px 矢量正文（`[测试]`，2026-11-21）

字模在 `apps/iot/crates/core/src/ui/font.rs`，表是 `crates/core/assets/font/ui12.bin`（486,881 B），由 `cargo run -p iot-tool -- font` 从 `crates/tool/assets/fonts/ui12.bdf`（2.1 MB，随字体附 MIT 与 OFL 两份许可）裁出。BDF 是 TakWolf 的 Fusion Pixel 12px Mono：`zh_Hans` 的 GB2312 7,475 个字形为底，再用 `font --merge` 并入 `zh_hant` 的 Big5 全表（`big5.txt`，13,706 个码点），共 15,466 个字形。Big5 有 866 个码点（805 个次常用汉字 + 61 个符号）上游 `zh_hant` 本身没有字形，按 `MISSING` 画 □；其余 12,840 个都有。`SIZE 12 75 75`、`FONT_ASCENT 10`、`FONT_DESCENT 2`。

- **表格式 `VLF1`**：32 B 头（字形数、行高、升部、降部）+ 每字形码点 `u16`、打包度量 `u32`、位图偏移 `u32`、位图。码点有序，二分查找；`Face::glyph/advance/measure/glyphs` 是唯一入口。
- **等宽**：ASCII 步进 6 列，汉字 12 列——一个汉字正好两格，网络名、口令与中英混排都落在同一张网格上。行高 12、行距 13（`LINE`/`ROW_PITCH`），升部 10 行、降部 2 行。
- **面板度量跟着字面走**：页脚 12 行（`FOOTER_ROWS`）、内容底边 228（`CONTENT_BOTTOM`）、左右边距 10（`MARGIN`）、触控最小 44 px（`TOUCH_MIN`）。`panel.rs` 的 `FACE_HEIGHT == LINE` 与 `the_panel_is_measured_in_the_face_it_draws_with` 把「字号改了而版面没改」挡在编译/测试里。
- **网络页的按压目标因此变高**：列表 3 行 × 45 px（`LIST_ROW_H >= TOUCH_MIN`），动作条顶边 184，键格 45 px，确认框 118 px、每行 34 格。网络名按像素截断（`ui::text::fit`），缓冲区按 UTF-8 字节而不是字形数：`NAME_BYTES = SSID_BYTES + 1`（截断标记占一字节）、`DIALOG_BYTES` 另留前缀与引号。
- **回归**：`font.rs` 钉住两套表的表头与度量（点阵 ASCII/汉字步进、缺字 fallback、非法 UTF-8 不卡住、`中` 保持 BDF 字框；矢量面 `中`=24 / `A`=12 px @24、升/降 21/3、`covering` 整行回退），`panel.rs` 钉住负号与字号一致，`typeset.rs` 钉住禁则与截断，`declare.rs` 钉住底色/span/装饰线/字号与滚动盒裁剪，`canvas.rs` 钉住矢量落墨的包围盒、AA 边缘、缺字回退与裁剪，`services/text.rs` 钉住样张被矢量面全覆盖，`services/tests.rs` 的 18 行黄金哈希钉住整页像素（`text` 行随本页改动更新，其余 16 行未变）。

- **矢量表 `VLV1`**：`crates/core/assets/font/vector.bin`（88,274 B，181 个字形 / 20,756 个折线点），由 `cargo run -p iot-tool -- outline` 用 `ttf-parser` 从 `crates/tool/assets/fonts/NotoSansMonoCJKsc-Regular.otf`（16,393,784 B，OFL-1.1，许可见同目录 `LICENSES/noto/OFL.txt`）裁出；码点集合就是 `core/assets/font/vector-sample.txt`，工具与页面共用这一份。Noto Sans Mono CJK 同时覆盖 CJK 与半角 Latin，因此不再需要 Hack；MiSans、HarmonyOS Sans 因禁止修改再分发而不引入。
- **表内容**：32 B 头（字形数、码点数、em = 64、四段偏移、typographic 升/降 56/8）+ 码点→字形对 + 每字形 18 B 元（步进、点区间、轮廓区间、包围盒）+ `(i16 x, i16 y)` 点（1/64 em，y 向上）+ 轮廓终点（绝对点号）。坐标不存 1/4 em：24 px 下那是 6 px 的格，圆弧会被量化成台阶。
- **行高包住墨迹再加字号的 1/8**：表头存 OS/2 typographic 升/降与全表墨迹包络的较大者（`g` 的 `ymin = -15` 比 typographic 降部更深），`Metrics::height() = ascent + descent`，`typeset::line_height = height + size/8`（24 px → 30）。只取 typographic 对（56 + 8 = 1 em）会把矢量行压到 24 px，`g`、`，` 等降部被自己的行盒切掉。`Metrics::ascent/descent` 对点阵是 `Face` 值 × 整倍放大，对矢量是 `units * size / 64` 四舍五入。
- **矢量面缺任一码点就整行退回点阵**（`Metrics::covering`），一行不会半矢量半点阵；正文里出现表外字符时 `the_wrap_paragraph_is_covered_by_the_vector_face` 会报出缺哪个字并提示重跑工具。
- **字号与单位**：`Style::font_size(Length)` 落成 `u8` 像素值；`Px` 精确，`Dp` = 5/4 px（2.0" 320×240 = 200 dpi，逻辑像素按 160 dpi 定义），`Rem` = 1/100 × 12 px，`Percent` 按根字号解释；`Auto` 与解析不出的百分比退回 12 px。
- **颜色与 span**：`Color(u32)` 是 ARGB32；`Style`/`RunStyle` 只存 `ColorId`（声明期 16 槽调色板去重，溢出在 debug 断言），`Canvas` 落墨时按覆盖率 src-over 到 RGB565，半透明因此与底下的像素混合。`Ui::span` 最多 8 条，只改颜色/装饰线/底色，不改行度量。
- **静态占用**：两张表合计 575,155 B 在 flash；`Ui` 11,896 B（60 节点 × 160 B + 文本 2 KB + span 表，上限 12,288 B），`Style` 84 B，`Length` 4 B。

字模在 flash：`ui12.bin` 486,881 B、`vector.bin` 88,274 B；BDF 与 OTF 只在工具侧，不进镜像。

## Text 页：两段独立滚动区，正文 24 px 矢量（`[测试]`，2026-11-21）

Text 页把长内容拆成两段命名滚动区：`WRAP` 是一段中英混排的换行长文（矢量面 24 px），`GLYPHS` 是逐行 12 px 字模样张。两块各 306×90 px，内容 336 / 216 px，可滚 246 / 126 px；正文样张在 24 px 下断成 25 行，页面把 25 行全部声明出来（窗口正好填满，不再出现 `…`；更长的段落仍在第 25 行截断）。

- **布局不夹偏移**：`Style::scroll_area(id)` 只命名并记录矩形与可滚距离，`Ui::scroll_info(id)` 读回；限界与回弹留在页面的 `ScrollState`，因为只有它知道越界多远。
- **构造时量两次**：面板 320×228 与内容都是静态的；第一次空跑拿到区宽，`typeset::wrap` 按该宽度断一次，第二次带着断好的行再布局并 `measure` 两区矩形与行程。
- **跟手**：`drag` 是 `[Option<Drag>; TEXT_AREAS]`，按下记 `(id, area, y, at_ms)`，按 id 找回自己那一格；越过 2 px 死区后 `GestureEvent::Drag` 每移动 2 px 报一次，页面用 `drag(area, delta)` 直接改偏移，越界半程阻尼。未按下的 id 在区内第一次 `Drag` 时认领该区，基线取当前点，所以丢了 `Press` 也不会跳；已有区的 id 不抢。两根手指各拖一区互不影响；两区都被按住后第三根手指找不到槽位就整条忽略，不会夺走已按住的区。被按住的区 `tick` 不弹回，另一区照常滑行；`on_hide` 清空全部槽位。抬手 `fling` 用拖动尾速（每个样本的瞬时速度取滑动平均，首样本直接采用），但只认抬手距最后一个拖动样本 ≤30 ms（三次触摸扫描）的尾速：更久说明手指在抬手前已经停住，尾速作废；长按（`Release.at_ms = None`）一律不甩。循环在手指停住时会 park，按帧数计的「静止」判据根本不走，所以这个判断必须用输入时钟。拖动过的抬手不补行，两区可同时带速度滑行。
- **离散意图不丢**：输入环满时 `Press`/`Swipe`/`Tap`/`LongPress`/`Button` 走 `IntentBus::send().await` 等空位；只有 `Drag`/`Touch`/`Motion`/`ChipGesture`/`Ghost` 允许被拒——丢一个 `Press` 整条手势就没了，丢一个 `Drag` 下一次采样会补上。
- **触点身份按近邻配对，槽号只破平局**：FT6336 的突发里带 `Pn_YH[7:4]` 槽位号（无效为 `0x0F`），驱动把它作为 `RawPoint::hw_id` 透传，`>= 2` 或 `0x0F` 记为 `NO_ID`。这块自容式面板会在两次采样间互换两点，所以 `TouchContinuity` 把这一帧的全部候选（新点 × 活动点，80 px 内）按距离从小到大配对，先到先得；不是按上报顺序逐点就近抢——第二根手指落在一根已按住的手指附近、又被面板先报时，就近抢会把它认成旧 id，旧手指反而变成新点，页面上一根手指跳 40 px、另一根找不到槽位，双指就只剩一指在动。距离完全相等时才用槽号裁决。够不着任何活动点就是新触点，不丢弃——按槽号硬配会把两指同时移动误判成跳变。幻点判据的半径是 16 px（一个指腹宽），只吃掉按在同一位置的重复回波；并排两指即使同时落下也各自成点。抬手要连续 3 个采样（约 30 ms）缺席才确认释放。这段时间里另一根手指在同一位置附近落下会续用旧 id，页面就当成同一根手指。被幻点判据拒绝的触点记住第一次落下的位置，之后被接受时 `Press` 报该位置（区绑定与 tap 落点都不漂），拖动基线仍取当前采样，所以延迟接受不会让内容跳。
- **滑动/滑行**：没有拖动过的快速上下滑仍先走 3 行（字模区 36 px，Wrap 按自己的 24 px 行高走 72 px），再按手势平均 `distance/held` 起滑；≥250 px/s 才滑，封顶 1600 px/s，每 20 ms tick ×0.875，<150 px/s 停，滑行约 `0.16·v0`。抬手距最后一个拖动样本超过 30 ms、或本来就是一记长按，释放速度为 0：拖完停住再抬手不会继续滑。
- **回弹**：越界上限是 `viewport/4`（区装得下或关掉回弹时为 0），越界后额外 ×0.5 阻尼，松手后每 tick 补一半差距，≤1 px 吸附。`ScrollConfig { bounce, bar }` 是页面唯一的开关，两者都不改布局，所以切换无需重测。
- **滚动条**：右侧 2 px 槽常驻；thumb 高 `max(8, viewport²/(viewport+max))`，位置 `(viewport-height)·clamp(offset,0,max)/max`，`max == 0` 或关掉时隐藏。thumb 用 `bordered()` 而不是 `filled()`——Text 页不声明底色，`fill_rect` 没有 ground 可翻，2 px 边框读起来是实心且与字形一样反色。
- **动起来**：`PageSystem` 按墙钟增量调 `LocalPage::tick`（首帧 0，夹到 `MAX_CATCH_UP_MS = 5·STEP_MS`），不再每次循环固定 `STEP_MS`：一次 52 ms 的光栅化曾让滑行只推进 20 ms，滚动正好在最慢的页上慢三倍。`FrameSnapshot::animating()` 让渲染器保持 `TimeDriven`；`Repaint::WhenOrOverlay` 在 Text 自己的 20 ms 步长内补画收尾那一帧（共享地板是 40 ms，但滚动帧只搬运已有像素、不重新栅格化，本页因此可以按帧循环的节拍走；40 ms 会让一次拖动两帧才动一次，读起来就是卡）。
- **稳态页**：`PageId::steady()` 只对 Text 返回真，快照带 `steady`；屏幕背光因此常 100（关灯才降 0），灯带停在声明颜色的相位 0 不再呼吸，页面滚动仍按 `animating()` 计时。`render/screen` 与 `render/lamp` 各有测试钉住这一点，`shared/route` 的 `only_the_reading_page_holds_the_light_still` 钉住稳态集合。
- **排版样例**：正文首行加粗 + 下划线，前 6 个字节用红色 span（`RunStyle::color` + `decoration_color`）；span 只换颜色/装饰线，不改行度量，断行结果与无 span 时相同。
- **粗体只落一次**：首行加粗不是把字形在 `left`、`left+1` 各落一次——在反色地面上第二次会把第一次的像素翻回去，首行因此发空。现在位图面按 `ink(x)|ink(x-1)`、矢量面按 `over(a,b)=a+b-ab/16` 合成一张覆盖掩码后只落一次；字形缓存键加入 weight，heavy 只在其多出的一列仍装进 24×24 槽时缓存。`canvas` 测试钉住重墨内部不被反色抵消、缓存与不缓存逐字节相等；黄金表只有 `text` 两行更新。
- **布局只量一次**：flex 跳过带显式 `basis` 的子节点的自然主轴测量（该值本来就被丢弃），零横向空间的滚动盒不排内容。Text 页的段落只在构造时 `wrap` 一次，声明是一行一个 `nowrap` + 显式宽度的节点，`layout_text` 对它们跳过 `max_content`，一帧不再逐字查字模表；18 行整页黄金哈希守着旧像素。
- **`[实测]` S3（12 px 点阵版，2026-11-20）**（240 MHz，`IOT_BOOT_PAGE=text`，DEBUG 日志）：首窗 `11 repaints in 2107 ms, held the executor 184 ms (8%), worst render 19 ms (fill 1), worst wait 21 ms, worst write 15 ms`，稳态 `5–7 repaints in 2019–2560 ms, held 79–111 ms (3–5%), worst render 15–16 ms (fill 1)`；改前同页是 `6 repaints in 2220 ms, held 1559 ms (70%), worst render 265 ms`。主栈 86,216 B、峰值 30,364 B，无 panic。静止页不重绘，拖动时的逐帧开销只能靠手指实测。
- **`[实测]` S3（24 px 矢量版，缓存前，2026-11-21）**（240 MHz，`IOT_BOOT_PAGE=text`，DEBUG 日志）：首窗 `6 repaints in 2355 ms, held the executor 67 ms (2%), worst render 52 ms (fill 1), worst wait 637 ms, worst write 16 ms`，次窗 `3 repaints in 15478 ms, held 160 ms (1%), worst render 54 ms`；主栈 86,216 B、峰值 30,108 B，无 panic，背光正常。52–54 ms 已靠近 40 ms 重绘地板，滚动时一帧就是这个量级。
- **第一版光栅器是 238 ms，改法是少做重复工作**：旧实现每画一个字形都把全部轮廓点按字号缩放四遍（四条子扫描线各一遍），每个像素行再扫一遍全部边；S3 上一帧 212 次矢量落墨、628k 次边遍历，`worst render 238 ms`。现在每个字形只缩放一次（`scaled`，1.5 KB），边按“采样窗口第一次够到它”的行分桶，逐行只遍历仍跨行的边（`heads`/`chain`/`active`），四种子扫描线共用一次边遍历。181 个字形逐像素哈希与旧实现完全一致，黄金表未动；主栈峰值不变（渲染在 86 KB 主栈上）。
- **`[实测]` S3（24 px 矢量 + 字形缓存，2026-11-21）**（240 MHz，`IOT_BOOT_PAGE=text IOT_LOG_LEVEL=debug`）：首窗 `6 repaints in 2367 ms, held the executor 75 ms (3%), worst render 55 ms (fill 1), worst wait 645 ms, worst write 16 ms`——首帧是冷缓存，成本仍是逐字栅格化；之后的窗口 `3 repaints in 11884 ms, held 90 ms (0%), worst render 30 ms`。主栈 86,216 B、峰值 34,268 B，无 panic。稳态绘制从 52–54 ms 降到 30 ms。
- **字形覆盖率缓存**（2026-11-21）：字形落在整像素坐标上，覆盖率只取决于码点与字号（轮廓在自己盒内的亚像素相位固定），所以 `GlyphCache` 按 `(char, size, weight)` 收 192 个 24×24 槽，每像素 5 bit——覆盖率 0..=16，4 bit 会把满墨的最后一格变成半透明。首次落墨时栅格化进槽，之后只 `blit`；表在堆上（约 69 KiB，S3 的 PSRAM 堆，C6 无屏不分配），因为主栈余量只有几百 B。宿主 release 下 3,620 次 24 px 落墨 42.7 ms → 16.3 ms（11.8 → 4.5 µs/字形）；`a_cached_glyph_lands_on_the_outline_s_pixels` 把 181 个字形在三个位置各画一遍，与不走缓存的帧逐字节相等。
- **镜像大小**（`moon run iot:image`，2026-11-22）：S3 `1,798,672 / 6,291,456 B (28.59%)`，C6 `1,614,288 / 6,291,456 B (25.66%)`；两张表进镜像后仍有 4.4 MB / 4.6 MB 余量。
- **`[实测]` 烧录后启动**（2026-11-21，显式 `IOT_PORT`）：S3 `[DISPLAY] stack peak 34,280 B of 82,840 B`、`[STACK] core 1 peak 2,160 B of 24,576 B`，首窗 `9 repaints in 2031 ms, held 544 ms (26%), worst render 62 ms (fill 1), worst wait 19 ms, worst write 15 ms (longest chunk 1 ms)`（冷缓存）；C6 打到 `[WATCHDOG] the first frame was shown and the network path settled; this boot is healthy`，无 panic。
- **滚动帧：落墨只写窄条，上屏写整框**（2026-11-21）：`AppPainter` 比较上一帧的两个滚动区偏移，`memmove` 已有像素后算两份 `FrameDamage`。**落墨集**只含进入视口的窄条、头部读数与 thumb，`Canvas` 按它裁剪落墨、`clear_damage` 按它铺回 ground，所以滚动帧不重新栅格化。**上屏集**把整个滚动框连同 header 与 thumb 交给 `St7789`：面板是 GRAM，不会自己搬像素，只写窄条会让框里其余行停在拖动开始时的内容上——旧文字成条残留、新内容从旁边滑过。快甩（`|delta| ≥ rect.h`）不再退回整帧：进入条钳到框高、`scroll_blit` 自行跳过，于是整框重墨但只上屏这个框；换页仍走整帧。两集都 ≤7 块（两区各 3 块 + footer），`St7789` 用 `CASET`/`RASET` 逐块写。实测几何下两区同时滚动的上屏量是 `2×(306×90 + 310×13 + 2×90)` ≈ 127 KB，加 footer 约 135 KB，按 80 MHz 约 12.7–13.5 ms（**推算，未实测**：滚动帧的 `worst write` 要手指按住才录得到）——接近整帧 153.6 KB，省下的是栅格化（30–55 ms → 搬像素约 1 ms），不是线上字节。底色改变、换页与相机帧仍走整帧，`ScreenPainter::reset` 让画布忘掉不可复用的上一帧。**调试徽标曾让这条路整条失效**：`keeps_frame` 一看到 overlay 就返回 false，产品镜像（`OVERLAY = true`）因此每帧整帧重绘，S3 实测拖动时 ~35 ms/帧、约 20 fps；现在徽标只把底部 320×12 的 footer 加进两份 damage，滚动帧仍搬运像素，徽标读数也继续刷新。`render::screen::painter` 钉住符号、上下搬运、两集块数与整框/窄条高度、清 ground、overlay 下仍保留滚动帧、footer 带尺寸，以及「把上屏集应用到模拟面板后与 buffer 逐字节相等」；`ui::canvas` 钉住脏矩形外的填充/字形不落墨。
- **抬手按按下点找区会永久占住一区**（2026-11-22）：`TextPage::bind` 原来用按下点 `area_at` 解析 Release，而 `Press` 在框外（header、margin、间隙、footer）不生成意图；手指在框外按下、拖进框后由 `Drag` 认领，抬手时按下点仍在框外，Release 直接 `None`，`drag[slot]` 再不释放——该区之后慢拖全被 `claim` 拒绝，快甩因 Release 落空走 `scroll()` 兜底还能动，换页 `on_hide` 才复位。现在 Release 先按 id 在已认领的 `drag` 里取 area，取不到才退回按下点；`a_press_outside_a_box_that_drags_in_still_releases_it` 钉住「框外按下 → 拖进 → 抬手 → 再慢拖必须动」。
- **只写脏矩形的第一版把面板写黑**（2026-11-21）：`write_rect` 发完 `0x2C` 后直接调 `write_chunk`，而 `write_chunk` 不碰 DC——`emit_command` 在无参数时把 DC 留在低电平，于是整块矩形的像素被当成命令字节送进 ST7789。抗锯齿覆盖率、accent 红与 thumb 让这几千字节里必然出现 `0x01`（SWRESET）、`0x28`（DISPOFF）一类命令；面板被复位后应用只再发 `0x2A/0x2B/0x2C`，不会重新初始化，所以第一次滚动就黑屏且不再恢复（不是 panic：串口仍在打 `[DISPLAY]`/`[INPUT]`）。现在 DC 由 `write_chunk` 自己拉高——它的两个调用方（整帧的 `write_pixels` 与矩形的 `write_rect`）都只发像素数据。同一条失败路径也不再冻结：`flush_pending` 写失败时按这一帧的 ground 重填并置 `page_dirty`，下一次重绘整页，而不是留下 `NeedsPaint` 让 `repaint` 因 `painted_color()` 为 `None` 一直早退——稳态页的底色不再变化，也就没有别的东西会重填。
- **回归**：`services::text::tests` 钉住两区都可滚且互不影响、滑点在区内才路由、拖动跟手、丢 `Press` 的 `Drag` 认领（基线不跳）、框外按下拖进后抬手仍释放该区、抬手滑行不补行、拖完停住再抬手不滑（距最后样本 >30 ms）、长按不甩、快速滑动仍补 3 行、拖动后的点击不补行、横向/区外忽略、`on_hide` 归零、样张无缺字、节点数与文本字节上限、ASCII 单格与样张对齐、双指端到端各拖一区、抬一指另一指继续、两区同甩、第三指忽略、样张被矢量面全覆盖、25 行与 56 节点预算；`drivers::input` 钉住幻点拒绝后 `Press` 报首次落点、拖动基线取接受时的采样、上报顺序互换仍保 id、后落手指被面板先报也不抢旧 id、无槽号按最近配对、双指同滑 10 帧不断流、同距才用槽号破平局、越界点不拖旧点；`ft6336` 钉住一次突发读出两点与 `0b01` 抬手帧被跳过；`render::screen::painter` 钉住滚动符号、上下搬运、两集块数与整框/窄条高度、清 ground、快甩仍走滚动路径；`ui::canvas` 钉住脏矩形外不落墨；`shared::kit::scroll::tests` 钉住速度阈值、滑行距离、回弹归位、按住不弹与两个开关；`services::tests::a_text_scroll_moves_one_region_and_its_thumb` 钉住偏移进画面且 thumb 跟着走。

## 一帧的声明占用（`[测试]`，2026-11-21）

布局层不保留树：`Ui` 在一次 `view` 里攒下声明，布局一次、落墨一次。节点数组 `MAX_NODES = 60`，每个节点 160 B；页面自己最满的是 Text（25 行 + 18 行字模 + 两个 thumb + 页壳 = 56），落墨时再加角标，离上限还有四格。`Ui` 整体 11,896 B，落在面板核 24 KiB 栈的预算内，`ui::tests::a_declaration_fits_the_stack_the_panel_core_was_given` 把它钉在 12 KiB 以内；`Style` 84 B、`Length` 4 B 也各有钉住的测试。

## 数字不能带动布局（`[测试]`，2026-11-20）

Attitude 页曾把三列读数都写成 `grow(1)`：flex 的起点是每列的**内容宽**，位数一变列宽就重新分配，整块文本跟着左右跳。修法分两层：

- **读数行定宽**：标签 4 格 + 空格 + 值右对齐到 7 格（`chrome::READING = 72 px`），值超出 7 格时保留自身宽度而不截断，行长因此不随数字变；
- **列宽显式**：三列各 `width(72)`，与内容无关；表盘 `absolute().right(MARGIN).bottom(0)` 放在 `stack` 的右下角，声明在文字之后，z 序在上。

顺带补了两处引擎缺口：`stack` 被父级 stretch 时，绝对定位子节点拿到的仍是内容宽（现在按 tight 约束定边）；`inset` 原来用 `0` 兼表「未设置」，`right(0)`/`bottom(0)` 因此无从表达，改为 `Option<i16>`。

回归：`services::attitude::tests::{the_numbers_do_not_move_the_layout, the_dial_owns_the_bottom_right_corner}`（零样本与负满位样本逐节点 rect 相同；表盘贴内容区右下边且不与文字相交）和 `ui::layout::tests::{a_stretched_stack_reaches_its_parents_edge, an_inset_of_zero_means_the_edge_not_an_unset_side}`。

## 一帧的绘制：布局把同一段文本量了五十遍（`[实测]`/`[测试]`，2026-11-20）

`[实测]` 起点是 Attitude 页每帧约 104 ms、Ambient `worst render 92 ms`。归因不靠猜：临时把 `Screen::paint` 里的 render 拆成 `fill` / `layout` / `draw` 三段，串口打出 `[PAINT] Attitude body 0.70 ms, layout 98.9 ms, finish 102.4 ms` —— 布局占 97%，落墨连同第二遍布局约 3.5 ms。

布局贵在重复测量：host 上数出 `wrap` 每帧 2,394 次，而 Attitude 只有 45 个文本节点，约 53 次/节点。flex 的每一趟都要问子节点尺寸（测量、摆放、再测量），`grow` 级联还会换一组约束再问；`layout_text` 每次都从头跑 `max_content` + `wrap`，两者都在 flash 里二分查字形（单次 advance ≈ 800–900 cycles）。

修法是在节点上记住尺寸对应的约束：`Node::measured_at: Measured { min_w, max_w }` 存 `enforce` 之后的宽度，`layout_node` 命中即返回；`Tree::push` 用 `..Node::default()` 构造，默认是一对任何约束都不会产生的哨兵，新声明因此天然未测；宽度真变会重测，`a_text_node_re_measures_when_the_width_changes` 钉住这一点。

| 读数（S3，240 MHz） | 改前 | 改后 |
| --- | --- | --- |
| `wrap` 调用/帧（host） | 2,394 | 41 |
| 扫过的文本字节/帧（host） | 28,656 | 4,708 |
| Attitude `[PAINT]` | body 0.70 / layout 98.9 / finish 102.4 ms | body 0.71 / layout 3.9 / finish 4.5 ms |
| Ambient `[DISPLAY]` | `worst render 92 ms (fill 2)`，`held 646 ms (29%)` | `worst render 25–33 ms (fill 1–2)`，`held 42–243 ms (1–11%)` |
| Attitude 整帧（host，release） | 1.104 ms | 96 µs |
| Splash `[PAINT]` | finish 1.65 ms | finish 0.47 ms |

`finish` 里还含一遍布局（此时全部命中）与落墨；产品路径只调一次 `finish`，所以这一段就是整帧的绘制。

像素层同时收紧，画面一字节不变（16 行整页黄金哈希不动）：`draw_glyph` 按行取位图、空字节跳过八个像素，盒子与裁剪只在入口求一次交；`fill_rect` 每行求一次 span 后整段写；`draw_text` 对 `Start` 且不换行的 run 直接画，不再跑断行器；`Screen::fill` 两个像素一次写。`[DISPLAY]` 行因此多了 `worst render N (fill M)`，把填充与绘制分开，作为后续回归指标。

Ambient 仍剩每帧 224 次 `wrap`、7 次不同约束的测量：那是 `grow` 级联（2^depth）造成的，继续压要改 flex 的测量顺序，本轮不动。整帧另有 15.36 ms 的线上地板（153,600 B / 80 MHz），重绘再快也不会低于它。

## panic 复位与看门狗：冻结时唯一的出路是重启（`[测试]`/`[实测]`/`[据称]`，2026-11-08）

- **panic 默认只是停住**：esp-backtrace 的 `panic-handler` 打完回溯后把本核停在关中断的空循环里，只有断电才能离开。本轮给工作区依赖打开 `custom-halt`，由 `iot-chip-esp` 定义的 `custom_halt()` 先置崩溃标志再调 `software_reset()`（`crashed_reset()`）。`custom-halt` 与 `halt-cores` 同时开启时后者优先（`abort()` 里 `cfg_select!` 的分支顺序），所以**本仓永远不要开 `halt-cores`** —— 开了 `custom_halt` 就再也不会被调用，而且编译不会报错。
- **RWDT 被 esp-hal 的 `init` 关掉**，所以看门狗只能在芯片层重新武装。`watchdog.rs` 用 `Rtc::new(steal())` 取 `RTC_TIMER`：`Board::new` 解构 `Peripherals` 时以 `..` 收尾，RTC 定时器不在它的字段里，这与 PSRAM 的 steal 是同一条 Safety 依据。`Rwdt` 存进 `critical_section::Mutex<RefCell<Option<_>>>`。
- **分两次武装**：`arm_boot_watchdog()` 在堆初始化与 `Board::new` 之前就以 **30 s** 超时先武装，覆盖各驱动自己的启动超时；`start_watchdog()` 在拿到 coordinator 执行器后收紧到 **5 s** 并起喂狗任务。Stage0 动作是 `ResetCore` 而不是 `ResetSystem`：后者会复位 RTC 域，而崩溃计数就存在 RTC 的 `store0` 里；`software_reset()`（panic 路径）本来也是 core reset，两者因此都保得住计数。
- **崩溃计数与安全模式**（`iot_core::boot` + `iot-chip-esp::boot_guard`）：`store0` 里一个字，高 16 位是 magic `0x5642`，bit 15 是「上一次启动 panic 过」，低 15 位是连续崩溃次数。panic 的 `custom_halt()`、堆/PSRAM/板级初始化失败都走 `crashed_reset()`，先置位再复位；看门狗复位由 reset reason 认出。每次启动 `note_boot()` 把这次复位折进计数：崩溃 +1，干净启动（上电、手动复位、重烧）回到 0 —— 所以「经常被复位」不会累积到安全模式。**第 5 次**连续崩溃复位后的启动进安全模式（之前有 4 次正常重试）。
- **开机是否健康不再按时间算**：`iot_core::boot::Readiness` 要两半都到才算这次启动成立——第一帧已经在每块有线屏上 `flush` 完成（coordinator 报），以及网络 worker 已经建好 IP 栈、或这个构建本来就没有 radio（`Wifi::Absent`/`Unattended` 由 `run` 报，worker 在 `embassy_net::new` 之后报）。两半都到，喂狗任务才调一次 `mark_healthy()` 把计数清零。关联失败或 DHCP 拿不到地址**不算**崩溃：设备活着，只是连不上。
- **安全模式仍保留 Wi-Fi 与只读诊断面**：安全分支不走 `Board::new`，只从 `Peripherals` 取 `WIFI`/`FLASH`/`TIMG0`/`FROM_CPU_INTR0`，用 `try_init_heap()`（失败只在串口停住，不再复位）和 `EspStation` 直接起栈。先用已存网络（或构建期凭据）做一次 station 尝试，20 s 内拿到地址就以 station 地址提供诊断；否则拉起开放 AP `vanling-safe-xxxx`（192.168.4.1），跑同一套 DHCP/DNS，页面是 `GET /`、`/status`、`/health` 的只读文本（崩溃次数、运行时长、堆余量、已存网络数、链路阶段与地址），其它方法/路径一律 400。AP 开放是因为正常 setup 口令显示在一块这条路径不信任的屏上，而页面只读。
- **安全模式锁存，不再制造复位环**：`enter_safe_mode()` 置位后，panic、`crashed_reset()`、堆/板级失败一律 `park_forever()`，不武装看门狗；串口每 10 s 提示一次。复位键、断电或重烧都会回到一次正常启动（这些 reset reason 都不算崩溃）。诊断路径自己 panic 时证据也还在。
- **`[测试]`（2026-11-08，`cargo test -p iot-core`）**：`safe.rs` 钉住诊断页包含每项事实、地址缺失/AP 模式、缓冲区过小被拒绝、只服务只读路径；`boot.rs` 钉住第 5 次才进安全模式与「首帧 + 网络」两半缺一不可。
- **喂狗是 coordinator 执行器上的普通任务**，每 1 s 醒一次；`1 s × 3 ≤ 5 s` 是编译期断言。判据正是想要的：只要那个执行器还在调度，狗就被喂；帧循环自己停住时没人喂 —— 包括 `renderers.flush().await` 永不返回（面板核不再接 DMA）这一种。它**不覆盖**独立执行器或另一颗核单独卡住而 coordinator 照跑的情况，那时 5 s 不会到。
- **WS2812 不再 panic**：C6 的灯是它唯一的面板，写失败原来每帧 `expect` 一次，panic → 复位 → 再 panic，会变成一周 5 s 的重启环。现在失败只记一次 `[RGB]` 警告，成功后清掉标志，下一次断流还会再报。
- **`[实测]`（2026-11-08，两块板各烧一次，旧镜像）**：C6 产品镜像开机打 `[WATCHDOG] armed for 5000 ms, fed every 1000 ms from the coordinator`，随后一次 `CTRL+R` 复位之后连续运行约 5 分钟没有任何复位（期间只有灯色日志）。S3 用 `-a watchdog-reset` 烧录后正常启动、关联已存网络，日志在 **5.7 s** 处仍有 `[STACK] core 1 peak …`，说明 5 s 的 RWDT 没有触发；S3 那行 `[WATCHDOG]` 抓不到 —— 原生 USB 的 CDC 在应用第一段日志之后才枚举，而这行在 C6 上由同一个函数打出。读数用 `espflash flash … -a watchdog-reset` 后立即 `cat` 串口取得（monitor 的 watchdog 复位会让端口重枚举，`espflash monitor` 自己会在那时候断掉）。这两次读的是 `ResetSystem` + 只在 coordinator 武装的旧镜像，`[WATCHDOG]` 行现在已改成两条（启动/运行）。
- **`[实测]`（2026-11-08，`moon run iot:emu-smoke`）**：新镜像在 C6 模拟器里从 ROM、2nd-stage bootloader 起，串口依次打出 `[BOOT] reset reason Some(ChipPowerOn); 0 crashed boot(s) in a row; Normal start` 与 `[WATCHDOG] armed before board bring-up for 30000 ms; tightened to 5000 ms once the coordinator runs`，随后 `[IOT] boot complete`。这证明 `store0` 在 C6 上可访问、未初始化的值被 magic 拒绝并记为干净启动，启动武装也没在正常启动路径上误触发。模拟器不模拟真正的 RWDT 到期。
- **`[据称]`**：四件台架读数还没做——(1) panic → `custom_halt` → 复位这一跳（从未故意 `panic!`）；(2) RWDT 真的到期时 `ResetCore` 保留 `store0`；(3) 连续崩溃真的走进安全模式、断电后回到正常启动；(4) 安全模式的 `vanling-safe-xxxx` AP 在真机浏览器上能打开并读到诊断页。能辩护的是寄存器语义、`ResetCore` 与 `software_reset()` 都属 core reset，以及上面的模拟器日志。

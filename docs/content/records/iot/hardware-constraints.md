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

**这 15.7 KB 全在 C6 一侧，出处已经量到**：`app/src/sounds.rs` 的 `play` 现在先解析再交给喇叭，而 HEAD 上解析发生在 S3 的驱动里——C6 的 `UnwiredSpeaker::play(Sound)` 什么都不做，LTO 就把解析摘掉了。证据是符号：HEAD 的 C6 镜像里 `from_midi`、`Score::into_notes` 的排序、`Tone::fill` **0 处**，本轮 7 处；`control_loop::poll` 因此长了 5,466 B，`iot_core::drivers::playback` 的符号在本轮合计 10,608 B。C6 的 `take_playback()` 返回 `None`，这条路径运行时不执行，付的是槽位不是周期。C6 还剩 206,176 B，所以这条只记录：要不要把解析重新关在编译期（软 feature，或只在有喇叭的构建里保留 `play`）是一个待定项。

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

---

## `IOT_BOOT_PAGE` 必须在构建期解析（`[实测]`，2026-10-29）

`[实测]` 症状：任何**设置了** `IOT_BOOT_PAGE` 的构建都在连网时死——日志停在 `[WIFI] this device saved a network; joining …` 之后，异常是 `StoreProhibited`（有时 `LoadProhibited`），PC 落在 esp-radio 的 `os_adapter::free` / `event_post` 里，`EXCVADDR` 是 `0x3`。同一份源码不设变量构建、烧进同一块板，每次都连上并拿到地址；HEAD（`a13e96f5`）设 `IOT_BOOT_PAGE=ambient` 也能连（峰值 41,880 B of 65,912 B）。

`[实测]` 崩的不在主栈上：异常的 `THREADPTR = 0x3fcd4908` 是电台任务的 TCB，`A1 = 0x3fcd4570` 比主栈底 `_stack_end = 0x3fcd9568` 低 20,472 B——那是电台任务自己的栈。写坏的是电台 blob 的静态量，`sta_con_timer`（`0x3fcd8974`）起那一整片就紧贴在主栈底下方。`.cargo/config.toml` 的 `ESP_HAL_CONFIG_STACK_GUARD_MONITORING = "false"` 让越界不出任何告警，只以「别人的代码崩了」的形式出现（见下一节）。

`[实测]` 差异锁在一处：`boot_page()` 当时在运行期解析 `option_env!`——`trim().to_ascii_lowercase()` 成 `String` 再查表。同一份代码，不设变量时连接处用到 23,409 B、峰值 42,392 B；设 `IOT_BOOT_PAGE=ambient` 时 27,745 B / 46,728 B，**+4,336 B**，然后必崩。多出变量那一版的主任务 poll 帧是 `entry a1, 0x6710`（26,384 B），HEAD 是 `entry a1, 0x5540`（21,824 B）——后者含本轮其它改动，所以帧的绝对差不能全记到启动页账上，能逐项对齐的是上面那对同源读数。

`[未分离]` 那 4,336 B 具体压坏了哪一块没有分开：可能是主栈越界、直接写进 blob 的 `.bss`；也可能只是帧变大改变了 internal heap 的布局，让电台任务的栈落到了 blob 的静态量上。两种都能解释这组读数，也都不影响修法。

`[实测]` 修法：启动页在**编译期**落成常量——`const BOOT_PAGE: DisplayPage = boot_page_from_env(option_env!("IOT_BOOT_PAGE"))`，`const fn` 做去空白与大小写比较；非法名字另存 `const UNKNOWN_BOOT_PAGE: Option<&str>`，由 `boot_page()` 打那行 `[IOT] IOT_BOOT_PAGE=… is not a page`（名字合法时它是 `None`，`if let` 整块被折掉）。修后 `IOT_BOOT_PAGE=attitude` 与产品构建逐项同值（`main stack 65,976 B`、峰值 `42,392 B`、连接处 23,409 B），并正常关联、拿到 `192.168.1.136`。桌面测试 `only_a_name_no_page_answers_to_is_reported` 钉住「非法名才报警、合法名在各种拼写下都不报」。

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

## 面板的字模：Spleen 5×8，一套字（`[测试]`，2026-10-22）

字模在 `apps/iot/crates/ui/src/font.rs`：95 个可打印 ASCII，5 列 × 8 行、列优先、bit 0 是顶行；节距仍是 9×13（`FONT_W 5 + OVERLAY_GAP 4`、`FONT_H 8 + OVERLAY_ROW_GAP 5`）。

- **第一次换是因为开口**：Adafruit `glcdfont` 的 `a/e/g/p/r/t/y` 在 5×7 里只剩 1 px 的开口，网络名和口令读不快。m5x7（Daniel Linssen，CC0）在同样的 5 列里画出 2 px 开口 —— 但它的 `m/M/w/W` 是 7 列、`0` 与 `O` 同形、`\` 缺字，这五处只能沿用 Adafruit，一句带 `m`/`w` 的话就混了两套笔画，上板后比换之前更难看（2026-10-22 的使用反馈）。
- **现在是 Spleen 5×8**（Frederic Cambus，BSD-2-Clause）：5×8 版本只含可打印 ASCII，字形原生就是 5 列 8 行，`m/M/w/W`、`0`、`\` 全在同一套里；不用拼字，也没有被裁掉的降部行。
- **只加一行**：`FONT_H` 7→8、`OVERLAY_ROW_GAP` 6→5，`ROW_PITCH` 仍是 13。FPS 徽标 `232 + 8 = 240` 刚好填满页脚，音频页 `WAVE_TOP 42` 刚好落在第二行字下方 —— 两处都是编译期等号断言，改字号时先断在这里。
- **回归**：`iot:test` 钉「非空格有墨」「形近字不同形」（`l/1`、`I/l`、`0/O`、`c/e`、`u/v`、`g/q`、`p/q`、`y/v`、`m/w`、`M/W`）与「降部到得了底行」（`g/p/q/y/j`）。换表时 `0/O` 真的同形，是第二条测试先报的。

字模在 flash：`[u8; 95 × 5] = 475 B`，与旧表同尺寸；静态墙前后都是 65,720 B（下限 65,536 B，余量 184 B）。

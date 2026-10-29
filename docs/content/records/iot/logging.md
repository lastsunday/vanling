+++
title = "日志实现记录"
weight = 82
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 日志实现记录

产品固件串口日志的取舍与实测。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

---

## 产品默认 `info`：稳态零行是正常

`[实测]` 同一块 S3、同一份默认产品构建（`IOT_BOOT_PAGE` / `IOT_DEMAND` / 凭据全不设），改前改后各抓一次串口：

| | 改前 | 改后（`info`） | 改后（`IOT_LOG_LEVEL=debug`） |
| --- | --- | --- | --- |
| 时长 | 43.8 s | 80 s | 40 s |
| 行数 | 169 | 35 | 137 |
| 开机之后 | 每 2 s 一条帧成本 + 每 500 ms 一条栈水位 + 每 2 s 一条 feed + 每 2 s 一条链路相位 | **最后一行在第 6.2 s，之后 73.8 s 一条没有** | 102 条 DEBUG（帧成本、feed、链路、扫描） |

改前 169 行里 81 行是 `[DISPLAY]`、25 行 `[WIFI]`、22 行 `[PLAY]`——三样都是**按窗口重复的同一句话**，而它们想回答的问题（面板贵不贵、feed 掉没掉、链路在哪一步）在稳态下答案不变。

复现：`espflash monitor --non-interactive --port <s3> --chip esp32s3 -a watchdog-reset`，拨动任意输入前后各截一段。**静默不算死机的判据是开机第一行 `[IOT] log level …` 之后的 `[IOT] boot complete` 与 `[HW] N devices: …`**（改名前是 `[POST]`）：这两行在，串口安静就是稳态。

## `[HW]` 的每一行由硬件自己给出

`[测试]` `hardware.rs` 的 `SelfCheck { name, id, check }`：驱动报自己的名字、自己的总线地址，并当场回答一次自检。板子只把对象交出去，不写名字也不写结论——旧版 `report.part("motion", State::Present, Provenance::Asserted)` 三样都是板子手写的，换一个驱动或接错一根线它都不会变。

```
[HW] panel             present      (declared)
[HW] button            present      (declared)
[HW] touch     0x38    present      (read)
[HW] motion    0x6A    present      (read)
[HW] mic       0x41    absent       (read)
[HW] speaker   0x18    degraded: not an ES8311 (read)
[HW] camera    0x3C    present      (read)
[HW] 7 devices: 5 present, 1 degraded, 1 absent; 5 read, 2 declared
```

- 第二列是 `Option<Address>`，SPI 面板、RMT 灯和 GPIO 按键没有地址，列留空。
- `(read)` 是向部件真读过一次（WHO_AM_I、chip id、GC2145 的 ID、ES7210 的配置寄存器、触摸的 GESTURE_ID）；`(declared)` 是这件东西没有回读通路，只能由板子声明（面板、灯、按键）。
- **行只出现在板子真正接了的槽位**：S3 七行，C6 两行。C6 没有面板/触摸/IMU/麦克风/喇叭/摄像头，那六样连行都没有——那是在复述 `Cargo.toml`，不是观测。接了却没起来（`Option` 是 `None`）仍占一行 `absent (read)`，因为那是驱动问过总线得到的答案。
- 汇总行由行本身数出来，`N` 与分布对不上只可能是计数漏了一行；读的人不必自己加。

## 级别只有一个旋钮，且只有一处持有

`[测试]` `configured_level` 的解析（`chip-esp`），`build_env.rs` 守着 `IOT_LOG_LEVEL` 必须出现在 `scripts/iot-xtensa.sh` 的 `FORWARDED_ENV` 里。

`IOT_LOG_LEVEL=error|warn|info|debug|trace`，缺省 `info`，**认不出的名字退回 `info` 而不是静音**：拼错一个字母应该只丢掉你想打开的那部分遥测，不该把整机日志关掉。级别在 `logging.rs` 用 `option_env!` 取，`enabled()` 读 `log::max_level()` 而不是自己存一份——两份状态就是两个能互相矛盾的答案。

**没有做按标签过滤**（`IOT_DISPLAY=off` 之类）：标签是 `[DISPLAY]` / `[PLAY]` / `[WIFI]`，一个全局旋钮够用，而多一个维度就多一套「为什么这条没出来」的排查面。标签留着这扇门：真要按域关，`enabled()` 里按 `metadata.target()` 分派即可。

`init_logging` 在 `chip-esp` 一处实现，两种芯片共用：日志器与级别都不是芯片事实。它开机打一行 `[IOT] log level INFO (IOT_LOG_LEVEL)`——**这是「这一版到底多安静」的唯一凭据**，固件安静时先看它，再看是不是真的没有事件。

## 事件 `info`、遥测 `debug`、越界 `warn`

| 行 | 级别 | 判据 |
| --- | --- | --- |
| 开机清单（`[HW]`、`[CREDS]`、`[HEAP]`、`[CLOCK]`、`[MOTION]`、`[DEMAND]`） | `info` | 每次开机恰好一次，是「这块板今天是什么样」的答案 |
| 状态迁移（页面、链路相位与地址、灯、配网模式的开关与前后端） | `info`，**只在变化时** | 重复的同一相位不是事件 |
| 用户动作（tap、按住、面板请求） | `info` | 有人在按，必须留痕 |
| 帧成本与栈水位、feed 节奏、链路心跳、扫描列表、描述符与寄存器 dump | `debug` | 按窗口重复，稳态答案不变 |
| 单次执行器持有 ≥ 30 ms、单帧写 > 20 ms 或占空 > 25%、feed 迟到或低于额定 95%、部件降级、DMA 重启 | `warn` | 越过阈值才算事件 |

同一句话既是遥测又是故障时用 `log::log!(level, …)` 一处写、两处给（`paint.rs` 的帧成本、`input.rs` 的执行器持有、`playback.rs` 的 feed 节奏）。**`warn` 留在 `info` 构建里是刻意的**：产品上唯一可见的输出是面板，串口上一条 warn 就是一次「本该不会有」的读数，出现即值得读。

`[实测]` 开机那次 `[INPUT] held the executor 1970 ms of 5003 ms … worst single hold 600 ms; worst source: motion 598 ms` 是真实开销：运动驱动首次轮询要等 QMI8658 出第一组样本，之后每 5 s 的窗口里最坏单次 5 ms 以下，于是那一行降到 `debug`。**它保留 `warn` 的形状是因为它第一次确实越界**，不是因为噪声。

## 变化比对要放在两条入口之后，否则报出的是「上一次触摸」

`[实测]` 页面切换本来只在 `Intent::Operation` 分支里比对并打印，而**开屏结束走的是 `Intent::Business`**（`SplashOver`）。于是开机那次 `Splash → Ambient` 不打印，`last_page` 停在 `Splash`，直到**第一次触摸**才把早就画好的页报出来：改前一次是 `[00:00:22.745] page is now Ambient`，而页面在 `00:00:02.7` 就已经是 Ambient。读的人会以为 22.7 s 有人翻页了。

比对现在放在 `match` 之外，两条入口共用：同一版固件在 `00:00:02.710` 报 `page is now Ambient`，紧接 `00:00:02.711` 报开屏结束，此后的触摸不再重复。**只降噪不够，变化检测本身也得盖住所有写入口**——否则安静下来的日志里会留下一条时间戳错的真事件，比噪声更难发现。

## 过滤发生在格式化之前

`log::debug!` 在级别不够时**不格式化参数**，成本是一次原子读 + 分支；降级到 `debug` 的周期行因此几乎不占执行器。由此有一条实现约束：**给降级行传的参数必须是便宜的**（`&'static str`、整数、已有的结构体字段），不要为了那一行先 `format!` 或读 I2C。`[CAM] decim regs` 是这条约束的边界——它读的是已经在手边的 `geometry`，不是每次现读寄存器。

`[实测]` **删掉的行并不省执行器**：改前 `[INPUT] held the executor 1426 ms of 5002 ms over 1001 ticks`，改后同一窗口 `1417 ms of 5000 ms`，差在 0.2% 以内——USB CDC 上的串口写没有进这条账。所以这次改动买到的是**读的人的时间**，不是 CPU；要省 CPU 得靠级别过滤掉格式化，而那只对真的按值携带大参数的调用有意义。

## 标签按「谁在说话」分

`[MIC]` → `[AUDIO]`、`[SPEAKER]` → `[PLAY]`、`[TOUCH]` → `[INPUT]`、`[RADIO]` → `[WIFI]`、`[PSRAM]` → `[HEAP]`、`[BACKLIGHT]` → `[DISPLAY]`；其余不变。

判据是**一个模块一个标签**，不是一种外设一个标签：麦克风与喇叭是同一个采集/放音路径的两端，触摸与按键是同一个输入路径的两端，PSRAM 就是堆，背光是面板的一部分，射频只服务于 Wi-Fi。改前 `[MIC]` 与 `[AUDIO]` 并存、`[SPEAKER]` 与 `[PLAY]` 并存，读的人得先知道哪个是驱动哪个是应用。

## 已知缺口

- **没有 post-mortem 环形缓冲**：日志只走 USB CDC，panic 或复位之后最后几行是否到达取决于主机是否还在读。`esp-backtrace` 打的是自己的栈回溯，与本日志不同一通道。要看复位前的现场，只能抓串口；`esp-hal` 的 `rtc_cntl` 里没有本仓用起来的持久区。
- **`HPF_CORNER_WALK` 在产品构建里仍是 `true`**，而[音频记录](@/records/iot/audio.md)写的是「生产构建关闭」。它开机走 8 个 corner 码、每个 3 s，`info` 下只留一行 `[AUDIO] HPF corner walk starting …`，但**它改变音频行为**（走完才回到工作 corner），所以这次没动：关掉它是一次音频决定，不是一次日志决定。
- 面板没有读回，`[HW] panel present (declared)` 的 `declared` 就是这一句：日志能证明驱动接受了配置，不能证明像素对。每行都带 `(read)` 或 `(declared)`，汇总行给出两者的条数，读的人不必逐行数。

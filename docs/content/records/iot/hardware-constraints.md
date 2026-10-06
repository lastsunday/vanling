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

`[实测]` `esp-hal` 自带的 `stack.x` 把主任务栈顶设在 `dram_seg` 末尾，`dram2_seg` 那块无人认领，`crates/app/linker/esp32s3-main-stack.x` 把它接上，栈因此跨两块内存连续。

`[实测]` 从 ELF 符号读出的真实几何：

```
_stack_end        = 0x3fcd154c
_stack_start_cpu0 = 0x3fced710   →  115 140 B = 112.4 KB
```

**为什么需要**：绘制一屏渲染时 `Diagnostics` 快照与两个 `[u16; ENVELOPE_COLUMNS]` 列数组按值在栈上，Audio 页曾在这个栈上崩溃（异常的 `A1` 落在 `_stack_end` 之下）。

`[实测]` 当前占用 32 617 B（28.3%），余量 82.5 KB。固件自带水位测量（`[DISPLAY] stack peak`），每重绘窗口采样一次，取最深值——不是估算。

**一个未解的疑问**：这个峰值从开机起几乎不变，多页切换也没有推高它。而历史崩溃点是 Audio 页的整帧 stamp。峰值到底属于哪条路径**尚未定位**；若它其实属于渲染，说明渲染比代码注释描述的还重，值得继续查。

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
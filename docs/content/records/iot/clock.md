+++
title = "运行期改 CPU 频率"
weight = 72
sort_by = "weight"
+++
title: "运行期改 CPU 频率：能，但走的是越界路径"
description: "esp-hal 文档写明频率被冻结；绕过它写寄存器在这块板上真的能把频率切到 80/160/240，比值实测 1 : 2.00 : 2.95。切频 1-50 µs，tick 不受影响，两个核都对。"
source_file_hash = ""
translated_at = ""
---

<!-- doc-audience: ai -->

# 运行期改 CPU 频率：能，但走的是越界路径

`clock-probe` 的完整记录。**本文只记录已在板上实测的数字**，未测的部分明确标注。

## esp-hal 不提供这个能力，这是文档写明的

esp-hal 1.2.2 的时钟文档有一节叫 **Frozen Clock Frequencies**：

> Once the clock configuration is applied, the clock frequencies become `frozen`
> and cannot be changed.

不是遗漏，是明确的设计。该模块全部运行期接口是：`cpu_clock()`（读）、`xtal_clock()`（读）、`RtcClock`、`PhyClockGuard`（**防止**时钟被关掉）、`ModemClockController`（给 modem 外设）、`init_radio_clocks`。**没有一个能改 CPU 频率。**

旧路径也已拆除——`Clocks` 结构与 `Clocks::get()` 已被删除，改成 lockless 自由函数。

**官方 API 路线只有换 `esp-idf-hal`**（封装 ESP-IDF 的 `esp_pm_configure` / `esp_pm_lock_*`）。那是 `std` + FreeRTOS 生态，与本仓的 `no_std` + embassy + 裸机不兼容，代价是重写，不是增量。

## 越界路径能做什么

`set_cpu_clock` 写三个公开外设加一次 ROM 调用，全部照抄 esp-hal 自己 `configure_cpu_clk_impl` 的可达子集：

| 寄存器 | 内容 |
|---|---|
| `SYSTEM.cpu_per_conf.cpuperiod_sel` | **2 bit**，唯一真正变化的字段 |
| `SYSTEM.cpu_per_conf.pll_freq_sel` | 恒 `1`（永远用 PLL_480） |
| `SYSTEM.sysclk_conf.soc_clk_sel` | 恒 `1`（PLL 源） |
| `LPWR.date.ldo_slave` | `0x7 >> (mhz / 80)` → 80 MHz 为 `0x3`，160 为 `0x1`，240 为 `0x0` |
| `ets_update_cpu_frequency_rom(mhz)` | ROM 时基标定 |

**不重配 PLL、不刷 flash 缓存、不等锁定。** 三个预设的 `pll_clk` 全是 `PllClkConfig::_480`，只有分频选择不同——所以仓库里 `esp32s3.rs` 旧注释说「160/240 预设把 PLL 从 480 改掉因而破坏 USB Serial/JTAG」**是读错了源码**，实测 240 MHz 控制台完好也否证了它。

**跳过的部分**：esp-hal 切频时还会经 `regi2c` 写数字/RTC 偏置，并做 PVT 标定（`pvt_supported()` / `dig_dbias_v1()`）。这些在 esp-hal 里是 `pub(crate)` 或私有 `fn`，外部拿不到。

**为什么跳过仍然安全**：esp-hal 在**升到** 240 MHz 时写入该频率的 PVT 偏置，在**降出**时调低省电。探针**只降不调**——频率低时偏置偏高是保守方向。

**「从低速启动再升上去」本来是担心项，已实测，不是推断**：见下节。

## 实测：频率真的变了

同一个工作单元（`#[inline(never)]` + `black_box`，256×64 次整数运算）：

| 预设 | 耗时 | 相对第一档 |
|---|---:|---:|
| `_80MHz` | **1731 µs** | 1.00× |
| `_160MHz` | **864 µs** | 2.00× |
| `_240MHz` | **587 µs** | 2.95× |

**1 : 2.00 : 2.95 ≈ 80 : 160 : 240。**

注：`esp_hal::clock::cpu_clock()` 在三档都仍报 240 MHz——它读的是配置树，不是硬件，所以本探针不以它为依据，而以「同一工作量在三档的耗时比」为依据。

## 实测：启动频率不影响结果

起始频率曾被认为有影响——esp-hal 的偏置分两套（240 那套约 dbias 28，非 240 那套约 27），以 80 MHz 启动后升到 240 就是**欠驱动**。这是可以便宜验证的：失败模式是复位，不是损坏。

**探针以 80 MHz 启动（即产品 `chip_init()` 的配置）跑完整轮，与 240 MHz 启动逐位一致：**

| 量 | 240 MHz 启动 | 80 MHz 启动 |
|---|---:|---:|
| 工作单元 @80 | 1731 µs | **1730 µs** |
| 工作单元 @160 | 864 µs | **864 µs** |
| 工作单元 @240 | 587 µs | **582 µs** |
| 切到 80（均/最坏） | 50 / 58 µs | **48 / 56 µs** |
| 切到 160 | 2 / 3 µs | **2 / 3 µs** |
| 切到 240 | 1 / 2 µs | **1 / 2 µs** |
| 128 次连续切换 | 1–2 µs 每次 | **1–2 µs 每次** |

**无欠驱动、无复位、比值照旧 1 : 2.00 : 2.97。**

**边界**：探针跑约 0.24 s。这证明的是「不会失败」，不是「最优」，也不是长期可靠性——本仓对 240 MHz 本来就带着「长期稳定性未测」的保留。

## 实测：切换代价

| 切到 | 均值 | 最坏 |
|---|---:|---:|
| `_80MHz` | **50 µs** | 58 µs |
| `_160MHz` | **2 µs** | 3 µs |
| `_240MHz` | **1 µs** | 2 µs |

首次贵得多，因为寄存器页第一次触碰。128 次连续切换稳定在 1–2 µs 每次，**无累积漂移**。

对照文献里「全芯片 DVFS 决策可达 6 ms、建议决策间隔 ≥20 ms」的阈值——**那些数字是大芯片上电压缩放的**，本板是两个 bit 的寄存器写，相差三个数量级，不适用。

## 实测：外围不受影响

跨切换后 `Instant` 推进 **43–47 µs**，紧接其后的工作单元耗时正确（1715 / 867 / 586 µs）。**SYSTIMER 不受 CPU 频率影响**，esp-hal 源码写明其「only clocked by XTAL divided by 2 or 2.5」。

时钟域归属：

| 外设 | 时钟域 | 只切 CPU 后 |
|---|---|---|
| embassy tick（SYSTIMER） | XTAL/2 | 不受影响 |
| I2S / I2C / SPI / TimerGroup | APB | APB 不动 → 不受影响 |
| USB Serial/JTAG | 独立域 | 不受影响（240 MHz 已实测） |

**S3 上没有任何外设挂在 CPU 时钟上。**

## 实测：第二核没有可观测差异

`ets_update_cpu_frequency_rom` 文档自述「on the current core only」，所以 core 1 的 ROM 时标定在 core 0 切换后是过期的。同一个工作量在两个核上各测一次：

**core 0 585 µs / core 1 566 µs，比值 1.03×。**

585 µs 落在 240 MHz 的单核耗时（582–587 µs）上，说明两个核确实都在 240 MHz，且**过期的 ROM 标定在这块板上没有产生可观测差异**。

这是**一个工作量、一块板**的结果。依赖 `ets_delay_us` 精确计时的代码不能据此假定两个核都准。

## 未测与明确不做

- **功耗**。探针不降偏置，所以测不出真实省电效果。这是刻意的取舍，不是遗漏。
- **APB 降频**。明确不做——那会动 I2S/I2S/SPI/TimerGroup 与 USB Serial/JTAG，失败模式是把板砖。
- **light sleep**。esp-hal 1.2.2 没有该 API，所以「降频」与「睡过去」之间没有中间档。
- **esp-hal 升级后的持续有效性**。本探针依赖 `configure_cpu_clk_impl` 的当前写法；esp-hal 若改变策略或封锁寄存器，探针需同步修改。
- **长期可靠性**。全程约 0.24 s。"从低速启动升到 240 不会失败"有实测支撑，"长期不会出问题"没有。

## 教训：探针自己踩了四个坑

**① LTO 把工作循环整个删掉了。** 第一轮三档都报 `21 / 20 / 21 µs`——那是两次时钟读取的成本，与频率无关。本仓 `architecture.md` 已记「LTO 让探针的计时括号被优化掉，所以 `process` 带 `#[inline(never)]`」，同一个坑踩了两次。修复是 `#[inline(never)]` + `core::hint::black_box`。

**② 第二核读数顺序反了。** 初版在 `measure()` 内读 core 1 的值，而握手发生在 `measure()` 之后，读到的是 `u64::MAX`。已改为：core 0 的切换全部完成 → 发信号 → 读本地值 → 等对端值 → 打印两者之比。

**③ 把日志写进了被计时的函数。** 为了让欠驱动复位可诊断，在 `switch()` 里加了 `log::info!`——而 `cost()` 的计时区间正好包着它。于是 80 MHz 下的切换开销读成 **2634 µs**，比真实值高 50 倍，而真实值是 48 µs。**那一版的 `switch()` 签名本身没法承认这件事。** 现已把日志移到调用方。

**④ 两个核同时测，把自己测慢了。** 先发信号再读 core 0 本地值，于是 core 1 的 executor 启动与 `local_reading()` 抢同一块内存——**两个读数都落在 160 MHz 区间（861 / 830 µs）而芯片在 240**。比值仍然可用，绝对值不可用。改为先读本地值再发信号，得到 **585 / 566 µs（1.03×）**，585 正落在 240 MHz 的单核耗时上。

③④ 有同一个特征值得记下：**两版都产出了看起来正常的数字**，没有一个报错。是「比值不对」和「绝对值落错区间」揭穿的，不是任何断言。

## 相关记录

- [AEC 的 CPU 预算](@/records/iot/aec.md) —— 240 MHz 是 AEC 侧强制的要求：80 MHz 下延迟搜索单独就破 30 ms 界
- [时钟与探针方法论](@/records/iot/_index.md) —— 「先量时钟，再谈结论」这条是怎么来的
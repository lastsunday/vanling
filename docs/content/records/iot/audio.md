+++
title = "音频实现记录"
weight = 40
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 音频实现记录

`apps/iot` 采集路径的实现判断与被否方案。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

---

## 静音必须读作"没变化"，否则整条 diff 链空转

`[测试]` `a_silent_capture_stops_reading_as_changed_once_the_ring_has_wrapped`（`core/src/drivers/audio.rs`）。

窗口写满之后 `cursor` 每轮都在推进，`#[derive(PartialEq)]` 于是把每一次推进都算成一次变化。实测：600 次连续静音轮询中，比较相等 **0 次**。

后果不是显示抖动，而是整条渲染 diff 管线在一个安静房间上按渲染节拍（20 ms）反复重跑，每轮都要重画整帧。诊断派生的 `Diagnostics` 内嵌 1.6 KB 音频包络，每次比较都是一次按值拷贝。

所以 `AudioEnvelope` 手写 `PartialEq`，只比**面板画得出来的那部分**：已提交列数与被绘制的列值，不比写指针。判据是「渲染层能看见的东西变了没有」——写指针变了而画面没变，不算变化。

---

## 采集链只有一处能停，停了就等于麦克风死掉

`[实测]` 复现：烧默认固件，连续快速点屏幕让 Camera 页反复进出（`IOT_BOOT_PAGE` 不用，用触摸走真实路径），读 220 s 串口。故障轮的判据是 `[INPUT] held … worst source: … capture N ms` 里 capture 从 17–20 ms 变成 **0 ms** 并此后一直是 0。

**`0 ms` 不是「变便宜了」，是「没有环可排空」。** 故障轮里 130 秒 0 条 `[AUDIO]`、input 占用从 52% 掉到 27%、而 `[DEMAND]` 全部 46 行都是 `capture on`——需求没关它，是链自己没了。

失效形状值得原样记下，因为它安静且不可逆：

1. watchdog 判定采集静默，开火要修
2. 修复路径**先把自己的链取走**，再去 arm——于是 arm 找不到任何东西
3. 环和 codec 侧一起被 drop，没有日志
4. 此后 `sample()` 在「没有链」处提前 return，而 watchdog 在那之后，够不到
5. 于是永远返回静音，永远不报警

第 4 步是关键：**提前 return 把 watchdog 屏蔽在自己外面**，所以「链没了」表现为「一切正常，只是没声音」。

修法把不变量变成结构性的而不是约定：**只有 arm() 能停一条在跑的链。** `Chain<A>` 同时持有「在线上」和「已装好待命」两份，唯一的取空方法 `take_to_arm()` 必定先 `stop()`。类型放在 `iot-core`（那里已有 `CaptureWatchdog` / `CaptureBacklog` / `SampleStream` 等可 host 测的逻辑），因为真正值得持有的类型是 I2S DMA transfer——恰好是 host 上造不出来的东西，而此前 `set_collecting` / `arm` / `restart` **一个测试都没有**。

`[测试]` `parking_and_arming_returns_the_same_chain`、`repairing_a_stalled_capture_never_drops_the_chain`、`a_capture_with_the_demand_off_is_parked_and_not_lost`、`a_re_arm_that_refuses_reports_itself_rather_than_going_quiet`、`parking_nothing_is_not_a_second_park`（`core/src/drivers/audio.rs`）。其中第二条对修复前的代码会失败。

`[据称]` 修好后硬件未复现：同一段操作下负载更重（183 次页切换 vs 46，相机链重装 74 次 vs 9），capture 全程 12–20 ms 未掉到 0，watchdog 一次未开火——**所以修复本身未经硬件检验**，只是不再有可能静默丢弃。

附带一条：`sample()` 现在会区分「按需求 park」与「链丢了」，后者打一条 error 且只打一次。这两者从 `sample()` 里看**完全一样**（都是没有在跑的链），区别只在于还留着没有。

---

## 标定走查挂在需求门控的轮询上

`[实测]` 复现：烧 `IOT_DEMAND='audio=off'`，读 40 s 串口；再烧 `IOT_BOOT_PAGE=camera IOT_DEMAND='motion=on:30000,audio=on:30000'`，同样读 40 s。

> 本文所有「批到 30 s」的行都测于**已删除的 `Budget` 机制**（`IOT_BUDGET=thrift`：Secondary → 30 s、Primary → 10 s）。那一层因为本板 IMU 无 FIFO 而恒等于直接写节奏，已移除，理由见[架构文档](@/development/iot/architecture.md#需求模型要什么--多频繁--多敏感)。**数值不变**，`IOT_DEMAND` 的等价写法在每处给出。

`[AUDIO] HPF corner walk` 是一次性开机走查（`HPF_CORNER_WALK`，生产构建关闭）：走完 8 个 corner 码再回到工作码。它由 `sample()` 推进，而 `sample()` 是需求门控的轮询。后果是走查的**完成时间由预算决定**：

| 构建 | audio 需求 | 走查预计耗时 | 实测 corner 报告 |
|---|---|---|---|
| 默认 | `Continuous@Secondary`，20 ms | 24 s | 每 3 s 一条 |
| Camera 页 + motion 批到 30 s | 30 s | ~240 s | 0 条 / 40 s |
| `audio=off` | 链被 park | 永不完成 | 0 条 / 40 s |

`[实测]` 这解释了早先一个没被归因的现象：`build-s3-thrift`（`audio=off`）的日志里 `capture stopped` 之后没有任何 corner 报告，当时只当作「没推进」，实际是**永远走不到**。

对一次性有界的字段辅助工具不致命，但耦合必须写下来而不是在日志里被发现。**若将来有必须按时完成的标定，它不能住在 `sample()` 里**——那个函数是被需求关掉的。

---

## 一个平面的边际代价随别的任务醒着而变，不是常数

`[实测]` 复现：从 `apps/iot/` 起五个构建，只改 `IOT_DEMAND`，各烧录后读 45 s 的 `[INPUT] held`（8 个 5 s 窗口，中位数）。

| `IOT_DEMAND` | `[INPUT] held` | 减地板 | 得到 |
|---|---|---|---|
| `motion=off,audio=off,display=off` | 10.87% | — | 交互面地板 |
| `audio=off,display=off` | 23.4% | 12.52% | motion 独自跑 |
| `audio=off,display=on-change` | 26.6% | 15.73% | motion + 面板醒着 |
| `motion=off,display=off` | 34.0% | 23.13% | audio 独自跑 |
| 默认（`motion Continuous + audio Continuous + display Continuous`） | 49.4% | — | — |

（默认构建 = 全默认，即 `motion Continuous + audio Continuous + display Continuous`。）

两条结论：

**一、争用只与「有没有别的任务醒着」有关，与它做多少无关。** `display=on-change` 与 `display=off` 之间的差远小于 `OnChange` 与 `Continuous` 之间的差，因为 `Mode::Off` 让 render 循环 park（`Activity::Idle`），面板不再每 tick 唤醒执行器；`OnChange`/`Continuous` 都让它持续唤醒，于是照样付争用——**做得少不等于醒得少**。

**二、所以固定倍数换算不成立。** `motion Continuous` 单独跑是 12.52%，旁边有个醒着的任务就变 15.73%，+26%；`probe-demand-s3` 隔离测得 11.40%，又是另一种数。任何按「隔离值 × 一个系数」构造的预算都会在场景开的面越多时错得越多，而那正是预算唯一存在的理由。

还有一条口径上的坑：差分给出的是**边际**代价，且依赖上下文。上表里 `motion` 的 12.52% 是在 `audio` 关着且面板黑着时测的，15.73% 是在面板醒着时测的——同一个配置，两个数。所以差分得到的数字不是「这个平面的代价」，是「这个平面在这个场景里的增量」，被当成前者引用就会在换场景时失效。要贵的那个上下文：亮面板那个读数（157_300）。

`[测试]` `an_isolated_row_would_still_not_be_addable_to_a_device_row`、`the_row_is_the_device_reading_and_the_probe_only_found_the_floor`、`the_differenced_rows_land_near_the_whole_device_build_they_were_taken_from`（`core/src/demand.rs`）钉住这三种口径不可混用。

## 节流把执行器从一处搬到另一处，不省设备总量

`[实测]` 只把 motion 批到 30 s（`IOT_DEMAND='motion=on:30000,audio=on:30000,display=on:10000'`；旧写法 `IOT_BUDGET=thrift`），与默认构建各 8 个 5 s 窗口：

| 构建 | `[INPUT] held` | `[DISPLAY] held`（每 2 s 窗口） | 两侧相加 |
|---|---|---|---|
| 默认 | 2473/5000 = 49.5% | 923 ms = 45.3%（35 帧） | 94.8% |
| motion 批到 30 s | 1774/5000 = 35.5%（motion 30 s） | 1179 ms = 58.5%（45 帧） | 94.0% |

motion 让出 14.0 点，render 循环拿走 13.2 点，设备总量基本不动。每帧仍是 26 ms（10 ms 渲染 + 16 ms 整帧 SPI 写），变的是帧数——协作执行器把时间给醒着的那个，面板动画时钟只要有 CPU 就多画帧。

**所以「给 latency 加一个百分比上限」这条路的形状是错的**：两个百分比加起来才是设备占用，而它们互为此消彼长。两侧必须分开看，且面板那一侧是残差、不能与 poll 侧相加——相加会超过一个核的 100%。

## 采集平面不能被节流，否则是时钟停摆而不是读数变旧

`[实测]` 把 capture 也批到 30 s（`IOT_DEMAND='motion=on:30000,audio=on:30000,display=on:10000'`）：

```
[00:00:31.023] WARN - [AUDIO] capture ring 10842 B deep, the input loop is behind the codec
[00:01:31.023] INFO - [AUDIO] capture DMA had stopped, re-armed (1)
```

90 s 里 HPF corner walk 只走了一步（默认构建每 3 s 一步）。环 60 ms 就撞满、I2S 引擎自行停机，而 watchdog 只在 `sample()` 里跑、`stalled()` 又只在「本次 poll 一个字节都没拿到」时才判——所以它是在**排空之后那一次** poll 才发现自己刚把一个已经停了的链接上了。

这与 `audio=off` 那次是同一个约束的两条路：`Off` 走「主动停链」，节流走「不许更稀」。修法是 `InputSource::accepts_slower_poll()`，默认 `true`，`Es7210Rx` 答 `false`，`apply()` 据此把 plan 的 cadence 折回板级并如实打印。修后同一构建打印 `motion on 30000 ms, capture on 20 ms`，corner walk 恢复每 3 s，无 backlog、无 re-arm。

`[测试]` `a_plane_whose_driver_needs_its_poll_is_not_stretched`（`app/src/input.rs`）钉住这条。

## poll 侧按节奏缩放，两端都与实测对上

平面在**板级节奏下的速率**才是它的单位，所以被批处理的平面更便宜。缩放的前提——「节奏改变的是多久一次，不是一次有多贵」——由探针实测：板级 20 ms 下每次 poll **2281 µs**，200 ms 的格子里 **2282 µs**，差 0.04%。探针为此改成在每个格子自己的节奏上跑，够得着的实测、够不着的印 DERIVED。

`[实测]` 缩放后 poll 侧与整机实测的对照（各 8 个 5 s 窗口）：

| 构建 | `[DEMAND]` poll 侧 | `[INPUT] held` 实测 | 误差 |
|---|---|---|---|
| 默认 | 497,300 | 494,600 | **+0.6%** |
| motion 批到 30 s | 340,104 | 354,800 | **−4.1%** |

缩放之前 thrift 那一栏报 497,300（motion 按 20 ms 算），对着实测的 354,800 高 **40%**。

`[测试]` `the_cadence_scaled_poll_side_predicts_what_the_device_measures`（`core/src/demand.rs`）把两端与 5% 的容差一起钉住。

## 采集积压判据的三档深度来自实测

`[测试]` `the_depth_a_healthy_capture_settles_at_is_not_called_a_backlog`（`core/src/drivers/audio.rs`）。

一次按时到达的轮询只等约一个轮询周期的音频；一个比链路固定落后一个节拍的循环，则**永远**比那个稳态再多深一个周期。因此把「两周期」当积压线，等于让一条从不积压的环每轮都报警。

实测中环一直健康时面板报出的深度是 `7_708 / 7_934 / 8_188` 字节（环容量 `24_576`）。`CaptureBacklog` 的告警/清除两档就落在这里面，一档一个轮询周期。

`[据称]` 该面板曾把整条 ring 判为已耗尽并重建 DMA，中途丢掉正在播的音频。未定位触发条件，仅留痕。

---

## dBA 表对原始表：让两次测量是同一个测量

`[测试]` `a_low_tone_drives_the_raw_columns_but_not_the_weighted_twin`、`a_mid_band_tone_reads_the_same_through_both_columns`、`spl_is_dbfs_with_the_microphones_own_reference_back`（`core/src/drivers/audio.rs`）。

A 计权 1 kHz 处为 0 dB，62.5 Hz 处约 −26 dB。于是两套列并存：面板的 scope 与 PK/RMS 行读**未加权**（那是 codec 交出来的样子），角落读数读 A 加权（那是能和别处 dB(A) 比的数字）。

`AWeight` 只跑一遍级联，包络的峰值列与加权列分别吸收**同一批**样本。第二遍级联在系数与零状态下只能得到同一个答案，而实测那让采集路径的每样本成本翻倍。

---

## 量化噪声会喂出 65 dBA 的假底噪

`[测试]` `a_quiet_floor_reads_quiet_instead_of_feeding_a_limit_cycle`（`core/src/drivers/audio.rs`）。

A 计权在低频几乎平坦，而滤波器极点靠近单位圆。定标器状态位宽足够宽时，量化噪声被自身放大并循环：一块只听得到自己 codec 底噪的板子会稳定读出约 65 dBA。

`STATE_FRACTION` 的宽度按这个上限反推——安静底噪（几十 LSB）必须读成几十 LSB，而不是一个自持的几百。

---

## 0 dBFS = 102 dB SPL 是这一颗麦克风的标定

`[实测]` 推导链与复测方法见[硬件约束记录](@/records/iot/hardware-constraints.md)。

核心测 dBFS，偏移量归板级所有（`SPL_OFFSET_DECIBELS` 在 `bsp-esp`）。`core` 不能依赖携带该器件的板，所以 `spl()` 把偏移量当参数收，板级传自己的值进来。

`[测试]` `spl_is_dbfs_with_the_microphones_own_reference_back` 顺带钉住两个读数的差值必须**全部**是那个偏移：中间多出任何东西，两个列就已经不是同一个测量了。
---

## 完整推导、实测对照与方法，不在这份记录里

这条只留结论与代码位置；完整的测量表、两侧不可加的理由、`Sensitivity` 不改节奏的理由、单次持有界为什么不是百分比，以及每一项的复现构建与命令，都在[架构文档](@/development/iot/architecture.md)。

代码侧的三处硬约束也各在那里说明：`Cost::basis` 为什么是字段而不是注释、面板行为什么是残差不可相加、以及 `InputSource::accepts_slower_poll()` 为什么采集平面答 `false`（环只有 `sample()` 排空，节流不是让读数变旧而是让 I2S 时钟停）。

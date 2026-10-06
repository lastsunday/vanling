+++
title = "放音实现记录"
weight = 10
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 放音实现记录

`apps/iot` 放音路径的实现判断与被否方案。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。系统说明（契约、面板读数、寄存器）在[放音语义](@/development/iot/playback.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

---

## `Cp0Disabled`：合成音在 feed 中断里崩溃

### 症状

点 Speaker 页播放 chime，设备立即停住，屏幕与声音一起停止，**不复位**。静默的板子比崩溃更麻烦——它看起来像"卡了"，而"卡了"通常指调度问题。

### 根因

`Tone::fill` 在 Priority2 feed 中断里执行 FPU 指令，触发协处理器异常 `Cp0Disabled`（`EXCCAUSE: 0x20`）。板子停在第一帧 chime 上。

符号化调用栈（`addr2line`，容器内 Xtensa 工具链）：

```
Tone::fill
  ← Es8311Tx::feed
    ← feed_task::poll
      ← Executor::poll
        ← esp_rtos::embassy::handle_interrupt::<1>
```

**关键性质：这是中断上下文的性质，不是算术的性质。** 同一段代码跑在协作 executor 上完全正常。

`[实测]` 复现步骤：

1. 固件启动后每 1.2 s 往 `INTENT_BUS` 发一次 `Intent::Business(BusinessIntent::PlayNext)`，走与真实轻点完全相同的链路
2. `[PLAY] Asset` 正常，`[PLAY] Chime` 之后立即 panic
3. 第一次 chime **首帧**就崩，不存在累积过程

**为什么要自动化复现**：这个 bug 需要"轻点 chime"才会触发，人工点击既慢又不可重复。加上自触发后，一次烧录 + 一次抓日志即可判定，这直接决定了后面每一次修复验证都能自动化。

### 被否的两个假设

记在这里是因为它们都"看起来非常合理"，而且都会把人带向错误的修法。

#### 假设一：合成成本太高，饿死协作 executor

`[实测]` 第一次尝试时测到：chime 最坏单次 feed **4434 µs**，而节拍是 5000 µs——占 88.7%。Asset 同样帧数只是 memcpy，**5 µs**。结论看起来很清楚：把合成变便宜即可。

于是做了 512 项查表 + 线性插值。`[实测]` 降到 **5 µs**，887 倍改善。

**板子照样卡死。** 第一次 chime 首帧即崩——不存在"跑久了才饿死"的时间条件，成本假设与症状不相容。

#### 假设二：查表 + 插值就够了

表是 `f32` 的，插值也在 `f32` 里做，仍然执行 FPU 指令。

`[实测]` 为了区分"是成本问题还是浮点问题"，把振荡器临时改回逐样本 `sinf`（同一调用点，唯一变量）：**同样崩溃，异常完全相同**。

这一步才把根因锁定在浮点本身。

### 最终修法：运行时纯整数

`[测试]` 三个测试守住修法：

| 测试                                          | 守住什么                                     |
| --------------------------------------------- | -------------------------------------------- |
| `the_table_holds_the_oscillator_it_replaced` | 每项与 `sinf` 偏差 ≤1 LSB                    |
| `the_interpolated_oscillator_tracks_sinf_within_an_lsb` | 插值后仍 ≤1 LSB                 |
| `a_chime_stays_inside_the_table_it_indexes`   | 相位永不越出表范围（插值要读 `whole + 1`）   |

实现要点：相位是 1024 项静态表的 8 位小数定点下标，包络 Q16，`inv_ramp` 每音符预计算一次。逐样本路径上只剩整数乘法与移位。表在编译期由 `sine()` 生成（`libm::sinf` 不是 `const fn`），**保真度由测试钉住，不靠注释保证**。

`[实测]` 修后：40 次自动轻点（20 chime + 20 asset），**0 panic**，单次 feed 最坏 **786 µs**（节拍的 12%），`[PLAY]` 稳定 200 kHz，0 次 ring dry / restart / watchdog。

> 过程中还踩了两个自己写的测试抓出来的 bug：整数插值的 `low + (…) >> B` 因 `+` 优先于 `>>` 而整体右移了 8 位（chime 几乎静音）；以及漏掉 TAU 因子的参考值（报 38831 LSB 偏差）。两者都是测试报的，不是代码自己发现的。

### 教训

`audio-probe` 这个诊断固件一直是绿的，因为它的 feed 跑在**协作** executor 上——浮点在那里没问题。

**这类 bug 只有中断 executor 才暴露。** 同理，`HostSpeaker` 在 host 测试里也永远碰不到。排查与回归都要放在真实的中断上下文里做。

探针的 feed 直接 `join` 在主任务上（`run_audio_only`），并不经过 `SpeakerRunner`——那个枚举只在产品固件里出现。结论成立，机制如上。

---

## feed 节拍为什么独占一个 executor

`[实测]` 协作式调度下，"没有别的任务在跑"并不成立：capture 单次占 17 ms、面板一次写占 16 ms、input 诊断在 5000 ms 窗口里占 2470 ms。这些量级全都大于 5 ms 的节拍。

播放环有 120 ms 余量。`[实测]` 一次 150 秒暖机 soak 里余量被吃掉 2 次，**最坏一次轻点迟到 167 ms**，直接放空一次 DMA。

所以 `feed_loop` 放在 `FROM_CPU_INTR1` 的 `Priority2`，`control_loop`（含 I2C）留在协作 executor。

`[实测]` 挪过去后，同一块板同一环境 158 秒连续记录：节拍 199.6–202.9 kHz（目标 200 kHz），最坏间隔 6 ms，超过 20 ms 的间隔 0 次，ring dry 0 次，DMA restart 0 次。同一段时间里 capture 依旧占 17 ms、面板依旧写 16 ms——它们不再能推迟节拍。

`Priority2` 是有意的上限：再高一级就会反过来饿死 input 与渲染。

---

## ISR 与协作侧的职责边界

`feed()` 早期在中断里做两件它不该做的事：log（抢日志锁）和 DMA restart（分配新环）。中断可以在协作任务持有这些锁时抢占它，而那个任务要等 feed 返回才释放——**死锁，且无复位可清**。

`[实测]` 修法是拆开：`feed()` 只**记录**一个 `Recovery`，`recover()` 在协作侧执行真正的重建。

修法里有一处顺序是反直觉的、也是最容易改错的：

1. `feed()` 必须**先 push 再记录**——往活的 transfer 写只有这一条路，而刚排空的环全是空的，这个 push 就是给重启预热
2. `stop()` 返回的 buffer **绝不能再 push**——它整体返回且 `pre_filled` 已置满，再 push 拿到的是环尾之后的空隙，写不进去任何东西
3. `write()` 会从第一个 descriptor 重放整个环——所以第 1 步预热的意义就是让重放的是本节拍的音频，而不是卡住前正在放的那个音

---

## 两个判定在宿主上看不见，所以做成纯函数 + 显式测试

`still_sounding(source_done, arm_sounded, arm_in_flight)` 回答"声音还在不在响"。

**被否的方向**：源用尽即判定结束。**错**：源用尽不等于声音结束——正在播最后几帧的那一路（`arm_sounded`）得先放完。反过来写反了更糟：源用尽后仍然每次 arm 一个静音环，每一个都回答"还在响"，于是相位永远出不了 `Playing`，之后每次轻点都被记成"对一个已经结束的声音的丢弃"。而这个错误**在宿主上完全看不出来**：arm 只存在于板上。

watchdog 的方向错法同类，但更贵：`tx_idle` 在 FIFO 瞬时空的那一刻就拉高，而那是两次 feed 之间的常态。当成故障处理会在声音中途拆掉并重建 DMA，重启 codec 时钟并重放一整环陈旧音频。

`[测试]` `a_spent_source_never_reports_playing_again` 与 `a_brief_idle_between_feeds_is_not_a_drained_stream`（`core/src/drivers/playback.rs`）分别钉住这两个方向；后者按一个整环的播放时长逐节拍走完，避免任何一种 unlucky 的节拍组合把它藏过去。

---

## 音效配方

`asset.pcm` 是二进制，不自述来源，配方记在 `audio_out.rs` 的 `ASSET` 注释里：660 Hz 120 ms 接 880 Hz 160 ms，两音各为 1.0 / 2.76 / 5.40 倍分音之和。

- **2.76 与 5.40**：真实钟声的泛音不是基频的整数倍，整数倍只会听起来像蜂鸣器
- **音高选 660 + 880** 是对着 chime 的 880 + 1320 挑的，为了让目录里两个声音闭眼能分辨
- **两端静音**：每个音自己衰减到约 1%，首样本 2 ms 淡入，尾部 45 ms 静音——存下来的文件没法像 `Tone` 那样在末端长出包络，波形起于非零电压就是 click，而 click 比难听的音更吵

---

## 测试是否真的在守：break test

「测试绿着跑」不说明任何事——它只证明代码被执行了。判断一条测试是否有用只有一个办法：**故意改坏它守的不变量，确认它变红**。

做法：临时改产品代码 → 跑该测试 → 还原 → 比对 hash 确认工作树回原状。跨 47 个文件取 shasum 基线，每轮后核对。

**[实测]** 15 条候选，**15 条全部变红**：

| 破坏 | 测试 |
| --- | --- |
| 合成表某项偏 4 LSB | `the_table_holds_the_oscillator_it_replaced` |
| `WAVE_TURN` 与 `PHASE_BITS` 不一致 | `a_chime_stays_inside_the_table_it_indexes` |
| 包络去掉 `min(ramp)` 上限 | `a_chime_opens_and_closes_on_silence` |
| watchdog 改用 `tx_idle` 直判 | `a_brief_idle_between_feeds_is_not_a_drained_stream` |
| HPF 写序 `0x0A`/`0x2A` 互换 | `es7210` MockI2c trace 测试 |
| 掩码合并改为覆盖写 | `es7210` 落盘字节测试 |
| 去掉上电后的重复写 | `es8311` 上电值测试 |
| 未满窗也标记已提交 | `a_partial_window_leaves_the_column_untouched` |
| 未提交窗 floor 返回 1 | `a_window_of_silence_has_a_floor_of_zero` |
| 柱高量程偏 255/200 | `dbfs_agrees_with_the_band_it_is_drawn_on` |
| 无效样本不再早退 | `an_invalid_sample_advances_nothing` |
| 敲击去掉静音残余门限 | `recognizer` 敲击/转身测试 |
| 手势配对距离放宽 200 px | `input` 双击窗口测试 |
| tap 计数每归零 | `state` 的 `off_tap_…keeps_off` |
| 中频 biquad 系数偏 0.25 dB | `weighting` 频带测试 |

**结论：342 条里没有装饰性测试，一条未删。**

此前曾按测试名相似度筛出 4 组「疑似重复」（`dbfs`/`spl`/`scope_height` 三条不倒退、`off_tap`/`off_swipe` 两条全路径），逐个读完发现它们各测不同对象——`spl()` 内部调用 `dbfs()`，走的是第二条路径；两条 `off_…` 各断言不同计数器。**按名字删测试会删掉真覆盖。**

数量不是问题，可读性才是：342 条无法阅读，因此 `audio.md` / `playback.md` 的「测试」章节改为不变量索引表——每行一条不变量、点名守它的测试、附上面这张验证列。新增测试必须落进某一行；某行哪天从"变红"变成"一直绿"，它就不再是测试而是装饰。

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

1. 固件启动后每 1.2 s 往 `INTENT_BUS` 发一次 `Intent::Business(BusinessIntent::PlayNext)`（当时的命令名，今为 `Playback(PlaybackAction::Next)`），走与真实轻点完全相同的链路
2. `[PLAY] Asset` 正常，`[PLAY] Chime` 之后立即 panic（这两个名字是当时的；目录后来扩到五个声音，见[音效配方](#音效配方)）
3. 第一次 chime **首帧**就崩，不存在累积过程

**为什么要自动化复现**：这个 bug 需要"轻点 chime"才会触发，人工点击既慢又不可重复。加上自触发后，一次烧录 + 一次抓日志即可判定，这直接决定了后面每一次修复验证都能自动化。

### 被否的两个假设

记在这里是因为它们都"看起来非常合理"，而且都会把人带向错误的修法。

#### 假设一：合成成本太高，饿死协作 executor

`[实测]` 第一次尝试时测到：chime 最坏单次 feed **4434 µs**，而节拍是 5000 µs——占 88.7%。Asset（一个 PCM blob，同样帧数）只是 memcpy，**5 µs**。结论看起来很清楚：把合成变便宜即可。

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
| `every_partial_stays_inside_the_table_it_indexes`   | 相位永不越出表范围（插值要读 `whole + 1`）   |

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

配方现在是**文件本身**。`assets/*.mid` 五个文件一共 488 字节，任何 DAW 都能打开看里面是什么音、多长。

**配方的事实来源是 `crates/tool/src/midi.rs` 的 `RECIPES` 表**，`.mid` 文件是它生成的构建产物（虽然入库）。改音效是改表里的一行 + `moon run iot:gen-midi-assets`，不是改二进制。DAW 里改了文件而表没改，`the_files_are_what_the_recipes_generate` 会让构建变红——**这是替代原来「配方写在注释里、构建不运行」的那条**。

`bsp-esp` 那边还留着一个 `#[ignore]` 的 `write_the_catalogue_as_a_wav`，**不用烧板子就能听**：它把文件渲染成一个 WAV，走的是驱动自己的合成路径（`Tone` 跑解析出来的表）。改一批音高的验证成本从二十分钟的烧录加抓串口降到一秒。

### 为什么配方在工具里而不是在板里

生成资产不是测试，所以配方表和 `.mid` 生成器搬出了 `bsp-esp` 的测试模块，进了零依赖的 `iot-tool`。

代价是**两边看不见对方**：配方不再能读到 `Sound`，固件也读不到配方，唯一的联结只剩文件名约定（`Rise` → `rise.mid`，`label_of` 得说同一件事）。

这条约定只有一处能验：`the_board_names_the_same_files` 读 `bsp-esp/src/sounds.rs` 的 `include_bytes!` 清单，跟配方表对。少了它，一次只改一边的重命名会静默通过——两个文件都在，两边都认领了各自的，只有板子放错。

### 搬出去的代价：裸金属的 `--workspace`

`lefthook.yml` 的 `iot-clippy` 是全仓最后一处裸金属 `--workspace`（`cargo clippy --workspace --target riscv32imac-unknown-none-elf`）。它一直能跑，只是因为每个成员都恰好是固件代码。`iot-tool` 一进来就成了工作区里第一个 std crate，裸金属 target 没有 std，pre-commit 立刻 `E0463: can't find crate for std`。

**feature 修不了这个，已验。** `[lib]` / `[[bin]]` / `[[test]]` 都加上 `required-features` 之后，`cargo clippy --workspace --target <riscv>` 照样编译那个 crate、照样报错——`required-features` 管的是「一个包里的哪些 target 参与构建」，管不到「这个包本身」。加上 feature 唯一有效的场合是反向的：`cargo run -p iot-tool` 不带 feature 时会干净地报一句 `target ... requires the features`，而不是编到一半炸。

修法是 `--exclude iot-tool`。这本来就是那场迁移的收尾：[architecture.md 的「新增芯片家族」清单第 4 条](@/development/iot/architecture.md)写着 `moon`/`lefthook` 的 `--workspace --target` 要改按固件作用域，`moon.yml` 早就全改成显式 `-p iot-app` 了，lefthook 这条只是从来没改、也从来没需要改。代价是**再加一个主机侧工具就得来这里补一行**，所以注释里写明了。

## 为什么第一版五个音听起来不像五个

`[实测]` 上板听下来第一版「不够清晰」。原因量化出来是三条，每条都踩在耳标准则上：

**一、`TAP` 和 `ERROR` 用了同一个音高——E6 = 1319 Hz，零半音之差。** 五音里唯一一对连音高都共享的，只差在上行还是下行。

**二、10 个两两组合里，最近的音全部落在 0–5 个半音之内**，没有一个超过纯四度。总基频跨度只有 2 个八度，四个声音挤在 E5–G6。准则原文要的是「gross differences ... two or three octaves」。

**三、音符数是 `1/2/2/2/3`。** 准则里最有效的那个维度，三个声音占俩。时长也挤：290 / 330 / 338 挨着。

修法按准则本身来：音符数摊到 1/2/2/3/4，音区按 3 个半音一档排开，一个音高都不共用，时长摊成 200 / 262 / 312 / 350 / 384 ms。唯一重复的「2 音」落在 `ERROR` 与 `RISE`——唯一一对轮廓相反的组合。

### 为什么排得下：五个音正好吃光可用音区

`[实测]` 这不是设计出来的，是搜出来的。约束写死：音高 277–1760 Hz（下面喇叭发不出来，上面 ×2.76 的分音就超过 5 kHz），相邻音区至少隔 3 个半音。

结果：**「纯八度 + 下行三度 + 上行三度」三种形状加起来 20 个半音，加上四道间隔 12 个，正好 32，一个不剩。** 把间隔提到 4 个半音就无解。

也就是说这套目录没有余量：任何一条音往旁边挪一档，就得从另一条音的音区里抢。**要更大的区分度只有两条路——把频率范围拉开（低到 277 Hz 以下，或高到分音上限之外），或者减少声音数。** 文献那边也支持第二条：McGookin & Brewster 2004 测到同时呈现 4 个耳标时只有约 30% 被正确识别。

### 为什么 `TAP` 从两音变成三音

这是这一版最贵的一处退让。`TAP` 既是默认音也是开机音，是目录里最高频的那个交互，而 Google 的规矩恰恰是**交互越频繁越该安静**。三音比两音长 30 ms。

准则要音符数，Google 要克制，两条都在对方的领地上。`[判断]` 选了准则，因为用户反馈说的就是分不清；而 `[实测]` 上去板听之前，这一版还没验过声音好不好听——**这是唯一一条我听不到、只能靠数字兜底的改动**。

### 为什么不是做成琶音

`RISE` 原本和 `TAP` 同为「两个上行音」。耳标指南说过这种组合容易混淆，修法是「让节奏尽量不同」——**照那条做，第一版改成了四音符琶音**。（第二版把区分度的问题又推回来一次，见上面「为什么第一版五个音听起来不像五个」。）

**这是错的，被 Google 自己的十年数据否掉。** 他们的相机快门音最初「根本不像快门，带旋律，很有个性」，后来主动做减法：

> dialing up all individual sounds to that level often comes at the expense of the user experience. **Today, people don't really want to be hearing melodies when they're taking photos.**

> Good multisensory design often **disappears** — users might not actively notice it.

> **The more often an interaction happens, the less intrusive** that sound should be.

一个每天可能响几十次的功能音做成琶音 = 引人注意 = 削弱真正重要的时刻。**耳标文献测的是「用户能不能分辨 A 和 B」，那是数据界面；vanling 是桌面陪伴设备，五个音里没有一个绑数据参数，按 Google 的标准该往安静了做。**

修法改成**纯八度**，放在最低音区因此不与 `TAP` 撞。音高见[开发文档的目录表](@/development/iot/playback.md#声音从哪来)。

**留痕的理由**：这条决定的依据是外部产品的实践（Google 12 人专职声音团队、跨全公司产品），不是我们的实验。下次看到 `TAP` 和 `RISE` 音区接近想「改得更明显」，会再犯一次——**音区重叠在两个轮廓不同的音上不是缺陷，是设计**。

### 为什么只有两个分音：分音数是成本，不是审美

分音数直接乘在每样本成本上。`[实测]` 上板后节拍 199.8–200.3 kHz、最坏单次 feed **583 µs**（节拍的 11.7%），加两个分音后仍有四倍余量。

`583 µs` 比记录在案的 786 µs 还低，是因为 `render_slice` 同时从逐帧调用改成了整段调用——每拍少 240 次函数进入，**把两个分音的代价吃掉了还有余**。

8 个分音会把 per-sample 工作推过 5 ms 节拍。届时只有两条路：缓存波表（只接受谐波 → 回到蜂鸣器），或者拉长间隔。**加第三个分音之前先重新测一次 feed 成本**，别拿这条外推当许可。

### 目录里三个名字改过，各有各的理由

| 现在 | 原来 | 为什么 |
| --- | --- | --- |
| `Tap` | `Chime` | `Chime` 与 `Bell` 英文近义（都指钟/铃），两个名字并排放就难分；而且 `Tap` 说的是触发而非声音形状 |
| `Notice` | `Notify` | 原名是动词，同栏里 `Warn`/`Error` 是名词。同栏应该统一 |
| `Rise` | `Bell` | 它是纯八度上行，不是「铃」；而且五个音共用音色，按音色命名在这一栏串味。`Rise` 说的是音高运动 |

对照 Google Material 的官方音效库（`material_product_sounds`），我们对应的是：

| 我们 | Material |
| --- | --- |
| `TAP` | `ui_tap-variant-*` |
| `NOTICE` | `notification_simple-*` |
| `WARN` | `alert_high-intensity` |
| `ERROR` | `alert_error-*` |
| `RISE` | 无对应（Material 这里是 `hero_simple-celebration`） |

**Material 那一类我们缺**：它的 `ui_lock`/`ui_unlock` 是成对的状态翻转音，而 `PlaybackAction::SetMuted` 现在完全静默——喇叭不出声，只有屏幕上的 `MUTE`。补这一对是功能改动，不在本次范围。

Material 的三段式命名（`notification_simple`）搬不过来：面板的值槽是 7 个字位，20 字放不下。

本页讲 `Cp0Disabled` 的段落**保留当年的 `Chime` 字样**不改：那是一次真实故障的记录，改名会让它对不上当时的串口日志。

### 为什么是 MIDI 而不是采样

被否的方向：把五个声音存成 PCM blob。**当时的配方是写在 `audio_out.rs` 注释里的一串数字**，构建不运行，所以配方和代码可以各走各的而没人发现；31 KB 的二进制在 `git diff` 里看不出任何东西。

MIDI 不解决合成能力——它不带音色，逐音符包络两个版本都没有——它解决的是**出处**：文件是标准格式、可以被打开和修改、变更在 PR 里是一条可读的记录。代价就是上面那条 `ramp_ms` 从声明退成推断。

### 分音为什么要非谐

`TIMBRE = [(100, 180), (276, 76)]`。**2.76 是真实钟声的第二泛音比**，整数倍只会听起来像蜂鸣器——这也是原来那个声音必须存成采样、不能现场合成的原因。

`[测试]` `the_family_timbres_partials_are_inharmonic` 钉住 276 并断言它不是 100 的整数倍，防止有人「简化」成 3。`[测试]` `the_family_timbre_cannot_outrun_the_rail` 钉住增益加起来是满量程：两个分音对齐时推出满刻度就是声音末尾的一个 click。

### 单分音必须等于被替换的那套合成

`[测试]` `a_lone_partial_is_the_oscillator_it_replaced`：`Tone::on(score, &PLAIN)` 走单一满增益分音，结果与**被替换掉的那套合成原样写在测试里的参考实现**逐样本比对，跑完整条谱（1.6 万样本）。

写成参考实现而不是粘 16 个数字进来：粘进来的数组只钉住 16 个样本；而这条测试会在整条谱上发现任何一处 ramp 或插值的漂移。

---

## 测试是否真的在守：break test

「测试绿着跑」不说明任何事——它只证明代码被执行了。判断一条测试是否有用只有一个办法：**故意改坏它守的不变量，确认它变红**。

做法：临时改产品代码 → 跑该测试 → 还原 → 比对 hash 确认工作树回原状。跨 47 个文件取 shasum 基线，每轮后核对。

**[实测]** 15 条候选，**15 条全部变红**：

| 破坏 | 测试 |
| --- | --- |
| 合成表某项偏 4 LSB | `the_table_holds_the_oscillator_it_replaced` |
| `WAVE_TURN` 与 `PHASE_BITS` 不一致 | `every_partial_stays_inside_the_table_it_indexes` |
| 包络去掉 `min(ramp)` 上限 | `a_tap_opens_and_closes_on_silence` |
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

**[实测] 本次新增的测试另行 break 过一次，四处全部变红**：

| 破坏 | 测试 |
| --- | --- |
| 分音比值 2.76 改成整数 3 | `the_family_timbres_partials_are_inharmonic` |
| 增益加起来超过满量程 | `the_family_timbre_cannot_outrun_the_rail` |
| `RECIPES` 表里 Rise 的音高 | `the_files_are_what_the_recipes_generate` |
| ramp 推断从 `min(ms/4, 40)` 改成 `min(ms/2, 40)` | `a_note_ramps_at_both_of_its_ends` |

**结论：342 条里没有装饰性测试，一条未删。**

此前曾按测试名相似度筛出 4 组「疑似重复」（`dbfs`/`spl`/`scope_height` 三条不倒退、`off_tap`/`off_swipe` 两条全路径），逐个读完发现它们各测不同对象——`spl()` 内部调用 `dbfs()`，走的是第二条路径；两条 `off_…` 各断言不同计数器。**按名字删测试会删掉真覆盖。**

数量不是问题，可读性才是：342 条无法阅读，因此 `audio.md` / `playback.md` 的「测试」章节改为不变量索引表——每行一条不变量、点名守它的测试、附上面这张验证列。新增测试必须落进某一行；某行哪天从"变红"变成"一直绿"，它就不再是测试而是装饰。

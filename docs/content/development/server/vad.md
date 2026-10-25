+++
title = "VAD 与回合判定"
weight = 203
+++

# VAD 与回合判定

VAD 决定哪一段音频是语音，ASR 决定这段语音什么时候算说完。两者是两级闸门，不在同一层：VAD 判"语音结束"，ASR 判"回合结束"。

## 在链上的位置

每轮对话一条链，VAD 是第二环：

```text
AudioFrame → OpusDecodeNode → VadNode → AsrNode → TurnNode → LingNode → TtsNode
```

| 节点 | 做什么 | 关键约束 |
| --- | --- | --- |
| `OpusDecodeNode` | Opus 解码，rubato FFT 重采样到 16 kHz | 单帧上限 `MAX_OPUS_FRAME_MS = 120`；下游 VAD 与 ASR 都硬编码假设 16 kHz 输入，链上没有第二个重采样点 |
| `VadNode` | 按模型窗口切帧，维护语音前缀，产出边界事件 | 串行状态机，按轮从池中取用 |
| `AsrNode` | 喂流式识别器，产出 partial 与最终文本 | 静音确认 200 ms（见下文） |
| `TurnNode` | 把 `TurnText` 收尾为 `TurnComplete` | 只做边界闭合，不做判定 |

链上统一事件：`PcmFrame` / `SpeechStarted` / `SpeechEnded` / `PartialTranscript` / `TurnText` / `TurnComplete`。控制事件 `ListenMode`、`FinishTurn` 也走同一条流。

## Vad 契约与对象池

定义在 `service/src/component/vad.rs`：

```rust
pub trait Vad: Send + Sync {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError>;
    fn is_speech(&mut self) -> bool;
    fn clear(&mut self);
    fn window_size(&self) -> usize;
}
```

`accept_waveform` 送入**恰好一个 `window_size`** 的窗口，返回语音分数并推进内部状态机；`is_speech` 读当前状态；`clear` 清空跨轮残留。

`VadPool` 只有 `acquire` / `release`。池存在的理由不是省内存，而是 **VAD 是有状态的串行对象**：同一时刻只能被一个 Round 持有，不能像无状态服务那样共享。`acquire` 前一律 `clear()`，`release` 归还；`VadNode` 以 `ReleaseMode::Deferred` 在整轮结束时归还。

## Earshot 判据

`VadEarshot`（`api/src/component/vad/model/earshot/`）是默认模型：内嵌 Silero VAD 权重，纯 Rust 推理，不依赖 ONNX 运行时。

处理是两段：

1. 输入按 160 样本（10 ms）切帧，过 RNNoise 去噪后 clamp 到 `[-1, 1]`；
2. 去噪样本累积到 256 样本（16 ms）窗口，每次取一个窗口预测分数与 RMS。

一帧算"有声"要同时满足：

```text
rms >= 0.003  &&  score >= threshold(0.4)
```

**起点**（`is_speech` 由假变真）还要同时满足四条：

| 条件 | 默认值 | 作用 |
| --- | --- | --- |
| 连续有声帧 | ≥ 3 | 挡单帧突刺 |
| 累计预测条数 | ≥ 5 | 挡住持续不到的瞬态 |
| 累计语音时长 | ≥ `min_speech_duration` = 150 ms | 太短的气口不算一轮 |
| 最近 40 窗中有声占比 | ≥ 0.55 | 抖动窗口，避免断续触发 |

未进入语音态时，如果连续静音超过 80 ms，累计的预测与语音时长会被清空；80 ms 以内的间断不会清零，所以说得慢一点不会被反复打断。

**终点**用滞回：语音态下判"仍活跃"的门槛降到 `deactivation_threshold`（0.2）。只要有一帧活跃，静音计时归零；静音累计**超过** `min_silence_duration`（550 ms）才 `clear_state()`，回到非语音。

`VadVoid` 是测试替身：`is_speech` 恒真、分数恒 1、窗口 512 样本（只是为了有个值），用来跳过 VAD 决策验证下游。

## VadNode 行为

`VadNode` 不做判定，只负责切帧、缓冲和事件转换。

- **切帧**：每个 `PcmFrame` 按 `vad.window_size()` 切开；末尾不足一个窗口的样本原样作为一块送出，不攒到下一帧。
- **前缀缓冲**：每块样本先入 `prefix_buffer`，上限 `PREFIX_SAMPLES_MAX = 9600`（16 kHz 下约 600 ms）；超限从头部丢弃最旧的样本。这一段是 VAD 的检测延迟补偿——VAD 报"有语音"时，人其实已经开口了。
- **事件顺序**：每个窗口的 `PcmFrame` 先透传给下游，`SpeechStarted` 紧跟在触发那一块**之后**。这样 ASR 在收到 `SpeechStarted` 时，自己的前缀缓冲已经包含触发窗口，不会漏掉开头。`SpeechEnded` 在下降沿同样跟在块之后。
- **释放**：`ReleaseMode::Deferred`。`on_acquire` 从池取用，`on_release` 归还，`Drop` 兜底防泄漏；`stream()` 本身可重复调用，状态在 `Arc<Mutex<_>>` 里。
- **错误**：`accept_waveform` 返回错误时停止处理当前 `PcmFrame` 的剩余窗口，已产出的事件照常下发。

## 两级静音闸门

| 闸门 | 谁判 | 阈值 | 为什么是这个数 |
| --- | --- | --- | --- |
| 语音结束 | Earshot | 550 ms | 滞回要够长，正常换气、拖音不能被切断 |
| 回合结束 | `AsrNode` | 200 ms | VAD 已确认语音结束，这里只需要再等一小段确认，不必让用户多等 350 ms |

`AsrNode` 的确认条件是三个同时成立：

```text
spoken && !speech_active && silence_samples >= 200ms × sample_rate / 1000
```

`spoken` 由 `SpeechStarted` 置位，`speech_active` 由 `SpeechStarted` / `SpeechEnded` 切换，`silence_samples` 按收到的 `PcmFrame` 样本数累计。确认后 `finish_stream`：有文本出 `TurnText`，有流但空文本出 `EmptyInput`，无流出 `Nothing`。

流式模式下还会逐帧读 partial，只有文本变化时才发 `PartialTranscript`。

**按键模式**（`ListenMode { streaming: false }`）把两个动作分开：音频照常逐帧喂给识别器（预解码暖管），但抑制 partial 与静音确认，只有 `FinishTurn` 到来时才成文。所以按键说话不会因为中途停顿被提前截断。

`TurnNode` 只把 `TurnText` 换成 `TurnComplete`，其余事件透传；它用 `ReleaseMode::Immediate`，流结束即释放。

## 传输停滞检测

静音确认只能统计**已经到达**的样本。客户端中途停止发送音频时 `silence_samples` 不再增长，链永远不会收尾，于是 Session 用墙钟兜底：`check_transport_stall()`（`session/mod.rs`）在 `spoken` 仍活动且 `last_audio_received.elapsed() >= silence_voice_timeout`（1200 ms）时向链投喂 `FinishTurn`。

按键模式跳过该检测（`Listening` 且 `is_voice_break_detect == false` 时直接返回）——设备的 `listen(stop)` 是唯一回合边界，绝不因静默超时"抢答"。

**为什么静音确认不用墙钟**：测试与 CI 会把音频瞬间注入（没有实时速率），墙钟与音频流时间解耦，慢机器上会提前触发。按样本计数让触发点只取决于已解码内容，与消费速度无关。

**为什么要墙钟做传输停滞**：没有样本到达就没有样本可数，流断只能靠时间发现。

这套分层与 OpenAI Realtime 的 `silence_duration_ms`（音频级）+ `idle_timeout_ms`（墙钟级）一致。

## 回合终止路径

| 路径 | 触发条件 | 谁发起 |
| --- | --- | --- |
| 静音确认 | 流式模式静音 ≥ 200 ms | `AsrNode` 自行 `finish_stream()` |
| 传输停滞 | `speaking` 中 1200 ms 无新音频 | Session 投喂 `FinishTurn` |
| 设备按键 | `listen(stop)` | Session 投喂 `FinishTurn`（manual 唯一路径） |

打断（`Abort` / 新的 `listen(start)`）不在此表：它终止的是整个 Round，见[核心架构](@/development/server/architecture.md)的「中断（BargeIn）」。

## 参数速查

| 参数 | 值 | 谁读它 |
| --- | --- | --- |
| `threshold` | 0.4 | Earshot 起点判据 |
| `deactivation_threshold` | 0.2 | Earshot 终点滞回 |
| `min_speech_duration` | 150 ms | Earshot 起点：太短不算一轮 |
| `min_silence_duration` | 550 ms | Earshot 终点：静音多久算语音结束 |
| `rms_threshold` | 0.003 | Earshot：先过能量门，再看得分 |
| 滑动环 | 40 窗 × 0.55 | Earshot 起点：最近 640 ms 的有声占比 |
| `SILENCE_CONFIRM_MS` | 200 ms | `AsrNode`：语音结束后多久收回合 |
| `PREFIX_SAMPLES_MAX` | 9600 样本（约 600 ms） | `VadNode` 与 `AsrNode` 共用的前缀上限 |
| `VAD_WINDOW_SIZE` | 256 样本（16 ms） | `VadNode` 切帧粒度 |

## 测试与盲区

`api/tests/vad_test.rs` 四条，全部用 `api/resources/test/` 下的真实 wav：

| 测试 | 断言 |
| --- | --- |
| `test_noise_burst_no_false_positive` | 噪声突刺不触发 |
| `test_keyboard_tap_no_false_positive` | 键盘敲击不触发（`min_silence` 550 ms） |
| `test_tap_desk_no_false_positive` | 拍桌不触发（前 10 帧先断言，避免长尾掩盖） |
| `test_state_machine` | 语音 → 2 s 静音 → 语音，`is_speech` 正确翻转 |

`api/tests/vad_analysis_test.rs` 用 TEN-vad 标注集算分段精确率/召回率，属于分析而非 CI 门槛。

`api/tests/pipeline_nodes_test.rs` 守 `VadNode` 的两条契约：`SpeechStarted` 排在触发块之后；前缀缓冲在触发前被截到 9600 样本（经下游 ASR 首次收到的样本数断言）。

以下是当前**没有**自动化覆盖的部分，改这里时按盲区对待：

- 对象池在并发轮次下的复用（同一实例被两轮同时持有会怎样）；
- 200 ms 静音确认与真实设备帧节奏的边界（只有合成帧测试）；
- `Deferred` 释放路径在链提前取消时的行为；
- 真机麦克风前端的噪声底与 `rms_threshold` 的关系（阈值只在样本文件上调过）。

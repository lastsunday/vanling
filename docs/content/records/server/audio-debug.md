+++
title = "TTS 播放卡顿与音量归一化"
weight = 402
+++

<!-- doc-audience: ai -->

# TTS 播放卡顿与音量归一化

## 现象

实时 WebSocket TTS 音频播放：开始正常，几秒后出现明显**卡顿**和**电流声**。离线导出的 WAV 完全干净。

## 调查

### 1. 先排除服务端音频管线

Session 集成测试产生的 WAV 播放干净 → TTS → 重采样 → Opus → WebSocket 这条链本身没有质量问题，问题在**发送节奏**或**客户端消费**。

### 2. 逐个排除可调项

| 改动 | 效果 |
| --- | --- |
| `opus-rs` → `opus` crate（C binding） | 无效 |
| 输出采样率 16000 → 24000 Hz | 无效 |
| `sherpa-onnx::LinearResampler` → `rubato::FftFixedIn` | 无效 |
| `Application::LowDelay` → `Application::Audio` + FEC | 无效 |
| `Signal::Auto` → `Signal::Voice` + `Bandwidth::Superwideband` | 无效 |
| VBR → CBR（`set_vbr(false)`） | 无效 |
| 旧 pacing：elapsed 相对时间 → 绝对目标时间 | 无效 |

编码参数全部无效，说明失真不是编码引入的，而是播放端在等不到按时到达的数据。

### 3. 客户端代码

`apps/server-ui/public/test/device/js/core/audio/` 下三个事实：

- `player.js` 的 `_opus_decode` 是**同步 WASM 调用**，解码期间阻塞 JS 主线程；
- `player.js` 每次从队列批量取包（内层循环最多 99 包）；
- `stream-context.js` 用 Web Audio API 链式调度：`source.start(max(scheduledEndTime, currentTime))`，每段 120 ms。

主线程一旦被长解码阻塞，`scheduledEndTime` 就会过期，下一段无法接上——听感即中断加噪声。

## 根因

TTS 对一个完整句子先做约 13 秒推理，产出的 PCM 一次性切成 228 帧（60 ms/帧）同时涌进发送端。旧 pacing 以 `audio_start_time`（TTS::Start 时刻）为基准算绝对目标：

```
target = audio_start_time + paced_index × 60ms
```

13 秒后所有帧到达时，`target` 全部落在过去，`now > target` 恒成立 → 永不 sleep → **228 帧在 10 ms 内爆发发送**。客户端瞬间收到 200+ 包，一次性解码阻塞主线程超过 1 秒，Web Audio 调度断裂。

离线 WAV 不受影响，因为那里没有实时调度约束：全部帧解码后顺序拼接，解码阻塞不产生听觉后果。

## 修复

思路是把「绝对时间目标」换成「相对节拍」：首帧之后每帧从**当前时刻**起固定间隔，落后就跳过而不是追赶。这样发送节奏只与消费者的真实进度有关，与 TTS 何时开始推理无关。

当前实现位于 `service/src/session/mod.rs` 的输出循环（`audio_pacer`）：首帧到达时惰性创建 `interval_at(now + frame_duration, frame_duration)`，之后每帧 `pacer.tick().await`；非音频消息把 pacer 置空，下一段音频重新起拍。帧长取自 `AudioSpec::frame_duration_ms`，不写死 60 ms。

与最初补丁的两处差异（改这里时要留意）：最初用 `MissedTickBehavior::Skip` 显式跳过积压的拍子，现在用默认策略；最初首帧立即发，现在首个 `tick()` 会先等一个帧长。在持续供帧的前提下两者等效，但**突发场景下不再有跳过积压的保护**。

效果：初始突发从 228 帧降到 1 帧量级，稳态一帧一包，客户端批解码从 200+ 帧降到 1 帧，主线程阻塞从 >1 s 降到 <1 ms。

## 现象二：音量前后不一致

中英混读时，后半段英文明显更响，听感割裂。

### 方案一：固定参数 DRC（已废弃）

传统 feed-forward 压缩器，peak 检测包络 + 可选 soft knee。参数由网格搜索 + EBU R128 客观指标选出：threshold −28 dB、ratio 6、knee 0、attack 5 ms、release 80 ms、makeup 8 dB。结果 LRA 从 9.58 降到 5.66 LU，但 Crest Factor 从 15.4 掉到 6.2 dB——音量齐了，动态没了，人耳感知为「闷」。

### 方案二：自适应 RMS 增益归一化

`adaptive_normalize()`（`api/src/util/compressor.rs`）分六步：

| 步骤 | 做法 |
| --- | --- |
| 分帧 RMS | 200 ms 窗、10 ms 步长 |
| 目标响度 | 取所有帧 RMS 的 p30 |
| 帧增益 | `target_rms / frame_rms`，限幅 ±12 dB |
| 增益平滑 | attack 5 ms / release 300 ms，逐样点 IIR |
| 全局响度补偿 | 匹配原始 RMS 再 +3 dB |
| 软限幅 | −0.5 dBFS 上限，防削波 |

指标对比：

| 指标 | 原始 | Adaptive |
| --- | --- | --- |
| LRA | 9.08 LU | 1.59 LU（↓82%） |
| LUFS | −26.90 | −24.14（响约 3 dB） |
| Crest Factor | 15.3 dB | 14.7 dB（动态保住） |

10 秒音频总开销约 10 ms（分帧 O(n)、排序 ~1000 帧、逐样点 IIR），相对 TTS 推理数秒可忽略。

## 现状

`adaptive_normalize()` **没有被管线调用**（全仓只有定义处、没有调用方），`pcm_compress()` / `grid_search_compressor()` / `evaluate_compressed()` 也只剩 `compressor.rs` 内部自用。当时用来对比的 `test_compare_raw_vs_processed`、`test_grid_search_compressor` 已不存在，因此上面两组指标是当时的测量结果，用现在的代码复现不出来。要重新引入，得先补一条能跑的分析用例。

## 关键文件

| 文件 | 作用 |
| --- | --- |
| `apps/server/service/src/session/mod.rs` | `audio_pacer`：帧发送节拍（修复位置） |
| `apps/server/api/src/component/tts/model/matcha/mod.rs` | MatchaTts 音频生成 |
| `apps/server/api/src/util/compressor.rs` | `adaptive_normalize()` / `pcm_compress()` / `evaluate_compressed()`，当前无调用方 |
| `apps/server-ui/public/test/device/js/core/audio/player.js` | 客户端 Opus 解码与批量取帧 |
| `apps/server-ui/public/test/device/js/core/audio/stream-context.js` | Web Audio API 链式调度 |

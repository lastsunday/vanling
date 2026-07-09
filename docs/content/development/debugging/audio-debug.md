+++
title = "Audio 调试"
weight = 401
+++

# Audio 调试

## 问题

实时 WebSocket TTS 音频播放：开始正常，几秒后出现明显**卡顿**和**电流声**。

## 调查

### 1. Validate server output

- Session 集成测试产生 WAV 文件，播放干净
- **结论：服务端音频管线（TTS → resample → Opus → WebSocket）无质量问题**

### 2. Rule out each tunable

| 改动 | 效果 |
|------|------|
| `opus-rs` → `opus` crate (C binding) | 无效 |
| 输出采样率 16000 → 24000Hz | 无效 |
| `sherpa-onnx::LinearResampler` → `rubato::FftFixedIn` | 无效 |
| `Application::LowDelay` → `Application::Audio` + FEC | 无效 |
| `Signal::Auto` → `Signal::Voice` + `Bandwidth::Superwideband` | 无效 |
| VBR → CBR (`set_vbr(false)`) | 无效 |
| 旧 pacing: elapsed 相对时间 → 绝对目标时间 | 无效 |

### 3. Client code analysis

查阅 `apps/server-ui/public/test/device/js/` 下的客户端代码：

**Opus 解码（主线程同步 WASM 调用）：**

```javascript
// player.js
decode: function (opusData) {
    const decodedSamples = mod._opus_decode(    // ← 同步 WASM
        this.decoderPtr, opusPtr, opusData.length,
        pcmPtr, this.frameSize, 0
    );
    // 阻塞主线程直到解码完成
}
```

**Web Audio API 链式调度（每段 120ms）：**

```javascript
// stream-context.js
const startTime = Math.max(this.scheduledEndTime, currentTime);
this.source.start(startTime);
this.scheduledEndTime = startTime + audioBuffer.duration;
```

**批量取帧（内层循环最多 99 包）：**

```javascript
// player.js
const data = await this.queue.dequeue(99, 30);
// 99 包一次喂入解码循环
```

## 根因

### 时序线

```
TTS 模型推理 (~13s)           TTS::Start          AudioResult × N
│────────────────────────────┤─────────────────────►
                             │
audio_start_time = T=0       │
                             │   (13 秒后所有帧到达 OutputController)
                             │
Frame 228:                   │
  paced_index = 228 - 10     │
  target = 0 + 218 × 60ms    │
         = 13080ms ≈ 13s     │
  now = 13s + ϵ > target     │
  → sleep(0) → 立即发送      │
                             ▼
                    228 帧爆发发送 (<10ms)
```

### 具体机制

1. **TTS 模型**对完整句子生成连续 PCM（~13 秒推理时间）
2. PCM 被分割为 228 帧（60ms/帧），经 mpsc channel 同时到达 `OutputController`
3. **旧 pacing 逻辑**以 `audio_start_time`（TTS::Start 时刻）为基准做绝对目标计算：
   ```
   target = audio_start_time + paced_index × 60ms
   ```
   所有目标都在 13 秒前 → `now > target` → 永远不 sleep → **228 帧 <10ms 内爆发发送**
4. **客户端**瞬间收到 200+ Opus 包 → 解码循环单次批量处理全部
5. WASM 中 `_opus_decode` 是**同步调用**，阻塞 JS 主线程 >1 秒
6. 主线程被阻塞期间，Web Audio API 无法创建下一段 `AudioBufferSourceNode`
7. `scheduledEndTime` 已过 → 链式调度断裂 → 产生可听中断和噪声

### 为什么不影响离线 WAV？

- 离线测试（`test_tts_audio_collect`）解码全部 Opus 帧为连续 PCM 后写入文件
- 没有实时调度约束，WASM 解码阻塞不产生听觉影响
- 离线 WAV = 所有帧无缝拼接 → 完全干净

## 修复

### 方案

用 `tokio::time::interval_at` + `MissedTickBehavior::Skip` **惰性创建**，首帧立即发，后续帧从当前时刻起严格 20ms 间距。

### 代码

```rust
// output_controller.rs — 关键改动

use tokio::time::{Duration, Instant, MissedTickBehavior, interval_at};

pub struct OutputController {
    interval: Option<tokio::time::Interval>,
    frame_duration: u64,
    // ...
}

impl OutputController {
    async fn pace_audio(&mut self) {
        if let Some(interval) = &mut self.interval {
            interval.tick().await;  // 等 20ms → 发送 → 再等 20ms → 发送...
        } else {
            let start = Instant::now() + Duration::from_millis(self.frame_duration);
            let mut intv = interval_at(start, Duration::from_millis(self.frame_duration));
            intv.set_missed_tick_behavior(MissedTickBehavior::Skip);
            self.interval = Some(intv); // 首帧立即发，interval 从下一帧起算
        }
    }
}
```

### 改进后时序

```
TTS 推理 (~13s)           TTS::Start     AudioResult × N
│────────────────────────┤───────────────►
                         │
Frame 1:                 │
  interval = None        │
  → 创建 interval(now+20ms, 20ms)  ← 不 tick，立即发送
                         │
Frame 2:                 │
  interval.tick().await  │  → 等 20ms → 发送
Frame 3:                 │
  interval.tick().await  │  → 等 20ms → 发送
Frame 4:                 │
  interval.tick().await  │  → 等 20ms → 发送
  ...                    │  严格 20ms/帧
                         ▼
                    客户端稳定播放
```

### 效果对比

| 指标 | 旧方案 | 新方案 |
|------|--------|--------|
| 初始突发 | 228 帧 (<10ms) | 1 帧（首帧立即发）|
| 稳态间隔 | 无（全部爆发） | 严格 20ms/帧 |
| 客户端批解码 | 200+ 帧/批 | 1 帧/批 |
| 主线程阻塞 | >1 秒 | <1ms |
| Web Audio API | 调度断裂 | 连续调度 |

## 关键文件

| 文件 | 作用 |
|------|------|
| `apps/server/api/src/ws/session/output_controller.rs` | 帧发送 pacing 逻辑（修复位置） |
| `apps/server-ui/public/test/device/js/core/audio/player.js` | 客户端 Opus 解码 + 批量取帧 |
| `apps/server-ui/public/test/device/js/core/audio/stream-context.js` | Web Audio API 链式调度 |
| `apps/server/api/src/tts/model/matcha/mod.rs` | MatchaTts 模型音频生成 |
| `apps/server/api/src/ws/session/mod.rs` | WebSocket 会话帧发送 |
| `apps/server/api/src/tts/mod.rs` | `encode_sample_to_tts_packet` 帧分割编码 |

## TTS 测试工具

音频质量分析工具位于 `apps/server/api/tests/common/tts.rs`，TTS 集成测试位于 `apps/server/api/tests/tts_test.rs`。

### 共享辅助函数

| 函数 | 说明 |
|------|------|
| `ws_root()` | monorepo 根路径（`CARGO_MANIFEST_DIR` 往上 3 层） |
| `test_audio_config()` | 测试标准 AudioConfig（16000Hz / mono / 20ms 帧） |
| `run_tts_test(tts_config, audio_config, wav)` | 完整流程：创建模型 → TTS 流式推理 → Opus 解码 → 写 WAV |
| `run_length_scale_scan(dir, audio_cfg, wav_prefix, ls_values)` | 扫描多个 `length_scale` 值，找到语速校准点 |
| `tts_stream(text)` | 从字符串创建 TTS 输入 `Stream` |
| `analyze_audio(samples, sample_rate, gen_elapsed, std_duration_secs)` | 返回 `TtsAudioDiagnostics` 诊断结果 |
| `estimate_std_duration(text)` | 基于文本内容估算标准时长（OmniVoice 权重系统） |

### 标准时长估算

`estimate_std_duration(text)` 使用 OmniVoice RuleDurationEstimator 权重系统，为文本分配 Unicode 范围语音权重，再以固定速度因子 12.0 weight/sec（≈4 汉字/秒 或 150 WPM 英文）计算标准时长。结果**不随模型或 length_scale 变化**，提供跨模型一致的参考标尺。

**TEST_TTS_TEXT**：`2024年5月11号，拨打110或者18920240511，花了99块钱。我在学习machine learning和artificial intelligence。`

标准时长：~14.1 秒（权重 ≈168.8 / 12.0）。

### 三维评分体系

`TtsAudioDiagnostics` 独立报告三个维度，**不合并总分**：

| 维度 | 字段 | 评分依据 | 等级 |
|------|------|---------|------|
| **Audio**（音质） | `shimmer_pct`, `dynamic_range_db` | shimmer 70% + DR 30% | A/B/C/D/F |
| **Perf**（性能） | `rtf` | RTF 实时因子阈值 | A/B/C/D/F |
| **Timing**（语速） | `duration_secs`, `std_duration_secs`, `std_diff_secs` | 偏离标准时长百分比 | A/B/C/D/F |

**等级字母：**

| 得分范围 | 等级 |
|----------|------|
| ≥ 86 | A |
| 66–85 | B |
| 41–65 | C |
| 21–40 | D |
| < 21 | F |

#### Audio 评分

基于 shimmer（帧间振幅变动率）和动态范围。

**Shimmer 等级（临床语音病理学参考）：**

| 等级 | Shimmer (%) | 含义 |
|------|-------------|------|
| A | < 3.81 | 健康人声水平 |
| B | 3.81–5.0 | 正常范围，轻微可感 |
| C | 5.0–6.0 | 警告区，明显不稳定 |
| D | 6.0–10.0 | 病理范围，粗糙/抖动 |
| F | > 10.0 | 超出算法可靠上限 |

**Dynamic Range 等级：**

| 等级 | dB | 含义 |
|------|-----|------|
| A | > 20 | 达到自然语音水平 |
| C | 15–20 | 偏压缩，动态不足 |
| F | < 15 | 明显扁平/沉闷 |

**综合分值（权重 70% + 30%，线性插值）：**

| Shimmer (%) → 得分 | DR (dB) → 得分 |
|--------------------|-----------------|
| < 3.81 → 100 | > 20 → 100 |
| 3.81–5.0 → 100→75 线性下降 | 15–20 → 0→100 线性上升 |
| 5.0–6.0 → 75→50 线性下降 | < 15 → 0 |
| 6.0–10.0 → 50→25 线性下降 | |
| >= 10.0 → 0 | |

**公式**：`audio_score = shimmer_score × 0.7 + dr_score × 0.3`

#### Perf 评分

基于 RTF（生成耗时 / 音频时长）：

| RTF | 得分 | 等级 |
|-----|------|------|
| < 0.1 | 100 | A |
| 0.1–0.3 | 100→80 | B |
| 0.3–0.5 | 80→60 | C |
| 0.5–1.0 | 60→0 | D |
| >= 1.0 | 0 | F |

#### Timing 评分

基于偏离标准时长的百分比 `|actual - std| / std`：

| 偏离 | 得分 | 等级 |
|------|------|------|
| < 5% | 100 | A |
| 5–20% | 100→80 | B |
| 20–50% | 80→40 | C |
| 50–100% | 40→0 | D |
| >= 100% | 0 | F |

### 输出格式

```
Audio:scr=30(D) Perf:scr=84(B) Timing:scr=74(B) | sh=18.07%(F) dr=25.9dB(A) rtf=0.26 gen=2.8s dur=10.60s(std=14.1s-25%) Marginal...
```

三部分一目了然：音质→P、性能→G、语速→G，原始指标跟在 `|` 后。

### 参数调优测试

```bash
# length_scale 语速校准扫描
cargo test --package api --test tts_analysis_test -- test_tts_matcha_zh_en_scan_ls --ignored --nocapture
```

### 已知问题

**matcha-icefall-zh-en**：shimmer ~17%，远超算法可靠上限（12%）。

**length_scale 默认值校准（使时长接近标准 14.1s）：**

| 模型 | 默认 `length_scale` |
|------|-------------------|
| matcha-icefall-zh-en | **1.0** |

校准值已写入 `default_length_scale()`（`api/src/tts/mod.rs`），在 `tts_options` 未指定 `length_scale` 时自动生效。

---

## Volume 波形归一化

### 问题

TTS 音频前后音量不一致（例如中英混读场景下后半段英文部分明显更响），导致听觉体验割裂。

### 方案演进

#### 方案一：固定参数 DRC 压缩器（已废弃）

传统 feed-forward 压缩器，支持 peak-detector 包络跟随和可选 soft knee。通过网格搜索 + EBU R128 客观指标找到最优参数：

**网格搜索范围：**

| 参数 | 范围 | 步长 |
|------|------|------|
| threshold | -32 ~ -20 dB | 2 dB |
| ratio | 2 ~ 8 | 2 |
| knee | 0 ~ 6 dB | 3 dB |
| attack | 1 ~ 5 ms | 1 ms |
| release | 80 ~ 200 ms | 40 ms |

**最佳结果：** threshold=-28, ratio=6, knee=0, attack=5ms, release=80ms, makeup=8dB

```
LRA 9.58 → 5.66 LU（↓41%）
Crest 15.4 → 6.2 dB ❌ 声音发闷
```

Crest Factor 被压缩到 6.2 dB（太低），人耳感知为"闷"——音量均衡了但动态丧失。

#### 方案二：自适应 RMS 增益归一化（现行方案）

`adaptive_normalize()` 在 `apps/server/api/src/util/compressor.rs`，零配置，自动调整。

**算法：**

| 步骤 | 说明 |
|------|------|
| 1. 分帧 RMS 分析 | 200ms 窗口，10ms 步长计算每帧 RMS |
| 2. 目标响度 | 取所有帧 RMS 的 p30（30%分位） |
| 3. 帧增益 | 每帧 target_rms / frame_rms，限幅 ±12 dB |
| 4. 增益平滑 | attack=5ms / release=300ms 逐样点 IIR 平滑 |
| 5. 全局响度补偿 | 匹配原始 RMS + 额外 +3 dB |
| 6. 软限幅 | -0.5 dBFS 硬限制防止削波 |

**结果对比：**

| 指标 | Raw | Adaptive |
|------|-----|----------|
| LRA | 9.08 LU | **1.59 LU**（↓82%）|
| LUFS | -26.90 | **-24.14**（比原始响 ~3 dB）|
| Crest Factor | 15.3 dB | **14.7 dB** ✅ 动态保留 |

Crest Factor 维持在 14.7 dB——几乎是原始水平，完全没有"闷"感。

### EBU R128 客观指标

`apps/server/api/src/util/compressor.rs` 中 `evaluate_compressed()` 报告三个指标：

| 指标 | 全称 | 含义 | 目标 |
|------|------|------|------|
| LRA | Loudness Range | 响度范围，越低越一致 | 大幅降低 |
| LUFS | Loudness Units relative to Full Scale | 整体集成响度 | 匹配原始 +3 dB |
| Crest Factor | Peak-to-RMS Ratio | 动态余量，越高越有力度 | 保持 ≥ 原始 |

### 性能影响

| 环节 | 复杂度 | 10s 音频耗时 |
|------|--------|-------------|
| 分帧 RMS | O(n) | < 1ms |
| 排序取 p30 | O(f log f), ~1000 帧 | < 1ms |
| 增益平滑 | O(n), 每样点 IIR | < 10ms |
| **总开销** | | **~10ms**（TTS 推理数秒，可忽略）|

### 测试命令

```bash
# 对比测试：Raw vs 重采样+Opus vs Adaptive Normalize，生成 WAV + 打印 EBU R128 指标
cargo test --package api --test tts_analysis_test -- test_compare_raw_vs_processed --ignored --nocapture

# 网格搜索压缩器（保留历史参考）
cargo test --package api --test tts_analysis_test -- test_grid_search_compressor --ignored --nocapture
```

### 输出文件

| 文件 | 说明 |
|------|------|
| `./test_data/compare_raw.wav` | 原始 PCM（sherpa-onnx 直接输出） |
| `./test_data/compare_processed.wav` | 当前管道（重采样 + Opus） |
| `./test_data/compare_adaptive.wav` | adaptive_normalize 处理后 |

### 关键文件

| 文件 | 作用 |
|------|------|
| `apps/server/api/src/util/compressor.rs` | `adaptive_normalize()`、`evaluate_compressed()`、历史 `pcm_compress()` / `grid_search_compressor()` |
| `apps/server/api/src/util/compressor.rs` | `adaptive_normalize()` 定义位置（当前未被管线调用） |
| `apps/server/api/tests/tts_analysis_test.rs` | `test_compare_raw_vs_processed`、`test_grid_search_compressor` |

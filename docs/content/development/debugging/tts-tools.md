+++
title = "TTS 音频质量评估"
weight = 20
+++

# TTS 音频质量评估

`api/tests/common/tts.rs` 把「合成 → 解码 → 打分」串成一条可复现的流程，用来比较模型、调 `length_scale`、判断某个改动是否损害音质。分析型用例需要真实模型，默认 `#[ignore]`。

## 共享辅助

| 函数 | 说明 |
| --- | --- |
| `ws_root()` | monorepo 根路径（`CARGO_MANIFEST_DIR` 上溯 3 层） |
| `test_audio_config()` | 测试标准 `AudioConfig`（16000 Hz / mono / 20 ms 帧） |
| `run_tts_test(tts_config, audio_config, wav)` | 完整流程：建模型 → 流式推理 → Opus 解码 → 写 WAV |
| `tts_stream(text)` | 从字符串构造 TTS 输入 `Stream` |
| `opus_pipeline(samples, sample_rate, encode_sr)` | 走一遍编码再解码，用于估量 Opus 环节的损失 |
| `analyze_audio(...)` | 返回 `TtsAudioDiagnostics`（见下） |
| `estimate_std_duration(text)` | 按文本权重估算标准时长 |
| `collect_rule_fsts(dir)` | 收集目录下的 rule FST（文本正则） |

`run_length_scale_scan(...)` 不是共享辅助，它定义在 `api/tests/tts_analysis_test.rs` 里。

## 标准时长

`estimate_std_duration` 用 OmniVoice 的 RuleDurationEstimator 权重：按 Unicode 区段给每个字符分配语音权重，再以固定 12.0 weight/sec（约 4 汉字/秒 或 150 WPM 英文）折算。它不随模型或 `length_scale` 变化，所以能当跨模型的一致标尺。

`TEST_TTS_TEXT`：`2024年5月11号，拨打110或者18920240511，花了99块钱。我在学习machine learning和artificial intelligence。`

标准时长约 14.1 秒（权重≈168.8 / 12.0）。

## 三维评分

`TtsAudioDiagnostics` 分三个维度独立报告，**不合并总分**——音质、速度、语速是三个正交问题，合成一个分数会掩盖主因。

| 维度 | 字段 | 依据 | 等级 |
| --- | --- | --- | --- |
| Audio（音质） | `shimmer_pct`、`dynamic_range_db` | shimmer 70% + DR 30% | A/B/C/D/F |
| Perf（性能） | `rtf` | RTF 阈值 | A/B/C/D/F |
| Timing（语速） | `duration_secs`、`std_duration_secs`、`std_diff_secs` | 偏离标准时长的百分比 | A/B/C/D/F |

等级换算：≥86 → A，66–85 → B，41–65 → C，21–40 → D，<21 → F。

### Audio

shimmer 是帧间振幅变动率，阈值取自临床语音病理学：<3.81% → A（健康人声），3.81–5.0 → B，5.0–6.0 → C，6.0–10.0 → D，>10 超出算法可靠上限 → F。

动态范围：>20 dB → A（自然语音水平），15–20 dB → C（偏压缩），<15 dB → F（扁平发闷）。

`audio_score = shimmer_score × 0.7 + dr_score × 0.3`，两段都线性插值：shimmer 3.81→5.0 对应 100→75、5.0→6.0 对应 75→50、6.0→10.0 对应 50→25；DR 15→20 dB 对应 0→100。

### Perf

RTF = 生成耗时 / 音频时长。`<0.1` → 100（A），0.1–0.3 → 100→80（B），0.3–0.5 → 80→60（C），0.5–1.0 → 60→0（D），`>=1.0` → 0（F）。

### Timing

偏离 `|actual - std| / std`。<5% → 100（A），5–20% → 100→80（B），20–50% → 80→40（C），50–100% → 40→0（D），`>=100%` → 0（F）。

## 输出

```
Audio:scr=30(D) Perf:scr=84(B) Timing:scr=74(B) | sh=18.07%(F) dr=25.9dB(A) rtf=0.26 gen=2.8s dur=10.60s(std=14.1s-25%) glitch=3 clip=0 cf=14.2dB ev=0.0012 sc=1850Hz sf=0.08 sr=4200Hz zcr=0.12 dc=0.0003 snr=28.5dB lead=0ms trail=0ms Marginal...
```

三段的 `scr`/等级在前，原始指标在 `|` 之后。

| 字段 | 说明 |
| --- | --- |
| `cf` | 波峰因子（峰值/RMS），越高动态越大 |
| `ev` | 帧间能量方差，低值通常意味着交叉淡入淡出把细节糊掉 |
| `sc` | 频谱质心 Hz，语音约 1500–3000 Hz |
| `sf` | 频谱平坦度 0–1，越高越接近噪声 |
| `sr` | 频谱滚降点 Hz（85% 能量阈值） |
| `zcr` | 过零率，语音约 0.05–0.15 |
| `dc` | 直流偏移，应接近 0 |
| `snr` | 估算信噪比 dB |
| `lead` / `trail` | 前导 / 尾部静音毫秒 |

## 运行

```bash
# 扫描 length_scale，找语速校准点
cargo test --package api --test tts_analysis_test -- test_tts_matcha_zh_en_scan_ls --ignored --nocapture

# 单个模型端到端，产出 WAV
cargo test --package api --test tts_test -- test_tts_matcha_zh_en --ignored --nocapture
```

`tts_test` 的 `#[ignore]` 理由是 Matcha 真实模型输出偶发削波，clipping 门禁随模型数据波动，不适合进 CI。

## 当前基线

`matcha-icefall-zh-en` 的 shimmer 约 17%，高于 12% 的算法可靠上限——这是模型本身的特征，不是管道引入的。

`length_scale` 默认 1.0，写在 `api/src/component/tts/model/matcha/mod.rs` 的构造参数里（`opts` 缺省时生效）；校准到标准 14.1 秒用的就是这个值。

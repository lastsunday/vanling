+++
title = "ASR 语音识别"
weight = 402
+++

# ASR 语音识别

（本页是识别器的调试与基准入口；它在链上的位置与回合判定见[VAD 与回合判定](@/development/server/vad.md)。）

## 架构

ASR 的 trait 在 `service/src/component/asr.rs`，引擎实现与单例在 `api/src/component/asr/`，底层是 **sherpa-onnx**。

```rust
pub trait Asr: Send + Sync {
    fn create_stream(&self) -> Box<dyn AsrStream>;
    async fn transcribe(&self, sample_rate: u32, samples: &[f32])
        -> Result<RecognizerResult, AppError>;
}
```

`RecognizerResult { text: String, prob: f32 }` 是两条路径的共同输出。流式路径由 `AsrStream` 承担：`accept_waveform` / `decode` / `is_endpoint` / `get_partial` / `finish` / `reset`。

```rust
AsrManager::init(config).await;          // 启动时初始化
let model = AsrManager::global().default(); // Arc<dyn Asr>，可跨轮共享
let mut stream = model.create_stream();
stream.accept_waveform(&samples);
stream.decode();
```

初始化在服务端启动流程里；消费点有两处：`AsrNode`（实时语音链）与 Matrix 客户端（整段转录）。

## 模型一览

| 模型 | 配置名 | 位置 | 说明 |
| --- | --- | --- | --- |
| **XAsr** | `x_asr` | `data/asr/model/x_asr/default/` | 中/英/日/韩/粤，流式与整段两种用法 |
| **Void** | `void` | 无文件 | 返回固定文本 `"void"`、置信度 1.0，用于测试链路 |

模型来自 <https://github.com/k2-fsa/sherpa-onnx>，由下载器 manifest 管理：

```shell
moon run server:downloader -- asr
moon run server:downloader -- asr x_asr
```

## 基准与诊断

`api/tests/common/asr.rs` 把每个测试的耗时折算成统一诊断行：

```text
CER=XX.X%(X) WER=XX.X% Acc=XX.X% Perf=XX.X% Total=XX.X% | RTF=X.XXX ASR=X.Xs Audio=X.XXs <verdict>
```

| 指标 | 计算方式 |
| --- | --- |
| CER | 字符级 Levenshtein 距离 / 参考文本长度 |
| WER | 词级 Levenshtein 距离 / 参考词数 |
| RTF | ASR 耗时 / 音频时长 |
| Accuracy | 1 − CER |
| Score | Accuracy × 70 + (1 − min(RTF, 1)) × 30 |

| 等级 | CER | RTF |
| --- | --- | --- |
| A | < 3% | < 0.05 |
| B | 3–6% | 0.05–0.10 |
| C | 6–10% | 0.10–0.20 |
| D | 10–20% | 0.20–0.50 |
| F | ≥ 20% | ≥ 0.50 |

> 旧版页面记录的「TTS 闭环 CER 0%、RTF 0.110」是重构前的实测值，**未在本轮复测**；用下面的命令重跑才知道现在是多少。

## 测试

`api/tests/asr_test.rs` 五条，都直接调 `AsrManager::create_model`：

| 测试 | 模型 | 断言 |
| --- | --- | --- |
| `test_asr_model_void` | Void | `text == "void"`、`prob == 1.0` |
| `test_asr_model_void_stream` | Void | 流式接口可用：无 endpoint、无 partial、finish 返回同上 |
| `test_asr_model_x_asr_transcribe` | XAsr | 整段转录非空、`prob > 0` |
| `test_asr_model_x_asr_streaming` | XAsr | 每 100 ms 喂入，至少产生一个 partial，finish 非空 |
| `test_asr_transcribe_short_audio` | XAsr | 对比 1 s 截断、补 300 ms 静音、整段三种输入的诊断 |

XAsr 的测试需要 `data/asr/model/x_asr/default/` 已下载（`moon run server:downloader -- asr`）。

```shell
# Void：无需模型文件
cargo test --package api --test asr_test -- test_asr_model_void

# XAsr：需要模型
cargo test --package api --test asr_test -- test_asr_model_x_asr_transcribe --nocapture
cargo test --package api --test asr_test -- test_asr_model_x_asr_streaming --nocapture

# release 模式跑得快一些
cargo test --package api --test asr_test --release -- test_asr_model_x_asr_streaming --nocapture
```

链路层的回归在 `api/tests/session/`：`test_chat_flow_listen_manual`、`test_chat_flow_listen_auto`、`test_chat_flow_listen_realtime`、`test_chat_flow_voice_text_voice_round_trips`、`test_chat_flow_resample_48k_multi_round`。

## 新增 ASR 模型

1. **manifest**：`apps/server/src/downloader/manifests/asr/<model>.json`
2. **实现**：`api/src/component/asr/model/<model>/mod.rs`，实现 `Asr`（以及需要的 `AsrStream`）
3. **注册模块**：`api/src/component/asr/model/mod.rs` 加 `pub mod <model>`
4. **枚举**：`api/src/config/mod.rs` 的 `AsrModel` 加变体
5. **工厂**：`api/src/component/asr/mod.rs` 的 `create_model()` 加 match arm
6. **测试**：`api/tests/asr_test.rs` 加整段 + 流式两条，至少断言非空与 partial

## 关键文件

| 文件 | 作用 |
| --- | --- |
| `apps/server/service/src/component/asr.rs` | `Asr` / `AsrStream` trait、`RecognizerResult` |
| `apps/server/api/src/component/asr/mod.rs` | Manager 单例与模型工厂 |
| `apps/server/api/src/component/asr/model/x_asr/` | XAsr 实现 |
| `apps/server/api/src/component/asr/model/void/` | Void 替身 |
| `apps/server/api/src/config/asr.rs` | `AsrConfig` |
| `apps/server/api/src/config/mod.rs` | `AsrModel` 枚举 |
| `apps/server/src/downloader/manifests/asr/` | 下载 manifest |
| `apps/server/api/tests/asr_test.rs` | 上述五条测试 |
| `apps/server/api/tests/common/asr.rs` | 诊断与评分 |

+++
title = "ASR 语音识别"
weight = 402
+++

# ASR 语音识别

## 架构

ASR 模块位于 `apps/server/api/src/asr/`，使用 **Manager 单例模式**管理模型实例，底层集成 **sherpa-onnx** 的 `OfflineRecognizer`。

### Manager 模式

```rust
AsrManager::init(config).await;                             // 应用启动时初始化
let mut model = AsrManager::global().default().lock().await; // 获取 Arc<Mutex<Box<dyn Asr>>>
let result = model.transcribe(sample_rate, &samples).await;
```

初始化在 `api/src/lib.rs:121`，消费点在 `ws/session/listener.rs:238`（WebSocket 实时语音）和 `matrix/client.rs`（Matrix 协议）。

### Asr trait

```rust
#[async_trait]
pub trait Asr: Send + Sync {
    async fn transcribe(
        &mut self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<RecognizerResult, ModelError>;
}
```

所有模型共享 `RecognizerResult { text: String, prob: f32 }` 作为输出。

## 模型一览

| 模型 | 配置名 | 模型文件 | 大小 | 语言 | 架构 |
|------|--------|---------|------|------|------|
| **XAsr** | `x_asr` | `model.int8.onnx` | 228MB | 中/英/日/韩/粤 | OfflineXAsr |
| **Void** | `void` | 无 (no-op) | 0 | — | — |

所有模型均来自 <https://github.com/k2-fsa/sherpa-onnx>，通过下载器 manifest 管理。

```shell
# 下载当前配置的 ASR 模型
moon run server:downloader -- asr

# 下载指定 ASR 模型
moon run server:downloader -- asr x_asr
```

## 性能基准

测试条件：debug 模式，MatchaTTS 生成 ~11.84s 中英混合音频，回环送入 ASR 转录。

| 模型 | CER | WER | RTF | ASR 耗时 | 准确性 | 性能分 | 总分 | 判定 |
|------|-----|-----|-----|---------|--------|--------|------|------|
| **XAsr** | **0.00%(A)** | 0.00% | 0.110 | 1.3s | 100% | 89.0% | 96.7% | ✅ 适合日常 |

**结论：当前仅 XAsr 达到可用水平，为默认模型。**

| 观察 | 说明 |
|------|------|
| XAsr 精度 | TTS 闭环 CER 为 0%——对合成语音完美识别 |
| 性能 | RTF < 0.12（debug 模式），release 模式更快 |

### TTS-ASR 闭环测试

将 `TEST_TTS_TEXT` 通过 MatchaTTS 合成语音，再送入 ASR 转录，计算 CER/WER：

| 测试名 | ASR 模型 | 阈值 |
|--------|---------|------|
| `test_tts_asr_loopback` | XAsr | CER < 6% (实测 0%) |

**TTS 生成耗时**：~6.1-6.2s (debug) → 相当于 RTF=0.52（TTS 生成 + ASR 转录）

## 测试框架

### 诊断指标 (`common/asr.rs`)

```
CER=XX.X%(X) WER=XX.X% Acc=XX.X% Perf=XX.X% Total=XX.X% | RTF=X.XXX ASR=X.Xs Audio=X.XXs <verdict>
```

| 指标 | 计算方式 |
|------|---------|
| **CER** | 字符级 Levenshtein distance / 参考文本长度 |
| **WER** | 词级 Levenshtein distance / 参考词数 |
| **RTF** | ASR 耗时 / 音频时长 |
| **Accuracy** | 1.0 - CER |
| **Score** | Accuracy × 70 + (1 - min(RTF,1)) × 30 |

### CER 等级

| 等级 | 范围 | 含义 |
|------|------|------|
| A | < 3% | 极佳 |
| B | 3–6% | 良好 |
| C | 6–10% | 一般 |
| D | 10–20% | 较差 |
| F | ≥ 20% | 不可用 |

### RTF 等级

| 等级 | 范围 | 含义 |
|------|------|------|
| A | < 0.05 | 极快 |
| B | 0.05–0.10 | 快 |
| C | 0.10–0.20 | 正常 |
| D | 0.20–0.50 | 慢 |
| F | ≥ 0.50 | 不可用 |

## 全部测试

| 测试文件 | 测试名 | 模型 | 断言 |
|---------|--------|------|------|
| `asr_test.rs` | `test_asr` | XAsr | 无 (诊断) |
| `asr_test.rs` | `test_asr_model_void` | Void | text=="" prob==1.0 |
| `asr_test.rs` | `test_asr_with_reference_audio` | XAsr | 含预期子串 (zh/en/yue) |
| `asr_test.rs` | `test_tts_asr_loopback` | XAsr | CER < 6% |

### Session 集成测试

| 测试名 | ASR 角色 | 说明 |
|--------|---------|------|
| `test_asr_voice_input_manual` | XAsr + Echo LLM | 发送 JFK WAV 音频，断言 STT 文本精确匹配 |
| `test_chat_flow_listen_auto` | XAsr (全管道) | VAD 判断语音结束 → ASR → LLM → TTS |
| `test_chat_flow_listen_realtime` | XAsr (全管道) | 实时模式，发送音频 + Detect 文本 |
| `test_chat_flow_listen_realtime_silent_voice_connection_timeout` | Void (断开测试) | 静默超时断开，无需真实 ASR |
| `test_chat_flow_handle_text_message` | XAsr (全管道) | 文本输入 → LLM → TTS |
| `test_chat_flow_handle_text_message_multiple_time` | XAsr (全管道) | 19 轮文本对话 |
| `test_chat_flow_break` | Void (中断测试) | 中断恢复，无需真实 ASR |

### 测试命令

```shell
# Void 模型测试（无需模型文件，非 ignore）
cargo test --package api --test asr_test -- test_asr_model_void --nocapture

# XAsr 基础转录
cargo test --package api --test asr_test -- test_asr --ignored --nocapture

# 参考音频测试
cargo test --package api --test asr_test -- test_asr_with_reference_audio --ignored --nocapture

# TTS-ASR 闭环测试
cargo test --package api --test asr_test -- test_tts_asr_loopback --ignored --nocapture

# release 模式（更快）
cargo test --package api --test asr_test --release -- test_asr --ignored --nocapture

# 全部 ASR 测试
cargo test --package api --test asr_test -- --ignored --nocapture
```

## 新增 ASR 模型

1. **创建 manifest**：`apps/server/src/downloader/manifests/asr/<model>.json`
2. **实现 model module**：`apps/server/api/src/asr/model/<model>/mod.rs`，实现 `Asr` trait
3. **注册模型**：`api/src/asr/model/mod.rs` 添加 `pub mod <model>`
4. **添加枚举变体**：`api/src/config/mod.rs` 的 `AsrModel` enum 添加 `<Model>`
5. **注册 Manager**：`api/src/asr/mod.rs` 的 `create_model()` 添加 match arm
6. **添加测试**：`api/tests/asr_test.rs` 添加参考音频和闭环测试

## 关键文件

| 文件 | 作用 |
|------|------|
| `apps/server/api/src/asr/mod.rs` | Asr trait、Manager、RecognizerResult |
| `apps/server/api/src/asr/model/x_asr/mod.rs` | XAsr 模型实现 |
| `apps/server/api/src/asr/model/void/mod.rs` | Void no-op 模型 |
| `apps/server/api/src/config/asr.rs` | AsrConfig 结构体 |
| `apps/server/api/src/config/mod.rs` | AsrModel 枚举 |
| `apps/server/src/downloader/manifests/asr/` | 下载 manifest |
| `apps/server/api/tests/asr_test.rs` | 3 个 ASR 集成测试 |
| `apps/server/api/tests/common/asr.rs` | AsrDiagnostics 诊断系统 |
| `apps/server/api/tests/session/asr.rs` | Session 集成 ASR 测试 |

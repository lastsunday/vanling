+++
title = "核心架构"
weight = 200
+++

# 核心架构

> **注意**：此为概览层文档，反映作者对系统架构的理解。详见[文档风格指南](@/discussions/documentation-style.md)。

## 会话概览

chobits 服务端使用 **Session + Round** 模型管理对话：

- **Session**：一次 WebSocket 连接的生命周期。管理连接、认证、状态转换。
- **Round**：每轮对话（用户发言 → 服务端响应）。一个 Session 包含多个 Round。

Session 在 `service/src/chobits/session/mod.rs` 中定义，不绑定具体传输协议（WebSocket / Matrix 等），通过 `Frame` 枚举与外界通信。

## 数据流

```
Client → WebSocket
            ↓ (Message)
         ProtocolTranslator
            ↓ (Frame)
         InputFilters [McpRouterFilter, RecorderInputFilter]
            ↓ (Frame)
         Session (state machine)
            ↓ (OutputMessage)
         OutputFilters [RecorderOutputFilter]
            ↓ (OutputMessage)
         ProtocolTranslator
            ↓ (Message)
         WebSocket → Client
```

- **ProtocolTranslator**：`api/src/ws/protocol_translator.rs`，在 WebSocket Message 和内部 Frame 之间转换。
- **InputFilters**：对入站 Frame 做预处理，如截获 MCP 消息路由到 DeviceMcpSession。
- **OutputFilters**：对出站 OutputMessage 做后处理，如录像。
- **Session**：核心状态机，处理 Frame 并产出 OutputMessage。

## Session 状态机

Session 有四种阶段（`Phase`）：

```
Idle → Ready → Listening → Speaking → Ready → ...
                ↑              │
                └── BargeIn ───┘
```

### Idle

初始状态，等待客户端发送 `Hello`。

- 收到 `Frame::Hello` → 回复 Hello 响应（包含 `session_id`、`audio_params`），迁移至 **Ready**
- 自动创建第一个 Shadow Round

### Ready

等待用户输入。可接收：

- `Frame::ListenStart` → 开始监听，迁移至 **Listening**。如果已有 Running Round 则 BargeIn 中断
- `Frame::Input { text }` → 文本输入，升级 Shadow Round，迁移至 **Speaking**
- `Frame::Voice { data }` → 音频数据，转发给 Listener（VAD 处理）

### Listening

接收音频数据，VAD 检测语音边界。

- `Frame::Voice { data }` → 传给 Listener（VAD 判断是否在说话）
- `Frame::ListenStop` → 结束监听，从 Listener 取 ASR 结果，升级 Shadow Round，迁移至 **Speaking**
- 静默超时 → 自动触发 ListenStop 等效逻辑

### Speaking

LLM 推理 + TTS 合成 + 音频流式输出。

- 向 Running Round 发送 Command::Chat，触发 LLM → TTS 管道
- 完成后自动回到 **Ready** 等待下一轮

## Round 生命周期

Round 在 `service/src/chobits/session/round.rs` 中实现，管理单轮对话的 LLM 推理和 TTS 合成。

### 双 Round 模式

Session 同时维护两个 Round：

```
Shadow Round    Running Round
    │               │
    │ (准备中)       │ (正在执行)
    │               │
    └── upgrade ──→ │ (新版本替换旧版本)
```

- **Shadow Round**：新请求到达时预先创建，等待升级为 Running Round
- **Running Round**：当前正在执行的 Round（LLM + TTS 管道）
- **Upgrade**：Shadow Round 就绪后升级为 Running Round，旧 Running Round 被取消

这种设计允许无缝处理 BargeIn——新请求可以立刻开始准备，不阻塞当前处理。

### Round 内部流程

```
ChatParam → Round
  ├── Chii.ask() → LLM 流式输出
  │     ├── LLMResult (文本片段)
  │     └── ToolCall (→ MCP → LLM)
  └── Tts.stream() → 音频帧
        ├── TTSResult (状态事件)
        └── AudioResult (Opus 编码音频)
```

Round 的 LLM 和 TTS 并行执行流式输出，通过 OutputMessage 发送。

## ChiiCore

ChiiCore（`api/src/chii/`）是 LLM + MCP 的编排层。

```
ChiiCore
  ├── HistoryManager (消息历史管理 / 截断)
  ├── LlmClient (Qwen3 / Echo)
  ├── McpRegistry (MCP 工具聚合)
  └── TextSplitter (LLM 输出 → 句子分割 → TTS)
```

流程：

1. 用户文本 → HistoryManager 构建 ChatHistory
2. LlmClient.stream() → LLM 流式响应
3. 若 LLM 返回 ToolCall → McpRegistry.call_tool() → 结果回喂 LLM
4. LLM 文本 → TextSplitter 分句 → TTS

## Listener

Listener（`service/src/chobits/listener.rs` trait，`api/src/ws/default_listener.rs` 实现）编排 VAD + ASR：

1. 接收音频数据（`ListenInput::Audio`）
2. VAD 检测语音活动（Earshot Silero VAD）
3. 静默超时触发 ASR 转录（XAsr sherpa-onnx）
4. 返回 `ListenResult::Text` 或 `ListenResult::Audio { text, prob }`

## 输入输出过滤器

### InputFilter trait

```rust
#[async_trait]
trait InputFilter: Send + Sync {
    async fn process(&self, ctx: &FilterCtx, frame: Frame) -> FilterAction<Frame>;
}
```

返回 `FilterAction::Continue(frame)` 继续传递，`FilterAction::Consumed` 拦截，或 `FilterAction::Break` 中断管道。

内置过滤器：
- **McpRouterFilter**：截获客户端的 MCP 消息，路由到 DeviceMcpSession
- **RecorderInputFilter**：记录入站帧

### OutputFilter trait

```rust
#[async_trait]
trait OutputFilter: Send + Sync {
    async fn process(&self, ctx: &FilterCtx, msg: OutputMessage) -> FilterAction<OutputMessage>;
}
```

内置过滤器：
- **RecorderOutputFilter**：记录出站帧

## SessionBuilder

Session 通过 Builder 模式构造：

```rust
SessionBuilder::new()
    .with_id(session_id)
    .with_listener(DefaultListener::new(vad, asr))
    .with_chii(ChiiCoreBuilder::new(llm, mcp_registry).build())
    .with_tts(TtsManager::default())
    .with_config(session_config)
    .with_audio_config(audio_config)
    .build()  // 返回 SessionContext
```

`SessionContext` 包含：
- `session`：Session 实例
- `input_tx`：向 Session 发送 Frame 的通道
- `output_rx`：从 Session 接收 OutputMessage 的通道

## 启动流程

```
main.rs
  → run()
    → Server::new(args)           // 加载配置、初始化日志
    → async_main(&server)
      → api::start(StartParams)
        → Jwt::init()             // JWT 验证初始化
        → 数据库连接 + 迁移
        → TtsManager::init()      // OnceLock 单例
        → VadManager::init()
        → AsrManager::init()
        → LlmManager::init()
        → create_router()         // 注册 HTTP 路由
        → axum::serve()           // 启动 HTTP 服务
        → (可选) Matrix Client
```

### 路由结构

| 路径 | 模块 | 说明 |
|------|------|------|
| `/chobits/{version}` | `ws/` | WebSocket 端点（Xiaozhi 协议） |
| `/mcp` | `mcp/` | MCP Streamable HTTP 服务 |
| `/api/auth/*` | `auth/` | 登录 / Token 刷新 |
| `/api/ota*` | `ota/` | OTA 固件升级 |
| `/api/record/*` | `record/` | 会话录制查询 |
| `/docs` | — | OpenAPI Scalar UI |

### AI Manager 模式

所有 AI 模块使用 **Manager + OnceLock** 单例模式：

```rust
TtsManager (OnceLock)
  ├── init(config) → create_model() → 存入 INSTANCE
  ├── default() → Arc<dyn Tts>
  └── global() → &'static TtsManager
```

模型枚举通过配置选择，启动时创建对应实现并缓存。

## 会话活动超时

Session 在主循环中通过空闲时间戳实现无活动超时：

- `idle_since: Option<Instant>` 记录进入空闲的时间点，非空闲时重置为 `None`
- `close_connection_no_activity_time`（默认 30s）无活动超时断连
- `silence_voice_timeout`（默认 1200ms）VAD 静默判定

## 中断（BargeIn）

用户可打断 TTS 播放：

1. 客户端发送 `Abort` 帧或新的 `ListenStart`
2. Session 收到后调用 `stop_round(RoundStopReason::BargeIn)`
3. 当前 Running Round 被取消
4. Epoch 递增，后续过期的 OutputMessage 被丢弃
5. 新 Shadow Round 升级为 Running Round

## 日志与可观测性

### 输出格式

日志以结构化格式输出：

```
2026-07-20T08:58:12.870289Z DEBUG [<SESSION> asr result] component="session" event="asr_result" session_id=... text=Yeah.
```

- 括号对 `[<COMPONENT> message]` 之间为纯人类可读文本
- 其后为 `key=value` 结构化字段，可被机器解析
- 组件名大写：`SESSION`、`VAD`、`ROUND`、`LISTENER`、`ASR`、`MCP`、`WS`

### 结构化字段约定

| 字段 | 用途 | 示例 |
|------|------|------|
| `component` | 组件名 | `session`、`vad`、`ws` |
| `event` | 事件名 | `asr_result`、`voice_received`、`round_upgraded` |
| `session_id` | 会话 ID | — |
| `reason` | 原因 | `timeout`、`barge_in` |

### 输出目标

- **Console**（text/compact）：`[<COMPONENT> msg]` 格式 + `FmtSpan::NONE`
- **File**（JSON）：完整结构化日志 + `FmtSpan::CLOSE`
- **Pretty** / **Json**：不使用 bracket 前缀

### 日志级别指南

| 级别 | 用途 |
|------|------|
| `error!` | 不可恢复错误、连接中断、ASR/TTS 失败 |
| `warn!` | 异常但可恢复的情况 |
| `info!` | 关键流程（session 开始/结束、round 升级） |
| `debug!` | 详细事件（ASR 结果、VAD 状态变更） |
| `trace!` | 数据帧内容、调试用细节 |

禁止 `println!()` / `eprintln!()`。

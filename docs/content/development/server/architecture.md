+++
title = "核心架构"
weight = 200
+++

# 核心架构

> **注意**：此为概览层文档，反映作者对系统架构的理解。详见[文档风格指南](@/discussions/documentation-style.md)。

## 会话概览

vanling 服务端使用 **Session + Round** 模型管理对话：

- **Session**：一次 WebSocket 连接的生命周期。管理连接、认证、状态转换。
- **Round**：每轮对话（用户发言 → 服务端响应）。一个 Session 包含多个 Round。

Session 在 `service/src/session/mod.rs` 中定义，不绑定具体传输协议（WebSocket / Matrix 等），通过 `Frame` 枚举与外界通信。

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

Session 有三个阶段（`Phase`）：

```
Idle → Listening ⇄ Speaking
             ↑       │
             └─ BargeIn ┘
```

### Idle

初始状态，等待客户端发送 `Hello`。

- 收到 `Frame::Hello` → 回复 Hello 响应（`session_id`、`audio_params` 由构建时 `capabilities()` look up `AudioSpec` 决定），迁移至 **Listening**
- 自动创建第一个 Shadow Round

### Listening

监听输入。

- `Frame::Voice { data }` → 转发为 `PipelineEvent::AudioFrame` 到 Shadow Round 链首（VAD 在链内检测语音边界）
- `Frame::Input { text, mode }` → 文本输入（`mode=Wake` 标记唤醒语境），投喂 `TurnText`；Shadow Round 升级为 Running，迁移至 **Speaking**
- `Frame::ListenStart` → 已有 Running Round 则 BargeIn 中断；更新监听参数（barge-in / 断音检测）
- `Frame::ListenStop` → 向链投喂 `FinishTurn`；空输入（无有效语音）由 Session 辨别类型（`EmptyKind`）分级提示：manual 无人声注入 `Prompt{Manual, count}` 触发"没听清"引导语；auto 说了但 ASR 空为 `AutoSpoke`、完全静默为 `Silence`、回复后连续监听为 `Continuing`（静默不提示）——详见 `pipeline-redesign.md`「静音 / 空输入判别」子章节

### Speaking

表达阶段：TurnComplete 后 Shadow Round 升级为 Running Round 自动合成 TTS 并流式输出，同时新建下一轮 Shadow Round 监听。

- 表达结束自动回到 **Listening** 等待下一轮输入

## Round 生命周期

Round 在 `service/src/session/round.rs` 中实现，管理单轮对话的 LLM 推理和 TTS 合成。

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

Round 拥有单条节点链，并以内层观察者身份订阅广播统一消费：

```
Session │
        ↓ (RoundEvent: SpeechStarted / TurnComplete / EmptyTurn / SpokenEnd)
Round 拥有 NodeChain (opus→vad→asr→turn→ling→tts)
  ├── VAD/ASR 产 TurnText → TurnNode 关回合为 TurnComplete → 发 STT + 通知 Session 升级
  ├── Ling 文案 → TTS 产 AudioOut（逐句转发：SentenceStart / Audio / SentenceEnd）
  └── 内层广播统一处理：TTS 状态机 / barge-in（含 lockout）/ 超时 / tail Err
        ↓ (OutputMessage)
     客户端
```

Round 通过 `OutputMessage` 输出；Session 只做生命周期决策（shadow→running 升级 / 相位 / 打断），不再轮询链。

## LingCore

LingCore（`api/src/component/ling/mod.rs`）实现 `Ling` trait，是 LLM + MCP + 历史的编排层，产出逐句 `TextChunk`：

```
LingCore
  ├── model       (Arc<dyn Llm>：Qwen3 / Echo)
  ├── history     (消息历史 / 截断)
  ├── mcp_registry (MCP 工具聚合)
  └── splitter     (LLM 输出 → 句子 → TTS)
```

流程：

1. LLM 流式响应 → 若返回 ToolCall → mcp 工具调用 → 结果回喂 LLM
2. 文本按 `Sentence` 分割，逐句产出 `TextChunk { text, emotion }` → TTS 节点

## 回合终止判定：静默确认 + 传输停滞

回合终止判定位于 **AsrNode（音频时间静默确认）** 与 **Session（墙钟传输停滞）** 两层；
两者均只对 `streaming=true`（`auto`/`realtime`）生效，**按键录音（`manual`，`ListenMode{streaming:false}`）不实时识别**，
仅缓冲整段音频，待设备 `listen(stop)` 投喂 `FinishTurn` 时一次性识别（见 [`dialogue-flow.md`](@/development/server/dialogue-flow.md)）。

### 1. 音频时间静默确认（AsrNode）

`AsrNode`（`service/src/pipeline/nodes/asr_node.rs`）以**已消费的音频样本数**度量静默时长，而非 `Local::now()` 墙钟：

- `spoken: bool` — VAD 是否曾检测到语音
- `silence_samples: u64` — 语音结束后累计的静音样本数，语音帧时清零
- `SILENCE_CONFIRM_MS`（200ms）— `streaming && spoken && !speech_active && 静音样本 ≥ 阈值·sample_rate/1000` → 立即 `finish()` 本轮
- `streaming: bool` — 由 `ListenMode{streaming}`（`listen(start)` 时注入）切换。`false`（manual）时**仍逐帧喂流预解码**（`accept_waveform`+`decode`），但抑制 `PartialTranscript` 发射与静音确认；`FinishTurn` 走同一 `finish_stream()`，只 drain 残留帧，近瞬成文——把解码摊到按住期间，避免 stop 后冷启动整段识别

finish 三态：有效输入 → `TurnText`；有流但空文本 → `EmptyInput`（触发提示语）；无流 → `Nothing`。
manual 复用同三态，唯其**无流（VAD 未检到语音）时 `finish_stream` 返回 `Nothing`**——
由 Session 空输入逻辑（`EmptyKind::Manual`）唯一驱动提示语，避免与链内 `EmptyInput` 双触发。

### 2. 传输停滞检测（Session）

当客户端停止发送音频时，`silence_samples` 无法增长，用墙上时钟兜底：`check_transport_stall()`
（`session/mod.rs`）在 `spoken` 仍活动且 `last_audio_received.elapsed() ≥ silence_voice_timeout`（1200ms）时向链投喂 `FinishTurn` 强制收尾。
**manual 模式跳过该检测**（Listening 且 `is_voice_break_detect == false` 时直接返回）——设备的 stop 是唯一回合边界，绝不因静默超时"抢答"。

**为什么不用墙钟做静默确认**：测试和 CI 环境下音频可被瞬间注入（无实时速率），墙钟与音频流时间解耦，导致静默判定在慢机器上提前触发。音频计数方式让触发点仅取决于已解码内容，与消费速度无关。

**为什么需要墙钟做传输停滞检测**：音频时间计数只能统计已到达的样本。当客户端完全停止发送音频后，没有样本可计数，必须用墙上时钟检测流断。

与 OpenAI Realtime（`silence_duration_ms` 音频级 + `idle_timeout_ms` 墙钟级）分层一致。

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

Session 通过 Builder 模式构造，注入**裸节点原型集合**（api 站点按配置动态组链，决定链的阶段/顺序）：

```rust
SessionBuilder::new()
    .with_id(session_id)
    .with_node_templates(templates)   // Vec<Arc<dyn Node>>：opus→vad→asr→turn→ling→tts
    .with_config(session_config)
    .build()  // 返回 SessionContext
```

`build()` 时从模板 `capabilities()`（`downcast_ref::<AudioSpec>()`）look up 下行音频能力：无 TTS 节点则握手不声明
`audio_params`（无下行语音能力、不构造 pacer）。已删除的旧参数：`with_listener` / `with_ling` / `with_tts` / `with_audio_config`。

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
| `/vanling/{version}` | `ws/` | WebSocket 端点（Xiaozhi 协议） |
| `/mcp` | `mcp/` | MCP Streamable HTTP 服务 |
| `/api/auth/*` | `auth/` | 登录 / Token 刷新 |
| `/api/ota*` | `ota/` | OTA 协议（设备注册、激活验证） |
| `/api/devices/*` | `device/` | 设备管理（列表/激活/禁用/删除） |
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
- `silence_voice_timeout`（默认 1200ms）传输停滞判定（speaking 中长时间无音频 → `FinishTurn` 强制收尾）

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

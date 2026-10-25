+++
title = "核心架构"
weight = 200
+++

# 核心架构

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
- `Frame::ListenStop` → 向链投喂 `FinishTurn`；空输入（无有效语音）由 Session 辨别类型（`EmptyKind`）分级提示，判别条件与提示语分级见[对话流程](@/development/server/dialogue-flow.md)

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

Round 持有一条节点链（`opus→vad→asr→turn→ling→tts`），并以**内层观察者**身份订阅链上广播，统一消费 barge-in（含 lockout）、逐句 `AudioOut` 转发、TTS 状态机与超时；Session 作为**外层观察者**只做生命周期决策（shadow→running 升级 / 相位 / 打断），不轮询链。事件命名、两层观察者的分工与链内边界见[统一管道](@/development/server/pipeline.md)。

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

## 回合终止

回合终止由两层闸门决定：**静音确认**（`AsrNode`，按已消费的音频样本计数，200 ms）与**传输停滞**（Session，按墙钟，1200 ms 无新音频）。两者只对 `streaming=true`（`auto`/`realtime`）生效；按键录音（`manual`）只认设备的 `listen(stop)`，不吃任何超时。判据、阈值来源与「为什么这一层用这个时钟」见 [VAD 与回合判定](@/development/server/vad.md)。

打断是第三条路径，见下文「中断（BargeIn）」。

## 输入输出过滤器

入站 `Frame` 与出站 `OutputMessage` 各有一层过滤器链，位于 `ProtocolTranslator` 与 `Session` 之间，只做横切处理、不含业务判断：

| 层 | trait | 内置实现 |
| --- | --- | --- |
| 入站 | `InputFilter::process(&FilterCtx, Frame)` | `McpRouterFilter`（截获客户端 MCP 消息路由到 `DeviceMcpSession`）、`RecorderInputFilter`（记录入站帧） |
| 出站 | `OutputFilter::process(&FilterCtx, OutputMessage)` | `RecorderOutputFilter`（记录出站帧） |

`process` 返回 `FilterAction::Continue`（放行）、`Consumed`（吞掉）或 `Break`（中断管道）。新增过滤器不需要改 Session。

## SessionBuilder

```rust
SessionBuilder::new()
    .with_id(session_id)
    .with_node_templates(templates)   // Vec<Arc<dyn Node>>：裸原型，链由调用方组装
    .with_config(session_config)
    .build()  // → SessionContext { session, input_tx, output_rx }
```

`build()` 时从模板 `capabilities()` look up 下行音频能力，无 TTS 节点则握手不声明 `audio_params`。能力协议、模板→每轮实例的链构建与节点生命周期见[统一管道](@/development/server/pipeline.md)。

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
2. Session 先消化已入队的 `RoundEvent`，再判活——该轮可能已开始表达、但 `TurnComplete` 还没被处理
3. 存在 Running Round 则调用 `stop_round(RoundStopReason::BargeIn)`
4. Epoch 递增，后续过期的 OutputMessage 被丢弃；节奏门控保证任意时刻最多一帧音频在途
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

## 新增模块

- **Rust 业务**（分层见上文「启动流程」与「路由结构」）：`migration/` 建表 → `entity/` 定义 → `api/` handler → `service/` 业务逻辑 → 经 `create_routes(state)` 注册路由 → 测试验证
- **AI 模型**（模式见上文「AI Manager 模式」）：实现 Trait → 配置枚举变体 → manifest → `model/mod.rs` 注册 → Manager match arm → 测试 → 更新配置示例

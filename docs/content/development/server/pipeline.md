+++
title = "统一管道"
weight = 202
+++

# 统一管道

> 一轮对话的整条链：`opus → vad → asr → turn → ling → tts`。节点是纯流变换，观察者由引擎在节点边界自动广播；Round 消费内层广播，Session 只订阅 Round 的生命周期事件。
> 这条链为什么长成这样、替代了哪些形态，见[管道演进记录](@/records/server/pipeline-evolution.md)。

## 设计约束

- **每个 Round 一条链**：`opus→vad→asr→turn→ling→tts` 首尾相接，`TurnComplete` 在链内进入 `ling→tts`。
- **所有节点同构**：统一为单一 `Node::stream(upstream, ctx)` 流变换，没有能力子 trait、没有 `InputChain`，ling/tts 不享有特权。
- **观察者内建抽象层**：`NodeContext` 携带观察者注册点（Round 注入 `EventSink`）与 `session_id`（供节点日志关联 / 状态归属）；**业务节点完全无感**，广播由引擎自动处理。
- **引擎取引用 + 池化**：ASR 共享引擎（`create_stream()` 天然可并发）；VAD 是串行状态机，用**对象池**复用（取用前 `clear()`）。
- **上一轮 Round 默认继续跑**：仅当新回合需占用输出通道 / 打断正在说的话 / 抢 websocket 时才中断它（epoch 防串扰）。

## 角色与归属

| 角色 | 职责 | 关键 |
|---|---|---|
| **Session（总调度）** | 连接 / phase / Round 生命周期（shadow/upgrade/stop）/ 空闲超时；把入站音频转成 `PipelineEvent` 发到 active Round 链首 | 只做**生命周期管理**，不驱动识别/输出、不轮询链；构建时从模板 look up 能力驱动握手声明 |
| **Round（唯一处理器）** | **拥有单条链**，自动注入为观察者，订阅广播流；集中响应 barge-in / 回合推进 / 音频逐句转发 / 超时 / 错误 | 一句一回合；链是内部实现 |
| **Ling（决策中枢 trait）** | 接收已识别结果 → 决策 → 产出表达意图；参考 LLM / DB / 传感 / 网络 | 实现可 LLM/agent · 状态机 · 混合 · 传声筒 |
| **节点（引擎适配）** | Opus 解码 / VAD / ASR / turn / LLM / TTS / 数据获取 | 统一 `Node::stream` 流变换，**业务无感观察者**，可插拔组合；`capabilities()` 泛化申报会话级能力，`release_mode`/`on_acquire`/`on_release` 生命周期 |

```
Session(总调度) —— 管理 Round 生命周期；转发 Frame::Voice → active Round 链首；注册为 Round 的外层观察者（RoundEvent）
   Frame::Voice ──> PipelineEvent::AudioFrame ──> Round 链首
                        ↓
   Round(唯一处理器) 拥有单条链(内部实现):
     opus → vad → asr → turn ──(TurnComplete 链内自动进)──> ling → tts
       引擎在节点边界自动广播(event 带时机标签 Before/After)
            │ 内层：Round 作为广播观察者订阅，统一消费（barge-in 检测/TurnComplete/AudioOut 逐句转发/TTS 状态机/tail Err）
            ▼
        Round → RoundEvent 通道（SpeechStarted / TurnComplete / EmptyTurn / SpokenEnd）
            │ 外层：Session 注册为 Round 的观察者，据此做 shadow→running 升级 / 相位切换 / epoch / ListenStop
            ▼
        OutputMessage ──> 客户端
```

## 单链 + 观察者协议

### 文件：`service/src/pipeline/mod.rs`

```rust
pub enum PipelineEvent {
    // 感知
    AudioFrame(Vec<u8>),                   // Opus 原始音频，源自 Frame::Voice
    PcmFrame(Vec<f32>, u32),               // Opus 解码后的样本 + 采样率
    SpeechStarted,                         // VAD 上升沿
    SpeechEnded,                           // VAD 下降沿
    PartialTranscript(String),
    TurnText { text: String, prob: f32 },  // 关卡：一轮识别完成（ASR 产出）
    TurnComplete { text: String, prob: f32 }, // 回合边界收尾（turn 节点把 TurnText 标为回合结束，边界判定在 AsrNode + Session）
    Configure(AudioParam),                 // 下行音频参数：经流内事件触发重配
    FinishTurn,                            // 内部控制：请求 ASR 立即 finish
    // 表达
    TextChunk { text: String, emotion: Option<String> },
    AudioOut { audio: Vec<Vec<u8>>, is_first: bool, is_last: bool },
}

pub struct NodeContext {
    pub cancel: CancellationToken,
    pub emit: EventSink,          // Round 注入的观察者广播发送端
    pub session_id: String,       // 会话标识：Round 注入，供节点日志关联/状态归属
}

// 统一节点协议 —— 业务节点是无感的纯变换：从上游流读取关心变体，透传其余，可叠加新事件。
// **原型（Prototype）**：Node 既是模板又是实例，自带 `new_instance` 以自身为配方克隆新实例；
// Session 持一组模板，每 Round 让每个模板 `new_instance` 出独立实例（Opus/VAD 等有状态节点须新鲜实体）。
/// 会话级能力值：任何 `Send + Sync + 'static` 的具体类型（`NodeCapability = dyn Any + Send + Sync`）
/// 包装为 `Box<NodeCapability>` 即可被节点声明、被 Session 以 `downcast_ref::<T>()` 按类型 look up。
pub type NodeCapability = dyn Any + Send + Sync;

pub enum ReleaseMode {
    /// 节点进程完成（其流消费到 `None`）即释放：适合无跨事件状态的节点。
    Immediate,
    /// 整轮 round 结束才释放：适合贯穿一轮识别/表达的有状态节点。
    Deferred,
}

pub trait Node: Send + Sync {
    /// 克隆：以自身为模板产出一个新的、可跑在链里的实例（供每个 Round 独立使用）。
    fn new_instance(&self) -> Arc<dyn Node>;
    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream;
    /// 下行配置（模板级）：Session 在 hello 时把传输 `Configure` 转发给模板一次，
    /// 更新模板自身状态；后续每轮 `new_instance` 从中继承。默认空实现 = 不接受配置。
    fn on_configure(&self, _event: &PipelineEvent) {}
    /// 泛化能力 look up：节点声明的会话级能力集合（空 = 无）。Session 按类型 downcast 检索；
    /// 新增能力只需 `Box::new(具体类型)` 并入 `capabilities()`，不改 Node trait。
    fn capabilities(&self) -> Vec<Box<NodeCapability>> { Vec::new() }
    /// 释放策略（框架强制）：默认 `Deferred`；Immediate 节点在其流消费到 `None` 时被框架立即 `on_release`。
    fn release_mode(&self) -> ReleaseMode { ReleaseMode::Deferred }
    /// 轮次生命周期：round 开始时被 `NodeChain::begin` 驱动（默认 no-op）。
    fn on_acquire(&self) {}
    /// 轮次生命周期：Immediate 节点在 `with_observer` 流末触发，Deferred 节点在 `NodeChain::finish` 触发。
    fn on_release(&self) {}
}
```

**观察者/广播（时机标签）**——附在广播载荷，**不是** `NodeContext` 字段：

```rust
pub enum TapPoint { Before, After }        // 进入节点输入前 / 节点产出后
pub struct Tapped { pub point: TapPoint, pub event: PipelineEvent }
```

- `EventSink`：Round 注入的可 clone 广播发送端（`broadcast::Sender<Tapped>` 或双端 unbounded）。
- **广播由引擎自动做**：在 `compose`/`NodeChain` 驱动层，于每个节点边界把
  `Tapped{Before, ev}` 与 `Tapped{After, ev}` 都发到 `emit`。业务节点**不感知**观察者存在——
  它只写纯 `stream` 变换，不调 `ctx.emit`。

**语义**：

- **单一 `Node` trait**：能力一律经 `capabilities()` 申报，不用能力子 trait 区分节点。reset 不是公共协议——
  VAD per-Round（池取用前 `clear()`）；reconfigure 分两层：Session 在 hello 时把流内 `Configure`
  事件转发给**模板**（`Node::on_configure`），模板 `new_instance` 把配置态克隆给每轮实例。
- `NodeContext` 承载 `cancel` + `emit`（观察者入口）+ `session_id`（Round 注入，供节点日志关联 / 状态归属）；
  不含任何「时机」配置——Before/After 是
  广播载荷的标签，由引擎贴，不与上下文耦合。
- 错误复用 `AppError`，经 `TryStreamExt` 自动短路透传 `Err`；不塞 `PipelineEvent::Error`。

**两个时机的用途**（互补）：

- **Before（进入前）**：Round 感知**原始输入事件**——`SpeechStarted`（barge-in 打断）、音频到达
  （重置空闲计时）。这类信号在链内会被下游变换吞噬，只有 Before 广播能让 Round 及时拿到。
- **After（产出后）**：Round 感知**变换结果**——`TurnComplete`（推进说段）、`AudioOut`（逐句转发）、
  `Err`（超时/中断）。

### 下行配置（模板级，`Configure` → `Node::on_configure`）

**语义**：`Configure(AudioParam)` 是传输层（客户端 hello `audio_params`）下发的运行时参数，当前只被
`OpusDecodeNode` 消费（解码采样率）。它**不逐轮发送**，而是——Session 在 `handle_connect` 时把该事件
**转发给模板一次**（`templates.iter().for_each(|t| t.on_configure(&Configure(params)))`）；`OpusDecodeNode`
覆写 `on_configure` 更新模板态，且 `new_instance` **从 self 继承**已配置采样率 → 每轮新实例解码器/重采样器
天然正确。首轮因 `on_idle` 先建 round 后 connect，仍补一次流内 `Configure`；**无持久化字段、无每轮重喂**。

**采样率归一（opus → 16k）**：`OpusDecodeNode` 负责 `AudioFrame→PcmFrame`，解码后把 PCM **重采样到
16000** 再输出（`PcmFrame(_, 16000)` 恒 16k）。因为下游 VAD（Earshot）与 ASR（sherpa X-ASR）都硬编码
假设 16k 输入、链上无其它重采样点；客户端按 hello 声明异于 16k 的上行采样率（如 24k）时，统一在此降 16k，
VAD/ASR 总拿到正确采样率。复用 `rubato`（api 已有依赖，`matcha` 同款 `Fft::new` + 分块处理模式）。

### 能力 look up（Node → Session 上行声明，泛化）

与**下行配置**（`Configure(AudioParam)`，Session→Node 下发运行时参数）正交，节点还提供**上行能力声明**。
能力**泛化**：不穷举为 Node trait 方法，而是以类型别名 `NodeCapability = dyn Any + Send + Sync` 包装
任意 `Send + Sync + 'static` 的具体类型即可作为能力被节点声明、被 Session 按类型 `downcast_ref::<T>()`
look up。当前唯一能力成员是 `AudioSpec`（下行音频格式）：

```rust
#[derive(Debug, Clone)]
pub struct AudioSpec { pub sample_rate: u32, pub channel: u32, pub frame_duration_ms: u64 }

// Node trait 默认：capabilities() -> Vec<Box<NodeCapability>> { Vec::new() }
impl Node for TtsNode {
    fn capabilities(&self) -> Vec<Box<NodeCapability>> {
        vec![Box::new(self.tts.audio_spec())]   // 任意具体类型直接 Box 包装
    }
}
```

 - **`Tts::audio_spec() -> AudioSpec`**（service `component/tts.rs`）：引擎自报下行输出格式。`TtsMatcha`
   返回自身 `output_sample_rate/output_channel/output_frame_duration`；`TtsMute`（无音频、仅透传
   文本）返回默认 `(16000, 1, 60)`。
- **Session 解析**：`build()` 时一次性遍历 `node_templates`，在每个模板的 `capabilities()` 里
  `downcast_ref::<AudioSpec>()`，`find_map` 首个命中，存为 `Option<AudioSpec>` 字段。
- **语义（不兜底）**：无 TTS 节点 ⇒ `None`——握手回包 `audio_params: None`（显式"无下行语音能力"，
  客户端不再期待音频），pacer 亦不构造（`AudioResult` 只来自 TTS，天然不触发）。
- **扩展性**：新增能力（如上行格式、会话 token 预算）只需 `Box::new(具体类型)` 并入
  `capabilities()`，**不改 Node trait**。
- **配置源只有一处**：api `config::audio::AudioConfig`（流向 `TtsManager`/`TtsMatcha`）；session 侧不复制音频配置。

### 节点生命周期（框架强制的资源释放）

有状态节点（VAD / ASR）在一轮识别内持有资源（VAD 状态机取自注入的 pool、ASR 流贯穿喂帧→finish），需要
**生命周期钩子**统一驱动。释放策略由节点 `release_mode()` 声明，框架强制：

- **Immediate**：`with_observer` 在每个叶子流的 `with_observer` 包裹层检测到该流消费到 `None`（进程完成）
  时，若该叶子 `release_mode()==Immediate` 即立即调 `leaf.on_release()`。因 `compose_chain` 入口对每个
  叶子恰好 `with_observer` 包一次，触发**每叶恰好一次**，嵌套 compose 不会重复。
- **Deferred**：资源贯穿整轮，由 `NodeChain` 在 round 结束时统一释放。

**触发载体——`NodeChain` 持叶子列表**：`compose_chain` 把传入的有序 `Vec<Arc<dyn Node>>` 折叠成嵌套
组合，叶子引用被吞没；为驱动生命周期，`NodeChain::new(head, leaves)` 保留叶子副本：

```rust
// session 构链时
let leaves: Vec<Arc<dyn Node>> = templates.iter().map(|t| t.new_instance()).collect();
let head = compose_chain(leaves.clone()).expect("chain");
let chain = NodeChain::new(head, leaves);
chain.begin();   // 对所有叶子逐一 on_acquire()
// round 结束
chain.finish();  // 仅对 Deferred 叶子逐一 on_release()（Immediate 已在流末释放，不重复）
```

| 节点 | `release_mode` | `on_acquire` | `on_release` |
|---|---|---|---|
| `OpusDecodeNode` | Immediate | — | 重置解码/重采样态 |
| `VadNode` | Deferred | `pool.acquire()` 存实例 | `take()` 归还 `pool.release()` |
| `AsrNode` | Deferred | 重置内部流/缓冲/标志 | 活跃流则 finish |
| `TurnNode` | Immediate | — | — |
| `LingNode` | Immediate | — | — |
| `TtsNode` | Immediate | — | — |

### 单链组合

`compose(a, b)` 把 `a.stream` 输出接成 `b.stream` 输入，返回新 `Node`（纯 pipe，不广播）。
**统一拼链函数 `compose_chain(Vec<Arc<dyn Node>>)`** 逐叶子用 `with_observer` 包一次（自动广播）后再
`reduce(compose)` 折叠成单条链——顺序与数量由外部传入的 `Vec` 决定：

```rust
pub fn compose_chain(nodes: Vec<Arc<dyn Node>>) -> Option<Arc<dyn Node>> {
    nodes.into_iter().map(with_observer).reduce(compose)
}
```

产出的链形：`opus → vad → asr → turn → ling → tts`。
组装者只给**裸原型** `Vec<Arc<dyn Node>>`；`with_observer` 对组装者完全透明，由 `compose_chain` 内部统一处理（保证每个叶子恰好广播一次）。

## 链构建（模板 + 每轮实例）

- **只有三层**：模板集合 → 每轮 `new_instance` → `compose_chain`。没有 `PipelineFactory` / `NodeFactory` /
  `NodeDeps` / `ChainBuilder` 这类中间层。
- **`Node` = 原型**：每个节点自带 `new_instance(&self) -> Arc<dyn Node>`，以自身为模板克隆实例。
  模板自持所需共享引用（`Arc<VadPool>` / `Arc<dyn Asr>` / `Arc<dyn Ling>` / `Arc<dyn Tts>`），
  `new_instance` 时克隆出每 Round 独立的新实体。节点**不自存 per-session `session_id`**——会话标识由
  Round 注入 `NodeContext.session_id`，节点仅从 `ctx` 读取用于日志关联。
- **Session** 持裸原型集合 `Vec<Arc<dyn Node>>`（`SessionBuilder::with_node_templates`），每 Round：
  `templates.iter().map(|t| t.new_instance())` → `compose_chain(...)` 产链。
- **Session 构建时从模板 look up 会话级能力**：`find_map` downcast `AudioSpec`（见「能力 look up」），
  驱动握手音频声明与 pacer；无 TTS 节点则不声明下行音频（`audio_params: None`）。
- **调用方（api 站点 ws / matrix / 测试）** 按配置条件组装裸原型 `Vec<Arc<dyn Node>>`：条件 `push` /
  换序即可决定链上有哪些阶段、什么顺序。
- 每 Round 一条链 = 每次 `new_instance` 从 VAD 池 `acquire` 一个新 VAD 实例。

## 节点归属

| 节点 | 文件 | 实现 |
|---|---|---|
| `OpusDecodeNode` | `service/src/pipeline/nodes/opus_node.rs` | `AudioFrame→PcmFrame`；覆写 `on_configure` 更新模板态 + `new_instance` 继承；解码后重采样到 **16k** 输出 |
| `VadNode` | `service/src/pipeline/nodes/vad_node.rs` | `PcmFrame→{PcmFrame, SpeechStarted, SpeechEnded}`；实例取自 VAD 池，`Drop` 归还 |
| `AsrNode` | `service/src/pipeline/nodes/asr_node.rs` | `→{PartialTranscript, TurnText}`；内部 `create_stream()`，共享引擎 |
| `TurnNode` | `service/src/pipeline/nodes/turn_node.rs` | `TurnText→TurnComplete`；规整 ASR 流与文本直入（`TurnText`）两路径，显式关闭回合边界（边界判定在 AsrNode 静默确认与 Session 控制事件） |
| `LingNode` | `service/src/pipeline/nodes/ling_node.rs` | `TurnComplete\|TurnText→TextChunk`（unfold 内部流；文本直答也走此） |
| `TtsNode` | `service/src/pipeline/nodes/tts_node.rs` | `TextChunk→{TextChunk, AudioOut}`（统一 `Node::stream`）；`capabilities()` 上报 `AudioSpec`（`Tts::audio_spec()`） |

所有节点为**纯变换**：不感知观察者/广播；`TtsNode` 以 FIFO 配平 emotion，编码错误以 `Err` 透传。

## 引擎池化

- **VAD 对象池**：`VadPool { free: Mutex<Vec<Box<dyn Vad>>>, config }`。`acquire()` 空则建、
  否则取并 `clear()`；`release()` 归还。保证同时仅被一个 Round 持有（串行状态机，并发复用破 `is_speech`）。
- **ASR 共享引擎**：`Arc<dyn Asr>` 单例；每 Round 现取 `create_stream()`，`finish()` 后终结，不值得池化。

## Round 统一消费（两层观察者）

**Round 是「内层封装 pipeline 广播 + 外层可观察主体」的双层观察者模型。**

- **内层（Round ← pipeline 广播）**：Round 拥有单条 `NodeChain`；`start()` 时把自己注入为
  `EventSink` 观察者，订阅 `Tapped` 广播流统一消费单链。按 `Tapped` 的 `point` + `event` 分支：
  - `Before SpeechStarted` → 判定 barge-in（含 lockout，已过锁定期后）→ 通知 Session 决策
  - `After TurnComplete` → 发 STT + 通知 Session 升级
  - `After AudioOut` → 逐句 `SentenceStart`/`Audio`/`SentenceEnd` 转发 + TTS 状态机 + 超时
  - tail `Err` → `LlmNoUsableOutput` / `TtsEncode` / 中断
  - 其他（`PcmFrame`/`PartialTranscript`）→ 前向 / 日志
- **外层（Session ← Round 可观察主体）**：Round 本身实现观察者模式，暴露 `RoundEvent` 广播通道；
  **Session 注册订阅**（每 Round 唯一订阅者）。Round 判定信号并**内联转发输出**（STT/LLM/TTS/Audio → 
  `output_tx`）+ 维护 TTS 状态机；Session 只做**生命周期决策**（shadow→running 升级 / 相位切换 / epoch /
  打断 stop_round / ListenStop）。

### 文件：`service/src/session/round.rs`

```rust
pub enum RoundEvent {
    SpeechStarted,                             // 语音起始（barge-in 判定已过 lockout）
    TurnComplete { text: String, prob: f32 },  // 一轮识别完成（应升级 shadow→running / Speaking / ListenStop）
    EmptyTurn,                                 // 空输入完成（同 TurnComplete 轮转出新的 shadow，但不产 STT）
    SpokenEnd,                                 // TTS 表达结束（该 round 完成）
}

pub struct Round {
    chain: NodeChain,                                  // 单条链
    round_event_tx: broadcast::Sender<RoundEvent>,     // 外层观察者通道（Round 广播）
    round_event_rx: broadcast::Receiver<RoundEvent>,   // Session 注册订阅
    // ...
}
```

- `Round::event_receiver()` → 返回 `round_event_rx`，Session 注册。
- Round 的 `start()` 起观察者 task：`select!` 于 { tail.next()（驱动 + Err） / tap_rx.recv()（控制） / cancel }，
  统一消费并负责逐句输出转发。

## 回合联动

- 回合边界判定在 **AsrNode 的静音确认**（`SILENCE_CONFIRM_MS=200`，VAD 报非语音后触发 finish）与
  **Session 控制事件**（`FinishTurn`：`ListenStop` / 静音超时 / transport stall）完成；`TurnNode` 只做
  `TurnText→TurnComplete` 显式收尾标记（D2）。
- **规则收尾（默认）**：AsrNode 按静音确认阈值（200ms）收尾一段完整意图——VAD 报非语音后
  `silence_samples ≥ SILENCE_CONFIRM_MS·sample_rate/1000` 即 finish；另有静音超时、transport stall、
  `FinishTurn` 控制事件、prefix。
- **上一轮 Round 的处置**：默认继续跑；仅当新回合需占用输出通道 / 打断说话 / 抢 websocket 时才中断——
  复用 `RoundStopReason::Upgrade/BargeIn` + epoch 防陈旧输出。

## Session 总调度（Round 生命周期管理）

- phase 状态机保留：`Idle` / `Listening` / `Speaking`。
- `on_listening(Frame::Voice)` → 转发 `AudioFrame` 到 active Round 链首（D1）。
- **Session 注册为 current Round 的外层观察者**：在 `RoundEvent` 通道上 `select!`，
  收到信号做**生命周期决策**：
  - `SpeechStarted` → barge-in：`next_round_epoch()` + `stop_round(BargeIn)`（该打断由当前 shadow 继续听，
    其 `TurnComplete` 再升级，不再 `new_shadow_round`）
  - `TurnComplete{text, prob}` → `on_turn_complete`：`shadow→running` 升级、相位 Speaking、ListenStop，
    并**立即新建下一轮 shadow**（`new_shadow_round`）以支持说话中 / 连续输入监听
  - `SpokenEnd` → 表达结束（shadow 相位处理 / 空闲计时）
- Session 不持有 `drain_hearing` / `handle_hearing_event` / `active_hearing`：响应逻辑在 Round 观察者里。
- `with_node_templates(Vec<Arc<dyn Node>>)` 注入**裸原型集合**（api 站点动态组，决定链的阶段/顺序）。
- 握手（`handle_connect`）回包 `audio_params` 由构建时 look up 的 `AudioSpec` 决定；无 TTS 节点则下发
  `audio_params: None`（无下行语音能力，不兜底默认）。pacer 节奏取 `frame_duration_ms`（无 spec 不构造）。
- epoch 防串扰保留；输出转发也算活动（重置 `idle_since`）。

### 空输入决策（中枢 gatekeeper）

空输入的判别表、计数与提示语分级归[对话流程](@/development/server/dialogue-flow.md#空输入行为)；这里只留机制：会话进入 `Listening` 时 Session 依「模式 + 前一 turn」定出 `EmptyKind`，观察 `(After, EmptyInput)` / `(After, EmptyTurn)` 后产出 `Prompt{kind, count}`（重新注入链首，各节点透传未识别事件）、`Silence`（不注入表达）或 `GiveUp`（回 `Idle`）。`Ling` 只按 Act 组 prompt，Echo 按 kind/count 取固定句——判别与渲染不交叉。

## 测试

| 层面 | 测试 | 断言 |
| --- | --- | --- |
| 广播 | `service/src/pipeline/mod.rs` `with_observer_broadcasts_before_and_after_once_per_leaf` | 每个叶子在每个事件上恰好收到 `Tapped{Before}`、`Tapped{After}` 各一次 |
| 广播 | `nested_compose_does_not_duplicate_broadcast` | 嵌套 `compose` 不重复广播同一事件 |
| 组链 | `new_instance_reproduces_fresh_instance_and_compose_chain_broadcasts_once` | `new_instance` 产出独立实例，`compose_chain` 整体仍只广播一次 |
| 生命周期 | `lifecycle_immediate_releases_at_stream_end_deferred_at_finish` | Immediate 在流末释放、Deferred 在 `finish()` 释放，各恰好一次 |
| 节点 | `api/tests/pipeline_nodes_test.rs`（16 条 + 1 条 `#[ignore]`） | 解码重采样到 16k、VAD 起点与前缀截断、ASR 空输入、TTS 编码与 `AudioSpec` 能力、ling 取消 |
| 端到端 | `api/tests/session/`（`chat.rs`、`empty_input_round.rs`） | 握手 `audio_params`、听/说轮转、barge-in、空输入的提示分级 |

```shell
cargo test -p service --lib pipeline
cargo test -p api --test pipeline_nodes_test
cargo test -p api --test session
```

## 相关文件

| 路径 | 作用 |
| --- | --- |
| `service/src/pipeline/mod.rs` | `PipelineEvent`、`NodeContext{cancel, emit, session_id}`、`EventSink`/`Tapped`/`TapPoint`、`Node` trait（原型 + `on_configure` + `capabilities()` + 生命周期）、`compose`/`compose_chain` 与节点边界广播 |
| `service/src/pipeline/nodes/` | 六个节点：`opus` / `vad` / `asr` / `turn` / `ling` / `tts`，均为纯 `stream` 变换 + `new_instance` 克隆 |
| `service/src/session/mod.rs` | Round 生命周期（shadow/upgrade/stop/phase/空闲）、`with_node_templates` 注入裸原型、构建时 look up `AudioSpec`、`handle_connect` 向模板转发一次 `Configure` |
| `service/src/session/round.rs` | 持单链；内层订阅 `Tapped` 统一消费；外层广播 `RoundEvent`（`SpeechStarted`/`TurnComplete`/`EmptyTurn`/`SpokenEnd`） |
| `service/src/component/tts.rs` | `Tts` trait，含 `audio_spec() -> AudioSpec`（`AudioSpec` 定义在 pipeline） |
| `api/src/component/tts/model/` | `TtsMatcha` 按自身字段上报 `audio_spec()`；`TtsMute` 返回 16000·1·60 |
| `api/src/component/tts/opus_encoder.rs` | `StreamingOpusEncoder` |
| `api/src/ws/mod.rs`、`api/src/matrix/client.rs` | 按配置组裸原型 `Vec<Arc<dyn Node>>`，交给 `with_node_templates` |
| `api/tests/pipeline_nodes_test.rs` | 节点级契约：重采样、VAD 起点与前缀、ASR 空输入、TTS 编码与能力声明 |
| `api/tests/session/` | 端到端：握手音频声明、听/说轮转、barge-in、空输入提示分级 |

## 硬约束

- **VAD 池回收**：VAD 是串行状态机，acquire/release 严格配对，同一实例同时只能被一个 Round 持有。
- **回合判定归属**：边界由 AsrNode 的静音确认与 Session 控制事件（`FinishTurn` / 静音超时 / transport stall）决定，`TurnNode` 只标 `TurnComplete`。
- **广播顺序**：Before/After 标签只由引擎在节点边界贴；节点自己不调 `ctx.emit`，否则观察者会收到错序或重复信号。
- **epoch 与终止**：epoch 防陈旧输出，不随链结构调整。
- **emotion**：TTS 的 emotion 靠 FIFO 配平，不做文本键映射。

+++
title = "统一管道重构"
weight = 205
+++

# 统一管道重构（Pipeline Redesign）

> **本文档描述目标架构**（含当前实现校验）。若需新开会话推进，以此为准。
> 结构：背景与动机 → 角色与归属 → 单链 + 观察者协议 → 引擎自动广播 → 链构建 → Round 统一消费 →
> 回合联动与数据回路 → Session 总调度 → 验证 → 差异 → 相关文件 → 风险与控制。

## 背景与动机

把 VAD / ASR / LLM / TTS 等引擎从「硬编码编排」统一为「可插拔的节点链」，用统一的 `PipelineEvent`
流过整条链。核心主张：**流天生是单向的**（`audio → ASR → text → LLM → text → TTS → audio`），
只是一串节点首尾串联。因此**所有节点应是同构的流变换器**，链是它们的嵌套折叠——「上一节点的输出
= 下一节点的输入」。

本蓝图解决既有实现的结构性问题：

1. **平行 Vec + 魔法索引**：旧 `InputChain{nodes, resettable, reconfigurable}` 三个平行 Vec 靠固定
   索引对齐（`nodes[2]`=ASR、`resettable[1]`=ASR）。换节点顺序即静默错位，编译期不可见。
2. **ling/tts 特权**：Ling 与 TTS 被当成一对特权节点一起出现（`with_ling_node` 旁必有 `with_tts_node`），
   割裂「它们只是链中几个普通节点」的同构本质。
3. **听/说两段链**：`build_hearing`（`opus→vad→asr→turn`）+ `build_speaking`（`ling→tts`）被当作两段
   独立链，由 Session 轮询听段链尾、Round 起长 task 消费说段链尾——**两条生命周期、两套消费逻辑**。
4. **工厂层级冗余**：`PipelineFactory` trait + `NodeFactory` struct 两层抽象，只是为了「service 不
   import api 具体类型」；本质只需求「按 Round 建链」的能力。
5. **职责错位**：Session 既总调度又驱动识别/输出两段链，与 Round 的 per-turn 处理职责重叠。

### 设计原则

- **每个 Round 一条链**：`opus→vad→asr→turn→ling→tts` 首尾相接；`TurnComplete` 在**链内**自动进入
  `ling→tts`，无需两段链、无需 Session 轮询。
- **所有节点同构**：统一为单一 `Node::stream(upstream, ctx)` 流变换；删除能力子 trait、`InputChain`、
  特权的 ling/tts 配对。
- **观察者内建抽象层**：`NodeContext` 携带观察者注册点（Round 注入 `EventSink`）与 `session_id`（供节点日志关联 / 状态归属）；**业务节点完全无感**，
  广播由引擎自动处理。
- **引擎取引用 + 池化**：ASR 共享引擎（`create_stream()` 天然可并发）；VAD 是串行状态机，用**对象池**
  复用（取用前 `clear()`），而非每回合裸建。
- **旧 Round 默认继续跑**：仅当新回合需占用输出通道 / 打断正在说的话 / 抢 websocket 时才中断旧 Round
  （复用 epoch 防串扰语义）。

## 角色与归属

| 角色 | 职责 | 关键 |
|---|---|---|
| **Session（总调度）** | 连接 / phase / Round 生命周期（shadow/upgrade/stop）/ 空闲超时；把入站音频转成 `PipelineEvent` 发到 active Round 链首 | 收敛为**生命周期管理**，不再驱动识别/输出、不再轮询链；构建时从模板 look up 能力驱动握手声明 |
| **Round（唯一处理器）** | **拥有单条链**，自动注入为观察者，订阅广播流；集中响应 barge-in / 回合推进 / 音频逐句转发 / 超时 / 错误 | 一句一回合；链是内部实现 |
| **Ling（决策中枢 trait）** | 接收已识别结果 → 决策 → 产出表达意图；参考 LLM / DB / 传感 / 网络 | 实现可 LLM/agent · 状态机 · 混合 · 传声筒；缺数据以 `NeedsInfo` 声明（Stage E） |
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

   数据回路(NeedsInfo, Stage E): 回合收尾 → 编排数据节点 → 数据环绕回 Ling → 再决策 → 新回合
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
    Configure(AudioParam),                 // 替代 Reconfigurable：经流内事件触发重配
    FinishTurn,                            // 内部控制：请求 ASR 立即 finish
    // 表达
    TextChunk { text: String, emotion: Option<String> },
    AudioOut { audio: Vec<Vec<u8>>, is_first: bool, is_last: bool },
    // Stage E：多模态与数据回路
    // ImageInput / VideoInput / Command(String) / VideoOut / NeedsInfo{query, via}
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

- **单一 `Node`**：删除 `Resetable` / `Reconfigurable` / `StreamingNode`。reset 非公共协议——
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
- **删除跨层复制**：`session::AudioConfig` / `SessionBuilder::with_audio_config` / api `to_audio_config()`
  全部移除；真正配置源 api `config::audio::AudioConfig`（流向 `TtsManager`/`TtsMatcha`）保持不变。

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

## 链构建（原型 + compose_chain，去 Factory / NodeDeps / 闭包注入）

- 删除 `PipelineFactory` trait、`NodeFactory` struct、`build_chain` 函数、`NodeDeps` struct 与
  `ChainBuilder`（`with_chain_builder` 闭包注入）。
- **`Node` = 原型**：每个节点自带 `new_instance(&self) -> Arc<dyn Node>`，以自身为模板克隆实例。
  模板自持所需共享引用（`Arc<VadPool>` / `Arc<dyn Asr>` / `Arc<dyn Ling>` / `Arc<dyn Tts>`），
  `new_instance` 时克隆出每 Round 独立的新实体。节点**不自存 per-session `session_id`**——会话标识由
  Round 注入 `NodeContext.session_id`，节点仅从 `ctx` 读取用于日志关联。
- **`compose_chain(Vec<Arc<dyn Node>>)`**（service `pipeline/mod.rs`）统一拼链：内部逐叶子
  `with_observer` + `reduce(compose)`。组装者只给**裸原型**，广播对组装者透明。
- **Session** 持裸原型集合 `Vec<Arc<dyn Node>>`（`SessionBuilder::with_node_templates`），每 Round：
  `templates.iter().map(|t| t.new_instance())` → `compose_chain(...)` 产链。
- **Session 构建时从模板 look up 会话级能力**：`find_map` downcast `AudioSpec`（见「能力 look up」），
  驱动握手音频声明与 pacer；无 TTS 节点则不声明下行音频（`audio_params: None`）。
- **调用方（api 站点 ws / matrix / 测试）** 动态组裸原型 `Vec`——按配置条件 `push` / 换序，构成
  `Arc::new(X) as Arc<dyn Node>`，**零 `with_observer`、零 `NodeDeps`、零建链闭包**。
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
| `DataNode`（MCP/DB/传感） | 新增 | 响应 `NeedsInfo`，返回数据事件（Stage E） |

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
  打断 stop_round / ListenStop），**不再轮询链尾**。

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

## 回合联动与数据回路

- 回合边界判定在 **AsrNode 的静音确认**（`SILENCE_CONFIRM_MS=200`，VAD 报非语音后触发 finish）与
  **Session 控制事件**（`FinishTurn`：`ListenStop` / 静音超时 / transport stall）完成；`TurnNode` 只做
  `TurnText→TurnComplete` 显式收尾标记（D2）。
- **规则收尾（默认）**：AsrNode 按静音确认阈值（200ms）收尾一段完整意图——VAD 报非语音后
  `silence_samples ≥ SILENCE_CONFIRM_MS·sample_rate/1000` 即 finish；另有静音超时、transport stall、
  `FinishTurn` 控制事件、prefix。
- **旧 Round 语义**：默认**继续跑**；仅当新回合需占用输出通道 / 打断说话 / 抢 websocket 时才中断——
  复用 `RoundStopReason::Upgrade/BargeIn` + epoch 防陈旧输出。
- **Ling 缺数据（Stage E）**：`NeedsInfo{query, via}` 流出 Ling → 数据节点获取 → 环绕回 Ling →
  再决策 → 新回合（Session 编排回流，非 Ling 反向命令）。

## Session 总调度（Round 生命周期管理）

- phase 状态机保留：`Idle` / `Listening` / `Speaking`。
- `on_listening(Frame::Voice)` → 转发 `AudioFrame` 到 active Round 链首（D1）。
- **Session 注册为 current Round 的外层观察者**：在 `RoundEvent` 通道上 `select!`（取代旧链尾轮询），
  收到信号做**生命周期决策**：
  - `SpeechStarted` → barge-in：`next_round_epoch()` + `stop_round(BargeIn)`（该打断由当前 shadow 继续听，
    其 `TurnComplete` 再升级，不再 `new_shadow_round`）
  - `TurnComplete{text, prob}` → `on_turn_complete`：`shadow→running` 升级、相位 Speaking、ListenStop，
    并**立即新建下一轮 shadow**（`new_shadow_round`）以支持说话中 / 连续输入监听
  - `SpokenEnd` → 表达结束（shadow 相位处理 / 空闲计时）
- Session 不再有 `drain_hearing` / `handle_hearing_event` / `active_hearing`（响应逻辑移至 Round 观察者）。
- `with_node_templates(Vec<Arc<dyn Node>>)` 注入**裸原型集合**（api 站点动态组，决定链的阶段/顺序）。
- 握手（`handle_connect`）回包 `audio_params` 由构建时 look up 的 `AudioSpec` 决定；无 TTS 节点则下发
  `audio_params: None`（无下行语音能力，不兜底默认）。pacer 节奏取 `frame_duration_ms`（无 spec 不构造）。
- epoch 防串扰保留；输出转发也算活动（重置 `idle_since`）。

### 静音 / 空输入判别（中枢 gatekeeper）

空输入（无有效语音）由中枢 **Session 辨别并决定反应（对话 Act）**，生成层只渲染文案。遵循业界
"中枢决策、NLG 渲染"分层：Session 负责判别 + 计数 + 决策；`Ling`/Echo（NLG）只按 Act 措辞。

**判别（`EmptyKind`）**，Session 在进入 `Listening` 时依据模式 + 前一 turn 判定：

| kind | 判定条件 |
| --- | --- |
| `Manual` | push-to-talk：`ListenStop` 且未检出语音（`!is_voice_break_detect` 且 `!speech_active`） |
| `Wake` | 唤醒词（`Input{mode:Wake}`）之后首次监听的空输入 |
| `AutoSpoke` | 免提 auto：VAD 触发但 ASR 文本为空（说了话但没听清，`asr_node` 产 `EmptyInput`） |
| `Silence` | 免提 auto/realtime 完全静默：VAD 从未触发，ASR 无流 `Nothing` |
| `Continuing` | 回复后连续监听（realtime `Speaking→Listening`）下的空输入 |

**计数（Rule of three）**：Session 维护 `empty_count`，每次 `EmptyTurn` `count++`，
成功 `TurnComplete`（真实输入）后复位；最多 3 次后收敛。
`Manual` 例外：事件驱动，每次按键无人声都提示一次，提示后 `empty_count` 归零，不受 Rule of
three 限次（避免 push-to-talk 连按无人声时被反复打扰的同时，保证每次按键都给一次引导）。

**决策（对话 Act）**，Session 观察 `(After, EmptyInput)`/`(After, EmptyTurn)` 后决定：

- `Prompt{kind, count}` → 重新注入链首（各节点透传未识别事件），`Ling` 渲染提示语
- `Silence` → 不注入表达（静默等待 / 回 idle，用于 `Continuing` 避免反复打扰）
- `GiveUp` → 停止提示，回 `Idle`

**提示语分级**（真实 LLM 按 Act 组 prompt，Echo 按 kind/count 返回固定句）：

| kind | count 1 | count 2 | count 3 |
| --- | --- | --- | --- |
| `Manual` | 温柔引导"没听到，请再说"（每次按键无人声都提示一次，之后回到监听等待） | 同上 | 同上 |
| `Wake` | 引导式"想让我帮你做什么？" | 更具体 | 回 idle |
| `AutoSpoke` | "没听清，请重说" | 给可操作示例 | 优雅收尾静默 |
| `Silence` | 温柔引导不指责 | 更具体 | 回 idle |
| `Continuing` | 静默等待（不提示） | 静默 | 回 idle |

## 验证

- `cargo check --workspace --all-targets` / `cargo fmt --all` / `cargo clippy -p service -p api` 零警告。
- `cargo test -p service -p api` 全绿；新增：
  - 引擎**时机广播**测试：喂一 event → 链处理时以 `Tapped{Before,ev}` 与 `Tapped{After,ev}`
    双时机广播给观察者，验证标签与事件内容。
  - 能力 look up 测试：`TtsNode` 上报 `AudioSpec`、Session 从模板 `capabilities()` `downcast_ref::<AudioSpec>()` 解析
    （握手 `audio_params` 字段三值断言）；无 TTS 模板时握手 `audio_params: None`。
  - 单链（`opus→vad→asr→turn→ling→tts`）构建 + 全链消费测试。
  - VAD 池 / turn 语义保持既有基线。

## 差异（相对现实现）

| 维度 | 现实现 | 新蓝图（单链 + 观察者） |
|---|---|---|
| 链结构 | 听段 `build_hearing` + 说段 `build_speaking` 两段 | **单条链** `opus→vad→asr→turn→ling→tts` |
| 建链抽象 | `PipelineFactory` trait + `NodeFactory` struct | **`Node` 自带原型（`new_instance`）+ `compose_chain` 统一拼链** + `with_node_templates` 注入裸原型集合 |
| 观察者 | 无（Session 轮询听段链尾；Round 长 task 消费说段链尾） | **双层**：内层 Round ← pipeline 广播（`Tapped`）；外层 Session ← `RoundEvent`（Round 可观察主体） |
| 广播时机 | 无 | `Tapped{Before, After}` 双时机，引擎自动贴标签，业务节点无感 |
| Session 驱动来源 | 轮询听段链尾（`drain_hearing`/`handle_hearing_event`/`active_hearing`） | **注册订阅 `RoundEvent`**，只在 RoundEvent 上 `select!` 做生命周期决策 |
| Session 职责 | 驱动识别（轮询）+ 驱动输出 | **仅 Round 生命周期**（shadow/upgrade/stop/phase/空闲）；不再轮询链 |
| Round | 起长 task 消费说段 | **双层观察者**：内层统一消费（barge-in/推进/转发/超时/错误）；外层暴露 `RoundEvent` 供 Session 订阅 |

## 相关文件

| 路径 | 变化 | 作用 |
|---|---|---|
| `service/src/pipeline/mod.rs` | 改 | `NodeContext{ cancel, emit, session_id }` + `EventSink`/`Tapped`/`TapPoint`；删 `PipelineFactory`；`Node::new_instance` 原型 + `on_configure` 模板级下行配置 + `NodeCapability`/`capabilities()` 泛化能力 look up + 生命周期（`release_mode`/`on_acquire`/`on_release`）；`compose_chain` 统一拼链；引擎广播 |
| `service/src/session/mod.rs` | 改 | 收敛 Round 生命周期；注册订阅 `RoundEvent`；删 `drain_hearing`/`handle_hearing_event`/`active_hearing`/`ChainBuilder`/`AudioConfig`/`with_audio_config`；构建时 look up 能力；`with_node_templates`；`handle_connect` 把 `Configure` 转发给模板一次 |
| `service/src/session/round.rs` | 改 | 持单链 + 内层订阅 `Tapped` 广播统一消费；外层暴露 `RoundEvent`（`SpeechStarted`/`TurnComplete`/`SpokenEnd`）供 Session 订阅 |
| `service/src/component/tts.rs` | 改 | `Tts` 增 `audio_spec() -> AudioSpec`（引擎自报，`AudioSpec` 定义于 pipeline） |
| `service/src/pipeline/nodes/mod.rs` | 改 | 删 `NodeFactory` / `NodeDeps` / `build_chain` |
| `service/src/pipeline/nodes/*.rs` | 改 | 纯变换 + 实现 `Node::new_instance` 原型克隆；适配 `NodeContext`（业务无感广播）；`TtsNode` 上报 `AudioSpec`；`OpusDecodeNode` 覆写 `on_configure` + 解码后重采样到 16k + `new_instance` 继承配置 |
| `api/src/component/tts/model/*` | 改 | `TtsMatcha`/`TtsMute` 实现 `audio_spec()`（matcha 自字段 / mute 默认 16000·1·60） |
| `api/src/component/tts/mod.rs` | 改 | `StreamingOpusEncoder` 拆出至同目录 `opus_encoder.rs`（`mod opus_encoder; pub use`），`component/tts/model/matcha/` 改用新路径引用 |
| `api/src/ws/mod.rs`、`matrix/client.rs` | 改 | 动态组**裸原型** `Vec<Arc<dyn Node>>` + `with_node_templates`；删 `to_audio_config`/Session `AudioConfig` 跨层复制 |
| `api/tests/*` | 改 | 适配；新增时机广播 / `compose_chain` / `capabilities` look up / 单链用例 |

## 风险与控制

- **VAD 池回收竞态**：串行状态机，acquire/release 严格配对，保证同时仅一个 Round 持有。
- **回合判定迁移**：原 Session 静音/停摆逻辑搬入 turn 节点 + Round 观察者，用既有单测当基线保行为不变。
- **广播顺序**：Before/After 标签需与引擎广播点严格一致，避免观察者收到错序信号。
- **epoch / 终止**：机制完全保留，不因重构改动。
- **emotion**：TTS FIFO 配平，不依赖文本键映射。

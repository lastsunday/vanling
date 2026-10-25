+++
title = "管道演进记录"
weight = 403
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 管道演进记录

单链 + 观察者形态取代了什么、当时为什么这么选。管道现在是什么样，见[统一管道](@/development/server/pipeline.md)。

## 解决的结构性问题

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

## 被替换的形态

| 维度 | 替换前 | 替换后 |
|---|---|---|
| 链结构 | 听段 `build_hearing` + 说段 `build_speaking` 两段 | **单条链** `opus→vad→asr→turn→ling→tts` |
| 建链抽象 | `PipelineFactory` trait + `NodeFactory` struct | **`Node` 自带原型（`new_instance`）+ `compose_chain` 统一拼链** + `with_node_templates` 注入裸原型集合 |
| 观察者 | 无（Session 轮询听段链尾；Round 长 task 消费说段链尾） | **双层**：内层 Round ← pipeline 广播（`Tapped`）；外层 Session ← `RoundEvent`（Round 可观察主体） |
| 广播时机 | 无 | `Tapped{Before, After}` 双时机，引擎自动贴标签，业务节点无感 |
| Session 驱动来源 | 轮询听段链尾（`drain_hearing`/`handle_hearing_event`/`active_hearing`） | **注册订阅 `RoundEvent`**，只在 RoundEvent 上 `select!` 做生命周期决策 |
| Session 职责 | 驱动识别（轮询）+ 驱动输出 | **仅 Round 生命周期**（shadow/upgrade/stop/phase/空闲）；不再轮询链 |
| Round | 起长 task 消费说段 | **双层观察者**：内层统一消费（barge-in/推进/转发/超时/错误）；外层暴露 `RoundEvent` 供 Session 订阅 |

## 未实施的蓝图部分

同一份蓝图里有两块没有落地，代码里找不到对应实现：

| 设想 | 现状 |
| --- | --- |
| Stage E 数据回路：Ling 以 `NeedsInfo{query, via}` 声明缺数据，数据节点（MCP/DB/传感）取数后绕回 Ling 再决策 | `PipelineEvent` 没有 `NeedsInfo`，也没有 `DataNode`；MCP 与历史由 `LingCore` 自己持有 |
| ling 内部 sub-pipeline：把 `LingCore` 里约 200 行 `while has_next_step` 命令式循环拆成 `web_node` / `agent_node` / `data_node` / `re_decision_node` | 仍是命令式循环 |

这两项都不在 [roadmap](@/roadmap/_index.md) 里跟踪：要推进就得先由人给优先级，否则就保持"未做"。

## 布局与能力的取舍

| 决定 | 依据 |
| --- | --- |
| `pipeline.rs` → `pipeline/mod.rs` | 统一 `X/mod.rs` 惯例 |
| `StreamingOpusEncoder` 独立成 `api/src/component/tts/opus_encoder.rs` | 编码器与 TTS 引擎实现分开，`mod.rs` 只留引擎装配 |
| `NodeCapability` 用类型别名 `dyn Any + Send + Sync`，不用自定义 trait + `as_any()` | blanket impl 经 `dyn` 动态分派会丢 `TypeId`，`downcast_ref` 返回 `None` |

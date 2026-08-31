+++
title = "组件与模块结构"
weight = 206
+++

# 组件与模块结构（Components & Module Layout）

> **本文档描述目标架构**：核心骨架层级、双层 `component/` 组织、`ling` 特殊节点语义，以及二期
> sub-pipeline 规划。若需新开会话推进，以此为准。

## 目标核心骨架

数据流/包含层级自上而下：

```
socket / other → filter → session → round → pipeline → ling → sub-pipeline
```

- **socket**：传输入口（WebSocket / Matrix）。`api/src/ws/`、`api/src/matrix/`
- **filter**：入/出站过滤器（MCP 路由、录像）。`api/src/ws/filter/`
- **session**：连接生命周期、phase 状态机、Round 生命周期管理（shadow/upgrade/stop）。
  `service/src/session/`
- **round**：每轮对话（用户发言 → 服务端响应），**拥有单条 NodeChain**。`service/src/session/round.rs`
- **pipeline**：外层节点链（`opus→vad→asr→turn→ling→tts`），`Node`/`NodeChain`/`PipelineEvent`
  框架。`service/src/pipeline/`
- **ling**：决策中枢（特殊节点），接收已识别结果 → 决策 → 产出表达意图。`service/src/ling/`；
  实现位于 `api/src/component/ling/`
- **sub-pipeline**：ling 内部决策子管道（**二期**，暂未实现）。见下文「sub-pipeline（二期）」

## service/src 全面平铺

service crate **去掉 `ling/` 总命名空间**，全面平铺为顶层模块：

```
service/src/
  lib.rs        # pub mod component; pub mod ling; pub mod message; pub mod pipeline; pub mod session;
  component/    # 引擎契约：vad.rs  asr.rs  tts.rs  llm/{mod,token_converter}.rs  mcp/{mod,registry}.rs
  ling/         # Ling 决策引擎（特殊节点）：Ling trait（ask → OutputBlock stream）
  pipeline/     # Node 框架：mod.rs（即原 pipeline.rs）+ nodes/
  session/      # session / round / history；TurnEvent 并入 round.rs
  message/      # 传输协议：hello/audio/close/llm/mcp/stt/tts + Message/Type/Transport/AudioFormat
  frame.rs      # Frame / FrameResult / OutputMessage（传输呈现层）
  types.rs      # 共享类型：EmptyKind / Sentence / Input / OutputBlock / ContentBlock
```

要点：

- **`component/`** = 引擎契约（trait + 类型）。**可独立使用**：既挂外层 pipeline，也挂 ling 的
  sub-pipeline（二期）。
- **`ling/`** = 仅 `Ling` trait（决策引擎）。因需判断/处理一切，作为**特殊节点**必须显式定义全部
  可能 node（见下文「Ling 特殊节点」）。
- **`types.rs`** = 从原 `ling/core.rs` 拆出的共享类型（`EmptyKind`/`Sentence`/`Input`/
  `OutputBlock`/`ContentBlock`），被 pipeline、session、component、api 等跨层引用。
- **`message/` + `frame.rs`** = 传输协议/呈现层，对应骨架最外层的「socket」。
- `TurnEvent` 已并入 `session/round.rs`（回合事件属 round 生命周期）。

## 双层 component

**service 与 api 双层各自收拢引擎**：

| 层 | 内容 | 用途 | 关键路径 |
|---|---|---|---|
| **service `component/`** | 引擎**契约**（trait + 协议类型） | 被 pipeline 节点、ling 引用；不绑定具体模型 | `service/src/component/{vad,asr,tts,llm,mcp}` |
| **api `component/`** | 引擎**实现**（Manager + model） | 启动时按配置择模型，实现契约 | `api/src/component/{vad,asr,llm,tts,mcp}` |

- 范围**只含五引擎**：vad / asr / llm / tts / mcp。
- 依赖方向严格单向：**api 依赖 service，service 不依赖 api**。
- `Ling` 实现 `LingCore` 位于 `api/src/component/ling/`，与其它引擎实现同层（对称）。
- 引擎可**独立实例化**（composition root 决定挂哪里），并从 `component/` 能力 look up 中复用
  AudioSpec 等下行情报。

## Ling 特殊节点

`Ling`（`service/src/ling/`）是**决策中枢**，作为 pipeline 链上的一个节点（`LingNode`）工作，但
与普通纯变换节点不同：

- 它需要**判断/处理一切**（LLM 流、MCP 工具、历史、切句），因此必须显式定义其内部可能出现的
  全部 node。
- 实现侧是 `api/src/component/ling/` 的 `LingCore`（LLM + MCP + history + splitter 编排），产出
  逐句表达意图。
- 缺数据时可声明 `NeedsInfo`（Stage E 数据回路），由 Session 编排回流再决策。

## sub-pipeline（二期）

**本期只做容器重组**，sub-pipeline **不实现**（二期 node 化）。

现状：`LingCore` 内约 200 行命令式 `while has_next_step` 循环（LLM 流 + MCP tool + history +
splitter）。二期将其抽成 ling 内部的**决策子管道**，节点化并行/编排：

```
web_node / agent_node / data_node / re_decision_node ...
```

届时各数据/工具节点同样复用 `service/src/component/` 契约，构成可插拔的 sub-pipeline。

## 惯例修复

- **`pipeline.rs` → `pipeline/mod.rs`**：统一 `X/mod.rs` 惯例。
- **`StreamingOpusEncoder` 独立成文件**：从 `api/src/component/tts/mod.rs` 拆出到
  `api/src/component/tts/opus_encoder.rs`。
- **`NodeCapability` 为类型别名**（方案 A）：`pub type NodeCapability = dyn Any + Send + Sync;`，
  `capabilities() -> Vec<Box<NodeCapability>>`，消费端直接 `c.downcast_ref::<T>()`。
  不再使用自定义 trait + `as_any()`（因 blanket impl 经 `dyn` 动态分派会丢失 `TypeId`，导致
  `downcast_ref` 返回 `None`）。

## 相关文档

- [核心架构](@/development/server/architecture.md)：Session/Round/pipeline 语义
- [统一管道重构](@/development/server/pipeline-redesign.md)：单链 + 观察者协议、能力 look up、节点生命周期

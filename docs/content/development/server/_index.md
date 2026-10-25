+++
title = "服务端"
weight = 100
sort_by = "weight"
+++

# 服务端

vanling 服务端基于 Rust + axum 构建，实现 Xiaozhi 智能音箱协议，提供实时语音对话能力。

阅读顺序：先看核心架构（Session 与 Round 的生命周期），再看组件与管道（一轮数据怎么流），然后才是 VAD/回合判定、对话流程与协议细节；部署按需查阅。

| 页面 | 说明 |
| --- | --- |
| [核心架构](@/development/server/architecture.md) | 整体架构、Session 状态机、Round 生命周期、过滤器与构建入口 |
| [组件与模块结构](@/development/server/components.md) | 核心骨架层级、双层 component、ling 特殊节点 |
| [统一管道](@/development/server/pipeline.md) | 节点契约、链的组装与释放、事件流约定 |
| [VAD 与回合判定](@/development/server/vad.md) | 语音起点/终点判据、两级静音闸门、回合终止路径、参数速查 |
| [对话流程](@/development/server/dialogue-flow.md) | 握手与通讯序列图、监听模式、空输入行为 |
| [WebSocket 通信协议](@/development/server/websocket-protocol.md) | 协议字段定义与二进制格式 |
| [模型与部署](@/development/server/models-and-deployment.md) | AI 模型规格、配置系统、资产安装、CUDA 部署 |

待办只有一处：[路线图](@/roadmap/_index.md)。定位与选型参照在[定位与取舍参考](@/discussions/positioning.md)；为什么原来的 TODO 被拆掉，见[服务端记录](@/records/server/todo-triage.md)。

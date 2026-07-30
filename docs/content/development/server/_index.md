+++
title = "服务端"
weight = 100
sort_by = "weight"
+++

# 服务端

vanling 服务端基于 Rust + axum 构建，实现 Xiaozhi 智能音箱协议，提供实时语音对话能力。

本小节包含以下文档：

| 页面 | 权重 | 说明 |
|------|------|------|
| [核心架构](@/development/server/architecture.md) | 200 | 整体架构、Session 状态机、Round 生命周期 |
| [对话流程](@/development/server/dialogue-flow.md) | 201 | 握手与通讯阶段的完整序列图 |
| [WebSocket 通信协议](@/development/server/websocket-protocol.md) | 202 | 协议字段定义与二进制格式 |
| [模型与部署](@/development/server/models-and-deployment.md) | 203 | AI 模型规格、配置系统、Downloader、CUDA 部署 |
| [TODO](@/development/server/TODO.md) | 204 | 待办事项与已知问题 |

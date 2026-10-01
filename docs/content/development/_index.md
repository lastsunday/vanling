+++
title = "开发文档"
weight = 30
sort_by = "weight"
+++

# 开发文档

Vanling 服务端和相关项目的开发指南。

## [服务端](@/development/server/architecture.md)

服务端架构设计、业务数据流、协议参考、模型规格与部署。

- [核心架构](@/development/server/architecture.md) — 会话状态机、并发模型、工厂模式
- [对话流程](@/development/server/dialogue-flow.md) — 握手、通讯、Listen Mode、MCP 流程
- [WebSocket 协议](@/development/server/websocket-protocol.md) — 协议字段参考
- [模型与部署](@/development/server/models-and-deployment.md) — 模型规格、CUDA 安装、参考规范
- [TODO](@/development/server/TODO.md) — 已完成 / 未完成清单（开工入口）
- [定位与取舍参考](@/development/server/research.md) — 功能探索、技术选型与参考项目

## [客户端](@/development/clients/app.md)

客户端应用开发文档。

- [App（Flutter）](@/development/clients/app.md)
- [管理后台（React）](@/development/clients/server-ui.md)
- [ESP32](@/development/clients/esp32.md)

## [调试](@/development/debugging/vad-listener.md)

调试和诊断相关文档。

- [VAD 与 Listener](@/development/debugging/vad-listener.md)
- [Audio 调试](@/development/debugging/audio-debug.md)
- [ASR 调试](@/development/debugging/asr-debug.md)

## [IoT 固件](@/development/iot/features.md)

Vanling 自有 ESP32 固件（`apps/iot`）开发文档。

- [Cargo Feature 判据与内聚](@/development/iot/features.md) — 硬/软 feature 定义、引入判据、内聚规则
- [无硬件仿真与回归冒烟](@/development/iot/emulation.md) — 宿主 harness + esp-emu、决策矩阵

实现怎么变成现在这样、被否过什么，在[开发记录](@/records/_index.md)（AI 产出，未经人类审阅）。

## [模型下载](@/development/downloader.md)

模型下载工具的使用说明。

## 相关项目

生态相关项目文档。

- [xiaozhi-esp32](@/development/related-project/xiaozhi-esp32.md)
- [xiaozhi-esp32-server](@/development/related-project/xiaozhi-esp32-server.md)
- [xiaozhi-esp32-server-java](@/development/related-project/xiaozhi-esp32-server-java.md)
- [xiaozhi-android-client](@/development/related-project/xiaozhi-android-client.md)

## 多语文档

改动 `docs/` 下中文页面后，需同步翻译为 `.en.md`，并执行 `git hash-object <source.md>` 更新其 front matter 中的 `source_file_hash` 与 `translated_at`。

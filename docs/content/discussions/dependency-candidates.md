+++
title = "依赖候选"
weight = 30
+++

# 依赖候选

服务端演进过程中被提到、但**尚未批准**的外部依赖。这里只记录候选与待定问题，不构成选型结论；按 [AGENTS.md](https://github.com/anomalyco/vanling/blob/main/AGENTS.md)，新增 crate 需要人类先同意。

| 候选 | 服务于 | 现状 | 待定问题 |
| --- | --- | --- | --- |
| `nanoid` | Invite Code 短 ID | 未引入；邀请码本身也未实现 | 短 ID 的碰撞概率是否值得多一个依赖 |
| `tower-governor` + `governor` | 限流（GCRA） | 已有自建固定窗口 + 登录失败锁，功能已覆盖 | 是否用 GCRA 替换现有实现，换来的平滑性是否够 |
| `qdrant` | 个性化记忆 / RAG | 未引入；`rig-core` 已支持多种向量后端 | 自托管还是嵌入式，运维成本谁承担 |
| `wasmtime` | 插件系统 | 未引入 | WASI P2 ABI 稳定性与每个插件的运行时开销 |
| `rumqttc` | MQTT 网关 / 设备间通话 | 未引入；当前传输只有 WebSocket | 是否把 MQTT 作为第二传输，还是继续单 WS |
| `rodio` | 音乐播放 | 未引入 | 播放放在服务端还是端侧，服务端播放与 TTS 输出如何共存 |
| `tokio-cron-scheduler` | Timer / 提醒 / 闹钟 | 未引入 | 单机内存调度够不够，重启后任务要不要恢复 |
| `csm.rs`（AGPL-3.0） | 对话韵律 TTS | 未引入 | AGPL 与项目许可的兼容性未评估 |
| `voirs-emotion` | 情绪自适应语调 | 未引入；`sherpa-onnx` 已有 SER 模型 | 与现有 SER 能力重叠，是否只差一层映射 |
| `webrtc-rs` | WebRTC 实时音视频 | 未引入 | 是否作为第三种传输，ICE/TURN 由谁部署 |
| `aec3` | 服务端回声消除 | 未引入；AEC 目前在端侧 | 端侧已有 AEC 时，服务端再做一遍的收益是多少 |
| `polyvoice` | Speaker diarization | 未引入；`sherpa-onnx` 也有说话人能力 | 与现有能力重叠，是否只需扩模型 |
| `web_socket_channel` | Flutter App WS | 未引入；App 侧 WS 尚未接入 | Flutter 侧传输选型是否已定 |

以下**不是**新依赖，只是现有依赖的能力扩展，不需要本表批准：

| 已有依赖 | 可扩展的能力 |
| --- | --- |
| `sherpa-onnx` | Piper/VITS 模型（多语言 TTS）、ECAPA-TDNN/WeSpeaker（声纹）、speaker embedding（声音克隆）、SER（情绪） |
| `rig-core` | 向量存储后端（RAG / 记忆）、Agent/Chain/Router（任务编排） |
| `rmcp` | 工具市场、per-agent 工具过滤、远程 MCP |

分层轮次与 VAD 调参提到过 LiveKit `v1-mini` 与 OpenAI 的配置模式，它们是模型/配置参考而不是 crate；是否作为 Layer 2/3 仍未决定，见[路线图](@/roadmap/_index.md) Session / Listener。

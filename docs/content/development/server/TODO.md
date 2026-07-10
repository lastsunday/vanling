+++
title = "TODO"
weight = 204
+++

# TODO

> **⚠️ 说明：本文档为功能探索性梳理，参考了行业标准（Alexa+、Gemini、Siri AI）和参考项目实现（xiaozhi-esp32-server、xiaozhi-esp32-server-java、xiaozhi-server-go）。大部分条目不会全部实现，仅作为 chobits 定位和取舍的参考依据。实际开发优先级以项目 Roadmap 为准。**

按功能模块分类的待办事项清单。修复前请阅读 [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) 了解开发规范。

## 安全 & 认证

| 优先级 | 项目 | 位置 | 描述 | 状态 |
|--------|------|------|------|------|
| 🔴 P0 | WS 认证 | `api/src/ws/mod.rs` | WS handler 未应用认证层，所有 WS 连接未认证 | 🔴 P0 |
| 🔴 P0 | Invite Code 系统 | 新功能 | 无邀请码生成/验证/管理，新用户注册无控制 | 🔴 P0 |
| 🟡 P1 | Rate Limiting | `api/src/auth.rs` | 无登录限流/暴力破解防护 | 🟡 P1 |
| 🟡 P1 | Refresh 吊销 | `api/src/auth.rs` | refresh token 无吊销机制，logout 仅客户端清除 | 🟡 P1 |
| 🟡 P1 | Token 日志 | `api/src/auth.rs` | access token 明文记录在 tracing span | 🟡 P1 |
| 🟡 P1 | OTA 设备激活 | `api/src/ota.rs` | `activate` 端点为 stub，返回 "success" 但不验证设备，设备信息未存 DB | 🟡 P1 |
| 🟡 P1 | MCP 认证 | `api/src/mcp/mod.rs` | `/mcp` 端点认证被注释掉 | 🟡 P1 |

## 核心功能

| 优先级 | 项目 | 位置 | 描述 | 状态 |
|--------|------|------|------|------|
| 🔴 P0 | stop_round 竞态 | `service/src/chobits/session/round.rs` | `llm_tts_handle` 与 `stop_round` 之间缺少同步，可能 use-after-cancel | 🔴 P0 |
| 🔴 P0 | LLM 线程安全 | `api/src/llm/model/qwen3/mod.rs` | `thread::spawn` + `block_on`，未 `catch_unwind`，panic 静默崩溃 | 🔴 P0 |
| 🔴 P0 | LLM Echo 线程 | `api/src/llm/model/echo/mod.rs` | 同上 | 🔴 P0 |
| 🔴 P0 | Wake Word 支持 | `api/src/ws/` + 协议层 | ESP32 客户端已有 ESP-SR 离线唤醒，服务端需处理 `wake_word` 消息类型，当前协议无此字段。行业标配（Echo/Google/Xiaozhi 均有），响应延迟 <200ms | 🔴 P0 |
| 🟡 P1 | 声纹识别 (Voiceprint) | 新功能 | 参考项目 xinnan-tech 已实现（3D-Speaker 模型），与 ASR 并行处理，识别说话人身份传递给 LLM 实现个性化回复。sherpa-onnx 已支持 speaker identification（ECAPA-TDNN/WeSpeaker），无需新依赖。黑客365 Go 版使用 Qdrant 向量库 + 动态 TTS 声音切换，架构更成熟。需：注册/管理/识别流程 + DB 存储声纹向量 + LLM 上下文注入 | 🟡 P1 |
| 🟡 P1 | Opus 除零 | `api/src/ws/default_listener.rs` | channels=0 / sample_rate=0 时除零 | 🟡 P1 |
| 🟡 P1 | 时钟溢出 | `service/src/chobits/session/mod.rs` | `Local::now()` 非单调，减法可溢出 | 🟡 P1 |
| 🟡 P1 | 设备管理 | 新功能 | 无设备注册/绑定/列表，OTA 激活后无设备持久化。参考项目有完整设备生命周期管理（注册/状态/配置/OTA/批量操作） | 🟡 P1 |
| 🟡 P1 | 音乐播放 | MCP tool 或独立功能 | 参考项目 xinnan-tech 有 `play_music.py` + `hass_play_music.py`；joey-zhou 有 `MusicPlayer` 支持 LRC 歌词同步。行业标配 | 🟡 P1 |
| 🟡 P1 | Timer/提醒/闹钟 | MCP tool 或独立功能 | 行业标配（"Alexa, set a timer"），当前无任何定时/提醒机制 | 🟡 P1 |
| 🟡 P1 | Continued Conversation | `service/src/chobits/session/` | 回复后麦克风应短暂保持开放，允许用户免唤醒词追问。Gemini / Alexa+ 均支持 | 🟡 P1 |
| 🟡 P1 | 情绪识别完善 | `api/src/llm/model/` (analyze_emotion) | 当前 stub 返回 "happy"。行业方案：音频特征 (wav2vec2 SER) + 文本情感 (GoEmotions) 双通道融合，用于调整 TTS 语气和回复风格 | 🟡 P1 |
| 🟡 P1 | 个性化记忆/长期偏好 | 新功能 | 当前仅 chat history，无长期偏好存储。参考项目 xinnan-tech 有 PowerMem（用户画像 + 艾宾浩斯遗忘曲线 + 向量检索）；joey-zhou 有 3 种记忆模式（window/summary/long + 图检索） | 🟡 P1 |
| 🟡 P1 | RAG 知识库 | MCP 或内置模块 | 参考项目 xinnan-tech 集成 RAGFlow；joey-zhou 有 EmbeddingModelFactory + 图检索。当前 MCP 框架可接入但无内置向量检索 | 🟡 P1 |
| 🟡 P1 | 多语言 TTS | `api/src/tts/` | 当前仅单语言 TTS voice。ESP32 客户端已支持 25+ 语言 ASR，TTS 侧需匹配 | 🟡 P1 |
| 🟡 P1 | 家居集成 (Home Assistant) | MCP 或独立模块 | 参考项目 xinnan-tech 有 3 种 HA 集成方式（社区插件/HA 作为 LLM 工具/HA MCP Server）。智能家居是语音助手核心场景 | 🟡 P1 |
| 🟡 P1 | 设备间通话 | MCP tool 或独立功能 | 参考项目 xinnan-tech 有 `call_device.py`，ESP32 设备间可像电话一样互相呼叫，需 MQTT gateway + 通讯录管理 | 🟡 P1 |
| 🟡 P1 | Intent 识别 | 新功能 | 参考项目 xinnan-tech 支持 3 种模式：function_call（推荐）、intent_llm（专用 LLM）、nointent。当前 chobits 无独立 intent 层 | 🟡 P1 |
| 🟡 P1 | 音频热路径克隆 | `api/src/ws/default_listener.rs` | 每 20ms `data.to_vec()` 频繁克隆 | 🟡 P1 |
| 🟡 P1 | Quick Reply 预回复 | 新功能 | LLM 推理期间先播放"我在"/"来了"等短语，降低感知延迟。参考项目黑客365 Go 版已实现，UX 关键，实现简单 | 🟡 P1 |
| 🟡 P1 | 动态 TTS 声音切换 | 新功能 | 基于声纹识别自动切换不同 TTS 音色。参考项目黑客365 Go 版已实现（sherpa-onnx 声纹 + per-speaker TTS voice），声纹识别的自然延伸 | 🟡 P1 |
| 🟡 P1 | LLM 历史阻塞 | `api/src/llm/model/qwen3/mod.rs` | DB 落盘导致完整线程阻塞 | 🟡 P1 |
| 🟡 P1 | Recorder 无上限 | `api/src/record/recorder.rs` | `Vec<RecordEntry>` 无大小限制，高并发内存无限增长 | 🟡 P1 |
| ⚠️ P2 | 消息类型 | `api/src/ws/frame.rs` | 缺失 `system`、`alert`、`custom`、`wake_word` 消息类型（对比 xiaozhi-esp32 规范） | ⚠️ P2 |
| ⚠️ P2 | 多 ASR/TTS Provider | `api/src/asr/` + `api/src/tts/` | 参考项目 xinnan-tech 支持 12 ASR + 18+ TTS provider（含免费 EdgeTTS）；joey-zhou 有 7 STT + 8 TTS。chobits 仅 1 ASR + 1 TTS | ⚠️ P2 |
| ⚠️ P2 | 插件系统 | 新功能 | 参考项目 xinnan-tech 有 13 个内置插件 + 热加载机制。chobits 无插件架构，功能扩展需修改核心代码 | ⚠️ P2 |
| ⚠️ P2 | MCP Market / 工具市场 | 新功能 | 参考项目黑客365 Go 版实现 MCP 工具"应用商店"：聚合多第三方市场（如 ModelScope），一键导入远程 MCP 服务 + 热加载。chobits 当前无 MCP 工具发现/聚合机制 | ⚠️ P2 |
| ⚠️ P2 | MCP 调试控制台 | 新功能 | 参考项目黑客365 Go 版有 Agent/Device 维度 MCP 远程调试：Web 控制台生成每 Agent 独立 MCP 端点，实时调用测试，支持 per-agent 工具过滤。chobits 当前无 MCP 调试工具 | ⚠️ P2 |
| ⚠️ P2 | 配置向导+全链路测试 | 新功能 | 参考项目黑客365 Go 版有首次运行向导（OTA/VAD/ASR/LLM/TTS 逐步配置）+ 每组件延迟测试 + 可视化图表。chobits 当前无部署向导 | ⚠️ P2 |
| ⚠️ P2 | MCP 工具聚合 | 新功能 | 参考项目 xiaozhi-mcp / yuexianga/xiaozhi-mcp 提供预置工具库（钉钉/QQ/系统监控/WebPilot/数学计算），开箱即用。chobits 无内置 MCP 工具包 | ⚠️ P2 |
| ⚠️ P2 | MQTT 网关 | 新功能 | 参考项目 xinnan-tech/xiaozhi-mqtt-gateway 实现 MQTT+UDP → WS 桥接：分布式部署 + 动态负载均衡 + HMAC 认证 + MCP 命令下发。chobits 当前仅 WS 单协议 | ⚠️ P2 |
| ⚠️ P2 | VAD 采样率 | `api/src/vad/` | 硬编码 16kHz，非 16kHz 输入无声失败 | ⚠️ P2 |
| ⚠️ P2 | ASR | `api/src/asr/` | SenseVoice (sherpa-onnx)，无 `Sync` trait，仅 16kHz 单声道 | ⚠️ P2 |
| 🟢 P3 | 视觉感知 (VLLM) | 新功能 | 参考项目 xinnan-tech 支持 GLM-4V / Qwen-VL 视觉模型，可拍照识物。chobits 当前纯语音 | 🟢 P3 |
| 🟢 P3 | Proactive 主动建议 | 新功能 | Gemini Daily Brief / Alexa+ 主动提醒交通/降价/日程。需定时任务 + 用户上下文推理 | 🟢 P3 |
| 🟢 P3 | 多模态（语音+屏幕+视频） | 新功能 | Gemini 2.5 / GPT-Realtime / Siri AI 均支持摄像头/屏幕输入。chobits 当前纯语音 | 🟢 P3 |
| 🟢 P3 | 跨设备连续性 | 新功能 | Alexa+: Echo→手机→电脑无缝切换对话上下文。需 session 状态同步机制 | 🟢 P3 |
| 🟢 P3 | Speaker Diarization | `api/src/listener/` | 说话人分离，多人场景下区分不同用户。sherpa-onnx 已支持（ECAPA-TDNN + AHC 聚类） | 🟢 P3 |
| 🟢 P3 | 声音克隆 | `api/src/tts/` | 参考项目 xinnan-tech 支持火山引擎语音克隆；joey-zhou 支持按角色声音克隆。MatchaTTS 已支持 reference audio，需暴露配置接口 | 🟢 P3 |
| 🟢 P3 | AEC 服务端降噪 | `api/src/ws/default_listener.rs` | joey-zhou 已实现 WebRTC AEC3 服务端回声消除（含噪声抑制 + 高通滤波 + 自适应增益）。chobits 仅客户端 AEC | 🟢 P3 |
| 🟢 P3 | WebRTC 实时音视频 | 新功能 | 参考项目 dairoot/xiaozhi-webrtc 实现 WebRTC 低延迟 + Live2D + 多模态视觉 + MCP。chobits 当前仅 WS 协议 | 🟢 P3 |
| 🟢 P3 | 音频标准化集成 | `api/src/util/compressor.rs` → 管道 | `adaptive_normalize()` 已实现但未集成到 TTS 输出管道 | 🟢 P3 |
| 🟢 P3 | Live2D 头像 | 客户端功能 | 参考项目 Android 客户端（TOM88812）已实现：多模型切换 + 实时动画 + 自定义角色 + 情绪模式。chobits Flutter 客户端可参考 | 🟢 P3 |
| 🟢 P3 | 具身 AI / GPIO | 新功能 | 参考项目 py-xiaozhi 已实现：树莓派/Jetson/STM32 直接控制硬件（电机/传感器/LED），摄像头视觉理解。垂直场景，非通用语音助手核心 | 🟢 P3 |
| 🟢 P3 | Session 导出/删除 | `api/src/record/` | Session 仅可查看，不可导出或删除 | 🟢 P3 |
| 🟢 P3 | describe O(n) | `api/src/llm/model/qwen3/mod.rs` | 实时构建全消息历史 | 🟢 P3 |
| 🟢 P3 | TTS 循环克隆 | `api/src/tts/` | `Arc<str>` vs `String` 克隆风暴 | 🟢 P3 |
| 🟢 P3 | 双重序列化 | `api/src/record/recorder.rs` | record 路径双重 JSON 序列化 | 🟢 P3 |

## 基础设施

| 优先级 | 项目 | 位置 | 描述 | 状态 |
|--------|------|------|------|------|
| 🟡 P1 | email 约束 | `migration/src/m20241230_000001_init.rs` | entity 标注 `#[sea_orm(unique)]`，迁移未实现 UNIQUE | 🟡 P1 |
| 🟡 P1 | 外键约束 | `migration/src/m20241230_000001_init.rs` | 缺少 FK: `round.session_id`、`round_data.round_id`、`frame.round_id` | 🟡 P1 |
| 🟡 P1 | MCP 锁顺序 | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server 锁顺序 ABBA，可能死锁 | 🟡 P1 |
| 🟡 P1 | 优雅关闭顺序 | `framework/src/signal.rs` + `apps/server` | 各模块关闭顺序缺失 | 🟡 P1 |
| 🟡 P1 | Panic 处理 | `framework/src/panic.rs` | `eprintln!` 而非 `tracing::error!`，绕过 Sentry | 🟡 P1 |
| 🟡 P1 | Runtime 竞态 | `framework/src/runtime.rs` | `OnceLock` 初始化存在竞态 | 🟡 P1 |
| 🔴 P0 | Signal 宏 | `framework/src/signal.rs` | 使用不存在的 `debug_error!` 宏，非 unix 编译失败 | 🔴 P0 |
| 🟢 P3 | 时间戳自动填充 | `entity/src/config.rs` | `Config` 实体缺失 `ActiveModelBehavior`，时间戳未自动填充 | 🟢 P3 |
| 🟢 P3 | MCP 错误处理 | `api/src/mcp/` | 不完善 | 🟢 P3 |

## 前端 (apps/server-ui)

| 优先级 | 项目 | 位置 | 描述 | 状态 |
|--------|------|------|------|------|
| 🟡 P1 | Dashboard 页 | `routes/_pathlessLayout.admin/index.tsx` | 空壳，仅渲染 "Hello"，无统计/监控内容 | 🟡 P1 |
| 🟡 P1 | User CRUD 管理 UI | 新页面 | 无用户列表/创建/删除/角色管理界面 | 🟡 P1 |
| 🟡 P1 | 多用户/RBAC 管理 | 新功能 | 参考项目 busy-worker Java 管理平台有 Token 用量监控 + 对话时长 + 设备活跃度 + 数据可视化 + RBAC。chobits Dashboard 应包含 | 🟡 P1 |
| 🟢 P3 | 系统监控 | 新页面 | 无服务器健康/连接数/资源使用/错误率仪表盘 | 🟢 P3 |
| 🟢 P3 | MCP 仪表盘 | 新功能 | 参考项目 xiaozhi-mcphub 有 React 前端：多端点管理 + 工具同步 + 群组访问控制 + 日志。chobits MCP 管理参考 | 🟢 P3 |

## 移动端 (apps/app)

| 优先级 | 项目 | 位置 | 描述 | 状态 |
|--------|------|------|------|------|
| 🟡 P1 | Flutter WS 集成 | `apps/app/` | Flutter app 存在脚手架但未集成 WS + 认证 | 🟡 P1 |
| 🟡 P1 | App CI/CD | `.github/workflows/` | 无 iOS/Android 构建/签名/发布流水线 | 🟡 P1 |

## 测试

| 优先级 | 项目 | 位置 | 描述 | 状态 |
|--------|------|------|------|------|
| 🔴 P0 | WS 认证测试 | `apps/server/api/tests/` | WS 端点零认证，无对应测试 | 🔴 P0 |
| 🔴 P0 | Wake Word 测试 | `apps/server/api/tests/` | Wake word 消息处理无测试，需验证协议解析和 session 唤醒流程 | 🔴 P0 |
| 🟡 P1 | Auto 模式 + AEC 测试 | `apps/server/api/tests/session/` | Auto 模式（barge_in=false）在有/无 AEC 场景下无专门测试 | 🟡 P1 |
| 🟡 P1 | Button Talk 测试 | `apps/server/api/tests/session/` | Manual 模式（push-to-talk）缺少端到端测试 | 🟡 P1 |
| 🟡 P1 | Continued Conversation 测试 | `apps/server/api/tests/session/` | 回复后麦克风保持开放的追问流程无测试 | 🟡 P1 |
| 🟡 P1 | Rate Limiting 测试 | `apps/server/api/tests/` | 登录限流功能不存在，无测试 | 🟡 P1 |
| 🟡 P1 | 情绪识别测试 | `apps/server/api/tests/` | analyze_emotion stub 无测试覆盖 | 🟡 P1 |
| 🟡 P1 | 声纹识别测试 | `apps/server/api/tests/` | 声纹注册/识别/LLM 注入流程无测试 | 🟡 P1 |
| 🟡 P1 | 音乐播放测试 | `apps/server/api/tests/` | 音乐播放功能无测试 | 🟡 P1 |

## 待确认 / 探索

| 项目 | 描述 |
|------|------|
| 声纹方案选择 | 3D-Speaker (xinnan-tech 使用) vs sherpa-onnx speaker ID (已在技术栈中) vs pyannote。sherpa-onnx 最佳：无需新依赖，支持 ECAPA-TDNN/WeSpeaker/CAM++ 多种模型 |
| 插件架构设计 | xinnan-tech 有 13 内置插件 + 热加载。chobits 是否需要类似机制，还是通过 MCP 扩展即可？ |
| 多 Provider 扩展策略 | 参考项目支持 12+ ASR / 18+ TTS provider。chobits 是否需要多 provider 架构，还是保持精简？ |
| 音乐播放 | 行业标配（Spotify/Apple Music 集成），需确认实现方式：MCP tool 调用外部 API / 内置音频播放 / TTS 扩展 |
| ESP32 CI/CD | 独立项目 (xiaozhi-esp32)，非本仓库范围，chobits 作为后端提供 WS + OTA 接口即可 |
| Token 持久化存储 | 考虑用 DB 表存储 refresh token 支持多设备管理 |
| Smart Home 集成 | Alexa+ / Google Home 核心能力。参考项目有 3 种 HA 集成方式，需评估 chobits 作为 hub 的定位 |
| Agentic 能力 | Alexa+ "Experts" / Gemini Spark 24/7 代理，可自主浏览网页/填表/下单。LLM + MCP 工具调用已有基础，需评估实现深度 |
| 语音情感 TTS | Gemini 2.5 / Cartesia Sonic-3 支持根据情绪调整 TTS 语气。MatchaTTS 需评估是否支持 style/prosody 控制 |
| 本地隐私处理 | 行业趋势：Echo/Fire 设备向本地处理迁移。chobits 已有 local Qwen3，可扩展 local ASR/TTS 全链路 |
| 视觉能力 | 参考项目 xinnan-tech 支持 VLLM（GLM-4V/Qwen-VL）拍照识物。chobits 是否需要视觉能力？ |
| MCP Market 架构设计 | 参考项目黑客365 Go 版实现 MCP 工具"应用商店"（聚合多市场+热加载）。chobits 是否需要 MCP 工具发现/聚合机制？ |
| 动态 TTS 声音切换方案 | 参考项目黑客365 Go 版基于声纹识别自动切换 TTS 音色。chobits 声纹识别后如何联动 TTS？ |
| 配置向导设计 | 参考项目黑客365 Go 版有首次运行向导 + 全链路延迟测试。chobits 是否需要部署向导降低使用门槛？ |
| MQTT 网关架构 | 分布式部署需要 MQTT+UDP 桥接 + 动态负载均衡（参考 xinnan-tech/xiaozhi-mqtt-gateway）。chobits 是否需要 MQTT 网关层？ |
| MCP 工具聚合策略 | 参考项目有预置工具库（钉钉/QQ/系统监控/WebPilot/数学计算等），chobits 是否需要内置 MCP 工具包？ |

---

## 附录：相关项目

> 以下是 chobits 定位和功能取舍的主要参考项目，按类别组织。

### 后端服务（替代实现）

| 项目 | 语言 | Stars | 对 chobits 的参考价值 |
|------|------|-------|----------------------|
| [xinnan-tech/xiaozhi-esp32-server](https://github.com/xinnan-tech/xiaozhi-esp32-server) | Python | 10k+ | 全功能参考：12 ASR + 18 TTS + 13 插件 + PowerMem + RAGFlow + VLLM |
| [joey-zhou/xiaozhi-esp32-server-java](https://github.com/joey-zhou/xiaozhi-esp32-server-java) | Java | 1.3k | DDD 架构 + WebRTC AEC3 + 3 记忆模式 + RBAC + A/B 设备协同 |
| [AnimeAIChat/xiaozhi-server-go](https://github.com/AnimeAIChat/xiaozhi-server-go) | Go | — | 商用级：VLLM 图片安全 + Quick Reply + 角色系统 + UPX 压缩 |
| [hackers365/xiaozhi-esp32-server-golang](https://github.com/hackers365/xiaozhi-esp32-server-golang) | Go | — | MCP Market + OpenClaw + MCP Audio Server + 配置向导 + 动态 TTS |
| [78/xiaozhi](https://github.com/78/xiaozhi) | — | 772 | 原始官方版本（已弃用） |
| [mm7h/XiaoZhi.Net](https://github.com/mm7h/XiaoZhi.Net) | C# | 35 | .NET 8 实现 + sherpa-onnx + 插件系统 |
| [daxpot/xiaozhi-cpp-server](https://github.com/daxpot/xiaozhi-cpp-server) | C++ | 32 | C++20 协程架构 + EdgeTTS + Doubao |
| [Hyrsoft/xiaozhi_linux_rs](https://github.com/Hyrsoft/xiaozhi_linux_rs) | Rust | 47 | **首个 Rust 客户端**：ALSA + Opus + MCP 动态加载 |

### 客户端实现

| 项目 | 语言 | Stars | 参考价值 |
|------|------|-------|----------|
| [huangjunsen0406/py-xiaozhi](https://github.com/huangjunsen0406/py-xiaozhi) | Python | 3.4k | 跨平台 AI 客户端：摄像头视觉 + GPIO + Live2D + MQTT |
| [TOM88812/xiaozhi-android-client](https://github.com/TOM88812/xiaozhi-android-client) | Flutter | — | Live2D + Mood Mode + HTML 预览 + 思维链可视化 |
| [shenjingnan/xiaozhi-client](https://github.com/shenjingnan/xiaozhi-client) | TS | 282 | MCP CLI 桥接：聚合多 MCP 端点 → Cursor/Cherry Studio |
| [TOM88812/xiaozhi-web-client](https://github.com/TOM88812/xiaozhi-web-client) | HTML | 184 | 浏览器端语音对话：WebRTC + AudioWorklet + Opus |
| [SylarLi/xiaozhi-unity](https://github.com/SylarLi/xiaozhi-unity) | C# | 57 | Unity3D + VRM 虚拟形象 + uLipSync + 米家控制 |
| [coloz/xiaozhi-library](https://github.com/coloz/xiaozhi-library) | Arduino | 10+ | Arduino 库：100+ 板型 + WS/MQTT 双协议 + LVGL + 20 语言 |

### MCP 生态工具

| 项目 | 语言 | 参考价值 |
|------|------|----------|
| [xinnan-tech/mcp-endpoint-server](https://github.com/xinnan-tech/mcp-endpoint-server) | Python | 轻量 MCP 注册中心，WebSocket 协议，Docker 部署 |
| [huangjunsen0406/xiaozhi-mcphub](https://github.com/huangjunsen0406/xiaozhi-mcphub) | TS | 企业级 MCP 管理：多端点 + 向量路由 + RBAC + React 仪表盘 |
| [avxxoo/xiaozhi-mcp](https://github.com/avxxoo/xiaozhi-mcp) | Python | MCP 工具聚合：钉钉/QQ/系统监控/WebPilot/数学计算 |
| [yuexianga/xiaozhi-mcp](https://github.com/yuexianga/xiaozhi-mcp) | Python | 18 工具 MCP 服务器：文件/Telegram/系统/Git/邮件/截图 |
| [mcp2xiaozhi](https://pypi.org/project/mcp2xiaozhi/) | Python | 通用 MCP 桥接：stdio/SSE/HTTP → WS，PyPI 包 |
| [ZhongZiTongXue/xiaozhi-MCPTools](https://github.com/ZhongZiTongXue/xiaozhi-MCPTools) | VB6 | GUI MCP 部署：25+ 开放 API 工具 + 音乐播放 |
| [johz-chen/mcp-bridge](https://github.com/johz-chen/mcp-bridge) | Rust | Rust MCP 桥接：WS/MQTT + 进程管理 + 心跳 |
| [dsw0000/xiaozhi-openclaw-plugin](https://github.com/dsw0000/xiaozhi-openclaw-plugin) | JS | OpenClaw 双向通信：消息/设备控制/Agent 任务 |

### Home Assistant 集成

| 项目 | 语言 | 参考价值 |
|------|------|----------|
| [RealDeco/xiaozhi-esphome](https://github.com/RealDeco/xiaozhi-esphome) | ESPHome | 767 星，15+ 设备支持，HA 语音卫星，无需 xiaozhi 服务器 |
| [AleksSem/xiaozhi-assistant](https://github.com/AleksSem/xiaozhi-assistant) | Python | 最完整 HA 集成：Conversation Agent + STT/TTS + MCP + OTA |
| [c1pher-cn/ha-mcp-for-xiaozhi](https://github.com/c1pher-cn/ha-mcp-for-xiaozhi) | Python | HA 直接作为 MCP Server：WebSocket + 多实体代理 |
| [mac8005/xiaozhi-mcp-ha](https://github.com/mac8005/xiaozhi-mcp-ha) | Python | HACS MCP 代理：SSE proxy + 自动重连 |

### MQTT 网关

| 项目 | 语言 | 参考价值 |
|------|------|----------|
| [xinnan-tech/xiaozhi-mqtt-gateway](https://github.com/xinnan-tech/xiaozhi-mqtt-gateway) | Python | MQTT+UDP → WS 桥接：动态负载均衡 + HMAC 认证 + MCP 命令下发 |

### 嵌入式硬件适配

| 项目 | 平台 | 参考价值 |
|------|------|----------|
| [78/xiaozhi-sf32](https://github.com/78/xiaozhi-sf32) | SiFli SF32 | 蓝牙 PAN 联网 + LCD + AEC + OTA |
| [100askTeam/xiaozhi-linux](https://github.com/100askTeam/xiaozhi-linux) | 嵌入式 Linux | NXP/Allwinner/Canaan/Rockchip/STM32 多 BSP |
| [QuecPython/solution-xiaozhiAI](https://github.com/QuecPython/solution-xiaozhiAI) | Quectel 4G | 4G 蜂窝模块 + 语音唤醒 + WebSocket |
| [D-Robotics/xiaozhi-in-rdk](https://github.com/D-Robotics/xiaozhi-in-rdk) | Horizon RDK | 地平线 RDK 开发板适配 + 边缘 AI 加速 |

### 部署工具

| 项目 | 描述 |
|------|------|
| [haotianshouwang/xiaozhi-server-installer-docker.sh](https://github.com/haotianshouwang/xiaozhi-server-installer-docker.sh) | 一键 Docker 部署脚本 + 交互式配置 + 15+ API 支持 |
| [jsntwdj/xiaozhi-esp32-server](https://hub.docker.com/r/jsntwdj/xiaozhi-esp32-server) | ARM64 Docker 镜像（树莓派等） |
| [78/xiaozhi-assets-generator](https://github.com/78/xiaozhi-assets-generator) | Web 资源生成器：自定义唤醒词/字体/表情/聊天背景 |

### 管理监控

| 项目 | 描述 |
|------|------|
| [busy-worker/xiaozhi-esp32-server-java](https://github.com/busy-worker/xiaozhi-esp32-server-java) | Java 管理平台：Token 用量 + 对话时长 + 设备活跃度 + 数据可视化 |
| [joey-zhou/xiaozhi-concurrent](https://github.com/joey-zhou/xiaozhi-concurrent) | WS 并发压测工具：指标仪表盘 + 自动性能报告 |

---

## 优先级说明

| 等级 | 含义 | 行动 |
|------|------|------|
| 🔴 P0 | 必须立即修复 | 编译错误、无认证、数据不完整 |
| 🟡 P1 | 应该修复 | 竞态、内存泄漏、安全风险、核心功能缺失 |
| ⚠️ P2 | 功能缺失 | 协议不完整、配置化不足 |
| 🟢 P3 | 优化 | 性能、代码质量、非核心功能 |

+++
title = "TODO"
weight = 204
+++

# TODO

> **⚠️ 说明：本文档为功能探索性梳理，参考了行业标准（Alexa+、Gemini、Siri AI）和参考项目实现（xiaozhi-esp32-server、xiaozhi-esp32-server-java、xiaozhi-server-go）。大部分条目不会全部实现，仅作为 chobits 定位和取舍的参考依据。实际开发优先级以项目 Roadmap 为准。**

按功能模块分类的待办事项清单。修复前请阅读 [AGENTS.md](https://github.com/anomalyco/chobits/blob/main/AGENTS.md) 了解开发规范。

## 安全 & 认证

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🔴 P0 | WS 认证 | `api/src/ws/mod.rs` | WS handler 未应用认证层，所有 WS 连接未认证 | 建议: `axum-jwt-auth` — JWT 中间件，JWKS 缓存 | |
| 🔴 P0 | Invite Code 系统 | 新功能 | 无邀请码生成/验证/管理，新用户注册无控制 | 建议: `nanoid` — 短 URL 安全 ID 生成器 | |
| 🟡 P1 | Rate Limiting | `api/src/auth.rs` | 无登录限流/暴力破解防护 | 建议: `tower-governor` + `governor` — GCRA 算法 | |
| 🟡 P1 | Refresh 吊销 | `api/src/auth.rs` | refresh token 无吊销机制，logout 仅客户端清除 | 已有: `redis-rs` + `sea-orm` | |
| 🟡 P1 | Token 日志 | `api/src/auth.rs` | access token 明文记录在 tracing span | 已有: `tracing` | |
| 🟡 P1 | OTA 设备激活 | `api/src/ota.rs` | `activate` 端点为 stub，返回 "success" 但不验证设备，设备信息未存 DB | — | |
| 🟡 P1 | MCP 认证 | `api/src/mcp/mod.rs` | `/mcp` 端点认证被注释掉 | 已有: `rmcp` | |

## 语音输入与处理

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🔴 P0 | Wake Word 支持 | `api/src/ws/` + 协议层 | ESP32 客户端已有 ESP-SR 离线唤醒，服务端需处理 `wake_word` 消息类型，当前协议无此字段。行业标配（Echo/Google/Xiaozhi 均有），响应延迟 <200ms | — (ESP32 端 ESP-SR，服务端仅协议解析) | |
| 🟡 P1 | 声纹识别 (Voiceprint) | 新功能 | 参考项目 xinnan-tech 已实现（3D-Speaker 模型），与 ASR 并行处理，识别说话人身份传递给 LLM 实现个性化回复。sherpa-onnx 已支持 speaker identification（ECAPA-TDNN/WeSpeaker），无需新依赖。黑客365 Go 版使用 Qdrant 向量库 + 动态 TTS 声音切换，架构更成熟。需：注册/管理/识别流程 + DB 存储声纹向量 + LLM 上下文注入 | 已有: `sherpa-onnx` (ECAPA-TDNN/WeSpeaker) | |
| 🟡 P1 | Opus 除零 | `api/src/ws/default_listener.rs` | channels=0 / sample_rate=0 时除零 | — | |
| 🟡 P1 | 音频热路径克隆 | `api/src/ws/default_listener.rs` | 每 20ms `data.to_vec()` 频繁克隆 | — | |
| 🟡 P1 | 分层轮次检测 | 新功能 | 4 层架构替代纯 VAD：Layer 1 Silero VAD（<1ms/帧）→ Layer 2 持续时间检查（min_silence_ms）→ Layer 3 标点/语义完整性（`.?!` 立即提交，无标点延长窗口）→ Layer 4 填充词检测（uh/um 延长 reopen）。LiveKit Turn Detector v1.0（2026.06）实测 300ms 延迟下 9.9% 误切率（vs Deepgram 12.9%）；OpenAI 用 `silence_duration_ms=500` + `prefix_padding_ms=300` 配置服务端 VAD。当前 chobits 仅 Layer 1（Earshot Silero），缺少语义层。arxiv 2606.13450 展示可提前 2.56s 预测端点，减少 505ms 平均延迟 | 建议: LiveKit `v1-mini`（量化版 CPU 可推理）; OpenAI 配置模式; arxiv 2606.13450 Endpoint Anticipation | |
| 🟡 P1 | 动态 VAD 参数 | `api/src/ws/default_listener.rs` + `vad/` | **当前瓶颈**：VAD 参数在配置时固定，`silence_voice_timeout=1200ms` 固定。OpenAI Realtime 用 `threshold=0.5` + `prefix_padding_ms=300` + `silence_duration_ms=500` 配置，LiveKit 支持 `update_options()` 运行时修改。**方案**：助理刚说完→缩短 silence 到 300ms（快速回复）；用户长发言→延长 max_speech_ms；能量预过滤静音帧跳过 VAD 推理（省 CPU）。vui 通过命令队列实时修改 `stop_secs`，speech-to-speech 有 RuntimeConfig | 参考: vui `asr_worker.py:198-206`; speech-to-speech `RuntimeConfig`; OpenAI `server_vad` 配置 | |
| 🟡 P1 | ASR Settle 延迟 + Speculative Reopen | `api/src/ws/default_listener.rs` | **当前瓶颈**：VAD silence 后立即 ASR，可能丢失尾部音素；无 turn reopen 机制。**方案**：(1) 分层提交：silence→等 120ms 让最后中间结果到达→标点检查→`.?!` 立即提交，无标点额外等 700ms trailing-off（vui tiered_commit 实测：干净句子 420ms，trailing off 1120ms）；(2) Speculative Reopen：参考 HF speech-to-speech 的 SpeculativeTurnTracker，64ms 静默即软结束开始 ASR/LLM，turn 保持 reopenable 1s，用户重新说话→revision+1 丢弃旧工作。**防止误提交**：用户说"嗯...其实..."时不会过早触发回复 | 参考: vui `voice_turn.py:696-771`; HF speech-to-speech `speculative_turns.py` | |
| ⚠️ P2 | 填充词/犹豫检测 | `api/src/ws/default_listener.rs` | ASR 输出 `uh/um/呃/嗯` 等填充词 + 短音频（<3s）→ 丢弃不触发 LLM。**方案**：(1) 轻量：ASR 后处理检查末尾填充词（vui `_ends_with_filler`，支持 `uh/um/uhhh/ummm` 变体）；(2) 音频级：desert-ant-labs `uhm` 项目，DistilHuBERT 分类器，iPhone 169-296x realtime，6 类无需 ASR；(3) 训练级：disfluency LoRA 微调 whisper-large-v3-turbo，75% filler 召回率。chobits 推荐方案 (1)，与分层轮次检测的 Layer 4 结合 | 参考: vui `_ends_with_filler` `voice_turn.py:25-35`; desert-ant-labs/uhm; disfluency LoRA | |
| ⚠️ P2 | VAD 采样率 | `api/src/vad/` | 硬编码 16kHz，非 16kHz 输入无声失败 | — | |
| ⚠️ P2 | ASR | `api/src/asr/` | XAsr (sherpa-onnx)，无 `Sync` trait，仅 16kHz 单声道 | 已有: `sherpa-onnx` | |
| ⚠️ P2 | 环境监听模式 | 新功能 | 医疗 AI（Nuance DAX/Nabla）的被动监听模式：非唤醒词触发，持续监听环境音频，主动响应用户需求。需隐私架构（Nabla 模式：不存储原始音频）。适合家庭/办公场景 | — | |
| 🟢 P3 | Speaker Diarization | `api/src/listener/` | 说话人分离，多人场景下区分不同用户。sherpa-onnx 已支持（ECAPA-TDNN + AHC 聚类） | 已有: `sherpa-onnx` / 建议: `polyvoice` | |
| 🟢 P3 | AEC 服务端降噪 | `api/src/ws/default_listener.rs` | joey-zhou 已实现 WebRTC AEC3 服务端回声消除（含噪声抑制 + 高通滤波 + 自适应增益）。chobits 仅客户端 AEC | 建议: `aec3` — 纯 Rust WebRTC AEC3 | |
| 🟢 P3 | WebRTC 实时音视频 | 新功能 | 参考项目 dairoot/xiaozhi-webrtc 实现 WebRTC 低延迟 + Live2D + 多模态视觉 + MCP。chobits 当前仅 WS 协议 | 建议: `webrtc-rs` (v0.17.x) | |
| 🟢 P3 | 音频标准化集成 | `api/src/util/compressor.rs` → 管道 | `adaptive_normalize()` 已实现但未集成到 TTS 输出管道 | — | |
| 🟢 P3 | 引导式推理对话 | 新功能 | 教育 AI（作业帮/有道）的引导模式：不直接给答案，逐步引导用户思考。适合儿童/学习场景，需 LLM prompt 工程 + 对话状态管理 | — | |

## 语言模型与推理

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🔴 P0 | LLM 线程安全 | `api/src/llm/model/qwen3/mod.rs` | `thread::spawn` + `block_on`，未 `catch_unwind`，panic 静默崩溃 | — | |
| 🔴 P0 | LLM Echo 线程 | `api/src/llm/model/echo/mod.rs` | 同上 | — | |
| 🟡 P1 | 情绪识别完善 | `api/src/llm/model/` (analyze_emotion) | 当前 stub 返回 "happy"。行业方案：音频特征 (wav2vec2 SER) + 文本情感 (GoEmotions) 双通道融合，用于调整 TTS 语气和回复风格 | 已有: `sherpa-onnx` (SER 模型) | |
| 🟡 P1 | 个性化记忆/长期偏好 | 新功能 | 当前仅 chat history，无长期偏好存储。参考项目 xinnan-tech 有 PowerMem（用户画像 + 艾宾浩斯遗忘曲线 + 向量检索）；joey-zhou 有 3 种记忆模式（window/summary/long + 图检索） | 建议: `qdrant` + `rig-core` — 向量 DB + RAG 框架 | |
| 🟡 P1 | RAG 知识库 | MCP 或内置模块 | 参考项目 xinnan-tech 集成 RAGFlow；joey-zhou 有 EmbeddingModelFactory + 图检索。当前 MCP 框架可接入但无内置向量检索 | 已有: `rig-core` (10+ 向量存储后端) | |
| 🟡 P1 | Intent 识别 | 新功能 | 参考项目 xinnan-tech 支持 3 种模式：function_call（推荐）、intent_llm（专用 LLM）、nointent。当前 chobits 无独立 intent 层 | 已有: `rmcp` (function_call) | |
| 🟡 P1 | LLM 历史阻塞 | `api/src/llm/model/qwen3/mod.rs` | DB 落盘导致完整线程阻塞 | — | |
| ⚠️ P2 | Agent 任务编排 | 新功能 | 参考项目 Alexa+ 自主执行 Uber/OpenTable/Grubhub；Rabbit R1 LAM 大动作模型；Doubao 超能模式自主分解复杂任务。LLM + MCP 工具链实现自主任务执行 | 已有: `rig-core` (Agent/Chain/Router) | |
| 🟢 P3 | describe O(n) | `api/src/llm/model/qwen3/mod.rs` | 实时构建全消息历史 | — | |

## 语音合成 (TTS)

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🟡 P1 | Piper/Kokoro TTS 集成 | `api/src/tts/` | 开源 TTS 替代方案：Piper（20M 参数/MIT/CPU 55ms 延迟/30+ 语言）和 Kokoro（82M/Apache 2.0/CPU 实时/54 声音）。可替换或补充当前 MatchaTTS，Piper 适合边缘部署，Kokoro 是最佳质量/体积比 | 建议: `sherpa-onnx` (Piper ONNX 模型) | |
| 🟡 P1 | 两阶段流式 TTS 架构 | `api/src/chii/splitter.rs` + `round.rs` + `api/src/tts/mod.rs` | **当前瓶颈**：Splitter 严格按 `。！？!?` 拆分，无子句拆分；MatchaTTS `generate_with_config` 一次性生成整句音频（callback 参数为 `None`，sherpa-onnx 实际支持流式 callback）。arxiv 2603.05413 实测流水线并行 TTFA 755ms（vs 串行 26.5s → 17x 改善）。**两阶段架构**（参考 Qwen3-TTS-streaming）：Phase 1 token 级首包 — LLM 第一个 token 到达即触发 TTS，快速出首包（3-5 词），降低 TTFA；Phase 2 句子级稳态 — 后续 15-20 词或 break_chars 兜底，吞吐优先。TTS 改用 callback 模式边生成边 Opus 编码边发送，取消时 callback 返回 false 中断。核心原则：延迟从 `STT+LLM+TTS` 变为 `max(STT,LLM,TTS)` | 参考: Qwen3-TTS-streaming 两阶段架构; vui `engine.py:73` `chunk_words`; arxiv 2603.05413; sherpa-onnx callback API | ✅已完成 2026-07-23 |
| 🟡 P1 | 音频 Hold Buffer + Fade-out | `api/src/tts/` | TTS 输出音频尾部 abrupt cutoff 产生 click。最后 N 帧（~240ms）缓存，尾部 200ms 做线性淡出消除杂音，参考 vui `tts_worker.py:713-783` 的 hold buffer + fade-out 实现 | 参考: vui `tts_worker.py:713-783` | |
| 🟡 P1 | 多语言 TTS | `api/src/tts/` | 当前仅单语言 TTS voice。ESP32 客户端已支持 25+ 语言 ASR，TTS 侧需匹配 | 建议: `sherpa-onnx` (Piper/VITS ONNX 模型) | |
| 🟡 P1 | Quick Reply 预回复 | 新功能 | LLM 推理期间先播放"我在"/"来了"等短语，降低感知延迟。参考项目黑客365 Go 版已实现，UX 关键，实现简单 | — | |
| 🟡 P1 | 动态 TTS 声音切换 | 新功能 | 基于声纹识别自动切换不同 TTS 音色。参考项目黑客365 Go 版已实现（sherpa-onnx 声纹 + per-speaker TTS voice），声纹识别的自然延伸 | 已有: `sherpa-onnx` (声纹+TTS 切换) | |
| 🟡 P1 | Hann Crossfade 防 click | `api/src/tts/mod.rs` | **当前问题**：TTS chunk 边界产生 click/pop 杂音。**方案**：Hann 窗 crossfade（512 samples @ 16kHz = 32ms），首包 fade-in、末包 fade-out、中间 crossfade。参考 Qwen3-TTS-streaming 项目（89 stars），业界标准做法。Overlap trimming 流程：crossfade 当前 HEAD 与前一个 TAIL → 保存完整 chunk → trim END before emission。实现位置：`StreamingOpusEncoder` 内部维护 `prev_tail: Vec<f32>`，每次 encode 时 crossfade | 参考: Qwen3-TTS-streaming `overlap_samples=512`; open-unified-tts 30-50ms crossfade | |
| 🟡 P1 | 首包静音裁剪 + 40ms preroll | `api/src/tts/mod.rs` | **当前问题**：TTS 输出首包有 leading silence，增加感知延迟。**方案**：1% amplitude threshold 检测首包静音，裁剪后保留 40ms preroll（软起音避免 abrupt start）。参考 speech-to-speech 项目 `trim_silence()`。预期减少感知延迟 50-200ms。实现位置：`TtsMatcha::stream()` 中 Opus 编码前增加 `trim_leading_silence()` 步骤 | 参考: speech-to-speech `trim_silence()`; Dupdub TTS latency optimization | |
| 🟡 P1 | Splitter 升级：sentencex 集成 | `api/src/chii/splitter.rs` | **当前问题**：Splitter 仅按 `。！？!?` 简单正则分割，无缩写处理（"Dr." "Mr." 被错误断句）、无 context lookahead（"$29." 被错误断句）、无 minimum sentence length。**方案**：使用 `sentencex` crate（Wikimedia，136 stars，MIT），支持 200+ 语言包括中文，手工编译缩写列表，英文 Golden Rule Set F1=100.00（NLTK 仅 72.33）。`cargo add sentencex` 即可集成。替换现有 `Splitter` 的正则逻辑 | 建议: `sentencex` — Rust 纯实现，Wikimedia 维护 | |
| 🟡 P1 | TTFA/RTF 性能测量 | `api/src/tts/mod.rs` | **当前问题**：无 TTS 性能可观测性。**方案**：记录每请求 TTFA（time-to-first-audio）和 RTF（real-time-factor），持久化原始样本计算 P50/P90/P95 百分位。P95 决定 silence_timeout 配置。实现：在 `TtsMatcha::stream()` 中记录 `Instant::now()` 到首次 Opus 帧输出的时间差 | 参考: Dupdub TTS latency optimization; Sherlock Calls P95 monitoring | |
| 🟡 P1 | Opus-to-PCM 解码预缓冲 | `service/src/chobits/session/round.rs` | **当前问题**：客户端收到 Opus 帧后立即解码播放，无预缓冲。网络抖动导致播放卡顿。**方案**：服务端或客户端增加 Opus→PCM 解码层，预缓冲 N 帧（~200ms）后再播放，平滑网络抖动影响。参考 speech-to-speech 项目 leftover sample carry 机制。ESP32 端可利用 Opus decoder 已有实现；WS 端需评估是否在服务端预解码（增加带宽但降低客户端复杂度） | 参考: speech-to-speech `leftover_samples` carry; sherpa-onnx Opus decoder | |
| ⚠️ P2 | 句间静音优化 | `api/src/tts/` | TTS 句间固定静音。改为按标点类型动态调整：逗号 0.3s，句号 0.6s，其他 0.3s，让对话节奏更自然 | 参考: RealtimeVoiceChat `ENGINE_SILENCES` `audio_module.py:22-26` | |
| ⚠️ P2 | 对话韵律 TTS | 新功能 | 参考项目 Sesame CSM（Apache 2.0 开源）生成呼吸/犹豫/笑声等对话韵律，让语音更像真人。Cartesia Sonic 3.5 使用 SSM 架构实现 <90ms TTS 延迟 | 建议: `csm.rs` — Rust Sesame CSM (AGPL-3.0) | |
| ⚠️ P2 | 情绪自适应语调 | 新功能 | 参考项目 Hume AI EVI 检测 600+ 情感标签（犹豫/讽刺/宽慰），自适应调整 TTS 语气。MiniMax Speech-2.8 支持 7 种情绪 + 0-100% 强度控制 + 插入标签 `(laughs)` `(sighs)` | 建议: `voirs-emotion` — 多维情绪控制 | |
| 🟢 P3 | 声音克隆 | `api/src/tts/` | 参考项目 xinnan-tech 支持火山引擎语音克隆；joey-zhou 支持按角色声音克隆。MatchaTTS 已支持 reference audio，需暴露配置接口 | 建议: `sherpa-onnx` (speaker embedding) | |
| 🟢 P3 | TTS 循环克隆 | `api/src/tts/` | `Arc<str>` vs `String` 克隆风暴 | — | |

## 工具与集成

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🟡 P1 | 家居集成 (Home Assistant) | MCP 或独立模块 | 参考项目 xinnan-tech 有 3 种 HA 集成方式（社区插件/HA 作为 LLM 工具/HA MCP Server）。智能家居是语音助手核心场景 | 已有: `rmcp` (HA MCP Server) | |
| 🟡 P1 | 设备间通话 | MCP tool 或独立功能 | 参考项目 xinnan-tech 有 `call_device.py`，ESP32 设备间可像电话一样互相呼叫，需 MQTT gateway + 通讯录管理 | 建议: `rumqttc` — 纯 Rust MQTT 客户端 | |
| 🟡 P1 | 音乐播放 | MCP tool 或独立功能 | 参考项目 xinnan-tech 有 `play_music.py` + `hass_play_music.py`；joey-zhou 有 `MusicPlayer` 支持 LRC 歌词同步。行业标配 | 建议: `rodio` — 跨平台音频播放 | |
| 🟡 P1 | Timer/提醒/闹钟 | MCP tool 或独立功能 | 行业标配（"Alexa, set a timer"），当前无任何定时/提醒机制 | 建议: `tokio-cron-scheduler` — Tokio 异步 cron | |
| ⚠️ P2 | MCP 认证 | `api/src/mcp/mod.rs` | `/mcp` 端点认证被注释掉 | 已有: `rmcp` | |
| ⚠️ P2 | 插件系统 | 新功能 | 参考项目 xinnan-tech 有 13 个内置插件 + 热加载机制。chobits 无插件架构，功能扩展需修改核心代码 | 建议: `wasmtime` — WASI P2 + Component Model | |
| ⚠️ P2 | MCP Market / 工具市场 | 新功能 | 参考项目黑客365 Go 版实现 MCP 工具"应用商店"：聚合多第三方市场（如 ModelScope），一键导入远程 MCP 服务 + 热加载。chobits 当前无 MCP 工具发现/聚合机制 | 已有: `rmcp` | |
| ⚠️ P2 | MCP 调试控制台 | 新功能 | 参考项目黑客365 Go 版有 Agent/Device 维度 MCP 远程调试：Web 控制台生成每 Agent 独立 MCP 端点，实时调用测试，支持 per-agent 工具过滤。chobits 当前无 MCP 调试工具 | 已有: `rmcp` | |
| ⚠️ P2 | MCP 工具聚合 | 新功能 | 参考项目 xiaozhi-mcp / yuexianga/xiaozhi-mcp 提供预置工具库（钉钉/QQ/系统监控/WebPilot/数学计算），开箱即用。chobits 无内置 MCP 工具包 | 已有: `rmcp` | |
| ⚠️ P2 | MQTT 网关 | 新功能 | 参考项目 xinnan-tech/xiaozhi-mqtt-gateway 实现 MQTT+UDP → WS 桥接：分布式部署 + 动态负载均衡 + HMAC 认证 + MCP 命令下发。chobits 当前仅 WS 单协议 | 建议: `rumqttc` — 纯 Rust，tokio 原生 | |
| ⚠️ P2 | 配置向导+全链路测试 | 新功能 | 参考项目黑客365 Go 版有首次运行向导（OTA/VAD/ASR/LLM/TTS 逐步配置）+ 每组件延迟测试 + 可视化图表。chobits 当前无部署向导 | — | |

## 会话与设备

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🔴 P0 | stop_round 竞态 | `service/src/chobits/session/round.rs` | `llm_tts_handle` 与 `stop_round` 之间缺少同步，可能 use-after-cancel | — | |
| 🟡 P1 | Continued Conversation | `service/src/chobits/session/` | 回复后麦克风应短暂保持开放，允许用户免唤醒词追问。Gemini / Alexa+ 均支持 | — | |
| 🟡 P1 | 时钟溢出 | `service/src/chobits/session/mod.rs` | `Local::now()` 非单调，减法可溢出 | 已有: `jiff` (单调时钟) | |
| 🟡 P1 | 设备管理 | 新功能 | 无设备注册/绑定/列表，OTA 激活后无设备持久化。参考项目有完整设备生命周期管理（注册/状态/配置/OTA/批量操作） | 已有: `sea-orm` | |
| 🟡 P1 | Recorder 无上限 | `api/src/record/recorder.rs` | `Vec<RecordEntry>` 无大小限制，高并发内存无限增长 | — | |
| 🟢 P3 | Session 导出/删除 | `api/src/record/` | Session 仅可查看，不可导出或删除 | — | |

## 协议与传输

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| ⚠️ P2 | 消息类型 | `api/src/ws/frame.rs` | 缺失 `system`、`alert`、`custom`、`wake_word` 消息类型（对比 xiaozhi-esp32 规范） | — | |
| ⚠️ P2 | 多 ASR/TTS Provider | `api/src/asr/` + `api/src/tts/` | 参考项目 xinnan-tech 支持 12 ASR + 18+ TTS provider（含免费 EdgeTTS）；joey-zhou 有 7 STT + 8 TTS。chobits 仅 1 ASR + 1 TTS | 已有: `sherpa-onnx` (多模型切换) | |

## 基础设施

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🔴 P0 | Signal 宏 | `framework/src/signal.rs` | 使用不存在的 `debug_error!` 宏，非 unix 编译失败 | — | |
| 🟡 P1 | email 约束 | `migration/src/m20241230_000001_init.rs` | entity 标注 `#[sea_orm(unique)]`，迁移未实现 UNIQUE | 已有: `sea-orm` (migration fix) | |
| 🟡 P1 | 外键约束 | `migration/src/m20241230_000001_init.rs` | 缺少 FK: `round.session_id`、`round_data.round_id`、`frame.round_id` | 已有: `sea-orm` (migration fix) | |
| 🟡 P1 | MCP 锁顺序 | `api/src/mcp/mcp_host.rs` | UnionMcpHost device/server 锁顺序 ABBA，可能死锁 | — | |
| 🟡 P1 | 优雅关闭顺序 | `framework/src/signal.rs` + `apps/server` | 各模块关闭顺序缺失 | — | |
| 🟡 P1 | Panic 处理 | `framework/src/panic.rs` | `eprintln!` 而非 `tracing::error!`，绕过 Sentry | 已有: `tracing` | |
| 🟡 P1 | Runtime 竞态 | `framework/src/runtime.rs` | `OnceLock` 初始化存在竞态 | — | |
| 🟢 P3 | 时间戳自动填充 | `entity/src/config.rs` | `Config` 实体缺失 `ActiveModelBehavior`，时间戳未自动填充 | 已有: `sea-orm` | |
| 🟢 P3 | MCP 错误处理 | `api/src/mcp/` | 不完善 | 已有: `rmcp` | |

## 前端 (apps/server-ui)

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🟡 P1 | Dashboard 页 | `routes/_pathlessLayout.admin/index.tsx` | 空壳，仅渲染 "Hello"，无统计/监控内容 | 已有: `@mantine/core` v9 + `@tanstack/react-query` | |
| 🟡 P1 | User CRUD 管理 UI | 新页面 | 无用户列表/创建/删除/角色管理界面 | 已有: `@mantine/core` v9 | |
| 🟡 P1 | 多用户/RBAC 管理 | 新功能 | 参考项目 busy-worker Java 管理平台有 Token 用量监控 + 对话时长 + 设备活跃度 + 数据可视化 + RBAC。chobits Dashboard 应包含 | 已有: `@mantine/core` v9 | |
| 🟢 P3 | 系统监控 | 新页面 | 无服务器健康/连接数/资源使用/错误率仪表盘 | 已有: `@mantine/core` v9 + `@tanstack/react-query` | |
| 🟢 P3 | MCP 仪表盘 | 新功能 | 参考项目 xiaozhi-mcphub 有 React 前端：多端点管理 + 工具同步 + 群组访问控制 + 日志。chobits MCP 管理参考 | 已有: `@mantine/core` v9 | |

## 移动端 (apps/app)

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🟡 P1 | Flutter WS 集成 | `apps/app/` | Flutter app 存在脚手架但未集成 WS + 认证 | 建议: `web_socket_channel` | |
| 🟡 P1 | App CI/CD | `.github/workflows/` | 无 iOS/Android 构建/签名/发布流水线 | GitHub Actions | |

## 测试

| 优先级 | 项目 | 位置 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|------|------|
| 🔴 P0 | WS 认证测试 | `apps/server/api/tests/` | WS 端点零认证，无对应测试 | 已有: `axum` test utils + `reqwest` | |
| 🔴 P0 | Wake Word 测试 | `apps/server/api/tests/` | Wake word 消息处理无测试，需验证协议解析和 session 唤醒流程 | 已有: `axum` test utils | |
| 🟡 P1 | Auto 模式 + AEC 测试 | `apps/server/api/tests/session/` | Auto 模式（barge_in=false）在有/无 AEC 场景下无专门测试 | 已有: `axum` test utils | |
| 🟡 P1 | Button Talk 测试 | `apps/server/api/tests/session/` | Manual 模式（push-to-talk）缺少端到端测试 | 已有: `axum` test utils | |
| 🟡 P1 | Continued Conversation 测试 | `apps/server/api/tests/session/` | 回复后麦克风保持开放的追问流程无测试 | 已有: `axum` test utils | |
| 🟡 P1 | Rate Limiting 测试 | `apps/server/api/tests/` | 登录限流功能不存在，无测试 | 已有: `axum` test utils | |
| 🟡 P1 | 情绪识别测试 | `apps/server/api/tests/` | analyze_emotion stub 无测试覆盖 | — | |
| 🟡 P1 | 声纹识别测试 | `apps/server/api/tests/` | 声纹注册/识别/LLM 注入流程无测试 | — | |
| 🟡 P1 | 音乐播放测试 | `apps/server/api/tests/` | 音乐播放功能无测试 | 已有: `axum` test utils | |

## 待确认 / 探索

| 项目 | 描述 | 开源方案/类库 | 状态 |
|------|------|------|------|
| 声纹方案选择 | 3D-Speaker (xinnan-tech 使用) vs sherpa-onnx speaker ID (已在技术栈中) vs pyannote。sherpa-onnx 最佳：无需新依赖，支持 ECAPA-TDNN/WeSpeaker/CAM++ 多种模型 | 已有: `sherpa-onnx` | |
| 插件架构设计 | xinnan-tech 有 13 内置插件 + 热加载。chobits 是否需要类似机制，还是通过 MCP 扩展即可？ | 建议: `wasmtime` — WASI 沙箱 + Component Model | |
| 多 Provider 扩展策略 | 参考项目支持 12+ ASR / 18+ TTS provider。chobits 是否需要多 provider 架构，还是保持精简？ | 建议: trait 抽象 (`AsrProvider`/`TtsProvider`) | |
| 音乐播放 | 行业标配（Spotify/Apple Music 集成），需确认实现方式：MCP tool 调用外部 API / 内置音频播放 / TTS 扩展 | 建议: `rodio` | |
| ESP32 CI/CD | 独立项目 (xiaozhi-esp32)，非本仓库范围，chobits 作为后端提供 WS + OTA 接口即可 | — | |
| Token 持久化存储 | 考虑用 DB 表存储 refresh token 支持多设备管理 | 已有: `sea-orm` + `redis-rs` | |
| Smart Home 集成 | Alexa+ / Google Home 核心能力。参考项目有 3 种 HA 集成方式，需评估 chobits 作为 hub 的定位 | 已有: `rmcp` (HA MCP) | |
| Agentic 能力 | Alexa+ "Experts" / Gemini Spark 24/7 代理，可自主浏览网页/填表/下单。LLM + MCP 工具调用已有基础，需评估实现深度 | 已有: `rig-core` | |
| 语音情感 TTS | Gemini 2.5 / Cartesia Sonic-3 支持根据情绪调整 TTS 语气。MatchaTTS 需评估是否支持 style/prosody 控制 | 建议: `voirs-emotion` | |
| 本地隐私处理 | 行业趋势：Echo/Fire 设备向本地处理迁移。chobits 已有 local Qwen3，可扩展 local ASR/TTS 全链路 | 已有: `sherpa-onnx` + `candle` | |
| 视觉能力 | 参考项目 xinnan-tech 支持 VLLM（GLM-4V/Qwen-VL）拍照识物。chobits 是否需要视觉能力？ | 建议: `reqwest` → Ollama/vLLM | |
| MCP Market 架构设计 | 参考项目黑客365 Go 版实现 MCP 工具"应用商店"（聚合多市场+热加载）。chobits 是否需要 MCP 工具发现/聚合机制？ | 已有: `rmcp` | |
| 动态 TTS 声音切换方案 | 参考项目黑客365 Go 版基于声纹识别自动切换 TTS 音色。chobits 声纹识别后如何联动 TTS？ | 已有: `sherpa-onnx` | |
| 配置向导设计 | 参考项目黑客365 Go 版有首次运行向导 + 全链路延迟测试。chobits 是否需要部署向导降低使用门槛？ | — | |
| MQTT 网关架构 | 分布式部署需要 MQTT+UDP 桥接 + 动态负载均衡（参考 xinnan-tech/xiaozhi-mqtt-gateway）。chobits 是否需要 MQTT 网关层？ | 建议: `rumqttc` | |
| MCP 工具聚合策略 | 参考项目有预置工具库（钉钉/QQ/系统监控/WebPilot/数学计算等），chobits 是否需要内置 MCP 工具包？ | 已有: `rmcp` | |
| 端到端 S2S 架构 | OpenAI/Hume/Sesame 使用单模型处理音频输入输出（非级联 STT→LLM→TTS），chobits 当前级联方案是否需要演进？ | 建议: `csm.rs` (AGPL-3.0) 或 `moshi` (Apache 2.0) | |
| 语义 VAD 实现方案 | OpenAI 的模型级轮替检测 vs 传统 VAD，chobits 如何实现更智能的中断/轮替判断？ | 建议: `wavekat-vad` | |
| 对话韵律 TTS | Sesame CSM 开源模型（Apache 2.0）可直接使用，chobits TTS 是否集成呼吸/犹豫/笑声等韵律？ | 建议: `csm.rs` (AGPL-3.0) | |
| 隐私策略设计 | Always-listening 设备的隐私架构（Bee 无音频存储 / Omi 本地处理），chobits 如何平衡功能与隐私？ | 已有: `sherpa-onnx` (全链路本地) | |
| SSM 架构 TTS | Cartesia 使用 State Space Model 替代 Transformer 实现 <90ms TTS，chobits TTS 是否考虑 SSM 架构？ | — (无 Rust 实现) | |
| Piper/Kokoro 评估 | 开源 TTS 模型 Piper（20M 参数/MIT）和 Kokoro（82M/Apache 2.0）是否适合替换或补充当前 MatchaTTS？Piper 适合边缘部署，Kokoro 是最佳质量/体积比 | 建议: `sherpa-onnx` (Piper ONNX) | |
| Step-Audio-TTS-3B 集成 | 阶跃星辰开源中文 TTS（Apache 2.0、情绪控制、方言支持），是否作为服务端 TTS 候选？需评估 GPU 需求和延迟 | 建议: `reqwest` → StepFun API | |
| 环境监听架构 | 医疗 AI 的被动监听模式（非唤醒词），chobits 是否支持？隐私如何保障？参考 Nabla（不存储原始音频）架构 | — | |
| Matter 协议支持 | 智能家居 Hub 标准协议（Matter 1.3+Thread 1.4），ESP32 已有 Thread 支持。chobits 是否需要完整 Matter SDK 集成以实现跨平台设备兼容？ | 建议: `rs-matter` | |
| Proactive 主动建议 | Gemini Daily Brief / Alexa+ 主动提醒交通/降价/日程。需定时任务 + 用户上下文推理 | 建议: `tokio-cron-scheduler` | |
| 跨设备连续性 | Alexa+: Echo→手机→电脑无缝切换对话上下文。需 session 状态同步机制 | — (需自定义：SQLite + WS delta 同步) | |
| UGC 角色市场 | 参考项目 Character.AI 有 1000万+ 用户创建角色；Doubao 智能体平台支持无代码创建 AI 角色。chobits 可支持用户自定义 AI 人格 + 声音 + 背景故事 | — | |
| 多模态（语音+屏幕+视频） | Gemini 2.5 / GPT-Realtime / Siri AI 均支持摄像头/屏幕输入。chobits 当前纯语音 | 建议: `webrtc-rs` (v0.17.x) | |

---

## 附录 B：开源语音模型

> 开源 TTS/语音模型，按质量/体积/许可证排序。chobits 可直接集成。

| 模型 | 参数量 | 许可证 | 延迟 | 声音克隆 | 适合 chobits |
|------|--------|--------|------|----------|-------------|
| [Piper](https://github.com/rhasspy/piper) | ~20M | MIT | 55ms（10s 音频） | 否 | ★★★★★ CPU 运行，30+ 语言，边缘部署首选 |
| [Kokoro v1.0](https://github.com/hexgrad/kokoro) | 82M | Apache 2.0 | CPU 实时 | 否（KokoClone 扩展支持） | ★★★★★ 最佳质量/体积比，54 声音 |
| [Step-Audio-TTS-3B](https://github.com/stepfun-ai/Step-Audio-TTS-3B) | 3B | Apache 2.0 | 需 GPU | 是 | ★★★★ 最佳中文语音质量+情绪控制+方言 |
| [Coqui XTTS v2](https://github.com/coqui-ai/TTS) | 467M | CPML（非商用） | <200ms | 是（6s 样本） | ★★★★ 最佳克隆，17 语言，但许可证限制 |
| [F5-TTS](https://github.com/SWivid/F5-TTS) | 335M | CC-BY-NC | 需 GPU | 是（零样本） | ★★★ 克隆质量优秀，中英混杂，但非商用 |
| [Orpheus TTS](https://github.com/canopylabs-ai/orpheus-tts) | 3B | — | 需 GPU | 否 | ★★★ Llama 骨干，风格控制 |
| [Bark (Suno)](https://github.com/suno-ai/bark) | — | MIT | 0.8x（太慢） | 否 | ★★ 表现力强（笑声/叹气/音乐），但实时性差 |

### 推荐架构

```
ESP32 (边缘)                    服务器 (chobits)
┌──────────┐                ┌─────────────────────┐
│ VAD      │                │ ASR: sherpa-onnx     │
│ Piper/   │◄── WebSocket ──│ LLM: Qwen3 candle    │
│ Kokoro   │                │ TTS: Kokoro/         │
│ (本地    │                │ Step-Audio-TTS-3B    │
│  TTS)    │                └─────────────────────┘
└──────────┘
```

- **Piper 或 Kokoro** 用于 ESP32 边缘 TTS（轻量、快速、CPU 运行）
- **Kokoro** 用于服务端 TTS（最佳质量/体积比，Apache 2.0）
- **Step-Audio-TTS-3B** 用于服务端高质量中文语音（情绪控制、方言支持）

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

### 商业产品参考

> 闭源商业产品的关键技术和 UX 模式，供 chobits 取舍参考。

#### 语音 AI 平台

| 产品 | 关键技术 | 对 chobits 的参考 |
|------|----------|-------------------|
| OpenAI Realtime | 端到端 S2S、语义 VAD、WebRTC、~232ms 延迟 | 语义 VAD 是轮替/中断金标准；WebRTC 传输架构可参考 |
| Cartesia Sonic 3.5 | SSM 架构 TTS、<90ms 延迟、3 秒声音克隆 | SSM 是 TTS 新方向，比 Transformer 更快 |
| ElevenLabs | 语音克隆（1-25 样本）、Expressive Mode、11.ai MCP 语音助手 | 语音 + MCP 集成模式；Agent coaching/evaluation |
| MiniMax Speech-2.8 | 插入标签 `(laughs)` `(sighs)`、7 情绪 + 0-100% 强度、逐句情绪控制 | 简单实用的情绪表达方案，无需 SSML |
| Inworld AI | 15 秒声音克隆、文本描述生成声音、<130ms TTS | 最低门槛声音个性化 |
| Hume AI EVI | 600+ 情感标签、情绪自适应语调、检测犹豫/讽刺/宽慰 | 情绪检测 + 自适应语调是关键差异化 |
| Sesame CSM | 对话韵律（呼吸/犹豫/笑声）、开源 Apache 2.0 | 可直接使用的开源模型，让语音更像真人 |

#### 语音 AI 基础设施

| 产品 | 关键技术 | 对 chobits 的参考 |
|------|----------|-------------------|
| LiveKit / Pipecat | 开源 SFU 架构、50+ AI 模型集成、OpenAI ChatGPT 底层 | 语音 AI 基础设施标准，可用作传输层 |
| Retell AI | ~600ms 端到端（免调优）、专有轮替模型、BYO-LLM | 轮替检测是核心差异化 |
| Vapi | BYO-stack（自选 STT/LLM/TTS）、A/B 测试、1000+ 模板 | 可组合架构模式 |

#### 智能音箱 / 手机助手

| 产品 | 关键技术 | 对 chobits 的参考 |
|------|----------|-------------------|
| Amazon Alexa+ | 自主任务执行（Uber/OpenTable/Grubhub）、模型无关路由、跨设备连续性 | Agent 链式执行模式；chobits 可通过 MCP 实现 |
| Google Gemini for Home | 自然语言创建自动化（"Ask Home"）、AI 摄像头理解 | "描述自动化" UX 模式 |
| 小米超级小爱 | 声纹识别家庭成员、AI 通话代接、灵动气泡视觉反馈 | 多成员声纹 + 家庭功能 |
| 小度 | 蓝牙 Mesh 本地网关、无网控制、声音人格持久化 | 本地网关架构；声音人格是情感纽带 |
| 天猫精灵 | "1+3+N" 架构、空间智能体、边缘场景调度 | 空间智能体：从命令式到上下文感知 |
| 讯飞星火 | 一句话声音克隆、74+ 方言、端到端语音翻译 | 一句话克隆是杀手级功能 |
| 豆包 | 超能模式（自主任务分解）、100M+ DAU、Coze 智能体平台 | Agent 任务分解 + UGC 角色平台 |

#### AI 可穿戴

| 产品 | 关键技术 | 对 chobits 的参考 |
|------|----------|-------------------|
| Meta Ray-Ban | 76% 市场份额、音频优先、视觉感知、7+ 语言实时翻译 | 音频优先 + 视觉感知是成功模式 |
| Omi（开源） | $89、MIT 许可、250+ 社区 App、本地录制+云端同步 | 最接近的开源可穿戴参考 |
| Bee AI（Amazon） | $50、7 天电池、无音频存储隐私策略 | 低价 + 长续航 + 隐私优先 |
| Looki L1 | 30g 挂坠、12 小时电池、AI 环境感知 + 生活日志 | 最小形态 + 感知共鸣理念 |
| Limitless Pendant | 始终监听、说话人分离、MCP 服务器集成、100+ 语言 | 环境上下文捕获 + MCP 集成模式 |

#### AI 陪伴应用

| 产品 | 关键技术 | 对 chobits 的参考 |
|------|----------|-------------------|
| Character.AI | 1000万+ 用户角色、Lorebook 世界观、PipSqueak 2 模型 | UGC 角色市场 + 持久人格模式 |
| Replika 2.0 | 跨月记忆重建、主动提醒、AR 体验、视频通话 | 主动记忆驱动建议是强大 UX |
| ChatGPT Voice | 端到端音频模型、320ms 延迟、22 种神经 TTS 声音 | 消费级语音 AI 标杆 |
| Microsoft Copilot | Work IQ 持久记忆、Agent 365 治理、计算机使用代理 | 最成熟的企业级记忆系统 |

#### 关键行业趋势

| 趋势 | 代表产品 | chobits 机会 |
|------|----------|-------------|
| 语义 VAD | OpenAI（模型级轮替检测） | 比纯 VAD 更智能的中断处理 |
| 对话韵律 | Sesame CSM（呼吸/犹豫/笑声） | 让 TTS 更自然 |
| 端到端 S2S | OpenAI/Hume/Sesame | 终极目标，当前级联更实际 |
| 情绪自适应 | Hume AI（检测→自适应语调） | 语音助手关键差异化 |
| SSM 架构 TTS | Cartesia（State Space Model） | 更快的 TTS 推理 |
| 持久记忆 | Work IQ / Replika / Character.AI | 从 chat history 到长期记忆 |
| MCP 语音集成 | OpenAI + MCP、ElevenLabs 11.ai | 语音对话中调用工具 |
| 声音克隆民主化 | Cartesia 3 秒 / Inworld 15 秒 / MiniMax 10 秒 | 最低门槛声音个性化 |
| 隐私优先 | Bee（无音频存储）、Omi（本地处理） | Always-listening 的隐私策略 |

#### 垂直行业语音

> 汽车、医疗、教育、翻译等垂直行业的语音 AI 方案，供 chobits 垂直场景参考。

| 行业 | 代表产品 | 关键技术 | 对 chobits 的参考 |
|------|----------|----------|-------------------|
| 汽车语音 | NIO NOMI、小鹏天机、SoundHound | 边缘离线处理、14 种情绪语调、RAG 车辆手册、Speech-to-Meaning | 边缘离线是关键；情绪语调定制；RAG 参考 |
| 医疗语音 | Nuance DAX、Nabla、Abridge、Suki AI | 环境临床记录、隐私优先（不存储数据）、HIPAA 合规、引导推理 | 被动监听模式；隐私优先架构；引导式对话 |
| 教育语音 | Duolingo Max、作业帮 P50、有道子曰 | 逐步引导推理、个性化学习路径、游戏化、实时发音纠正 | 引导式对话模式；游戏化驱动参与 |
| 翻译设备 | Pocketalk、Timekettle、Vasco | 双麦降噪、<1s 延迟、离线翻译包、专用硬件 | 双麦降噪+低延迟是关键；离线能力 |
| 智能家居 Hub | Echo Hub、HomePod、SmartThings | Matter 1.3/Thread 1.4、离线语音（0.2-0.4s 本地）、预测性自动化 | ESP32 作为隐私优先本地控制器 |

#### 中国 AI 创业公司（2025-2026）

> 中国 AI 六小虎等新兴公司的语音 AI 技术，供 chobits 技术选型参考。

| 公司 | 关键技术 | 对 chobits 的参考 |
|------|----------|-------------------|
| [阶跃星辰 StepFun](https://github.com/stepfun-ai) | Step-Audio 2（130B S2S）、Step-Audio-TTS-3B、Apache 2.0 开源、情绪控制+方言 | **最相关**：最佳开源语音 AI，可直接使用 |
| [智谱 AI](https://github.com/THUDM) | GLM-4V 视觉模型、85% 收入来自本地部署、港股 IPO | 本地部署商业模式参考；视觉能力 |
| [MiniMax](https://github.com/MiniMaxAI) | 海螺 AI、星野、1.75 万亿周 Token 调用量、Speech-2.8 | 消费级 AI 产品运营参考；情绪 TTS |
| [月之暗面 Kimi](https://github.com/MoonshotAI) | Kimi K2.5 + Kimi Claw、估值 3 个月从 $4.3B→$18B | 超长上下文 + 语音 AI 结合 |
| [百川智能](https://github.com/baichuan-inc) | 医疗垂直 LLM、3B 现金储备 | 垂直行业深耕模式 |

---

## 优先级说明

| 等级 | 含义 | 行动 |
|------|------|------|
| 🔴 P0 | 必须立即修复 | 编译错误、无认证、数据不完整 |
| 🟡 P1 | 应该修复 | 竞态、内存泄漏、安全风险、核心功能缺失 |
| ⚠️ P2 | 功能缺失 | 协议不完整、配置化不足 |
| 🟢 P3 | 优化 | 性能、代码质量、非核心功能 |

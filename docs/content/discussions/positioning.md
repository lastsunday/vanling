+++
title = "定位与取舍参考"
weight = 20
+++

# 定位与取舍参考

行业产品与开源实现的横向参照，用于判断 vanling 该做什么、不该做什么。优先级与待办只在[工作项](@/roadmap/worksheet.md)维护，本文不构成承诺。

## 开源语音模型

> 开源 TTS/语音模型，按质量/体积/许可证排序。vanling 可直接集成。

| 模型 | 参数量 | 许可证 | 延迟 | 声音克隆 | 适合 vanling |
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
ESP32 (边缘)                    服务器 (vanling)
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

## 相关项目

> 以下是 vanling 定位和功能取舍的主要参考项目，按类别组织。

### 后端服务（替代实现）

| 项目 | 语言 | Stars | 对 vanling 的参考价值 |
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

## 商业产品参考

> 闭源商业产品的关键技术和 UX 模式，供 vanling 取舍参考。

### 语音 AI 平台

| 产品 | 关键技术 | 对 vanling 的参考 |
|------|----------|-------------------|
| OpenAI Realtime | 端到端 S2S、语义 VAD、WebRTC、~232ms 延迟 | 语义 VAD 是轮替/中断金标准；WebRTC 传输架构可参考 |
| Cartesia Sonic 3.5 | SSM 架构 TTS、<90ms 延迟、3 秒声音克隆 | SSM 是 TTS 新方向，比 Transformer 更快 |
| ElevenLabs | 语音克隆（1-25 样本）、Expressive Mode、11.ai MCP 语音助手 | 语音 + MCP 集成模式；Agent coaching/evaluation |
| MiniMax Speech-2.8 | 插入标签 `(laughs)` `(sighs)`、7 情绪 + 0-100% 强度、逐句情绪控制 | 简单实用的情绪表达方案，无需 SSML |
| Inworld AI | 15 秒声音克隆、文本描述生成声音、<130ms TTS | 最低门槛声音个性化 |
| Hume AI EVI | 600+ 情感标签、情绪自适应语调、检测犹豫/讽刺/宽慰 | 情绪检测 + 自适应语调是关键差异化 |
| Sesame CSM | 对话韵律（呼吸/犹豫/笑声）、开源 Apache 2.0 | 可直接使用的开源模型，让语音更像真人 |

### 语音 AI 基础设施

| 产品 | 关键技术 | 对 vanling 的参考 |
|------|----------|-------------------|
| LiveKit / Pipecat | 开源 SFU 架构、50+ AI 模型集成、OpenAI ChatGPT 底层 | 语音 AI 基础设施标准，可用作传输层 |
| Retell AI | ~600ms 端到端（免调优）、专有轮替模型、BYO-LLM | 轮替检测是核心差异化 |
| Vapi | BYO-stack（自选 STT/LLM/TTS）、A/B 测试、1000+ 模板 | 可组合架构模式 |

### 智能音箱 / 手机助手

| 产品 | 关键技术 | 对 vanling 的参考 |
|------|----------|-------------------|
| Amazon Alexa+ | 自主任务执行（Uber/OpenTable/Grubhub）、模型无关路由、跨设备连续性 | Agent 链式执行模式；vanling 可通过 MCP 实现 |
| Google Gemini for Home | 自然语言创建自动化（"Ask Home"）、AI 摄像头理解 | "描述自动化" UX 模式 |
| 小米超级小爱 | 声纹识别家庭成员、AI 通话代接、灵动气泡视觉反馈 | 多成员声纹 + 家庭功能 |
| 小度 | 蓝牙 Mesh 本地网关、无网控制、声音人格持久化 | 本地网关架构；声音人格是情感纽带 |
| 天猫精灵 | "1+3+N" 架构、空间智能体、边缘场景调度 | 空间智能体：从命令式到上下文感知 |
| 讯飞星火 | 一句话声音克隆、74+ 方言、端到端语音翻译 | 一句话克隆是杀手级功能 |
| 豆包 | 超能模式（自主任务分解）、100M+ DAU、Coze 智能体平台 | Agent 任务分解 + UGC 角色平台 |

### AI 可穿戴

| 产品 | 关键技术 | 对 vanling 的参考 |
|------|----------|-------------------|
| Meta Ray-Ban | 76% 市场份额、音频优先、视觉感知、7+ 语言实时翻译 | 音频优先 + 视觉感知是成功模式 |
| Omi（开源） | $89、MIT 许可、250+ 社区 App、本地录制+云端同步 | 最接近的开源可穿戴参考 |
| Bee AI（Amazon） | $50、7 天电池、无音频存储隐私策略 | 低价 + 长续航 + 隐私优先 |
| Looki L1 | 30g 挂坠、12 小时电池、AI 环境感知 + 生活日志 | 最小形态 + 感知共鸣理念 |
| Limitless Pendant | 始终监听、说话人分离、MCP 服务器集成、100+ 语言 | 环境上下文捕获 + MCP 集成模式 |

### AI 陪伴应用

| 产品 | 关键技术 | 对 vanling 的参考 |
|------|----------|-------------------|
| Character.AI | 1000万+ 用户角色、Lorebook 世界观、PipSqueak 2 模型 | UGC 角色市场 + 持久人格模式 |
| Replika 2.0 | 跨月记忆重建、主动提醒、AR 体验、视频通话 | 主动记忆驱动建议是强大 UX |
| ChatGPT Voice | 端到端音频模型、320ms 延迟、22 种神经 TTS 声音 | 消费级语音 AI 标杆 |
| Microsoft Copilot | Work IQ 持久记忆、Agent 365 治理、计算机使用代理 | 最成熟的企业级记忆系统 |

### 关键行业趋势

| 趋势 | 代表产品 | vanling 机会 |
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

### 垂直行业语音

> 汽车、医疗、教育、翻译等垂直行业的语音 AI 方案，供 vanling 垂直场景参考。

| 行业 | 代表产品 | 关键技术 | 对 vanling 的参考 |
|------|----------|----------|-------------------|
| 汽车语音 | NIO NOMI、小鹏天机、SoundHound | 边缘离线处理、14 种情绪语调、RAG 车辆手册、Speech-to-Meaning | 边缘离线是关键；情绪语调定制；RAG 参考 |
| 医疗语音 | Nuance DAX、Nabla、Abridge、Suki AI | 环境临床记录、隐私优先（不存储数据）、HIPAA 合规、引导推理 | 被动监听模式；隐私优先架构；引导式对话 |
| 教育语音 | Duolingo Max、作业帮 P50、有道子曰 | 逐步引导推理、个性化学习路径、游戏化、实时发音纠正 | 引导式对话模式；游戏化驱动参与 |
| 翻译设备 | Pocketalk、Timekettle、Vasco | 双麦降噪、<1s 延迟、离线翻译包、专用硬件 | 双麦降噪+低延迟是关键；离线能力 |
| 智能家居 Hub | Echo Hub、HomePod、SmartThings | Matter 1.3/Thread 1.4、离线语音（0.2-0.4s 本地）、预测性自动化 | ESP32 作为隐私优先本地控制器 |

### 中国 AI 创业公司（2025-2026）

> 中国 AI 六小虎等新兴公司的语音 AI 技术，供 vanling 技术选型参考。

| 公司 | 关键技术 | 对 vanling 的参考 |
|------|----------|-------------------|
| [阶跃星辰 StepFun](https://github.com/stepfun-ai) | Step-Audio 2（130B S2S）、Step-Audio-TTS-3B、Apache 2.0 开源、情绪控制+方言 | **最相关**：最佳开源语音 AI，可直接使用 |
| [智谱 AI](https://github.com/THUDM) | GLM-4V 视觉模型、85% 收入来自本地部署、港股 IPO | 本地部署商业模式参考；视觉能力 |
| [MiniMax](https://github.com/MiniMaxAI) | 海螺 AI、星野、1.75 万亿周 Token 调用量、Speech-2.8 | 消费级 AI 产品运营参考；情绪 TTS |
| [月之暗面 Kimi](https://github.com/MoonshotAI) | Kimi K2.5 + Kimi Claw、估值 3 个月从 $4.3B→$18B | 超长上下文 + 语音 AI 结合 |
| [百川智能](https://github.com/baichuan-inc) | 医疗垂直 LLM、3B 现金储备 | 垂直行业深耕模式 |
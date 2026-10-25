+++
title = "Positioning and Trade-off Reference"
weight = 20

[extra]
translated_at = "2026-10-25T00:00:00Z"
source_file_hash = "15656ad7a263f1d16effd17c1a93c54d33ab959c"
+++

# Positioning and Trade-off Reference

Cross-reference of industry products and open-source implementations, used to decide what vanling should and should not do. Priorities and the backlog live only in the [Roadmap](@/roadmap/_index.en.md); nothing here is a commitment.

## Open Source Voice Models

> Open-source TTS/voice models, sorted by quality/size/license. vanling can integrate directly.

| Model | Params | License | Latency | Voice Cloning | Best for vanling |
|-------|--------|---------|---------|---------------|-----------------|
| [Piper](https://github.com/rhasspy/piper) | ~20M | MIT | 55ms (10s audio) | No | ★★★★★ CPU run, 30+ languages, edge deployment first choice |
| [Kokoro v1.0](https://github.com/hexgrad/kokoro) | 82M | Apache 2.0 | CPU realtime | No (KokoClone extension) | ★★★★★ Best quality/size ratio, 54 voices |
| [Step-Audio-TTS-3B](https://github.com/stepfun-ai/Step-Audio-TTS-3B) | 3B | Apache 2.0 | Needs GPU | Yes | ★★★★ Best Chinese voice quality + emotion + dialects |
| [Coqui XTTS v2](https://github.com/coqui-ai/TTS) | 467M | CPML (non-commercial) | <200ms | Yes (6s sample) | ★★★★ Best cloning, 17 languages, license restricts |
| [F5-TTS](https://github.com/SWivid/F5-TTS) | 335M | CC-BY-NC | Needs GPU | Yes (zero-shot) | ★★★ Excellent cloning, Chinese-English mixed, non-commercial |
| [Orpheus TTS](https://github.com/canopylabs-ai/orpheus-tts) | 3B | — | Needs GPU | No | ★★★ Llama backbone, style control |
| [Bark (Suno)](https://github.com/suno-ai/bark) | — | MIT | 0.8x (too slow) | No | ★★ Expressive (laughter/sighs/music), poor real-time |

### Recommended Architecture

```
ESP32 (Edge)                    Server (vanling)
┌──────────┐                ┌─────────────────────┐
│ VAD      │                │ ASR: sherpa-onnx     │
│ Piper/   │◄── WebSocket ──│ LLM: Qwen3 candle    │
│ Kokoro   │                │ TTS: Kokoro/         │
│ (Local   │                │ Step-Audio-TTS-3B    │
│  TTS)    │                └─────────────────────┘
└──────────┘
```

- **Piper or Kokoro** for ESP32 edge TTS (lightweight, fast, CPU)
- **Kokoro** for server TTS (best quality/size ratio, Apache 2.0)
- **Step-Audio-TTS-3B** for server high-quality Chinese (emotion control, dialect support)

---

## Reference Projects

> Key reference projects for vanling positioning and trade-off decisions, organized by category.

### Backend Services (Alternative Implementations)

| Project | Language | Stars | Value for vanling |
|---------|----------|-------|-------------------|
| [xinnan-tech/xiaozhi-esp32-server](https://github.com/xinnan-tech/xiaozhi-esp32-server) | Python | 10k+ | Full-featured reference: 12 ASR + 18 TTS + 13 plugins + PowerMem + RAGFlow + VLLM |
| [joey-zhou/xiaozhi-esp32-server-java](https://github.com/joey-zhou/xiaozhi-esp32-server-java) | Java | 1.3k | DDD architecture + WebRTC AEC3 + 3 memory modes + RBAC + A/B device coordination |
| [AnimeAIChat/xiaozhi-server-go](https://github.com/AnimeAIChat/xiaozhi-server-go) | Go | — | Production-grade: VLLM image safety + Quick Reply + character system + UPX compression |
| [hackers365/xiaozhi-esp32-server-golang](https://github.com/hackers365/xiaozhi-esp32-server-golang) | Go | — | MCP Market + OpenClaw + MCP Audio Server + config wizard + dynamic TTS |
| [78/xiaozhi](https://github.com/78/xiaozhi) | — | 772 | Original official version (deprecated) |
| [mm7h/XiaoZhi.Net](https://github.com/mm7h/XiaoZhi.Net) | C# | 35 | .NET 8 implementation + sherpa-onnx + plugin system |
| [daxpot/xiaozhi-cpp-server](https://github.com/daxpot/xiaozhi-cpp-server) | C++ | 32 | C++20 coroutine architecture + EdgeTTS + Doubao |
| [Hyrsoft/xiaozhi_linux_rs](https://github.com/Hyrsoft/xiaozhi_linux_rs) | Rust | 47 | **First Rust client**: ALSA + Opus + MCP dynamic loading |

### Client Implementations

| Project | Language | Stars | Value |
|---------|----------|-------|-------|
| [huangjunsen0406/py-xiaozhi](https://github.com/huangjunsen0406/py-xiaozhi) | Python | 3.4k | Cross-platform AI client: camera vision + GPIO + Live2D + MQTT |
| [TOM88812/xiaozhi-android-client](https://github.com/TOM88812/xiaozhi-android-client) | Flutter | — | Live2D + Mood Mode + HTML preview + thinking chain visualization |
| [shenjingnan/xiaozhi-client](https://github.com/shenjingnan/xiaozhi-client) | TS | 282 | MCP CLI bridge: aggregates MCP endpoints → Cursor/Cherry Studio |
| [TOM88812/xiaozhi-web-client](https://github.com/TOM88812/xiaozhi-web-client) | HTML | 184 | Browser voice chat: WebRTC + AudioWorklet + Opus |
| [SylarLi/xiaozhi-unity](https://github.com/SylarLi/xiaozhi-unity) | C# | 57 | Unity3D + VRM avatar + uLipSync + Xiaomi home control |
| [coloz/xiaozhi-library](https://github.com/coloz/xiaozhi-library) | Arduino | 10+ | Arduino library: 100+ boards + WS/MQTT dual protocol + LVGL + 20 languages |

### MCP Ecosystem Tools

| Project | Language | Value |
|---------|----------|-------|
| [xinnan-tech/mcp-endpoint-server](https://github.com/xinnan-tech/mcp-endpoint-server) | Python | Lightweight MCP registry, WebSocket protocol, Docker deployment |
| [huangjunsen0406/xiaozhi-mcphub](https://github.com/huangjunsen0406/xiaozhi-mcphub) | TS | Enterprise MCP management: multi-endpoint + vector routing + RBAC + React dashboard |
| [avxxoo/xiaozhi-mcp](https://github.com/avxxoo/xiaozhi-mcp) | Python | MCP tool aggregation: DingTalk/QQ/system monitor/WebPilot/math |
| [yuexianga/xiaozhi-mcp](https://github.com/yuexianga/xiaozhi-mcp) | Python | 18-tool MCP server: file/Telegram/system/Git/email/screenshots |
| [mcp2xiaozhi](https://pypi.org/project/mcp2xiaozhi/) | Python | Universal MCP bridge: stdio/SSE/HTTP → WS, PyPI package |
| [ZhongZiTongXue/xiaozhi-MCPTools](https://github.com/ZhongZiTongXue/xiaozhi-MCPTools) | VB6 | GUI MCP deployment: 25+ open API tools + music playback |
| [johz-chen/mcp-bridge](https://github.com/johz-chen/mcp-bridge) | Rust | Rust MCP bridge: WS/MQTT + process management + heartbeat |
| [dsw0000/xiaozhi-openclaw-plugin](https://github.com/dsw0000/xiaozhi-openclaw-plugin) | JS | OpenClaw bidirectional communication: messages/device control/agent tasks |

### Home Assistant Integration

| Project | Language | Value |
|---------|----------|-------|
| [RealDeco/xiaozhi-esphome](https://github.com/RealDeco/xiaozhi-esphome) | ESPHome | 767 stars, 15+ devices, HA voice satellite, no xiaozhi server needed |
| [AleksSem/xiaozhi-assistant](https://github.com/AleksSem/xiaozhi-assistant) | Python | Most complete HA integration: Conversation Agent + STT/TTS + MCP + OTA |
| [c1pher-cn/ha-mcp-for-xiaozhi](https://github.com/c1pher-cn/ha-mcp-for-xiaozhi) | Python | HA as MCP Server directly: WebSocket + multi-entity proxy |
| [mac8005/xiaozhi-mcp-ha](https://github.com/mac8005/xiaozhi-mcp-ha) | Python | HACS MCP proxy: SSE proxy + auto-reconnect |

### MQTT Gateway

| Project | Language | Value |
|---------|----------|-------|
| [xinnan-tech/xiaozhi-mqtt-gateway](https://github.com/xinnan-tech/xiaozhi-mqtt-gateway) | Python | MQTT+UDP → WS bridge: dynamic load balancing + HMAC auth + MCP command dispatch |

### Embedded Hardware Adaptation

| Project | Platform | Value |
|---------|----------|-------|
| [78/xiaozhi-sf32](https://github.com/78/xiaozhi-sf32) | SiFli SF32 | Bluetooth PAN + LCD + AEC + OTA |
| [100askTeam/xiaozhi-linux](https://github.com/100askTeam/xiaozhi-linux) | Embedded Linux | NXP/Allwinner/Canaan/Rockchip/STM32 multi-BSP |
| [QuecPython/solution-xiaozhiAI](https://github.com/QuecPython/solution-xiaozhiAI) | Quectel 4G | 4G cellular + voice wake + WebSocket |
| [D-Robotics/xiaozhi-in-rdk](https://github.com/D-Robotics/xiaozhi-in-rdk) | Horizon RDK | Horizon RDK board + edge AI acceleration |

### Deployment Tools

| Project | Description |
|---------|-------------|
| [haotianshouwang/xiaozhi-server-installer-docker.sh](https://github.com/haotianshouwang/xiaozhi-server-installer-docker.sh) | One-click Docker deploy + interactive config + 15+ API support |
| [jsntwdj/xiaozhi-esp32-server](https://hub.docker.com/r/jsntwdj/xiaozhi-esp32-server) | ARM64 Docker image (Raspberry Pi etc.) |
| [78/xiaozhi-assets-generator](https://github.com/78/xiaozhi-assets-generator) | Web asset generator: wake words/fonts/emojis/chat backgrounds |

### Management & Monitoring

| Project | Description |
|---------|-------------|
| [busy-worker/xiaozhi-esp32-server-java](https://github.com/busy-worker/xiaozhi-esp32-server-java) | Java admin platform: token usage + conversation duration + device activity + data visualization |
| [joey-zhou/xiaozhi-concurrent](https://github.com/joey-zhou/xiaozhi-concurrent) | WS concurrent load testing: metrics dashboard + auto performance reports |

---

## Commercial Products Reference

> Key technologies and UX patterns from closed-source commercial products, for vanling trade-off reference.

### Voice AI Platforms

| Product | Key Technologies | Value for vanling |
|---------|-----------------|-------------------|
| OpenAI Realtime | E2E S2S, semantic VAD, WebRTC, ~232ms latency | Semantic VAD is gold standard for turn-taking/interruption; WebRTC transport reference |
| Cartesia Sonic 3.5 | SSM architecture TTS, <90ms latency, 3-second voice cloning | SSM is new TTS direction, faster than Transformer |
| ElevenLabs | Voice cloning (1-25 samples), Expressive Mode, 11.ai MCP voice assistant | Voice + MCP integration pattern; Agent coaching/evaluation |
| MiniMax Speech-2.8 | Insertion tags `(laughs)` `(sighs)`, 7 emotions + 0-100% intensity, per-sentence emotion control | Simple practical emotion expression, no SSML needed |
| Inworld AI | 15-second voice cloning, text-describe-generate voice, <130ms TTS | Lowest barrier voice personalization |
| Hume AI EVI | 600+ emotion tags, emotion-adaptive intonation, detects hesitation/sarcasm/relief | Emotion detection + adaptive intonation is key differentiator |
| Sesame CSM | Conversational prosody (breathing/hesitation/laughter), Apache 2.0 | Usable open-source model, more human-like speech |

### Voice AI Infrastructure

| Product | Key Technologies | Value for vanling |
|---------|-----------------|-------------------|
| LiveKit / Pipecat | Open-source SFU architecture, 50+ AI model integration, OpenAI ChatGPT backbone | Voice AI infrastructure standard, usable as transport layer |
| Retell AI | ~600ms E2E (no tuning), proprietary turn detection, BYO-LLM | Turn detection is key differentiator |
| Vapi | BYO-stack (choose STT/LLM/TTS), A/B testing, 1000+ templates | Composable architecture pattern |

### Smart Speakers / Phone Assistants

| Product | Key Technologies | Value for vanling |
|---------|-----------------|-------------------|
| Amazon Alexa+ | Autonomous task execution (Uber/OpenTable/Grubhub), model-agnostic routing, cross-device continuity | Agent chain execution pattern; vanling can implement via MCP |
| Google Gemini for Home | Natural language automation ("Ask Home"), AI camera understanding | "Describe automation" UX pattern |
| Xiaomi Super Xiaoai | Speaker ID for family members, AI call answering, dynamic bubble visual feedback | Multi-member voiceprint + family features |
| Xiaodu | Bluetooth Mesh local gateway, offline control, voice personality persistence | Local gateway architecture; voice personality as emotional bond |
| Tmall Genie | "1+3+N" architecture, spatial agent, edge scenario dispatch | Spatial agent: from command to context-aware |
| iFlytek Spark | One-sentence voice cloning, 74+ dialects, E2E voice translation | One-sentence cloning is killer feature |
| Doubao | Super mode (autonomous task decomposition), 100M+ DAU, Coze agent platform | Agent task decomposition + UGC character platform |

### AI Wearables

| Product | Key Technologies | Value for vanling |
|---------|-----------------|-------------------|
| Meta Ray-Ban | 76% market share, audio-first, visual perception, 7+ language translation | Audio-first + visual perception is success model |
| Omi (open-source) | $89, MIT license, 250+ community apps, local recording + cloud sync | Closest open-source wearable reference |
| Bee AI (Amazon) | $50, 7-day battery, no audio storage privacy strategy | Low price + long battery + privacy-first |
| Looki L1 | 30g pendant, 12-hour battery, AI ambient perception + life log | Smallest form factor + perception resonance |
| Limitless Pendant | Always-listening, speaker diarization, MCP server integration, 100+ languages | Ambient context capture + MCP integration pattern |

### AI Companion Apps

| Product | Key Technologies | Value for vanling |
|---------|-----------------|-------------------|
| Character.AI | 10M+ user characters, Lorebook worldview, PipSqueak 2 model | UGC character marketplace + persistent persona |
| Replika 2.0 | Cross-month memory reconstruction, proactive reminders, AR, video calls | Proactive memory-driven suggestions is powerful UX |
| ChatGPT Voice | E2E audio model, 320ms latency, 22 neural TTS voices | Consumer voice AI benchmark |
| Microsoft Copilot | Work IQ persistent memory, Agent 365 governance, computer-use agent | Most mature enterprise memory system |

### Key Industry Trends

| Trend | Representative Products | vanling Opportunity |
|-------|------------------------|---------------------|
| Semantic VAD | OpenAI (model-level turn-taking) | Smarter interruption than pure VAD |
| Conversational Prosody | Sesame CSM (breathing/hesitation/laughter) | More natural TTS |
| E2E S2S | OpenAI/Hume/Sesame | Ultimate goal; current cascaded more practical |
| Emotion-adaptive | Hume AI (detect → adaptive tone) | Key voice assistant differentiator |
| SSM Architecture TTS | Cartesia (State Space Model) | Faster TTS inference |
| Persistent Memory | Work IQ / Replika / Character.AI | From chat history to long-term memory |
| MCP Voice Integration | OpenAI + MCP, ElevenLabs 11.ai | Voice conversation calling tools |
| Voice Cloning Democratization | Cartesia 3s / Inworld 15s / MiniMax 10s | Lowest barrier voice personalization |
| Privacy-first | Bee (no audio storage), Omi (local processing) | Always-listening privacy strategy |

### Vertical Industry Voice

> Voice AI solutions in automotive, healthcare, education, translation and other verticals, for vanling vertical scenario reference.

| Industry | Representative Products | Key Technologies | Value for vanling |
|----------|------------------------|------------------|-------------------|
| Automotive Voice | NIO NOMI, Xpeng Tianji, SoundHound | Edge offline processing, 14 emotion tones, RAG vehicle manual, Speech-to-Meaning | Edge offline is key; emotion tone customization; RAG reference |
| Healthcare Voice | Nuance DAX, Nabla, Abridge, Suki AI | Ambient clinical documentation, privacy-first (no data storage), HIPAA, guided reasoning | Passive listening; privacy-first architecture; guided conversation |
| Education Voice | Duolingo Max, Zuoyebang P50, Youdao Ziyue | Guided reasoning, personalized learning paths, gamification, pronunciation correction | Guided conversation pattern; gamification drives engagement |
| Translation Devices | Pocketalk, Timekettle, Vasco | Dual-mic noise reduction, <1s latency, offline packs, dedicated hardware | Dual-mic + low latency key; offline capability |
| Smart Home Hub | Echo Hub, HomePod, SmartThings | Matter 1.3/Thread 1.4, offline voice (0.2-0.4s local), predictive automation | ESP32 as privacy-first local controller |

### Chinese AI Startups (2025-2026)

> Voice AI technologies from China's "AI Six Tigers" and emerging companies, for vanling technology selection reference.

| Company | Key Technologies | Value for vanling |
|---------|-----------------|-------------------|
| [StepFun](https://github.com/stepfun-ai) | Step-Audio 2 (130B S2S), Step-Audio-TTS-3B, Apache 2.0, emotion control + dialects | **Most relevant**: best open-source voice AI, usable directly |
| [Zhipu AI](https://github.com/THUDM) | GLM-4V vision model, 85% revenue from local deployment, HK IPO | Local deployment business model reference; vision capability |
| [MiniMax](https://github.com/MiniMaxAI) | Hailuo AI, Xingyao, 1.75T weekly token usage, Speech-2.8 | Consumer AI product operations reference; emotion TTS |
| [Moonshot Kimi](https://github.com/MoonshotAI) | Kimi K2.5 + Kimi Claw, valuation $4.3B→$18B in 3 months | Ultra-long context + voice AI integration |
| [Baichuan](https://github.com/baichuan-inc) | Healthcare vertical LLM, 3B cash reserve | Deep vertical industry approach |

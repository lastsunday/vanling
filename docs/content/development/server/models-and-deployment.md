+++
title = "模型与部署"
weight = 203
+++

# 模型与部署

## 文件结构

```
apps/server/
├── src/                      # 服务端主入口（CLI、Downloader、Config 加载）
│   ├── main.rs               # 二进制入口
│   ├── mod.rs                # run() / async_main()
│   ├── server.rs             # Server 生命周期（包装 api::server::Server）
│   ├── clap.rs               # CLI 参数：serve / downloader
│   └── downloader/           # 模型下载系统（manifest、checksum、mirror）
├── api/                      # HTTP 路由、AI Manager、WebSocket 处理
│   └── src/
│       ├── lib.rs            # start() / create_router()
│       ├── config/           # Config 定义（figment 分层加载）
│       ├── server.rs         # 服务端运行时状态
│       ├── auth.rs           # 认证路由（login / refresh / reset_password）
│       ├── index.rs          # Health / Version 端点
│       ├── ota.rs            # OTA 固件升级
│       ├── device.rs         # 设备管理（admin CRUD）
│       ├── ws/               # WebSocket 处理器（核心实时管道）
│       ├── ling_core/        # LingCore：LLM + MCP 编排
│       ├── asr/              # ASR Manager + 模型（XAsr）
│       ├── tts/              # TTS Manager + 模型（MatchaTTS）
│       ├── vad/              # VAD Manager + 模型（Earshot）
│       ├── llm/              # LLM Manager + 模型（Qwen3 / Echo）
│       ├── mcp/              # MCP Server + Client（rmcp Streamable HTTP）
│       ├── record/           # 会话录制 REST API
│       ├── matrix/           # Matrix 聊天集成
│       └── common/           # 共享辅助（device selection、errors）
├── service/                  # 业务逻辑层（trait 定义 + Session 状态机）
│   └── src/ling/
│       ├── session/          # Session 状态机 + Round 生命周期
│       ├── frame.rs          # Frame / FrameResult / OutputMessage
│       ├── listener.rs       # Listener trait（VAD + ASR 编排）
│       ├── core.rs           # Ling trait（LLM + MCP 编排）
│       ├── asr.rs / tts.rs / vad.rs  # AI trait 定义
│       ├── llm/              # Llm trait + ChatMessage / ToolDef
│       ├── mcp/              # McpClient trait + McpRegistry
│       └── message/          # 线协议消息定义（Hello / Abort / Audio / LLM / TTS 等）
├── entity/                   # Sea-ORM Entity（user / session / round / round_data / frame / config）
├── migration/                # 数据库迁移
└── web/                      # 静态文件服务（SPA）
```

## 数据流

```
Client → WS → ProtocolTranslator → InputFilters → Session → OutputFilters → ProtocolTranslator → WS
                ┌─────────────────┐     ┌──────────────────┐
                │ McpRouterFilter  │     │ RecorderFilter   │
                │ RecorderFilter   │     │ (frame recording)│
                └─────────────────┘     └──────────────────┘
```

详见 [对话流程](@/development/server/dialogue-flow.md)。

## 配置系统

配置使用 [figment](https://docs.rs/figment) 分层加载，优先级从低到高：

1. `VANLING_CONFIG` 环境变量指向的 TOML 文件
2. CLI `--config` 指定路径（可多个，会合并）
3. `VANLING_` 前缀环境变量（`VANLING_LLM_MODEL` 等）
4. CLI `-O key=value` 覆盖

### 热重载

`config::Manager` 使用 `AtomicPtr<Config>` + 线程本地缓存（8 槽历史记录）实现无锁热重载。配置变更时旧版本仍被引用中的协程安全持有，新版本即时生效。

### 主要配置项

| 分类 | 键 | 默认值 | 说明 |
|------|-----|--------|------|
| 服务器 | `address` | `127.0.0.1` | 监听地址 |
| 服务器 | `port` | `3000` | 监听端口 |
| 数据库 | `database_url` | `sqlite://db.sqlite?mode=rwc` | SQLite 或 PostgreSQL URL |
| 认证 | `auth_access_token_secret` | `QLjJTeVblAlM47de` | JWT 签名密钥 |
| TTS | `tts_model` | `matcha_tts` | 模型选择 |
| ASR | `asr_model` | `x_asr` | 模型选择 |
| LLM | `llm_provider` | `local_qwen3` | 模型选择 |
| VAD | `vad_model` | `earshot` | 模型选择 |
| Session | `silence_voice_timeout` | `1200` | 静默超时 (ms) |
| 日志 | `log_console_enabled` | `true` | 控制台日志 |
| Matrix | `matrix_enable` | `false` | Matrix 集成 |

完整配置示例见 `application-example.toml`。

## Downloader 系统

`vanling-server downloader` 子命令提供模型资产下载能力：

- **Manifest 驱动**：模型定义在 `src/downloader/manifests/` 下的 TOML 文件中，包含 URL、变体、checksum
- **分类安装**：`--category tts|asr|llm|vad|reference`
- **交互式向导**：`vanling-server downloader wizard` 引导式安装
- **镜像支持**：`--mirror hf-mirror.com` 为国内用户加速
- **校验**：自动 SHA256 校验下载文件
- **路径派生**：安装后的路径通过 `derive_tts_path` / `derive_asr_path` / `derive_llm_path` 自动注入配置

使用示例：

```shell
# 从 apps/server/ 目录执行
moon run server:run -- downloader install --data-dir ../../data --all
moon run server:run -- downloader wizard --data-dir ../../data
```

## 模型

### LLM

| 模型 | 内存 | 文件大小 | 备注 |
|------|------|---------|------|
| Qwen3-0.6B (Candle-GGUF) | ~1GB | 0.6B GGUF | Qwen3-0.6B-Q4_K_M.gguf |
| Qwen3-1.7B (Candle-GGUF) | ~2.5GB | 1.11GB | Qwen3-1.7B-Q4_K_M.gguf |
| Echo | 0 | 0 | 回显（测试用） |

### ASR

| 模型 | 内存 | 文件大小 | 语言 | CER (TTS 闭环) |
|------|------|---------|------|---------------|
| XAsr (sherpa-onnx) | ~600MB | ~50MB | 中/英 | — |
| Void | 0 | 0 | — | —（测试用） |

### TTS

| 模型 | 内存 | 文件大小 | 备注 |
|------|------|---------|------|
| MatchaTts (sherpa-onnx) | ~500MB | 72MB + 76MB (vocoder) | 中文/中英双语 |
| Mute | 0 | 0 | 静音（测试用） |

### VAD

| 模型 | 内存 | 文件大小 | 备注 |
|------|------|---------|------|
| Earshot (Silero VAD) | ~10MB | 内嵌 | 纯 Rust 实现，无 ONNX |
| Void | 0 | 0 | 固定返回有声（测试用） |

## Fedora 43 CUDA 安装与配置

```shell
sudo sh cuda_12.8.1_570.124.06_linux.run --toolkit --no-drm --silent --override
```

```zshrc
export PATH="/usr/local/cuda/bin:$PATH"
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/usr/local/cuda/lib64
export LIBRARY_PATH=$LIBRARY_PATH:/usr/local/cuda/lib64
```

```shell
conda create -n cuda
conda install conda-forge::gcc==14.3.0
conda install conda-forge::gxx==14.3.0
conda install anaconda::openssl
conda activate cuda
# 开始开发...
```

## 参考规范

<https://rust-lang.github.io/api-guidelines/>

<https://rust-coding-guidelines.github.io/rust-coding-guidelines-zh/overview.html>

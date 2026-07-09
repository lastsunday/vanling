+++
title = "模型下载器"
weight = 500
+++

# 模型下载器

`chobits downloader` 子系统负责 AI 模型文件的下载与管理。所有模型元数据以 JSON manifest 形式编译到二进制中，运行时无外部依赖。

## 目录结构

Manifest JSON 位于 `apps/server/src/downloader/manifests/{category}/`，编译时通过 `include_dir!` 嵌入二进制。

## Manifest 文件格式

每个 JSON 文件描述一个模型的所有变体与文件清单。

```json
{
  "config": { "category": "tts", "model": "matcha" },
  "default_variant": "matcha-icefall-zh-en",
  "variants": {
    "matcha-icefall-zh-en": {
      "files": [
        {
          "url": "https://huggingface.co/.../model.onnx",
          "path": "tts/model/matcha/matcha-icefall-zh-en/model.onnx",
          "sha256": null
        }
      ]
    }
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `config.category` | string | 分类名，与配置文件中的 `{cat}_model` 对应 |
| `config.model` | string | 模型名，与配置文件中的 `{cat}_model` 的值对应 |
| `default_variant` | string / null | 默认变体，`--variant` 和配置均未指定时使用 |
| `variants` | object | 变体名 → 文件列表 |
| `files[].url` | string | Hugging Face 或其它源的下载 URL |
| `files[].path` | string | 相对数据目录的存储路径 |
| `files[].sha256` | string / null | 可选的 SHA256 校验值，`null` 表示跳过校验 |

### archives 格式

除 `files` 外，manifest 也支持 `archives` 字段，用于下载并解压压缩包。3/4 的 manifest 使用此格式：

```json
{
  "config": { "category": "asr", "model": "sense_voice" },
  "default_variant": "default",
  "variants": {
    "default": {
      "archives": [
        {
          "url": "https://huggingface.co/.../sense-voice-zh-en-ja-ko-yue-2025-03-19.tar.bz2",
          "path": "asr/model/sense_voice/default/",
          "sha256": "abc123..."
        }
      ]
    }
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `archives[].url` | string | 压缩包下载 URL |
| `archives[].path` | string | 解压目标目录 |
| `archives[].sha256` | string / null | 可选的 SHA256 校验值 |

支持的压缩格式：`.tar.gz`, `.tar.bz2`, `.zip`。下载后自动解压到 `path` 目录。

当前使用 `archives` 的 manifest：`asr/sense_voice.json`、`llm/qwen3.json`、`reference/audio.json`。仅 `tts/matcha.json` 使用 `files` 格式。

### 路径推导规则

`path` 字段统一使用 `{category}/model/{model}/{variant}/{file}` 格式，全小写。例如 TTS Matcha 的配置文件路径为 `tts/model/matcha/matcha-icefall-zh-en/model.onnx`。

配置中的 `*_path` 项（如 `tts_path`）现在为可选项。当缺省时，系统根据 `{model}+{variant}` 自动推导路径。

## CLI 命令

### 下载器

```shell
chobits downloader <COMMAND>
```

| 子命令 | 说明 |
|--------|------|
| `install` | 下载 AI 模型到本地数据目录 |
| `wizard` | 交互式向导模式 |
| `list` | 列出所有可用模型及其变体 |
| `update-checksums` | 计算已下载文件的 SHA256 并写回 manifest JSON |

运行 `chobits downloader`（无子命令）时，自动显示帮助信息。

#### install

```shell
chobits downloader install [category] [model] [variant] [options]
```

| 参数 | 说明 |
|------|------|
| `category` | 分类：`tts`, `asr`, `llm`, `vad`, `reference`；缺省为全部 |
| `model` | 模型名，如 `qwen3`, `matcha`, `sense_voice` |
| `variant` | 变体名，如 `0.5b`, `1.7b`, `tiny`, `default` |
| `--data-dir <path>` | 数据目录，默认 `data` |
| `--quiet` | 静默模式，不输出进度 |
| `--mirror <url>` | 自定义镜像域名，可多次指定，替换内置的 `hf-mirror.com` |
| `--override <path>` | 覆盖文件路径或 URL，JSON 格式 |
| `--all` | 下载所有 manifests 中的所有文件（忽略配置文件） |
| `-c, --config <path>` | 显式指定配置文件（可选），缺省时自动查找 `application.toml` |

**示例：**

```shell
# 下载当前配置（自动查找 application.toml）所需的模型
chobits downloader install

# 下载当前配置所需模型，仅限 tts 分类
chobits downloader install tts

# 无配置文件时，使用默认配置（MatchaTts + Qwen3×2）
chobits downloader install

# 显式指定配置文件
chobits downloader install --config my-config.toml

# 使用自定义镜像
chobits downloader install --mirror https://my-mirror.example.com

# 下载所有 manifests 中的所有文件（忽略配置文件）
chobits downloader install --all
```

#### wizard

```shell
chobits downloader wizard [options]
```

| 参数 | 说明 |
|------|------|
| `--data-dir <path>` | 数据目录，默认 `data` |
| `--quiet` | 静默模式，不输出进度 |

#### update-checksums

```shell
chobits downloader update-checksums [options]
```

| 参数 | 说明 |
|------|------|
| `--data-dir <path>` | 数据目录，默认 `data` |
| `--quiet` | 静默模式，不输出进度 |

扫描 `data_dir` 中已下载的文件，计算 SHA256 并写回 manifest JSON。优先读取 `download-report.json` 中缓存的 SHA，避免重新读取大文件。

```shell
# 更新默认数据目录中的文件 SHA
chobits downloader update-checksums

# 指定数据目录
chobits downloader update-checksums --data-dir /path/to/models
```

### 列出模型

```shell
chobits downloader list [category] [--json]
```

以树形结构列出所有可用模型及其变体：

```
tts
  └── matcha
      └── matcha-icefall-zh-en (default)
```

**Moon tasks：**

```shell
moon run server:downloader                # 等价于 chobits downloader install
moon run server:downloader -- vad         # 仅下载 VAD
moon run server:downloader -- tts matcha  # 仅下载 TTS Matcha
moon run server:downloader-list           # 列出所有可用模型
moon run server:downloader-list -- --json # JSON 格式输出
moon run server:download-all-and-checksums # 下载所有模型并更新 SHA
```

## 下载工作流

### 请求方式

基于 `reqwest` + `rustls-tls`，无需链接 OpenSSL，纯 Rust TLS 栈。不依赖 `hf-hub` 库。

### 镜像回退

对于 Hugging Face URL，自动添加镜像候选：

1. 原始 URL
2. 在 domain 处替换为镜像域名（默认 `hf-mirror.com`）

镜像列表可通过 `--mirror` 参数完全替换。非 `huggingface.co` 的 URL 不经过镜像处理。

### 并发控制

通过 `tokio::sync::Semaphore` 限制最多 **4** 个并发下载，使用 `JoinSet` 有序收集结果。

### 下载流程

```
客户端 → 检查本地文件（SHA256 匹配则跳过）
       → 创建父目录
       → 生成候选 URL 列表（原始 + 镜像）
       → 依次尝试候选 URL（.tmp 文件写入）
       → SHA256 校验
       → 原子重命名 .tmp → 目标文件
       → 返回 (大小, SHA256)
```

### 缓存与校验

- 本地文件存在时，计算其 SHA256：
  - 匹配 → 跳过（缓存命中）
  - 不匹配 → 删除后重新下载
- `sha256: null` 表示跳过校验
- `downloader update-checksums` 将已下载文件的真实 SHA256 写回 manifest 文件

### 下载报告

每次下载完成后，在数据目录生成 `download-report.json`，包含完成时间、基础路径和每个文件的状态。`update-checksums` 优先读取此报告中的 SHA 缓存，避免重新读取大文件：

```json
{
  "completed_at": "2026-06-16T12:00:00Z",
  "base_dir": "data",
  "files": [
    { "path": "tts/model/matcha/matcha-icefall-zh-en/vocos-16khz-univ.onnx", "size": 1024, "sha256": "abc...", "status": "downloaded" }
  ]
}
```

## 配置集成

### config_to_targets

读取 `application.toml` 中的 `*_model` / `*_variant` 字段，将其转换为下载目标列表。各模型枚举的对应关系：

| 配置字段 | 可选值 | 下载目标 |
|----------|--------|----------|
| `tts_model` | `matchatts` / `mute` | `mute` 跳过 |
| `asr_model` | `sensevoice` / `void` | `void` 跳过 |
| `llm_model` | `qwen3` / `echo` | `echo` 跳过 |
| `vad_model` | `earshot` / `void` | 均跳过（earshot 内嵌，void no-op） |

### config 文件查找

`downloader` 命令与服务器共享同一套配置查找逻辑：

- `CHOBITS_CONFIG` 环境变量 → 当前目录 `application.toml` → 兜底 `application.toml`

无配置文件时，使用 `AppConfig` 的默认值：

| 模块 | 默认模型 | 说明 |
|------|----------|------|
| TTS | MatchaTts（`matcha-icefall-zh-en`） | 下载 `matcha` |
| ASR | SenseVoice（默认变体） | 下载 `sense_voice` |
| LLM | Qwen3（默认变体） | 下载 `qwen3` |
| VAD | Earshot | 内嵌权重，不下载 |

### load_selections / upsert_config

- `load_selections`：解析 `application.toml` 的 `[global]` 段，提取 `*_model` / `*_variant` / `*_path` 键值对
- `upsert_config`：将新的选择写入 `application.toml`，保留非 `[global]` 段

## Override 系统

`--override` 接受一个本地文件路径或 HTTP URL，JSON 格式：

```json
{
  "tts/model/matcha/matcha-icefall-zh-en/model.onnx": {
    "url": "http://localhost:8080/model.onnx",
    "sha256": "abc123..."
  },
  "llm/model/qwen3/0.6b/model.gguf": {
    "url": "http://localhost:8080/model.gguf",
    "sha256": null
  }
}
```

匹配规则：按 `path` 字段精确匹配。覆盖 `url`，可选覆盖 `sha256`（`null` 表示不校验）。

## 交互式向导

`chobits downloader wizard` 提供交互式选择流程：

1. **查找配置**：定位 `application.toml`，读取已有选择
2. **展示目录**：按分类列出所有可用模型及其变体
3. **循环选择**：方向键选择分类 → 方向键选择模型 → 方向键选择变体
4. **预览**：显示最终选择集及对应的配置项
5. **写入配置**：将选择写入 `application.toml`
6. **自动下载**：按分类顺序逐一执行下载

分类列表最后一项为 `done`，选择后结束循环。所有交互使用方向键导航、Enter 确认。

## 架构决策

| 决策 | 选择 | 理由 |
|------|------|------|
| TLS 栈 | rustls | 纯 Rust，无需链接 OpenSSL，消除构建瓶颈 |
| 运行时依赖 | 无 | Manifest 通过 `include_dir!` 编译时嵌入，二进制自包含 |
| 模型发现 | JSON manifest | 灵活可扩展，新增模型只需添加 JSON 文件 |
| 下载库 | reqwest | 功能齐全，支持流式下载、代理、TLS |
| 并发控制 | Semaphore + JoinSet | 简洁的异步并发模式 |
| 缓存策略 | SHA256 校验 | 避免重复下载，保证数据完整性 |
| 路径推导 | 自动 fallback | `*_path` 可缺省，减少配置量 |

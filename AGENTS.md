# AGENTS.md

## 索引

- [1. 技术栈](#1-技术栈)
- [2. 项目结构与命名](#2-项目结构与命名)
- [3. 核心禁忌](#3-核心禁忌)
- [4. 开发工作流](#4-开发工作流)
  - [4.1 开发环境设置（Lix）](#41-开发环境设置lix)
  - [4.2 多语言文档维护](#42-多语言文档维护)
- [5. 构建与 CI 调试](#5-构建与-ci-调试)
- [6. 文档编写规范](#6-文档编写规范)
- [附录 架构概述](#附录-架构概述)

## 1. 技术栈

### Rust (apps/server)

- **Edition 2024** - 使用 RPIT 生命周期捕获规则等新语法
- **Web**: Axum 0.8 + tower-http + utoipa-axum (Scalar OpenAPI)
- **ORM**: Sea-ORM (sqlx-postgres, sqlx-sqlite, with-chrono, with-rust_decimal)
- **Auth**: jsonwebtoken (HS256, access + refresh token) + bcrypt
- **Config**: figment (TOML)
- **ID**: xid (XID 格式)
- **DB**: SQLite (默认) / PostgreSQL (可选)
- **Error**: `thiserror` + `#[error]` 宏 (framework-macros)
- **Testing**: cucumber (BDD) + testcontainers
- **ML**: Candle (candle-core, candle-transformers, candle-nn)
- **Audio**: symphonia, wavers, rubato, opus
- **ASR**: sherpa-onnx — SenseVoice
- **TTS**: sherpa-onnx — Mute / MatchaTts
- **VAD**: earshot (Silero VAD)
- **LLM**: rig-core + Candle — Qwen3 / Echo
- **MCP**: rmcp (Model Context Protocol, 支持 SSE 与 Streamable HTTP)
- **Matrix**: ruma + ruma-client (聊天协议)
- **开发环境**: Nix (Lix) + flake + direnv（可复现开发环境）

### TypeScript

**管理后台 (apps/server-ui):**

- React 19 + Mantine v8 (@mantine/core, @mantine/hooks, @mantine/notifications)
- TanStack Router (文件路由, auto code-splitting) + TanStack Query (React Query)
- Axios + zod
- i18next + react-i18next
- UnoCSS
- Vite
- Prettier: `{ singleQuote: true }`

### Flutter (apps/app)

- 跨平台客户端应用 (WIP)
- Flutter SDK 版本管理: `.fvmrc`

## 2. 项目结构与命名

### 目录结构

```
├── docs/                      mdBook 项目文档
├── flake.nix                  Nix flake 配置
├── flake.lock                 Nix flake lock
├── .envrc                     direnv 自动激活（use flake）
├── rust-toolchain.toml        Rust 版本（由 flake 自动读取）
├── .node-version              Node.js 版本（由 flake 自动读取）
├── application-example.toml   示例配置文件
├── packages/                  workspace 占位
├── libs/                      共享库 (framework, macros, build-metadata)
├── apps/
│   ├── server/                Rust 后端
│   │   ├── src/               应用入口 (main.rs, clap, downloader, server 等)
│   │   ├── api/src/           API 路由层 + AI 模块 (ws, llm, tts, asr, vad, auth, ota, matrix, mcp, record, util, common, error 等)
│   │   ├── service/src/       业务逻辑层 (chobits/)
│   │   ├── entity/src/        Sea-ORM Entity
│   │   ├── migration/src/     数据库迁移
│   │   └── web/src/           Web 层 (rust-embed 静态文件服务)
│   ├── server-ui/src/         React 管理后台
│   ├── server-ui-e2e/         管理后台 E2E 测试 (Playwright)
│   └── app/                   Flutter 跨平台客户端应用
├── libs/
│   ├── framework/             框架层 (auth, config, data, database, deadlock, error, id, info, log, logging, middleware, panic, password, prelude, runtime, sentry, signal, trace, utils 等)
│   │   └── macros/            proc-macro crate (`#[implement]`)
│   ├── macros/                proc-macro crate (`#[config_example_generator]`)
│   └── build-metadata/        构建元数据 crate (git 信息)
```

新建文件请严格遵循对应的目录位置。

### 命名风格

| 语言/层         | 命名风格                  | 示例                                        |
| --------------- | ------------------------- | ------------------------------------------- |
| Rust 变量/函数  | snake_case                | `create_routes`, `start_app`                |
| Rust 类型/Trait | PascalCase                | `AppState`, `TtsFactory`                    |
| Rust 模块名     | snake_case                | `ws.rs`, `llm.rs`                           |
| TS 变量/函数    | camelCase                 | `loadJobs`, `handleSubmit`                  |
| TS 组件/类      | PascalCase                | `RouteComponent`                            |
| TS 组件文件     | PascalCase + .tsx         |                                             |
| TS 非组件文件   | camelCase + .ts           | `http.ts`                                   |
| DB 表/字段      | snake_case                | `user`, `config`                            |
| URI             | snake_case                | `/api/auth/access_token`                    |

## 3. 核心禁忌

### 绝对不能做的

- **不要**引入新依赖前未检查现有依赖是否已满足需求
- **不要**使用非 Edition 2024 的 Rust 语法（如 `'_` 生命周期 elision 规则、`impl<T>` 旧式 trait bound 等）
- **不要**手动编辑 `flake.lock`（使用 `nix flake update` 更新）

### 必须遵守的

- **提交信息**: 必须使用 Conventional Commits 格式（`feat:` / `fix:` / `perf:` / `remove:` / `deprecate:` / `security:`）。Lefthook commit-msg hook 自动校验格式，不满足会被拒绝。如需跳过用 `git commit --no-verify`。如需在 changelog 中展示详细说明，在 footer 中写入 `CHANGELOG: <description>`。破坏性变更使用 `feat!:` 或 `BREAKING CHANGE:` footer
- **CHANGELOG 更新**: 发布前运行 `moon run <project>:bump` 自动版本升级、生成 CHANGELOG、commit 并 tag。底层调用 `scripts/bump.sh`
- Rust: 不要遗漏 `use framework::lib` 等模块导入
- Rust: 路由必须在 `create_router` 中通过 `OpenApiRouter` 组织
- Rust: 提交前运行 `cargo fmt && cargo clippy` 保持代码风格
- AI 模块 (LLM/TTS/VAD/ASR) 使用 Factory 模式，通过 `XxxFactory::init()` 初始化，通过 `XxxFactory::get()` 获取实例
- Downloader 命令从 `apps/server/` 运行时使用 `--data-dir ../../data`（不是 `--data-dir data`）
- Rust: 引入新类型/新模式后（如用 `SessionObserver` 替代 `RecordCollector`、用 `Context` 替代裸 String），用 `rg <旧类型名> --type rust` 确认无残留引用。无引用则删除对应旧文件/旧模块，清零 `#[allow(...)]` 豁免
- **日志**:
  - 必须使用结构化 key-value 字段，禁用 format string 参数（`info!(field = %val, "msg")` ✅；`info!("msg {}", val)` ❌）
  - `#[instrument]` 仅用于入口函数，内部函数禁止
  - 所有事件显式携带 `session_id`（round 相关加 `round_id`）
  - 禁止在 async 代码中使用 `span.enter()`
  - INFO: 生命周期；DEBUG: 流转；WARN: 故障
  - 控制台 `FmtSpan::NONE`，文件 `FmtSpan::CLOSE`

## 4. 开发工作流

### 4.1 开发环境设置（Lix）

项目使用 **Lix**（Nix 的社区 fork）管理可复现的开发环境。

**首次设置：**

```bash
# 1. 安装 Lix
curl -sSf -L https://install.lix.systems/lix | sh -s -- install

# 2. 安装 direnv（推荐，自动激活环境）
nix profile install nixpkgs#direnv nixpkgs#nix-direnv
# 在 ~/.zshrc 或 ~/.bashrc 添加: eval "$(direnv hook zsh)"

# 3. 进入项目（direnv 自动激活，或手动 nix develop）
cd chobits
direnv allow
```

**devShell 选择：**

```bash
nix develop .#server    # 仅 Rust 后端（rustToolchain + openssl + sqlite + postgresql）
nix develop .#frontend  # 仅前端（nodejs + pnpm）
nix develop             # 默认完整环境（含 moon、just、mdbook、pkg-config 等）
```

### Git Hook & 模板自动安装

`.envrc` 中已配置 `lefthook install` + `git config commit.template`，进入目录时自动生效。

### 新增业务逻辑时 (Rust)

1. `migration/src/`: 创建 SQL 迁移
2. `entity/src/`: 更新 Sea-ORM 实体
3. `api/src/`: 实现路由 handler
4. `service/src/`: 实现具体业务逻辑
5. 在 `api/src/lib.rs` 的 `create_router` 中注册路由
6. 运行 `cargo check && cargo test` 验证

### 新增 AI 模型支持时

1. `api/src/llm/`（或 `tts/`, `asr/`, `vad/`）: 实现对应 Trait
2. 在 `api/src/config/` 中添加模型配置项（枚举变体 + 配置结构体）
3. 创建 manifest: `apps/server/src/downloader/manifests/` 添加 JSON
4. 在 `api/src/<category>/model/mod.rs` 注册模块
5. 在 Factory 中注册模型创建逻辑（`create_model()` 添加 match arm）
6. 添加测试（单元 + 集成 + 参考音频/闭环）
7. 更新 `application-example.toml`

### 新增前端页面时 (server-ui)

1. `routes/`: 按 TanStack Router 文件路由约定新建 `.tsx` 文件
2. `api/`: 添加对应的 API 调用函数
3. `components/`: 组件文件
4. 翻译文本添加到 `public/locales/{lang}/{namespace}.json`
5. 运行 `moon run server-ui:typecheck` 验证类型

### 4.2 多语言文档维护

多语言文档使用独立目录模式：中文为源语言（`docs/src/`），英文翻译在 `docs/en/`。

#### 目录结构

```
docs/
├── src/                     ← 中文源文件（源语言）
│   ├── book.toml
│   ├── SUMMARY.md
│   └── development/...
├── en/                      ← 英文翻译
│   ├── SUMMARY.md
│   └── development/...
├── book.toml                ← 公共配置（plugin/output）
├── translation-status.json  ← 翻译状态跟踪
├── index.html               ← 语言选择页
└── scripts/check-translation.sh
```

#### 工作流

| 场景 | 操作 |
|--------|------|
| 新增文档 | 在 `docs/src/` 中创建 → 让 AI 翻译后写入 `docs/en/` 对应路径 |
| 修改文档 | 改 `docs/src/` 中的文件 → 运行 `moon run docs:check-translation` 检查哪些过期 → 让 AI 重译过期文件 |
| 构建中文 | `moon run docs:build` |
| 构建英文 | `moon run docs:build-en` |
| 检查过期 | `moon run docs:check-translation` |

#### 翻译步骤

1. 用 AI 读取 `docs/src/<path>` 的中文内容
2. 要求 AI 翻译并写入 `docs/en/<path>`（保持相同目录结构）
3. 用 `git diff docs/en/` 审查翻译变更
4. 如有手动修正，直接编辑 `docs/en/` 下的 Markdown 文件
5. 更新 `translation-status.json` 中对应文件的 `status` 为 `"translated"`，填入 `source_hash`（`git log -1 --format=%H -- docs/src/<path>`）和 `translated_at`

#### translation-status.json 格式

```json
{
  "version": 1,
  "locales": {
    "en": {
      "files": {
        "development/server/architecture.md": {
          "status": "translated",
          "source_hash": "abc123def456",
          "translated_at": "2026-06-28T10:00:00Z"
        }
      }
    }
  }
}
```

状态说明：
- `"untranslated"` — 尚未翻译
- `"translated"` — 已翻译且同步
- 已翻译但 `source_hash` 与源文件不一致 = 过期（`check-translation` 标记 `⚠️ STALE`）

## 5. 构建与 CI 调试

### 构建工具

- **Monorepo**: Moon (@moonrepo/cli)
- **配置**: `.moon/workspace.yml`, `.moon/toolchains.yml`
- **JS/TS 任务**: Moon 自动从 `package.json` scripts 推断
- **Rust 任务**: 在 `moon.yml` 中显式定义
- **常用命令**:
  - `moon run <project>:<task>` — 运行某项目的特定任务
  - `moon run :<task>` — 所有项目运行某任务
  - `moon ci --affected` — CI 中运行受影响项目的 pipeline
  - `moon query projects` — 列出所有项目
  - `moon query tasks` — 列出所有任务
- **工具链**: 由 Nix flake 统一管理（Node.js → `.node-version`，Rust → `rust-toolchain.toml`，moon CLI 内置于 `flake.nix`）

### Fast Dev Loop

- Rust 路由改动后运行 `cargo check` 验证类型，不用 `cargo run` 全量编译
- 启动服务: `moon run server:run`
- 启动前端: `moon run server-ui:dev`
- 运行测试: `cargo test --package api`

## 6. 文档编写规范

### 6.1 协议/流程类文档

写前必读：

  禁止先列消息类型和字段。
  先画时序树（概览），再画时序图（图示），字段放附录（参考）。

  概览层只展示消息顺序，不混字段细节。
  字段表放附录，实现时才查。
  概览层应由人手写，AI 负责展开参考层。

验证：只看概览能画出完整交互，且不需要翻附录。

> 设计原理见 docs/src/discussions/documentation-style.md。

---

## 附录 架构概述

> 详细架构文档见 [Server Architecture](./docs/src/development/server/architecture.md)

### WebSocket 会话生命周期

详细文档见 [Server Architecture](docs/src/development/server/server-architecture.md#session-生命周期)。

```
Client → WS Connect → Auth(JWT) → Session Created
  ↓
[VAD: Voice Activity Detection] → 检测到语音
  ↓
[ASR: Automatic Speech Recognition] → 语音转文字
  ↓
[LLM: Large Language Model] → 对话推理
  ↓
[TTS: Text to Speech] → 文字转语音
  ↓
Client ← Audio Stream
```

### Rust 子 crate 依赖关系

```
build-metadata (编译时 git 信息)
framework-macros (proc-macro: #[implement])
chobits-macros (proc-macro: #[config_example_generator])
  └── framework (框架层: auth, config, data, database, deadlock, error, id, info, log, logging, middleware, panic, password, prelude, runtime, sentry, signal, trace, utils)
       ├── entity (Sea-ORM 实体) → depends on framework
       ├── migration (数据库迁移) → depends on framework, entity
       ├── web (rust-embed 静态文件) → standalone
       ├── service (业务逻辑) → depends on framework, entity
       ├── api (路由 + AI 模型管道) → depends on framework, entity, migration, service, web
       └── server (应用入口) → depends on api
```

### 框架错误码

| 范围 | 分类 | 说明 |
|------|------|------|
| 101xxx | 基础 | 数据库等 |
| 201xxx | 第三方 | JWT, 密码等 |
| 301xxx | 框架 | 校验, 请求格式等 |
| 401xxx | 严重 | 内部错误, 资源未找到 |
| 402xxx | 认证 | AuthErrorCode |
| 501xxx | 用户业务 | UserErrorCode |
| 502xxx | OTA | OtaErrorCode |
| 503xxx | AI 模型 | Chat/TTS/ASR |
| 504xxx | WebSocket | WsErrorCode |

完整架构分析见 [Server Architecture](./docs/src/development/server/architecture.md)。

> 待办事项清单见 [TODO](./docs/src/development/server/TODO.md)。

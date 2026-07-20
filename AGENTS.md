# AGENTS.md

专用于 chobits仓库的 AI 代理指令。优先遵循以下原则（降序）：

1. 保持代码库一致性（风格、模式、架构）
2. 首选已有的 crate/packages，不引入新依赖
3. 所有修改必须通过 `cargo check` / `moon run server-ui:typecheck`
4. 提交使用 Conventional Commits，hook 拦截不合规格式

## 关键命令

| 用途      | 命令                                                                                              |
| --------- | ------------------------------------------------------------------------------------------------- |
| 运行后端  | `moon run server:run`                                                                             |
| 运行前端  | `moon run server-ui:dev`                                                                          |
| 检查 Rust | `cargo check`                                                                                     |
| 检查 TS   | `moon run server-ui:typecheck`                                                                    |
| 运行测试  | `cargo test --package api`（Rust）/ `moon run server-ui:test`（前端）/ `-- <test_name>`（单模型） |
| 格式化    | `cargo fmt && cargo clippy`                                                                       |

## 技术栈

> 完整依赖见 `apps/server/Cargo.toml` / `apps/server-ui/package.json`

关键约束：Edition 2024（RPIT 捕获规则、无 `'_` elision）/ Mantine v9 / zod v4 / OXC 插件 / sherpa-onnx / rig-core / rmcp / Flutter WIP

## 边界

### ✅ Always

- **路由**: `create_routes(state) → OpenApiRouter`，在 `create_router` 的 `setup_*` 中注册。禁止直接挂载到主路由
- **AI 模块**: `XxxManager::init(config).await` 初始化 → `XxxManager::global().default()` 使用。禁止 `new()` 直接实例化
- **结构化日志**: 所有 tracing 调用必须含 `component` + `event` 字段；格式 `tracing::info!(component = "xxx", event = "yyy", key = %value, "human msg")`；禁止 `#[instrument]`（用 component/event 替代 span 前缀）；console→`FmtSpan::NONE`、file→`FmtSpan::CLOSE`；禁止 `println!()` / `eprintln!()`
- **日志格式**: `[<COMPONENT> human msg] component=xxx event=yyy session_id=...`；组件名大写（SESSION、VAD、ROUND、LISTENER、ASR、MCP、WS）；仅 Text/Compact 启用，Pretty/Json 不变
- **测试**: `apps/server/api/tests/` 按功能分类；每次修改增/改对应测试
- **命名**: Rust snake_case / PascalCase 类型；TS camelCase 变量 / PascalCase 组件 / PascalCase.tsx
- **提交**: Conventional Commits（`feat:|fix:|perf:|remove:|deprecate:|security:`）。破坏性用 `feat!:` 或 BREAKING CHANGE。禁止自由格式信息
- `cargo fmt && cargo clippy` 零警告后提交
- 运行下载器时用 `--data-dir ../../data`（从 `apps/server/` 执行）
- 重命名类型后用 `rg <旧名> --type rust` 确认无残留

### ⚠️ Ask First

- 添加新 crate / npm package
- 修改 `flake.nix` / `flake.lock`。版本号在 `versions` attrset，平台数据在 `platformData`。升级 moon/sherpa-onnx 请用 `scripts/update-moon.sh` / `scripts/update-sherpa-hashes.sh`
- 数据库 schema 变更或修改已有迁移
- 删除已有文件或模块

### ❌ Never

- 使用 Edition 2024 以外的 Rust 语法（`'_` elision、旧式 `impl<T>` bound）
- 手动编辑 `flake.lock`
- async 代码中使用 `span.enter()`
- 提交生成文件（dist/、target/、node_modules/）
- 跳过 pre-commit hooks（`--no-verify`）
- 自动提交或推送代码（必须等待用户明确确认后再提交）

### Definition of Done

- [ ] `cargo check` / `moon run server-ui:typecheck` 通过
- [ ] 新增功能有对应测试
- [ ] `cargo fmt && cargo clippy` 零警告
- [ ] 无遗留 `dbg!()` / `console.log()` / `TODO` / `FIXME`
- [ ] 提交信息符合 Conventional Commits

### When Stuck

1. `rg <pattern> --type rust` / `rg <pattern> --type ts` 搜索代码库
2. 搜索网络主流方案再动手
3. 查看同类模块测试文件了解预期行为
4. 阅读 trait 定义（`service/src/chobits/`）
5. 检查 `docs/content/development/server/` 架构文档
6. 分支实验 + `cargo check`

## 环境

首次: `curl -sSf -L https://install.lix.systems/lix | sh` → `nix develop`（全功能：Rust + Node + Flutter + Android SDK）。`.envrc` 自动执行 hook + commit template。

## 新增模块

| 模块 | 关键路径 |
|---|---|
| Rust 业务 | `migration/` → `entity/` → `api/` handler → `service/` 逻辑 → 注册路由 → 验证 |
| AI 模型 | 实现 Trait → config 枚举变体 → manifest → `model/mod.rs` 注册 → Manager match arm → 测试 → 更新配置示例 |
| 前端页面 | `routes/` `.tsx` → `data/` 类型 → `api/` 调用 → `components/` → i18n → typecheck |
| 多语文档 | 改 `docs/` 下中文 `.md` → 翻译为英文 `.en.md` → `git hash-object <source.md>` 获取 blob hash 填入 `source_file_hash` + 更新 `translated_at` → 提交 |

## 架构

> 详见 `docs/content/development/server/architecture.md`

`Client → WS → Auth → Session → [VAD] → [ASR] → [ChiiCore (LLM + MCP)] → [TTS] → Client`

要点：Xiaozhi 协议 / Manager + OnceLock 单例 / Config 原子指针 + 本地缓存 / Session + RoundLoop

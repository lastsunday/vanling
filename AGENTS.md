# AGENTS.md

专用于 vanling 仓库的 AI 代理指令。优先遵循以下原则（降序）：

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
| IoT 产物  | `moon run iot:image`（ELF + merged.bin，可从 `apps/iot/` 运行）                                   |

## 技术栈

Edition 2024（RPIT 捕获规则、无 `'_` elision）/ Mantine v9 / zod v4 / OXC / sherpa-onnx / rig-core / rmcp / Flutter WIP；完整依赖见 `apps/server/Cargo.toml` / `apps/server-ui/package.json`

## 边界

### ✅ Always

- **路由**: `create_routes(state)` → `OpenApiRouter`，经 `create_router` 的 `setup_*` 注册；禁止直接挂载主路由
- **AI 模块**: `XxxManager::init(config).await` 初始化 → `global().default()` 取用；禁止 `new()` 直接实例化
- **日志**: tracing 必带 `component` + `event`，格式 `tracing::info!(component = "x", event = "y", key = %v, "msg")`；行格式 `[<组件> msg] component=x event=y [session_id=…]`、组件名大写；console→`FmtSpan::NONE`、file→`FmtSpan::CLOSE`；禁 `#[instrument]`/`println!()`
- **测试**: `apps/server/api/tests/` 按功能分类；每次修改增/改对应测试
- **命名**: Rust snake_case/PascalCase 类型；TS camelCase 变量/PascalCase 组件（`.tsx`）
- **自文档化代码**: 语义命名承载意图；不写逐步注释、不为未改代码补注释；仅注不直观的 Why/约束；改代码须删/改过时注释
- **断言需证据**: 注释与文档中的每条断言，或有对应测试/实测支撑，或降级为"据称"并标明未复现；不得把未验证的观察写成事实。代码注释只写当前正确做法与当前硬约束，不写历史叙事（"曾经做错"、对比旧实现）——这类内容归 `docs/content/records/`（带 `<!-- doc-audience: ai -->`），该页标明 AI 产出未经人类审阅
- **提交**: Conventional Commits（`feat:|fix:|perf:|remove:|deprecate:|security:`）；破坏性用 `feat!:`/BREAKING CHANGE；禁自由格式
- `cargo fmt && cargo clippy` 零警告后提交
- 运行下载器时用 `--data-dir ../../data`（从 `apps/server/` 执行）
- 重命名类型后用 `rg <旧名> --type rust` 确认无残留
- **IoT 分层**: `iot-core(纯逻辑) → iot-chip-esp(esp 家族运行时) → iot-bsp-esp(每板接线) → iot-app(单任务二进制)`；接线只在 bsp 板模块（`Board::new` 固定引脚、业务零引脚号）；命名约定「板层用板名、应用层用产品名 `vanling`」；新增板/多芯片流程见 `docs/content/development/iot/architecture.md`
- **IoT 组合**: 三轴正交（硬件⊥模块⊥能力）；组合点 = bin 板清单（能力 move 注入、缺能力 = 编译错）；产品档 = feature 别名；渲染层运行时可插拔（`RenderController` + renderer 注册表）；详见 architecture.md
- **IoT 日志**: 只用 `log` façade（`log::info!` 等），输出通道由家族 `iot-chip-esp` 初始化（esp 为 `esp_println::logger`）；业务代码禁止 `println!` / `esp_println::println!`
- **IoT 软 feature**: 一个 feature = 一个模块（`mod` 处门控一次、off 即文件不存在）；消费者文件禁 `#[cfg(feature=...)]`（能力经 trait 注入）；新增须每子集独立编译过测；判据与六规则见 `docs/content/development/iot/features.md`
- **IoT 产物可本地复现**: `moon run iot:image` 一键生成与 CI 同名的 ELF + merged.bin（命名对齐 `reusable-iot-build.yml`，版本经 `scripts/version.sh`）；s3 的 Xtensa 构建经 `scripts/iot-xtensa.sh` 按环境分发——espup `esp` 1.95.0.0 工具链齐备（CI/发布）时原生编译，macOS Intel 本地自动回退 `espressif/idf-rust:esp32s3_1.95.0.0` 容器（同版本、字节可一致），Action 异常时可用本地产物替补上传；详见 `docs/content/development/iot/flashing.md`

### ⚠️ Ask First

- 添加新 crate / npm package
- 修改 `flake.nix` / `flake.lock`。版本号在 `versions` attrset，平台数据在 `platformData`。升级 moon/sherpa-onnx 请用 `scripts/update-moon.sh` / `scripts/update-sherpa-hashes.sh`
- 新增/回归 IoT 软 feature（非硬件轴的能力/行为开关）——先过 `docs/content/development/iot/features.md` 四判据
- 数据库 schema 变更或修改已有迁移
- 删除已有文件或模块
- 为 TODO/roadmap 文档分配或修改优先级：语义依据 `docs/content/roadmap/_index.md` 的「优先级说明」；**分辨不清时必须问人类，禁止自行推断**

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

## 环境

首次: `curl -sSf -L https://install.lix.systems/lix | sh` → `nix develop`（全功能：Rust + Node + Flutter + Android SDK）。`.envrc` 自动执行 hook + commit template。

## 参考文档

> 深度知识与操作流程见 `docs/content/development/`；卡住时先读对应域，再 `rg` 搜索 + 参照同类测试。

- **server**: `server/architecture.md` — 架构与数据流 / AI Manager / 新增模块路径；`server/TODO.md` — 已完成/未完成清单（开工入口）；`server/research.md` — 定位与取舍参考
- **iot**: `iot/architecture.md` — 分层与组合 / 新增板 / 新增芯片；`iot/features.md` — 软 feature 判据与内聚
- **clients**: `clients/server-ui.md` — 新增页面路径
- **多语文档**: 维护规则见 `development/_index.md`

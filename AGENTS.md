# AGENTS.md

专用于 vanling 仓库的 AI 代理指令。优先遵循以下原则（降序）：

1. 保持代码库一致性（风格、模式、架构）
2. 首选已有的 crate/packages，不引入新依赖
3. 所有修改必须通过对应目标的检查：server/UI `cargo check` + `moon run server-ui:typecheck`；IoT `moon run iot:check-s3` + `moon run iot:check-host`

## 关键命令

| 用途      | 命令                                                                                              |
| --------- | ------------------------------------------------------------------------------------------------- |
| 运行后端  | `moon run server:run`                                                                             |
| 运行前端  | `moon run server-ui:dev`                                                                          |
| 检查 Rust | `cargo check`                                                                                     |
| 检查 TS   | `moon run server-ui:typecheck`                                                                    |
| 运行测试  | `cargo test --package api`（Rust）/ `moon run server-ui:test`（前端）/ `-- <test_name>`（单模型） |
| 格式化    | `cargo fmt --check && cargo clippy -- -D warnings`（pre-commit 已拦）                             |
| IoT 检查  | `moon run iot:check-s3`（真实编 S3 目标）/ `check-host` / `check-features`                        |
| IoT 探针  | `moon run iot:probe-camera-s3`（另有 `audio`、`multicore`），配对 `flash-probe-*` 烧录              |
| IoT 产物  | `moon run iot:image`（ELF + merged.bin，可从 `apps/iot/` 运行）                                     |

## ❌ Never

- 用非 Edition 2024 的 Rust 语法（`'_` elision、旧式 `impl<T>` bound）
- 提交生成文件（`dist/`、`target/`、`node_modules/`）或手改 `flake.lock`
- 跳过 pre-commit hooks（`--no-verify`）、在 async 代码里用 `span.enter()`
- 自动提交或推送；为让检查通过而放宽断言、缩小范围、降低覆盖或跳过检查

## 技术栈

Edition 2024 / Mantine v9 / zod v4 / `@vitejs/plugin-react-oxc` / sherpa-onnx / rig-core / rmcp / Flutter WIP；完整依赖见 `apps/server/Cargo.toml` / `apps/server-ui/package.json`

## 工作方式

- **验证覆盖改动**: 投入与改动成比例——用前先确认命令真的编译/检查了改动的模块，查证规模随交付物走，不随主题走。IoT 单组件 feature 的 `cargo build -p iot-bsp-esp --features gc2145` 编不到 `virtual_components`、`lckfb_szpi_esp32s3`；S3 目标以 `moon run iot:check-s3` 为准
- **汇报附证据**: 报结论时附实际执行的命令与结果；没跑过的检查不写成通过，跳过的步骤写「未完成」与阻碍，不把没做的事写成「建议」
- **只问阻塞问题**: 请求、规则或证据已解决的直接做；只有阻塞且代价高的选择才问，一次问完并附默认选项
- **写完更新本文件**: 同一错误**复发**时才补（第一次踩坑只记在 `docs/content/records/`）。补之前先看能否改写已有条目或删掉过时条目——净行数不增。不要只在对话里说「下次注意」
- **批量改先备份**: 改文件前先备份，每次替换带断言，改完立刻编译
- **反复推翻去查证**: 同一结论被数据推翻两次以上就去查文档、依赖源码或寄存器手册，不要继续推理

## 边界

### ✅ Always

- **路由**: `create_routes(state)` → `OpenApiRouter`，经 `create_router` 的 `setup_*` 注册；禁止直接挂载主路由
- **AI 模块**: `XxxManager::init(config).await` 初始化 → `global().default()` 取用；禁止 `new()` 直接实例化
- **日志**: tracing 必带 `component` + `event`，格式 `tracing::info!(component = "x", event = "y", key = %v, "msg")`；行格式 `[<组件> msg] component=x event=y [session_id=…]`、组件名大写；console→`FmtSpan::NONE`、file→`FmtSpan::CLOSE`；禁 `#[instrument]`/`println!()`
- **测试**: `apps/server/api/tests/` 按功能分类；每次修改增/改对应测试
- **注释默认不写**: 语义命名承载意图。判据是 reviewer 会不会在此提问；不会就不写——不复述名字、类型、值、文件名，不写「这个在哪里被用到」，不写逐步注释。代码不够清楚到无需解释的，改代码而不是加注释。`//` 只写 Why 与当前硬约束；`///`/`//!` 写用途与用法，面向使用者，大段概念说明集中文件头一处。注释密度、行数、比例都不是目标，也没有阈值
- **安全与证据不可删**: `# Safety`、`// SAFETY:`、hazard 与已知限制任何清理都不得删除，只可收紧措辞。注释中的断言须有测试或实测支撑，否则标「据称」并注明未复现；不写历史叙事（曾经做错、对比旧实现），此类内容归 `docs/content/records/`（带 `<!-- doc-audience: ai -->`）
- **下载器**: `--data-dir ../../data`，从 `apps/server/` 执行
- **重命名**: 确认旧名无残留后再收工
- **IoT 分层**: `iot-core → iot-chip-esp → iot-bsp-esp → iot-app`；接线只在 bsp 板模块（`Board::new` 固定引脚、业务零引脚号）；板层用板名、应用层用产品名 `vanling`。新增板/多芯片流程见 `iot/architecture.md`
- **IoT 组合**: 组合点 = bin 板清单（能力 move 注入，缺能力即编译错）；产品档 = feature 别名；渲染层运行时可插拔（`RenderController` + renderer 注册表）。详见 architecture.md
- **IoT 日志**: 只用 `log` façade，输出通道由家族 `iot-chip-esp` 初始化（esp 为 `esp_println::logger`）；业务代码禁止 `println!` / `esp_println::println!`
- **IoT 软 feature**: 一个 feature = 一个模块（`mod` 处门控一次、off 即文件不存在）；消费者文件禁 `#[cfg(feature=...)]`（能力经 trait 注入）；新增须每子集独立编译过测；判据与六规则见 `iot/features.md`
- **IoT 产物**: `moon run iot:image` 生成的 ELF + merged.bin 与 CI 同名同版本；s3 的 Xtensa 构建在 espup 工具链缺失时自动回退同版本容器。详见 `iot/flashing.md`

### ⚠️ Ask First

- 添加新 crate / npm package
- 修改 `flake.nix` / `flake.lock`。版本号在 `versions` attrset，平台数据在 `platformData`。升级 moon/sherpa-onnx 请用 `scripts/update-moon.sh` / `scripts/update-sherpa-hashes.sh`
- 新增/回归 IoT 软 feature（非硬件轴的能力/行为开关）——先过 `iot/features.md` 四判据
- 数据库 schema 变更或修改已有迁移
- 删除已有文件或模块
- 为 TODO/roadmap 文档分配或修改优先级：语义依据 `docs/content/roadmap/_index.md` 的「优先级说明」；**分辨不清时必须问人类，禁止自行推断**

### Definition of Done

- [ ] `cargo fmt --check && cargo clippy -- -D warnings` 零警告
- [ ] 对应目标的 typecheck 通过（IoT 用 `moon run iot:check-s3`）
- [ ] 新增功能有对应测试
- [ ] 无遗留 `console.log`
- [ ] 计划项全部完成，或已标为未完成并说明阻碍

## 环境

首次: `curl -sSf -L https://install.lix.systems/lix | sh` → `nix develop`（全功能：Rust + Node + Flutter + Android SDK）。`.envrc` 自动执行 hook + commit template。

## 参考文档

> 深度知识与操作流程见 `docs/content/development/`；卡住时先读对应域，再 `rg` 搜索 + 参照同类测试。

- **server**: `server/architecture.md` — 架构与数据流 / AI Manager / 新增模块路径；`server/TODO.md` — 已完成/未完成清单（开工入口）；`server/research.md` — 定位与取舍参考
- **iot**: `iot/architecture.md` — 分层与组合 / 新增板 / 新增芯片；`iot/features.md` — 软 feature 判据与内聚
- **clients**: `clients/server-ui.md` — 新增页面路径
- **多语文档**: 维护规则见 `development/_index.md`

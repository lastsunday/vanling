+++
title = "速查"
weight = 22
+++

# 速查

本页是仓库全部 project 的命令入口：先列常用流程（多步组合，可按序粘贴执行），再按 project 列出单条命令，其后是固件专题、改动的影响范围、故障现象的排查起点与检查无法覆盖的内容。

本页只写命令与约束。为什么如此、实测数字多少，见文末链接的页面。

## 目录 {#contents}

- [约定](#conventions)：参数透传、缓存、任务全集的边界
- [常用流程](#workflows)：12 段多步组合，每段含前置条件、命令与成功标志
- [命令参考](#commands)：[server](#server)、[server-ui](#server-ui)、[server-ui-e2e](#server-ui-e2e)、[app](#app)、[iot](#iot)、[docs](#docs)
- [发布与维护](#release)
- [固件](#firmware)：[开发板与端口](#boards)、[编译期变量](#build-time-vars)、[日志](#logging)、[烧录前](#before-flashing)、[便利命令](#handy-commands)
- [改动的影响范围](#impact)
- [故障现象与排查起点](#troubleshooting)
- [检查无法覆盖的内容](#blind-spots)
- [完整内容](#further)

## 约定 {#conventions}

- 命令均在仓库根目录执行；`moon run <project>:<task>` 在任意子目录亦可执行。
- 向任务传参须置于 `--` 之后：`moon run <project>:<task> -- <参数>`。参数追加在任务命令末尾，不能插入命令中间。
- 任务命中缓存时不实际执行；`moon run --force <project>:<task>` 强制重跑。
- 任务全集以 `moon query tasks --project <project>` 为准。本页不列仅作为其他任务依赖存在的任务，它们由 moon 自动执行。

## 常用流程 {#workflows}

### 1. 首次启动服务端 {#flow-first-run}

前置：新克隆无 `application.toml`，`data/` 下无模型资产。以下三条按序执行。

```sh
moon run server-ui:build
moon run server:downloader-install
moon run server:run
```

成功标志：终端出现 `listening on 0.0.0.0:3000`，`http://127.0.0.1:3000/login` 可使用默认账号 `root` / `Change_Me` 登录。

- `server-ui:build` 经 `ensure-deps` 自动执行 `pnpm install`，无需先手工安装依赖。
- 缺少模型资产时，服务端在 TTS 初始化阶段终止，报 `Matcha acoustic model file (.onnx) not found in …`。
- `application.toml` 与 `application-dev.toml` 均被 gitignore，且缺失时 figment 静默跳过该文件：服务端以默认值启动，既不询问也不报错。

### 2. 前后端同时开发 {#flow-dual-terminal}

```sh
# 终端 A
moon run server:dev
# 终端 B
moon run server-ui:dev
```

成功标志：终端 A 出现 `listening on 0.0.0.0:3000`；终端 B 打印 `http://localhost:4200`，页面可加载数据。

- 前端不使用 Vite 代理，接口地址取自 `apps/server-ui/.env.development` 的 `VITE_BASE_URL=http://localhost:3000`。后端未启动时页面为空白，不会出现跨域报错。
- `server-ui:dev` 不依赖 `ensure-deps`，首次须先执行 `pnpm i`。
- `server:dev` 读取 `application-dev.toml`；该文件缺失时按默认值启动。

### 3. 不安装模型资产时启动 {#flow-minimal}

```sh
moon run server:dev-minimal
```

VAD 与 ASR 置为 `void`、TTS 置为 `mute`、LLM 使用 `local_echo`，无需任何模型资产；语音链路不可用，其余接口与页面照常。成功标志同流程 1。

`moon run server:dev-lite` 保留 VAD、ASR 与 TTS，仅将 LLM 换为 `local_echo`，仍需模型资产。

### 4. 只改前端并验证内嵌产物 {#flow-embedded-ui}

```sh
moon run server-ui:build
moon run server:run
```

成功标志：`http://127.0.0.1:3000/` 显示新界面。

`apps/server/web/dist` 是指向 `apps/server-ui/dist/` 的符号链接，后端内嵌的是磁盘上的构建产物；仅运行 `server-ui:dev` 不会改变 3000 端口上的页面。

### 5. 端到端测试 {#flow-e2e}

```sh
moon run server-ui-e2e:e2e
```

前置：无需手动启动服务。任务依赖 `server-ui:build` 与 `install-playwright`，Playwright 自行在 4300 端口执行 `vite preview`。

成功标志：用例通过，HTML 报告写入 `apps/server-ui-e2e/playwright-report`。

- 指向其他地址时设 `BASE_URL`：`BASE_URL=http://127.0.0.1:3000 moon run server-ui-e2e:e2e`。
- 4300 端口已有服务时 Playwright 复用该服务（`reuseExistingServer`，CI 下关闭）。

### 6. 固件改动后的检查 {#flow-firmware-checks}

```sh
moon run iot:check-s3
moon run iot:check-c6
moon run iot:check-host
```

三条互不替代：S3 走 Xtensa 工具链，C6 走 `riscv32imac`，`check-host` 检查 `host-smoke` 二进制。第三条在只改宿主侧代码时可省略。

### 7. 烧录产品镜像 {#flow-flash}

```sh
espflash list-ports -n
IOT_PORT=<port> moon run iot:flash-s3
```

成功标志：monitor 首行出现 `[IOT] log level INFO (IOT_LOG_LEVEL)`，随后出现 `[IOT] boot complete`。

- 两块板同时插入时 `IOT_PORT` 必须给出，否则 espflash 自行选择，可能写入 C6。
- 任务脚本带 `--monitor`，烧录完成后占用终端。

### 8. 单外设探针 {#flow-probe}

```sh
IOT_PORT=<port> moon run iot:flash-probe-audio-s3
```

每块探针镜像只启用一路外设，故障归属明确。`flash-probe-*` 已包含编译，无需先行运行对应的 `probe-*`；后者仅编译，供不烧录的场景使用。

- 主题全集：`aec`、`audio`、`camera`、`clock`、`demand`、`echo`、`multicore`、`wifi-s3`、`wifi-c6`。
- 探针烧录不指定分区表，设备上会写入 espflash 的默认表；恢复产品行为须重烧产品镜像。

### 9. 无硬件环境 {#flow-no-hardware}

```sh
moon run iot:smoke-host
moon run iot:emu-smoke
```

`smoke-host` 在宿主运行 `host-smoke`，命中缓存，重放须加 `--force`；`emu-smoke` 经 `esp-emu` 启动 C6 合并镜像并断言 `[IOT] boot complete`，该任务不缓存，每次都重新构建与启动。

### 10. 提交前 {#flow-pre-commit}

```sh
lefthook run pre-commit --force
moon run server:test
moon run docs:check-translation
```

pre-commit 只含格式与静态检查：`cargo fmt --check`、`cargo clippy`（服务端一次、固件三次）、`dart format`、`flutter analyze`。它不执行测试，也不校验译文，故后两条须另行执行。

- 无改动文件时钩子自动跳过，`--force` 强制运行。
- `server:test` 依赖 `test-download-resources`（`cache: false`），每次真实安装缺失的模型资产。

### 11. 与 CI 对齐 {#flow-ci}

```sh
moon ci --base=origin/dev
moon run docs:check-translation
```

CI 的 `ci` job 执行 `moon ci`（仅受影响任务），`docs-check` job 独立执行 `docs:check-translation`。两条均通过才等同 CI。

### 12. CUDA {#flow-cuda}

moon 无法向 `server:run` 透传 `--features`：该任务的命令已含 `--`，追加的参数会落到二进制自身的命令行。须直接调用 cargo：

```sh
cd apps/server && cargo run -p vanling-server --features cuda -- \
  -O 'address=0.0.0.0' -O 'data_dir=../../data' --config=application.toml
```

## 命令参考 {#commands}

### server {#server}

`apps/server`，监听 3000。`dev` 读 `application-dev.toml`，`run` 读 `application.toml`，两者均被 gitignore 且缺失时不报错。

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run server:dev` | 以 `application-dev.toml` 启动并监听 3000 | 缺配置时按默认值启动 |
| `moon run server:run` | 以 `application.toml` 启动 | 同上 |
| `moon run server:dev-lite` | 关闭 LLM（`local_echo`），保留 VAD、ASR、TTS | 仍需模型资产 |
| `moon run server:dev-minimal` | 关闭 VAD、ASR、TTS、LLM | 无需模型资产 |
| `moon run server:downloader-install` | 安装模型资产至 `../../data` | 新克隆后首次启动前须执行 |
| `moon run server:downloader-list` | 列出可安装条目 | — |
| `moon run server:format` | `cargo fmt --check` | 只检查，不写回 |
| `moon run server:lint` | `cargo clippy --workspace -- -D warnings` | 根 workspace 排除 `apps/iot`，不检查固件 |
| `moon run server:test` | 运行 `vanling-server` 与 `api` 的测试 | 依赖真实下载模型；moon 已设 `RUST_TEST_THREADS=2` |
| `moon run server:test-pg` | 同上，走 PostgreSQL 路径 | 设 `TEST_DATABASE=pg` |

### server-ui {#server-ui}

`apps/server-ui`，开发服务器 4200，预览 4300。

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run server-ui:dev` | 启动 Vite 开发服务器 | 不自动装依赖，首次须 `pnpm i` |
| `moon run server-ui:build` | 构建至 `dist/` | 经 `ensure-deps` 自动 `pnpm install` |
| `moon run server-ui:preview` | 预览构建产物 | `e2e` 会自行启动，无需手动 |
| `moon run server-ui:test` | `vitest run` | — |
| `moon run server-ui:typecheck` | `tsc --noEmit -p tsconfig.app.json` | 只覆盖 `src/**`，不含 `vite.config.ts`、`vitest.config.ts` |

### server-ui-e2e {#server-ui-e2e}

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run server-ui-e2e:e2e` | 运行 Playwright 用例，自起 4300 | 依赖 `server-ui:build` 与浏览器安装 |
| `moon run server-ui-e2e:e2e-install` | `playwright install --with-deps` | 首次或升级 Playwright 后执行 |

### app {#app}

`apps/app`，Flutter 客户端。构建任务的版本号取自 `scripts/version.sh`。

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run app:run` | 在目标设备上运行 | — |
| `moon run app:test` | `flutter test` | — |
| `moon run app:analyze` | `flutter analyze` | — |
| `moon run app:format` | `dart format .` | 写回源文件，不是检查 |
| `moon run app:build-android` | 构建 release APK | — |
| `moon run app:build-ios` | 构建 release iOS，不签名 | 仅 macOS |
| `moon run app:build-macos` | 构建 release macOS | 仅 macOS |
| `moon run app:build-web` | 构建 release Web | — |
| `moon run app:build-linux` | 构建 release Linux | 仅 Linux |
| `moon run app:build-windows` | 构建 release Windows | 仅 Windows |

### iot {#iot}

`apps/iot`。`check-s3` 与 `build-s3` 经 `scripts/iot-xtensa.sh` 分发：装有 espup 工具链时直接使用，否则回退到 `iot:xtensa-image` 构建的镜像。

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run iot:check-s3` | 对 S3 目标 type-check，覆盖全部探针二进制 | 不链接；内存超限在此不可见 |
| `moon run iot:check-c6` | 对 C6 目标 type-check，覆盖 `wifi-probe` | — |
| `moon run iot:check-host` | 对 `host-smoke` 二进制 type-check | — |
| `moon run iot:check-features` | 逐个单独编译每个组件 feature | 唯一覆盖单 feature 编译的任务；矩阵含约 15 个子集 |
| `moon run iot:format` | `cargo fmt --check` | 只检查，不写回 |
| `moon run iot:lint` | 对 S3 目标执行 clippy | 需 Xtensa 工具链或镜像 |
| `moon run iot:lint-host` | 对 `iot-core`、`iot-ui` 执行 clippy | — |
| `moon run iot:lint-smoke` | 对 `host-smoke` 执行 clippy | — |
| `moon run iot:test` | core、ui 与 app（host 后端）测试 | — |
| `moon run iot:test-bsp` | 板级测试，启用全部组件 feature | — |
| `moon run iot:test-tool` | 音效目录与生成表的一致性测试 | — |
| `moon run iot:smoke-host` | 宿主冒烟 | 命中缓存，`--force` 重放 |
| `moon run iot:emu-smoke` | `esp-emu` 启动 C6 合并镜像 | 不缓存，每次重新构建与启动 |
| `moon run iot:build-s3` | 编译 S3 产品镜像 | 只编不烧；内存超限在此暴露 |
| `moon run iot:build-s3-camera` | 编译开机停在相机页的镜像 | 与产品镜像共用输出路径，任务内部备份与恢复 |
| `moon run iot:build-s3-thrift` | 编译降档镜像（`audio=off`、`motion=on:30000`） | 同上 |
| `moon run iot:flash` | 编译并按仓库分区表烧录 C6，烧录后进 monitor | 占用终端 |
| `moon run iot:flash-s3` | 编译并烧录 S3，烧录后进 monitor | 两块板同时插入时须给 `IOT_PORT` |
| `moon run iot:flash-s3-camera` | 烧录相机页镜像 | — |
| `moon run iot:flash-s3-thrift` | 烧录降档镜像 | — |
| `moon run iot:flash-panel-typing-s3` | 烧录面板输入凭据的 bench 镜像 | 须给 `IOT_WIFI_SSID` 与 `IOT_WIFI_PSK` |
| `moon run iot:reset-s3` | 以 watchdog 复位把 S3 从 ROM 下载模式拉出 | 仅复位，不烧录 |
| `moon run iot:flash-probe-aec-s3` | 编译并烧录 AEC 探针 | 不指定分区表 |
| `moon run iot:flash-probe-audio-s3` | 编译并烧录放音探针 | 同上 |
| `moon run iot:flash-probe-camera-s3` | 编译并烧录相机与面板探针 | 同上 |
| `moon run iot:flash-probe-clock-s3` | 编译并烧录时钟探针 | 同上 |
| `moon run iot:flash-probe-demand-s3` | 编译并烧录数据面探针 | 同上 |
| `moon run iot:flash-probe-echo-s3` | 编译并烧录回声消除探针 | 同上 |
| `moon run iot:flash-probe-multicore-s3` | 编译并烧录双核与主栈探针 | 同上 |
| `moon run iot:flash-probe-wifi-s3` | 编译并烧录 Wi-Fi 探针（S3） | 凭据取自 `IOT_WIFI_SSID` / `IOT_WIFI_PSK` |
| `moon run iot:flash-probe-wifi-c6` | 编译并烧录 Wi-Fi 探针（C6） | 同上 |

### docs {#docs}

`docs`，Zola 站点。

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run docs:dev` | 本地预览并打开浏览器 | 热更新 |
| `moon run docs:build` | 构建至 `public/` | 报告内链与孤立页问题 |
| `moon run docs:check-translation` | 校验 `.en.md` 的 `source_file_hash` | 只遍历已存在的 `.en.md`，缺译不报错 |

## 发布与维护 {#release}

低频任务，按需执行。

| 命令 | 用途 | 注意事项 |
| --- | --- | --- |
| `moon run server:bump` | 计算服务端版本并写回 | 一次只处理一个包 |
| `moon run app:bump` | 计算 App 版本并写回 | 同上 |
| `moon run iot:bump` | 计算固件版本并写回 | 同上 |
| `moon run server:build-release` | 服务端 release 构建 | 发布流水线使用 |
| `moon run server:sweep` | `cargo sweep --maxsize 20GB` | 清理 `target` |
| `moon run server:downloader-all-and-checksums` | 安装全部资产并更新校验和 | 维护下载清单时使用 |
| `moon run iot:image` | 生成 ELF 与 `merged.bin` 至 `dist/` | 依赖 `image-s3` 与 `image-c6` |
| `moon run iot:xtensa-image` | 构建 Xtensa 工具链镜像 | 本机缺 espup 时构建一次，之后 `build-s3` 自动回退 |
| `moon run iot:gen-midi-assets` | 由配方重新生成 `assets/*.mid` | 写回源树；`test-tool` 校验其与生成表一致 |
| `moon run docs:build-gh` | 以 GitHub Pages 基址构建至 `public-gh/` | 由 CI 部署使用 |
| `moon run tools:update-moon` | 更新 moon 版本与哈希 | 修改 `flake.nix` |
| `moon run tools:update-moon-check` | 校验 moon 版本是否为最新 | 只检查 |

## 固件 {#firmware}

### 开发板与端口 {#boards}

台架常驻两块板，不可互相替代。**端口名每次插拔都会变，不是板卡标识。**

| 板 | feature | 复位语义 |
| --- | --- | --- |
| 立创 ESP32-S3（产品靶，16 MB） | `lckfb-szpi-esp32s3` | 原生 USB：烧录须 `-a watchdog-reset`；停留在下载模式时执行 `moon run iot:reset-s3` |
| ESP32-C6-DevKitC-1（冒烟靶，8 MB） | `esp32c6-devkitc-1` | 普通 DTR 复位即可启动；espflash 拒绝 `watchdog-reset` |

```sh
espflash list-ports -n                                # 列出已连接端口，仅输出端口名
espflash board-info --port <port>                      # 芯片型号 / flash 容量 / MAC 地址
IOT_PORT=<port> moon run iot:flash-s3                  # 两块板同时插入时必填
```

### 编译期变量 {#build-time-vars}

以下变量在编译期读入，改动后必须重新编译。

| 变量 | 作用 | 不设时 |
| --- | --- | --- |
| `IOT_BOOT_PAGE` | 开机停在哪一页 | `ambient` |
| `IOT_DEMAND` | 各数据面的开关与节奏 | 固件内的默认档 |
| `IOT_LOG_LEVEL` | 串口输出级别：`error` / `warn` / `info` / `debug` / `trace` | `info` |
| `IOT_WIFI_SSID` / `IOT_WIFI_PSK` | 编入固件的凭据 | 不编入，设备停在配网页 |
| `IOT_PANEL_BENCH` | 强制未配置状态，强制在面板输入一次密码 | 无 |

`IOT_PORT` 不属于此类：它只交给 espflash 定位端口，不进入固件。

`IOT_DEMAND` 按项宽松解析：无法识别的项只丢弃该项，构建仍然成功；若整串未识别出任何数据面，另打印一条 `[IOT] IOT_DEMAND named no plane this firmware recognises: ignored`。`IOT_BOOT_PAGE` 取到无效页名时退回 `ambient`，并打印 `[IOT] IOT_BOOT_PAGE=… is not a page, booting on Ambient`。

### 日志 {#logging}

产品默认 `info`：开机清单、状态迁移与用户动作各一行，**稳态可长时间无输出**——串口安静属正常，不代表死机。周期遥测（帧成本与栈水位、feed 节奏、链路相位与地址、扫描列表）均在 `debug`：

```sh
IOT_LOG_LEVEL=debug IOT_PORT=<port> moon run iot:flash-s3
```

记录中引用的 `[DISPLAY] stack peak …`、`[PLAY] … feeds …`、`[WIFI] … address …` 只在 `debug` 镜像中逐窗口出现，复现须按上一行重新编译。`warn` 仅在真正越过阈值时出现（执行器单次持有超过 30 ms、单帧写入超过 20 ms 或占空超过 25%、feed 迟到或低于额定速率 95%），因此**出现任何一条 warn 都值得阅读**。首行 `[IOT] log level …` 是该镜像实际生效的级别，怀疑日志缺失时先核对该行。

### 烧录前 {#before-flashing}

- 构建失败时**旧二进制仍在原处**，其时间戳不能作为判断依据。新增一行日志后执行 `strings <elf> | grep <字面量>`，命中才说明产物已更新。
- `iot:flash` 的脚本带 `--monitor`，烧录后会占用终端。无 tty 时以 `Failed to initialize input reader` 结束，镜像此时已写入完成。
- S3 构建经 Docker；`docker.sock/_ping` 返回 500 表示守护进程未启动，不是编译错误。
- `moon` 命中缓存时不重新执行；`moon run --force <任务>` 才会重跑。

### 便利命令 {#handy-commands}

```sh
# 实时日志：直接读串口，不经 flash stub
timeout 5 cat <port>

# 完整开机日志：monitor 需要 pty，CTRL+R（0x12）须在其连接后发送。完整配方见 flashing.md
({ sleep 6; printf '\x12'; sleep 10; } | script -q -t 1 /tmp/boot.log \
  espflash monitor --chip esp32s3 --port <port> -a no-reset) >/dev/null 2>&1

# 清除已保存的网络，无需重烧；espflash 不读设备上的表，须显式给出 csv
espflash erase-parts --chip esp32s3 --port <port> \
  --partition-table partitions/lckfb-szpi-esp32s3.csv creds

# 任意 cargo 子命令要跑在 S3 目标上，只能经此分发脚本
bash ../../scripts/iot-xtensa.sh tree -p iot-app --target xtensa-esp32s3-none-elf \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto

# 单条测试，并显示测试内的 println 输出
cargo test -p iot-core <测试名> -- --nocapture

# 主任务栈总量 = 两个符号之差（esp-rtos 采用同一算法）
nm <elf> | grep -E '_stack_(start|end)_cpu0'
```

## 改动的影响范围 {#impact}

| 改动 | 须重跑 |
| --- | --- |
| server 任意 Rust 代码 | `moon run server:format`、`server:lint`、`server:test` |
| server-ui 源码 | `moon run server-ui:typecheck`；后端需内嵌时另跑 `server-ui:build` |
| IoT 组件 feature 或 `Cargo.toml` | `moon run iot:check-features` |
| `scripts/iot-xtensa.sh` | `moon run iot:check-s3`、`iot:check-c6`、`iot:build-s3` |
| 文档中文页 | 重算 `source_file_hash`（`git hash-object <中文页>.md`），再跑 `docs:check-translation` |
| 文档英文页、模板或 `zola.toml` | `moon run docs:build` |
| 任一 `moon.yml` | 本页对应的命令表 |

## 故障现象与排查起点 {#troubleshooting}

| 现象 | 先查 |
| --- | --- |
| 启动即退出，报 `Matcha acoustic model file (.onnx) not found in …` | 未安装模型资产，执行 `moon run server:downloader-install` |
| 3000 端口页面空白 | 后端未启动，或前端构建产物未更新（见流程 4） |
| 烧录后不启动，日志为 `boot:0x23 (DOWNLOAD(USB/UART0))` | S3 烧录缺少 `-a watchdog-reset` |
| 一行日志都没有 | console 通道（`console-auto` / `console-uart`）；monitor 需要真实 tty，可用 `script` 包装 |
| 设备行为与代码不符 | 烧录的是旧产物，用 `strings` 核对字面量 |
| 链接期主栈断言失败 | `.bss` 增长；`check-s3` 不链接，无法发现 |
| 探针可运行，产品却不识别已保存的网络 | 探针烧录任务不传 `--partition-table`，设备上是 espflash 的默认表；重烧产品镜像恢复 |
| `server:test` 长时间停在下载 | 首次运行需下载模型资产（`test-download-resources`，`cache: false`） |

## 检查无法覆盖的内容 {#blind-spots}

- `check-s3` 只做 type-check，不链接：内存超限要到 `build-s3` 或 `iot:image` 才暴露。
- 编译期变量不在 moon 的缓存键中；`build-s3` 已设 `cache: false`，其余任务无此保护。
- 每个 feature 能否单独编译，只有 `check-features` 回答。
- 日志级别是编译期常量（`chip-esp/src/logging.rs` 的 `configured_level`）：修改 `IOT_LOG_LEVEL` 而不重新编译不生效。
- **无 gdb 与 openocd**：调试手段是日志，加外设即加一个探针（`flash-probe-*-s3`）。
- `server:lint` 为 `cargo clippy --workspace`，根 workspace 排除 `apps/iot`，不检查固件。
- `server-ui:typecheck` 只检查 `tsconfig.app.json` 的 include（`src/**`）；`vite.config.ts` 与 `vitest.config.ts` 归 `tsconfig.spec.json` 管，不在其中。
- pre-commit 不执行测试，也不校验译文。
- `apps/iot/target` 会增长到十几 GB；`cd apps/iot && cargo clean` 需付重新编译的代价，S3 部分最贵。

## 完整内容 {#further}

- [分层与组合](@/development/iot/architecture.md)、[feature 判据](@/development/iot/features.md)
- [固件安装](@/development/iot/flashing.md)——产物、分区表、bench 镜像、抓完整启动日志的配方
- [无硬件仿真](@/development/iot/emulation.md)
- [运动](@/development/iot/motion.md)、[音频计量](@/development/iot/audio.md)、[放音](@/development/iot/playback.md)
- [实现记录](@/records/iot/_index.md)——为什么是现在这样、实测数字（AI 产出，未经人类审阅）

+++
title = "无硬件仿真与回归冒烟"
weight = 40
+++

# 无硬件仿真与回归冒烟

无硬件 CI 回归冒烟分两层，行为覆盖优先、硅层冒烟可选的增强：

- **宿主 harness**（主线）：iot-app 以 std 后端跑在开发机上，虚构 `HostBoard` 驱动真实 `run<B>` 管线，断言意图→状态→渲染。C6 与 S3 共享同一套逻辑，行为覆盖与架构一致性都在这一层验证。
- **esp-emu 硅层冒烟**（可选增强，仅 esp32c6）：真实固件镜像在 esp-emu 模拟器里引导到 `[IOT] boot complete`。esp-emu 不支持 Xtensa，S3 的行为覆盖依赖宿主 harness、驱动回路依赖真机。

真机 HIL（espflash 烧录 + 实机 smoke）保留在 roadmap（P1），不作为 CI 门槛。

## 方案决策矩阵

| 方案                     | 芯片支持         | 板级保真 | CI 成本 | 维护成本 | 行为覆盖 | esp-hal 接入难度 | 结论         |
| ------------------------ | ---------------- | -------- | ------- | -------- | -------- | ---------------- | ------------ |
| 真机 HIL                 | 全部（按持有板） | 完全     | 高      | 高       | 高       | 无               | roadmap P1   |
| 宿主 harness（本方案）    | 全部（逻辑层）   | 无外设   | 低      | 低       | 高       | 无（std 后端）   | **主线 CI**  |
| esp-emu                  | esp32c6 等       | 较完整   | 中      | 中       | 中       | 直接跑固件       | **C6 增强**  |
| QEMU（espressif fork）   | esp32c3/c6/s3 等 | 较完整   | 中      | 中       | 中       | 直接跑固件       | 未选：esp-emu 更贴合 |
| Wokwi                   | 多种             | 元件级   | 中      | 中       | 低       | 需固件改造       | 未选：需改造 |
| Renode                  | 多家             | 板级     | 中      | 高       | 中       | 需外围建模       | 未选：建模成本高 |

选型理由：

- **宿主 harness 为主**：逻辑层已零 esp-hal 依赖（`iot-core` 的 `Board`/`HasLight`/`HasInput` 纯 trait），`input_task`/`render_loop`/`run<B>` 全部 host-clean，唯一 ESP 耦合在二进制入口。用既有依赖的 std feature 即可运行，不引入新 crate、无外设时序，是最稳的 CI 回归网。
- **esp-emu 为辅**：直接引导真实 merged 镜像（ROM → 二级 bootloader → 分区 → app → embassy 执行器），验证芯片启动链而非业务逻辑；esp-emu 为 Espressif 官方，贴 esp-hal 生态。
- 其余方案或需固件改造（Wokwi）、或外围建模成本高（Renode）、或与 esp-hal 生态贴合度不如 esp-emu（QEMU fork 更完整但当前仅做启动冒烟时价值重复）。

## 使用

冒烟任务通过 moon 编排，CI（`reusable-iot-build.yml`）与本地等价可运行：

```sh
moon run iot:smoke-host      # 宿主 harness：意图→状态→渲染 行为断言
moon run iot:emu-smoke       # C6 硅层：esp-emu 引导 merged 镜像至 [IOT] boot complete
```

- `smoke-host` 依赖 `check-host`（`cargo check --no-default-features --features host`），`emu-smoke` 依赖 `build-c6-emu`。
- `iot:check`/`iot:build` 用默认特性（含 `esp32c6-devkitc-1` + `console-auto`）编译设备固件；`check-s3`/`build-s3` 用 `lckfb-szpi-esp32s3,console-auto`。

### 脚本与产物

| 脚本                    | 作用                                                             | 产物                                              |
| ----------------------- | ---------------------------------------------------------------- | ------------------------------------------------- |
| `scripts/iot-emu.sh`    | 缓存下载 esp-emu（pin v0.42.0）、选 merged.bin、`--exit-on` 断言 | —                                                 |
| `scripts/iot-emu-image.sh` | 用 `CARGO_TARGET_DIR=target/emu` 隔离构建 + `espflash save-image --merge` | `dist/vanling-iot-esp32c6-devkitc-emu-merged.bin` |

- `target/emu/` 隔离构建**不污染**默认 C6 构建（`image-c6`/烧录路径）。
- Linux（CI）原生跑 esp-emu；macOS（Intel）自动走 docker `linux/amd64`。
- 产物命名与 CI 对齐；`dist/` 已 gitignore。

## console（UART）硬约束

- esp-emu 只扫描 UART0 用于 `--exit-on`，因此 **C6 冒烟构建必须 `console-uart`**；设备默认 `console-auto`（USB-Serial/JTAG 自适应）保持不变。
- 拆成两个 iot-app feature：`console-auto` = 设备生产默认（旧行为），`console-uart` = esp-emu 冒烟专用。`esp-println` 的 `auto`/`uart` 互斥，feature 天然约束二者不可并存。
- 宿主 harness 走 std 后端，不需要 console，不在此约束内。

## 局限

- esp-emu 不支持 Xtensa/esp32s3：S3 的原生行为靠宿主 harness 覆盖、驱动回路靠真机验证。
- 宿主 harness 不模拟真实外设时序（RMT、flash、Wi-Fi 等），只覆盖逻辑组合层。
- esp-emu 的 QEMU 层通常以 release 化固件模拟（时间敏感），开发 profile 仅作编译检查。
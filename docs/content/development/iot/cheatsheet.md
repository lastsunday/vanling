+++
title = "速查"
weight = 5
+++

# 速查

一页的抄本：常用命令与会咬人的地方。为什么是这样、实测数字多少，在文末链接的页面里。

## 日常循环

在 `apps/iot` 下，或在仓库任意处用 `moon run iot:<任务>`：

| 目的 | 命令 |
| --- | --- |
| 改完先查 S3（只 type-check，不链接） | `moon run iot:check-s3` |
| 查 C6 | `moon run iot:check-c6` |
| 宿主测试（core / 板级 / 配方） | `moon run iot:test`、`iot:test-bsp`、`iot:test-tool` |
| 无板冒烟 | `moon run iot:smoke-host`（会命中缓存；重放加 `--force`）、`iot:emu-smoke` |
| 编 + 烧（C6 冒烟靶，烧完进 monitor） | `moon run iot:flash` |
| 编 + 烧（S3 产品靶） | `moon run iot:flash-s3` |
| 只编不烧 | `moon run iot:build-s3` |
| 出发布产物到 `dist/` | `moon run iot:image` |
| 任务全表 | `moon query tasks --project iot \| jq -r '.tasks.iot\|keys[]'` |

## 板子与端口

台架常驻两块，不可互相代替；**端口名每次插拔都会变，它不是标识**。

| 板 | feature | 复位语义 |
| --- | --- | --- |
| 立创 ESP32-S3（产品靶，16 MB） | `lckfb-szpi-esp32s3` | 原生 USB：烧录必须 `-a watchdog-reset`；卡在 downloader 用 `moon run iot:reset-s3` |
| ESP32-C6-DevKitC-1（冒烟靶，8 MB） | `esp32c6-devkitc-1` | 普通 DTR 就启动；espflash 直接拒绝 `watchdog-reset` |

```sh
espflash list-ports                                    # 插着哪几块
espflash board-info --port /dev/cu.usbmodem1422301     # Chip type / Flash size / MAC address
IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-s3 # 两块都插着时必须给
```

## 改了就影响固件的变量（编译期，改完必须重编）

| 变量 | 作用 | 不设时 |
| --- | --- | --- |
| `IOT_BOOT_PAGE` | 开机停在哪一页 | `ambient` |
| `IOT_DEMAND` | 各数据面的开关与节奏 | 固件里的默认档 |
| `IOT_WIFI_SSID` / `IOT_WIFI_PSK` | 编进固件的凭据 | 不编，设备停在配网页 |
| `IOT_PANEL_BENCH` | 强制未配置，逼一次面板输密码 | 无 |

`IOT_PORT` 不属于这一类：它只给 espflash 用，不进固件。

## 烧之前

- 构建失败时**旧二进制还在**，时间戳不作数：加一行日志，`strings <elf> | grep <字面量>` 对上才算新产物。
- `iot:flash` 的脚本带 `--monitor`，烧完会占住终端；没有 tty 时它在最后一步报 `Failed to initialize input reader`，镜像其实已经写完。
- S3 构建走 Docker；`docker.sock/_ping` 报 500 是守护进程没起，不是编译错误。
- `moon` 命中缓存就不会真跑；`moon run --force <任务>` 才重跑。

## 症状 → 首查

| 症状 | 先查 |
| --- | --- |
| 烧完不启动，日志 `boot:0x23 (DOWNLOAD(USB/UART0))` | S3 少了 `-a watchdog-reset` |
| 一行日志都没有 | console 通道（`console-auto` / `console-uart`）；monitor 要真 tty，用 `script` 包一层 |
| 现象和代码对不上 | 烧的是旧产物，`strings` 验字面量 |
| 链接期主栈断言 | `.bss` 涨了；`check-s3` 不链接，看不见 |
| 探针能跑，产品回来不认已存的网络 | 探针烧录任务不带 `--partition-table`，写进设备的是默认表；重烧产品镜像恢复 |

## 便利命令

```sh
# 看实时日志：串口原样读，不经 flash stub
timeout 5 cat /dev/cu.usbmodem1422301

# 抓完整开机日志：monitor 要 pty；CTRL+R（0x12）得等它连上再发。完整配方见 flashing.md
({ sleep 6; printf '\x12'; sleep 10; } | script -q -t 1 /tmp/boot.log \
  espflash monitor --chip esp32s3 --port /dev/cu.usbmodem1422301 -a no-reset) >/dev/null 2>&1

# 清掉已存的网络，不用重烧；espflash 不读设备上的表，csv 必须给
espflash erase-parts --chip esp32s3 --port /dev/cu.usbmodem1422301 \
  --partition-table partitions/lckfb-szpi-esp32s3.csv creds

# 任意 cargo 子命令要跑在 S3 目标上，只能经这个分发脚本
bash ../../scripts/iot-xtensa.sh tree -p iot-app --target xtensa-esp32s3-none-elf \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto

# 单条测试，并看到测试里的 println
cargo test -p iot-core <测试名> -- --nocapture

# 主任务栈总大小 = 两个符号相减（esp-rtos 就是这么算的）
nm <elf> | grep -E '_stack_(start|end)_cpu0'

# 改了 Cargo.toml 的 feature，逐个单独编译一遍
moon run iot:check-features

# 不提交也跑一遍 pre-commit（`--force` 才不会被“没有改动文件”跳过）
lefthook run pre-commit --force
```

## 检查的盲区

- `check-s3` 只 type-check、不链接：内存超了要到 `build-s3` / `iot:image` 才报。
- 编译期变量不在 moon 的缓存键里；`build-s3` 已 `cache: false`，别的任务没有这层保护。
- 每个 feature 能否单独编译，只有 `check-features` 管。
- 日志级别写死 `Info`（`chip-esp/src/esp32c6.rs`、`esp32s3.rs` 里的 `set_max_level`）：`log::debug!` 不会打印。
- **没有 gdb / openocd**：调试手段是日志，加一外设一个探针（`probe-*-s3`）。
- `apps/iot/target` 会涨到十几 G；`cd apps/iot && cargo clean` 要付重编的代价，S3 那一半最贵。

## 完整内容

- [分层与组合](@/development/iot/architecture.md)、[feature 判据](@/development/iot/features.md)
- [固件安装](@/development/iot/flashing.md)——产物、分区表、bench 镜像、抓完整启动日志的配方
- [无硬件仿真](@/development/iot/emulation.md)
- [运动](@/development/iot/motion.md)、[音频计量](@/development/iot/audio.md)、[放音](@/development/iot/playback.md)
- [实现记录](@/records/iot/_index.md)——为什么是现在这样、实测数字（AI 产出，未经人类审阅）

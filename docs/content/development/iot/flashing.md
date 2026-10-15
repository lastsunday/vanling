+++
title = "固件安装"
weight = 30
+++

# 固件安装

本页说明如何将 vanling 固件安装到开发板上。也有[浏览器一键安装入口](../../../flasher/index.html)可用。

## 前置条件

- 一块受支持的开发板（esp32c6：`esp32c6-devkitc-1`；esp32s3：`lckfb-szpi-esp32s3`）
- 一个固件产物。发布产物（含 `merged.bin`）来自 CD release（tag `vanling-iot@x.y.z`）；本地则用：
  ```sh
  # 在 apps/iot 下
  moon run build
  espflash save-image --chip esp32c6 --merge \
    --flash-size 8mb --flash-mode dio --flash-freq 40mhz \
    target/riscv32imac-unknown-none-elf/debug/vanling \
    vanling-merged.bin
  ```
  > **8mb 是实测值**（`espflash board-info` 读出来的，实物就是 8 MB），此前写的 `4mb` 与板子对不上。
  > 两块板各自的实测值：**C6 = 8 MB（MAC `98:a3:16:7e:55:08`）**、**S3 = 16 MB（MAC `30:ed:a0:a8:2f:c8`）**。

## 台架上有哪些板，各自是什么定位

不要假设没有硬件——台架上一直接着两块。**端口名每次插拔都会变，不能当标识。**

| 板 | 仓库 feature | 定位 |
|---|---|---|
| 立创·实战派 ESP32-S3 | `lckfb-szpi-esp32s3` | **产品靶**：320×240 屏、FT6336 触摸、双麦+喇叭、QMI8658、PCA9557、GC2145 摄像头 |
| ESP32-C6-DevKitC-1 | `esp32c6-devkitc-1` | **冒烟/仿真靶**：只有 WS2812 和 Boot 键。`UnwiredSpeaker`、面板、Audio 页全是 `None`，**产品路径在它上面不存在**，两者不可互相代替 |

已实测的硬件事实（结论与复现见[开发记录](@/records/iot/_index.md)）：

- **摄像头是 GC2145，不是立创 wiki 写的 GC0308**（`sensor id 0x2145`，读窗口 1616×1208 @5:5）
- **AEC 延迟锁得住，消除量仍无读数**：延迟搜索两次实测全程只动一次；固件的 ERLE 计量器在板上从未出过非零数，唯一的 12.3 dB 来自一次已删除、不可复现的离线 dump

## 两块板同时插着时

`espflash` 会自己挑一个端口，**有可能挑错**（`--chip esp32c6` 配 S3 会直接失败）。先认板再烧：

```sh
espflash board-info --port /dev/cu.usbmodem14401   # C6 → 98:a3:16:7e:55:08
espflash board-info --port /dev/cu.usbmodem1422301 # S3 → 30:ed:a0:a8:2f:c8

IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-s3
IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-probe-echo-s3
```

不设 `IOT_PORT` 时任务行为与从前一致（`espflash` 自己挑），但两块板都插着时必须设。

## 产物本地生成

产物流程在 GitHub Action（`reusable-iot-build.yml`）与本地等价可运行：Action 异常时，可用本地产物替补生成并上传 release 工件。一键命令（在 `apps/iot` 下）：

```sh
moon run iot:image           # 全部板：ELF + merged.bin
moon run iot:image-c6        # 仅 esp32c6-devkitc-1
moon run iot:image-s3        # 仅 lckfb-szpi-esp32s3
```

> s3 的 Xtensa 构建经 `scripts/iot-xtensa.sh` 按环境分发：espup `esp` 1.95.0.0 工具链齐备（CI/发布）时原生编译；否则本地自动回退 `espressif/idf-rust:esp32s3_1.95.0.0` 容器（macOS Intel）。同版本工具链，产物字节一致。

> 依赖缓存：容器按 `-Z build-std` 解析 esp 工具链自带的 `library/Cargo.lock`（含 memchr 2.7.6 等与项目 `Cargo.lock` 不同的版本）。离线缓存不全时，首次构建会自动在线引导补齐一次（日志提示 `bootstrapping once online`），此后构建保持 `--offline`、字节一致，无需手工预热。

`iot:image` 依赖各板的 `build-*` 任务，产物写入仓库根 `dist/`，命名与 CI 逐字符一致：

| 产物        | 命名                                              |
| ----------- | ------------------------------------------------- |
| ELF         | `vanling-iot-<board>-<version>-<target>.elf`      |
| 整片镜像    | `vanling-iot-<board>-<version>-merged.bin`        |

`version` 经 `scripts/version.sh`（读取 `apps/iot/Cargo.toml`）生成，与 CI 的 `DEV_VERSION`（`iot-dev-release.yml`）同源，本地为 `x.y.z-dev.<run>.<date>.<sha>`。

## 单一设备安装

### 1. 浏览器直刷（推荐）

打开 [Vanling 固件安装器](../../../flasher/index.html)（Chrome / Edge 桌面版）：
1. 拖入 `merged.bin`
2. 检查解析出的目标芯片与版本信息
3. USB 连接开发板
4. 点「连接设备」，确认芯片比对通过后点「写入固件」

浏览器直刷基于 WebSerial 与 esptool-js，无需安装任何本地工具。

### 2. ESP Flash Download Tool（GUI）

- Windows：Espressif 官方 [ESP Flash Download Tool](https://www.espressif.com/en/support/download/other-tools)
- 地址填写与 `save-image --merge` 相同：整片镜像写 `0x0`
- 选择合适 SPI 参数（4MB / DIO / 40MHz）

### 3. esptool 命令行

```sh
python -m esptool --chip esp32c6 write_flash 0x0 vanling-merged.bin
```

`merged.bin` 已是整片镜像，只需写 `0x0` 一个地址。

## 批量安装

- 逐台并行运行 esptool（同一 PC 多 USB 口）：
  ```sh
  for p in /dev/ttyUSB{0..3}; do
    python -m esptool --port "$p" --baud 921600 write_flash 0x0 vanling-merged.bin &
  done; wait
  ```
- 工厂模式：烧录器支持 CRC32/数据校验，可接入治具流水线；esptool 自带 `mass_mfg`（只读工厂镜像批量烧录模板）可配置 MAC/序列号变量。

## 调试用（开发者）

```sh
# apps/iot 下直接编译并烧录（espflash 在 devShell 提供）
moon run flash
# 或手动
cargo build && espflash flash target/.../vanling
espflash monitor   # console 内带回车重启
```

`espflash monitor` 需要真 tty（picocom 在这台机器上认不出自己的参数）。macOS 的 `script -q <log> <cmd>` 会给它一个 pty 并顺带录一份，所以 monitor 要包在 `script` 里跑。

> `[实测，2026-10-15 更正]` **不要让人去按 RESET 键，也不要「烧录完立刻抓」**（后者是我早先的错误结论）。
>
> 这块板子走**原生 USB**（USB-Serial/JTAG），DTR/RTS 触发的是 *core reset*，**不重新采样 strapping 引脚**，所以任何软件复位都必然落回 `boot:0x23 (DOWNLOAD(USB/UART0))`，刷完根本起不来应用。esptool 的 ESP32-S3 文档（*Leaving Download Mode in USB-Serial/JTAG Mode*）给了出路：`-a watchdog-reset` 劫持 RTC WDT 做一次**真正的系统复位**，strapping 重新采样，于是正常启动（ROM 打 `boot:0x2b (SPI_FAST_FLASH_BOOT)`）。
>
> 抓完整开机日志（先接上、再复位，不丢任何一行）：
>
> ```sh
> espflash flash --chip esp32s3 --flash-size 16mb \
>   --partition-table partitions/lckfb-szpi-esp32s3.csv \
>   --port /dev/cu.usbmodem1422301 -a watchdog-reset target/.../wifi-probe
>
> ({ sleep 4; printf '\\x12'; sleep 30; } | script -q -t 35 /tmp/boot.log \
>    espflash monitor --chip esp32s3 --port /dev/cu.usbmodem1422301 -a no-reset) >/dev/null 2>&1
> ```
>
> 那个 `printf '\\x12'` 是 monitor 里的 `CTRL+R Reset chip`，从同一个 pty 喂进去 = 已接上 + 立即复位；它比烧录时那次复位晚几秒，所以 ROM 第一行也在日志里。`script` 给 monitor 一个 pty 并顺手录一份，加 `> /tmp/x.log` 存盘，别用管道刷屏。

## 构建失败先看 Docker，再看别的

S3 的构建走 `scripts/iot-xtensa.sh`：先试本机 esp 工具链，不全则回退容器。本机那份通常是不全的（`~/.espressif` 下只有 `xtensa-esp-elf` 的 gcc，缺 `bin/cargo` 与 `lib/rustlib/src/rust/library`，`has_native_esp` 因此判否），所以实际构建**依赖 Docker 守护进程**。

> `[实测]` 守护进程挂掉时的报错是 `docker: request returned 500 Internal Server Error ... /docker.sock/_ping`，退出码 125，**不是**任何编译错误。`open -a Docker` 在无 GUI 会话的 shell 里起不来；要直接跑 `/Applications/Docker.app/Contents/MacOS/Docker`，等 `com.docker.backend` 出现。
>
> 它的危险之处在于**构建失败后旧二进制仍在**，`espflash flash` 会若无其事地把上一次的镜像刷进板子。退出码被 `grep '^error'` 一过滤就什么都看不见，于是连测三轮才发现。**每次烧录前用 `strings <elf> | grep <新加的日志字面量>` 确认产物是新的**，不要只看命令退出码。

## 启动页与 bench 镜像

`IOT_BOOT_PAGE`（`.cargo/config.toml`，默认 `ambient`）决定固件开机停在哪一页 —— `ambient` / `attitude` / `audio` / `speaker` / `camera`，非法值退回 `ambient`。它是 `option_env!`，在**编译时**展开，所以要经 Docker 构建时得转发给容器（`scripts/iot-xtensa.sh` 的 `FORWARDED_ENV`）；`moon run iot:build-s3-camera` 把它设成 `camera`，对应 `moon run iot:flash-s3-camera`。

> `[实测]` 这两个任务**不能**共用一个产物路径。`build-s3-camera` 与 `build-s3` 曾都声明 `target/.../release/vanling`，而 `flash-s3` 和 `scripts/iot-image.sh` 都读那个文件 —— 谁最后跑谁决定刷进去的是什么，moon 命中缓存时不会把自己的写回去，于是"烧产品固件"实际烧的是 camera 固件。现在 bench 构建前后保存/还原产品镜像，另存 `vanling-camera`，两个镜像并存。

> `[实测]` `option_env!` 确实被 cargo 指纹跟踪：同一份源码只改 `IOT_BOOT_PAGE` 就会重编，两个产物字节不同。因此上面的问题只是共享路径，不是缓存键 —— 不需要 `touch` 源文件。

## 固件构成

发布产物的 `merged.bin`（`save-image --merge`）为整片 Flash 镜像：

| 段        | 地址       | 说明                           |
| --------- | ---------- | ------------------------------ |
| bootloader| `0x0`      | 二级引导程序，固定脚本产物     |
| partition | `0x8000`   | 分区表                         |
| app       | 见分区表   | vanling 应用 + 数据            |

> 布局以产出 bin 时实际分区表为准；安装时整片写 `0x0` 即可，不必单个段分别写。

**分区表由本仓库提供**：`apps/iot/partitions/<board>.csv`，`flash-*` 与 `scripts/iot-image.sh`
都显式传 `--partition-table`。两处必须指向同一张表 —— 镜像任务与烧录任务各写一份时，谁最后跑决定设备上是什么布局，而两边都编译通过。

S3 那张表有两处不是默认值，各有原因（详见该文件内的注释与
[Wi-Fi 实现记录](@/records/iot/wifi.md)）：

- **factory 6 MB**（espflash 默认 1 MB）：链接了 Wi-Fi 驱动的预编译 blob，固件放不进 1 MB。S3 是 16 MB。
- **没有 `phy_init`**：esp-radio 的 Xtensa 构建把驱动的标定恢复钩子实现为 `todo!()`，而驱动会去找标定分区；没有这个分区就不走那条路，代价是每次开机重新标定。

## 浏览器安装原理

安装器页面是完全静态的（托管于 GitHub Pages）。`docs/static/flasher/` 内含：

- `index.html` + `main.js`：自解析镜像头（magic `0xE9`、chip ID、flash 参数、段表、XOR 交叉校验、app 描述符 `0xABCD5432`）。
- `vendor/esptool-js@0.6.0.bundle.js`：Espressif 官方 WebSerial 烧录内核（Apache-2.0）。

不受 OTA（联网升级）影响：安装器解决首次刷入与整机还原，OTA 解决后续增量升级。
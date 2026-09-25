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
    --flash-size 4mb --flash-mode dio --flash-freq 40mhz \
    target/riscv32imac-unknown-none-elf/debug/vanling \
    vanling-merged.bin
  ```

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

## 固件构成

发布产物的 `merged.bin`（`save-image --merge`）为整片 Flash 镜像：

| 段        | 地址       | 说明                           |
| --------- | ---------- | ------------------------------ |
| bootloader| `0x0`      | 二级引导程序，固定脚本产物     |
| partition | `0x8000`   | 分区表                         |
| app       | `0x10000`  | vanling 应用 + 数据            |

> 布局以产出 bin 时实际分区表为准；安装时整片写 `0x0` 即可，不必单个段分别写。

## 浏览器安装原理

安装器页面是完全静态的（托管于 GitHub Pages）。`docs/static/flasher/` 内含：

- `index.html` + `main.js`：自解析镜像头（magic `0xE9`、chip ID、flash 参数、段表、XOR 交叉校验、app 描述符 `0xABCD5432`）。
- `vendor/esptool-js@0.6.0.bundle.js`：Espressif 官方 WebSerial 烧录内核（Apache-2.0）。

不受 OTA（联网升级）影响：安装器解决首次刷入与整机还原，OTA 解决后续增量升级。
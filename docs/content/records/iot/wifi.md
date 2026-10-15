+++
title = "Wi-Fi 实现记录"
weight = 45
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# Wi-Fi 实现记录

`apps/iot` 的 Wi-Fi 链路：从「扫到哪些网络」到「拿到一个地址」。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

## 状态

探针已在台架上跑完（扫描 + 关联 + DHCP），产品页未开始。未完成的两件事在[文末](#待办未完成)；其余都在下面。

## 台架实测（`[实测]`，2026-10-15）

刷 `wifi-probe`（`-a watchdog-reset` 启动，命令见 `@/development/iot/flashing.md`），带构建时凭据连 `whitehell`：

| 阶段 | 耗时 |
| --- | --- |
| 扫描 6 轮 | 每轮 0.65–1.0 s |
| **关联** | **2184 / 2203 / 2189 ms**（三次开机） |
| **DHCP** | **546 / 22 / 44 ms** |
| 链接总耗时 | 开机后 9428–9789 ms |
| 地址 | `192.168.1.136` |
| 驱动 C 堆 | **32,204 B 空闲 / 65,536 B**，40 秒内一条不差 |

`esp-radio` 官方文档给的 station 堆需求是 **47–57 KiB**（默认 `ControllerConfig`）：32 KiB 会让 `esp_wifi_init` 直接 `OutOfMemory`，64 KiB 正常，余量约 49%。

周围接入点（12 轮 + 6 轮并集，每行取最强）：

| 名称 / BSSID | 信道 | 最强 | 加密 |
| --- | --- | --- | --- |
| **whitehell** | 11 | **−50 dBm** | secured |
| `94:be:09:98:cf:29` | 1 | −78 dBm | secured |
| `2305` | 6 | −81 dBm | secured |
| `15876352517` | 11 | −80 dBm | secured |
| H3C_BC5DB8 | 6 | −82 dBm | secured |
| `50:5d:7a:db:5e:6a` | 8 | −84 dBm | secured |
| 8249 | 1 | −87 dBm | secured |

中文 SSID 显示成 BSSID 是设计如此：面板字库只有可打印 ASCII，画不出的名字退回 BSSID。

链接期（`[实测]`，链接符号）：

| | 主栈 | 对 65,536 B 断言 |
| --- | --- | --- |
| 产品，无 Wi-Fi | 77,880 B | +12,344 B |
| 产品，Wi-Fi，app 堆还在 `dram_seg` | 63,788 B | **−1,748 B，链接失败** |
| 产品，Wi-Fi，app 堆在 PSRAM | **125,788 B** | +60,252 B |
| `wifi-probe` | 102,872 B | +37,336 B |

产品镜像 497,744 B / 6 MB 分区（7.91%），探针 660,992 B（10.51%，比产品大是因为它把驱动的 IRAM 段也链进来了）。Wi-Fi 自己的 14,092 B 是驱动的静态量：`esp_phy::PHY_STATE` 2,008 B、blob 的 `s_wifi_nvs` 1,440 B、`esp_rtos` 的 scheduler 1,416 B、`g_ic`/`g_pm` 760/584 B、`phy_param` 740 B，其余每项 < 512 B。

**产品镜像里目前没有那 64 KiB 驱动堆**：`init_radio_heap()` 只被探针调用，产品还没有 `take_wifi()`，链接器把两个区域当死代码丢了（`vanling` 的 ELF 里没有 `INTERNAL_MEM` / `RECLAIMED_MEM`，驱动的静态量在 —— 17 个符号）。Stage B 接上之后那 32 KiB 的 `dram_seg` 区域会回到账里：125,788 − 32,768 = **93,020 B**，仍高 27,484 B。

## 这个世代没有 lwIP，只有「驱动 + embassy-net」

`[据称，已用两条独立途径查证]` esp-rs 世代（esp-hal 1.2 / esp-rtos 0.4）**没有 TCP/IP 栈 crate**：crates.io 上 `esp-netif` 返回 404（`esp-lwip` / `esp-network` / `esp-tcpip` 同样不存在），`esp-hal` 仓库里没有对应目录，`CHANGELOG.md` 全文 0 次 `netif`、`lwip`，`examples/` 下与网络相关的例子全走 **embassy-net**。

| 层 | crate | 做什么 |
| --- | --- | --- |
| 射频 | `esp-radio` | Wi-Fi 驱动（L2），`esp-wifi-sys-esp32s3` 装预编译 blob |
| 协议 | `embassy-net` | smoltcp：DHCPv4 / IPv4 / TCP / UDP / DNS |

`esp-radio` 的 `wifi` feature **自带 `embassy_net_driver::Driver` 实现**（改名包 `embassy-net-driver-02`），所以「拿到地址」是把两样东西接起来，不是写协议。

**版本钉死 `=1.0.0-beta.1`**（唯一与本仓 `esp-hal 1.2.2` / `esp-rtos 0.4.0` / `esp-config 0.8.0` / `esp-sync 0.3.0` 逐条对上的版本）。beta.2 的 S3 源码与 beta.1 **逐字节相同**（唯一改动是 S31 编译修复），所以换版本当前没有收益。

## 内存：三个决定

### 驱动的堆：两块内部 DRAM，一块 32 KiB reclaimed + 一块 32 KiB `dram_seg`

`[实测]` `esp-radio` 是闭源 blob，通过 `malloc` 分配，所以符号必须在最终镜像里，`esp-alloc` 提供它们，而这个模块是背后的内存。

- **reclaimed 区**（0x3FCF0000 以上）是二段 bootloader 留给 flash cache 的 DRAM，esp-hal 的链接脚本把它留给堆。但它的大小 = 64 KiB − 数据缓存，默认 64 KiB 缓存时**这段是空的**（本仓既有 ELF 在 0x3FCF0000 以上没有任何符号）。`scripts/iot-xtensa.sh` 把缓存压到 `32KB`，换来 32 KiB。
- **`dram_seg` 里那 32 KiB** 只有在 app 自己的堆搬去 PSRAM 之后才付得起（见下条）：那一步把主栈从 63,788 B 抬到 125,788 B，而实测峰值 39,809 B，所以这 32 KiB 是量出来的余量，不是赌的。

两块都标 `MemoryCapability::Internal`：驱动的 pbuf 池要走 MAC 的 DMA，那条路到不了 PSRAM。

`ESP_HAL_CONFIG_DATA_CACHE_SIZE` **不能**进 `.cargo/config.toml`：esp-config 把「对本芯片不生效的选项」当未知选项直接失败，而这个选项只在 S2/S3 生效，会把 C6 的构建炸掉。所以它放在 `iot-xtensa.sh` 里，并且**按 `--target` 判定**（无条件 export 时 `moon run iot:check-c6` 直接死在 esp-config 上）。

**代价**：面板与相机 DMA 在没有一半数据缓存的情况下跑。面板整帧是 DMA 写、渲染路径自己的工作集在内部 DRAM，所以量过的那几条路径不读它 —— 但**这是预期不是实测**，`[INPUT]` 的 17–25 ms 最坏 tick 与 AEC 那些数字需要重测。

### app 的堆：搬进 PSRAM，因为没有第三处

`[实测]` 产品要装下 Wi-Fi，64 KiB 的 Rust 堆必须离开 `dram_seg`，而剩下的两处是：动相机那 150 KiB arena（已经在售的页面，**要人点头**），或者用板上那 8 MB octal PSRAM。`chip_init_at` 顺带把 PSRAM 映射好（`Psram::new(PSRAM::steal(), PsramConfig::default())`；那个 peripheral 是 `Peripherals` 的一个字段，直接取走是 partial move、函数返回不了整个结构体，`steal()` 拿到的是同一个 handle），app 的 `main` 再把堆指过去。

**映射完必须自己验**：写一个步长为 7 的图案再读回，两端各 4 KiB。esp-hal 只保证它把窗口映射出来，不保证这个 build 的 SPI 时序对，而 PSRAM 时序错了不 panic，是「十分钟后随机崩」。所以三种结果分开报：没有窗口 / 有窗口但读不回 / 可用，产品在第二种报错复位。台架实测：`Ready(len: 8388608)`。

`PsramConfig::default()` **不填 core clock**：S3 的 SPI0/SPI1 挂在 APB 上，而 [`set_cpu_clock`] 只动 CPU 不动 APB，所以 CPU 频率不是 SPI 的源时钟，钉一个 core clock 就是拿一个不是源的时钟去除。

### `wifi_force_pd`：补上 esp-radio 没人调的 `init()`

`[实测]` esp-radio 1.0.0-beta.1 里有一个 `pub(crate) fn init()`，负责开 modem 时钟、给 WiFi 域上电、脉冲复位、取 RTC 唤醒锁 —— **全 crate 没有任何调用者**。本板实测停在 ROM 的 `APB_CTRL.wifi_clk_en = 0xfffce030` 与 `dig_pwc` bit 17（`wifi_force_pd`）= 1。`chip-esp::ungate_radio()` 现在代做这几条写（上游 `init_clocks` 与 `enable_wifi_power_domain` 的 S3 臂），探针在 `take_wifi()` 之前调用，实测 `0xfffce030 → 0xffffdfdf`、`0x20000 → 0`。

**它没有让扫描变稳**（改前 0/0/1，改后 0/1/1/1/1/1 与 1/1/0/1/0/1），所以射频的电源/时钟状态不是扫描抖动的成因。保留是因为谁也不该让驱动跑在上电状态没人设的射频上，不是因为它解决了什么。

## 扫描：一次不是测量，两次也是

`[实测]` 同一个镜像连刷三次，第一次扫描分别返回 0 / 0 / 1 个 AP。连扫 6 轮，两次开机：

```
开机 A:  0, 1, 1, 2, 2, 1 个
开机 B:  1, 1, 1, 2, 0, 0 个
```

**结论只能是「单次扫描不是测量」**：条数在同一次开机里就来回跳。附近最强的 AP 稳定在 −50 dBm，所以不是信号余量的问题。**曾经写成过「第一次扫描永远是空的」，被开机 B 直接推翻**（它第一次就有，而第 5、6 轮是空的）。

**两个因素被分开了：**

| | 每信道驻留 | 一轮耗时 | 12 轮的结果 |
| --- | --- | --- | --- |
| 驱动默认 | 10–20 ms | 135–160 ms | 1～2 个 |
| `CHANNEL_DWELL` 50–120 ms | 50–120 ms | 0.65–1.0 s | 1～6 个 |
| 同上，**相机不启动** | 50–120 ms | 0.73–0.88 s | **无一为空** |
| 同上，**相机在跑** | 50–120 ms | 0.65–1.0 s | **1 轮为空** |

**驻留时间决定扫得全不全**（1～2 → 1～6），代价是一次用户等待从 0.15 s 变成 0.8 s，换来的是完整列表而不是最强的两三张网。**相机决定会不会整轮为空**，机制未知：它是持续跑的 DVP DMA，与 WiFi 驱动任务同核，抢总线/中断是最可能的嫌疑。这一条不阻塞交付 —— 产品页按「空结果就继续扫」处理即可（探针现在就是这么做的，`SCAN_ROUNDS = 6`，间隔 500 ms）。

## 「已连接」= 关联 **且** 拿到地址，而地址要有人回说

`[据称，来自参考固件]` xiaozhi-esp32 的 `WifiStation` 里 `WIFI_EVENT_STA_CONNECTED` 的处理函数是**空的**，连接位是在 `IP_EVENT_STA_GOT_IP` 里置的。本仓库照这个口径：关联成功而没有地址的链路，产品在上面做不了任何事。

`[实测]` 带凭据跑起来才暴露：联上、拿到地址、40 秒报告全写 `phase Connecting` —— **`Connected` 这个分支全仓没有任何一处赋值**。地址归 embassy-net 的 stack 所有，而 `Station` trait 上只有 `status()`，拿到地址的那一层没有回说的接口。照这个接口做产品页，连上之后面板会一直显示「连接中」。

规则因此放进 core，成为可测的值而不是注释：`LinkStatus::with_address(Option<Ipv4>)`（有地址即 `Connected`；地址没了退回 `Connecting`；`Failed` 不被地址复活，三个 host 测试钉住）+ `Station::note_address`。调用点是**每个报告窗口一次**，不只是第一次 —— 租约失效只有那个循环看得见。改完再跑：`phase Connected` 稳 40 秒。

## 连接策略抄参考固件的三条

`[据称，来自 xiaozhi-esp32]` 本板是 ESP32-S3-WROOM-1-N16R8，与参考固件同一颗模组（注意：当前 checkout 里已经没有 `wifi_station.cc` 这个路径，引用前先重新定位）：

| 设置 | 参考固件的理由 | 本仓取值 |
| --- | --- | --- |
| `scan_method` | 默认快速扫描取第一个匹配，**最强的同名 AP 一次认证失败就被换成弱的那个**（那边实测连上 −79 dBm，而 −73 dBm 的同名 AP 就在范围内） | `AllChannels` |
| `failure_retry_cnt` | 同上，重试够了才退到弱 AP | 3（默认 1） |
| `listen_interval` | AP 可以在这多个信标间隔里跳过我们，省电换延迟 | 10 信标（默认 3） |

## 分区表

`[实测]` espflash 默认给 esp32s3 写 factory 1 MB，而本仓固件加 Wi-Fi 之前已经 630 KB。`apps/iot/partitions/lckfb-szpi-esp32s3.csv` 给 factory **6 MB**，并带一个 4 KiB 的 `phy_init`。

`phy_init` 原本**故意没写**：esp-radio 的 Xtensa 构建把驱动的标定恢复钩子实现成 `todo!()`（`ld/esp32s3_provides.x` 把 `misc_nvs_restore` 绑到它，blob 里是未定义引用），而驱动看得见标定分区就会走那条路。`[实测]` **这条路走不到** —— 分区在场时 `esp_wifi_init` 不 panic，扫描正常。分区保留，它就是驱动第一次开机写下标定数据的地方。

表按**板子**命名：`scripts/iot-image.sh` 一直按板名找 `partitions/<board>.csv`，而布局本来就是板级事实（同是 S3，换一块 8 MB 板子就是另一张表）。6 MB 的槽装不进 espflash 默认的 4 MB，所以每个 S3 烧录/镜像任务都带 `--flash-size 16mb`。C6 那张与 espflash 默认等价，但同样写成文件：镜像任务和烧录任务对布局只有一处认知。

## 特征与分层落点

`wifi` 是**元件 feature（模块轴）**，在 `iot-bsp-esp` 里一个文件一个模块，门控只在 `components/mod.rs` 一处；S3 板 feature 聚合它。`iot-core` 零 cfg：能力经组合点注入（`HasWifi`）。

`Station` 的方法用 RPITIT 返回 `impl Future`，**不要 `+ Send`**：产品把这些 future 与 input/render/control/feed 一起 `join` 在同一个任务栈上，没有 spawn。（`Speaker` 需要 `Send` 是因为它的 feed 在独立的高优先级 executor 上。）

`take_wifi()` 不能在 `Board::new` 里做：**驱动的启动需要调度器已经跑起来**（`esp_rtos::start()` 在 app 的入口里），所以板子交出的是**外设**而不是 station。

`iot-chip-esp` 的两个芯片 feature 各带五个芯片 feature 转发（`esp-hal` / `esp-rtos` / `esp-println` / `esp-backtrace` / `esp-bootloader-esp-idf`），console 另是一条轴（`console-auto` / `console-uart`），app 转发过来。原因是那三个 crate 的 build script 各自要求「恰好一个芯片 feature」和一个 console，平时都由 app 顺手给，于是任何**没有 app 在场**的构建死在别人的 build script 上 —— `iot-feature-check.sh` 第一次真跑到 `wifi` 就抓到了。这就是 `features.md` 规则 5：**用到的每一处都要自己写出来**。

## 纯逻辑部分（宿主机可测）

`iot-core::drivers::network` 里两条规则是产品决定而不是管道，所以放在能被测试打到的层：

- **扫描结果按 BSSID 去重、按 rssi 排序、截断到 8 条**。同名不同 AP 是两个网络（手机也这么显示），所以身份是 BSSID 而不是名字。
- **SSID 是字节**。802.11 允许任意八位组，面板字库只有可打印 ASCII，所以「这个名字能不能画出来」是一个带退路的判断（画不出就显示 BSSID）。非 ASCII 的名字仍然逐字节保存（连接需要它），只是不可绘制。

`Credentials::from_text` **拒绝**超长口令而不截断 —— 驱动也是拒绝，而截断过的口令对着任何东西都认证不上，读起来却像「密码错了」。

凭据走**构建时环境变量**（`IOT_WIFI_SSID` / `IOT_WIFI_PSK`，`option_env!`），不是 secret store：bench 镜像要说自己连的是哪个网络，而编进去的值就是构建命令行里的值。产品的凭据要在面板输入、存 NVS，绝不能编译进固件。

## 待办（`[未完成]`）

1. **相机的 DVP DMA 为什么能吃掉整轮扫描**：12 轮里 1 轮，没有机制。产品页按「空结果就继续扫」处理，不阻塞交付。
2. **产品页要调 `note_address`**：那是「已连接」唯一的通路，漏了就会一直显示「连接中」。
3. **改缓存之后重测** `[INPUT]` / AEC 的既有数字。
4. 产品页（`DisplayPage::Network`、意图、面板键盘、凭据持久化）尚未开始。它**不能拿一轮扫描的结果当答案**：实测单轮在 0～6 个之间跳。
5. `esp-radio` 是 beta：升级版本时重跑本页全部。
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

**两颗芯片的探针都在台架上跑完**（扫描 + 关联 + DHCP）。产品页分两片：**只读的那一片已落地**（`DisplayPage::Network`：阶段 + 当前 AP + 地址 + 扫描列表），**第二片也落了盘**（4×4 键盘、回显键、逐行滑动的列表、列表迟滞与缓存、选中行反色、开放网络、失败分档、扫描门控与退避，以及**工具条**：有链路时一个 `LEAVE` 按钮、无链路时一个 `SCAN` 按钮，前者执行前问一次、后者不问 —— 见「产品页」），**凭据已落盘**（`creds` 分区 + 自控槽格式，开机自己读回来、关联成功即写，见「凭据落在哪」），**已存网络在列表里画 `*`、长按那一行 = 忘记**，**点一下已存的那一行就用设备自己存的那份口令去连**（见「点已存的一行就用库里的口令」）。后四轮报来的缺陷与两处结构修正还在这份记录里。面板这条路有一条**不需要手指**的验法：`IOT_PANEL_BENCH` 构建开机后按面板自己的坐标把口令打一遍，再按工具条与确认框的坐标走完断开、点回那一行、断开、忘记、点回那一行、取消与重扫（见「按面板路径打字」与后四轮的读数）；它证明的是状态机与几何，而**触摸手感与纯黑/背光仍要人点一次面板才算验过**（板上只有 FT6336 产面板坐标，软件合成不了触摸）。状态层另有一层**可达状态穷举**（35 个动作、8,800 个状态、四条不变量，见「离开后点回那一行、以及一次丢掉的列表修正」）。**没有面板的板子也有了一扇前门**：`iot-core::provision` 是协议（45 条宿主机测试），`iot-app::provision` 把它接在「开放 AP + DHCP + 全答 DNS + 配置页」上，与面板那扇门在 `configure` 里 `select` 竞争（见「没有面板的板子怎么配网」）；面板那条路一点没动。**这一扇门两块板都已在台架上跑通**（C6 全链路、S3 修掉三处之后，见「台架：从链接到手机，再到四轮修正」）。**回到设置模式的入口之一是启动页**：两块板开机先画 1.5 s，按住 BOOT 才进设置模式，落 network 页；启动页这 1.5 s 里一条链路都不建（见「回到设置模式：启动页上按住 BOOT」）。**没有凭据 = 离线设备**：不自动进设置模式、不广播 AP，面板页只在有人看时扫描（页上保留 `SCAN | AP` 两格，按 `AP` 才起接入点，有屏板这一档默认带口令）；存盘网络带 BSSID/信道提示，下次开机直接去那个信道（`fast_connect`）。配置页已有第三版：**首屏列表内嵌、图标内联、一次刷新只开一条连接、只有按钮才唤醒扫描**，语言切换在页头、密码走底部弹窗、每行带 BSSID（见「刷新七八次就白屏」）；面板的列表行也在同一天补上了该网络自己的 BSSID（名字下一行，见同节）。未完成的事在[文末](#待办（[未完成]）)；其余都在下面。

## 台架实测（`[实测]`，2026-10-15）

同一份探针代码，两颗芯片各刷一次，带构建时凭据连本机那个网络（下文写作 `<ssid>`；网络名与口令都只进构建命令行，不落仓库）。S3 用 `-a watchdog-reset` 启动、C6 用普通复位（原因见 `@/development/iot/flashing.md`，命令也在那里）：

| | S3（LCKFB SZPI） | C6（DevKitC-1） |
| --- | --- | --- |
| 扫描 6 轮 | 每轮 0.65–1.0 s，1–6 个 AP | 每轮 0.73–0.80 s，1–4 个 AP |
| **关联** | **2184 / 2203 / 2189 ms**（三次开机） | **3246 / 3261 ms**（两次开机） |
| **DHCP** | **546 / 22 / 44 ms** | **31 / 548 ms** |
| 链接总耗时 | 开机后 9428–9789 ms | 开机后 10325–10995 ms |
| 地址 | `192.168.1.10` | `192.168.1.11` |
| 驱动 C 堆 | **32,204 B 空闲 / 65,536 B**，40 秒内一条不差 | **14,124 B 空闲 / 49,152 B** |
| `phase Connected` | 稳 | 稳 |

> `[实测，2026-10-16 更新]` 表里 S3 的 **65,536 B 是那天的预算**。从今天起两颗芯片都是 **49,152 B**（S3 的 `dram_seg` 那一半从 32 KiB 收到 16 KiB，原因见下面「驱动的堆」那一节）。拿着凭据把 S3 探针重跑一遍（同一段扫描 + 关联 + DHCP）：
>
> - 驱动初始化 + 6 轮扫描在 48 KiB 下通过，没有 `OutOfMemory`；
> - 关联用 **2,228 ms**，此后 **42 ms** 拿到地址 `192.168.1.10`（开机后 9,337 ms 链路起来）；
> - 连上之后驱动堆 **15,820 B 空闲 / 49,152 B**（32.2%），85 秒里 38 次读数一条不差 —— 表里那天 S3 的 32,204 B 是 64 KiB 预算下的读数；
> - 产品镜像走同一条路：`[WIFI] this build joins the network named …`、`associated`、`phase Connected`，无 panic。

两块板看到的是同一批 AP（`<ssid>` 最强 −45…−50 dBm，`ap1`、`ap5`、`ap6`、`ap7`、`AA:00:00:00:00:01` 等），所以两张表之间的差别是 part 的差别，不是环境的差别。

`esp-radio` 官方文档给的 station 堆需求是 **47–57 KiB**（默认 `ControllerConfig`，高端那一档对应 6 个动态 RX buffer，正是本仓的取值）：32 KiB 会让 `esp_wifi_init` 直接 `OutOfMemory`。实测两颗芯片的驱动各用掉 33,332 B（S3）/ 35,028 B（C6）—— **C6 多用 1,696 B**，所以同一份代码在两颗芯片上要有两种给法（总数现在都是 49,152 B），见「同一套代码，两颗芯片」。

周围接入点（S3 侧 12 轮 + 6 轮并集，每行取最强）。**名称与 BSSID 是占位**（`ap1`…、`AA:00:…`），信道、信号、加密是实测值：

| 名称 / BSSID | 信道 | 最强 | 加密 |
| --- | --- | --- | --- |
| **`<ssid>`** | 11 | **−50 dBm** | secured |
| `AA:00:00:00:00:01` | 1 | −78 dBm | secured |
| `ap1` | 6 | −81 dBm | secured |
| `ap2` | 11 | −80 dBm | secured |
| ap3 | 6 | −82 dBm | secured |
| `AA:00:00:00:00:02` | 8 | −84 dBm | secured |
| ap4 | 1 | −87 dBm | secured |

中文 SSID 显示成 BSSID 是设计如此：面板字库只有可打印 ASCII，画不出的名字退回 BSSID。

链接期（`[实测]`，链接符号；下限来自 `.cargo/config.toml` 的 `ESP_HAL_CONFIG_ENSURE_MAIN_STACK_MINIMUM = 65536`，由 esp-hal 的 `ld/sections/stack.x` 变成链接期 `ASSERT`）：

| | 主栈 | 对 65,536 B 断言 |
| --- | --- | --- |
| S3 产品，无 Wi-Fi | 77,880 B | +12,344 B |
| S3 产品，Wi-Fi，app 堆还在 `dram_seg` | 63,788 B | **−1,748 B，链接失败** |
| S3 产品，Wi-Fi，app 堆在 PSRAM | **125,788 B** | +60,252 B |
| S3 产品，Wi-Fi，驱动堆 48 KiB（2026-10-16） | **72,056 B** | +6,520 B |
| S3 产品 + Network 页（2026-10-16 晚） | **70,008 B** | +4,472 B |
| S3 产品 + T9 键盘与列表（2026-10-16 夜） | **68,792 B** | +3,256 B |
| S3 `wifi-probe` | 102,872 B | +37,336 B |
| C6 `wifi-probe` | 247,400 B | +181,864 B |
| C6 产品 `vanling` | 348,124 B | +282,588 B |

C6 侧余量这么大有两个原因，都是结构性的：它的内部 DRAM 有 400 KiB（S3 的 512 KiB 里有 150 KiB 被相机的帧 arena 占着），而这块板子上没有面板也没有相机。

S3 产品镜像 **852,432 B / 6 MB 分区（13.55%）**、探针 664,768 B（10.57%）—— 产品现在也把驱动的 IRAM 段链进来了（此前只有探针链），所以两者之差回到产品自己那部分代码。C6 侧不需要 6 MB：产品 **650,560 B / 1 MB（62.04%）** —— 接上驱动代码之后从 223,584 B（21.32%）涨了三倍，那张 1 MB 的槽只剩 38%，是两颗芯片里最紧的一个。Wi-Fi 自己的**静态量**是 **14,092 B**（S3 侧量得；不含 IRAM 代码在 `dram_seg` 里的镜像）：`esp_phy::PHY_STATE` 2,008 B、blob 的 `s_wifi_nvs` 1,440 B、`esp_rtos` 的 scheduler 1,416 B、`g_ic`/`g_pm` 760/584 B、`phy_param` 740 B，其余每项 < 512 B。

`[实测]` **这一段是产品接上 Wi-Fi 之前的状态。** 那时 `init_radio_heap()` 只被探针调用：产品的 `vanling` 里没有 `INTERNAL_MEM` / `RECLAIMED_MEM`，链接器把区域当死代码丢了，而驱动的静态量在 —— S3 侧 17 个符号。产品接上 `take_wifi()` 之后下面这条账出现，「为什么 S3 的预算最后收到 48 KiB」的答案也在里面。

`[实测]` **接上 Wi-Fi 之后两块都回到账里，而账比原估计多 40 KB。** 拿 HEAD 的 `vanling` ELF 当基线（`_data_start = 0x3fc8d0b0`、`_stack_start_cpu0 = 0x3fce9710`，主栈 125,788 B —— 就是上表那一行），产品接上 `init_radio_heap()` 之后 `ld` 的数据段游标停在 `0x3fcdc6f8`：静态量涨了 **72,518 B**，拆开是三块：

| 来源 | 字节 | 是什么 |
| --- | --- | --- |
| `INTERNAL_MEM` | 32,768 | `dram_seg` 里给驱动的那块，见下面「驱动的堆」那一节 |
| blob 的 IRAM 代码 | 31,980 | `.rwtext.wifi`；同一块 SRAM 有两个地址，所以 esp-hal 的 `.rwdata_dummy` 会在 `dram_seg` 里照它的大小**再留一份** |
| 应用自己的静态量 | 约 7.7 KB | `network.rs` 等接上 Wi-Fi 之后新引用的东西（没逐符号拆） |

`RECLAIMED_MEM` 那 32 KiB **不在这个账里**：它落在 `0x3fcf0000`，正好在 `dram_seg` 的尾巴外面，不占主栈。早先把 reclaimed 算进来、又漏掉 IRAM 镜像 —— 两块凑巧都约 32 KiB，所以总数看着对、拆分是错的。

早先这里按「只有 `dram_seg` 回来」算过 125,788 − 32,768 = 93,020 B，还以为能过 —— 两个错都在这条账里：IRAM 代码那 31,980 B 的镜像没算，`network.rs` 的静态量也没算。**以链接器游标为准**，而且这条失败可以一行复现：把 `INTERNAL_BYTES` 改回 32 KiB 重建，`ld` 报的还是同一句 —— `stack.x:13 cannot move location counter backwards (from 3fcdc6f8 to 3fcdb700)`，游标与上面那次完全一致。那一行要落到 `RWDATA` 的末尾，而静态量已经越过它 2,400 B，主栈位置只剩 53,272 B，比断言下限短 12,264 B。

`[实测]` **capture 平面（方向 ②）省回 11,464 B。** `Channel<T, N>` 的每个槽按 `size_of::<T>()` 分配，所以 `INTENT_BUS` 的槽宽由最宽的 `InputEvent` 变体决定 —— 采集窗口 `Audio(AudioSample)` 是 1,760 B，8 槽就被它定价。窗口改走单槽平面之后：

| | 之前 | 之后 |
| --- | --- | --- |
| `INTENT_BUS` | **14,184 B**（HEAD ELF 的符号，`nm -S`） | **896 B**（`size_of::<Intent>()` 104 B × 8 + 64 B 通道状态） |
| 采集窗口 | 在 `INTENT_BUS` 里 | `CapturePlane` **1,824 B** |

差口因此从约 23.7 KB 收到 **12,264 B**。

## 这个世代没有 lwIP，只有「驱动 + embassy-net」

`[据称，已用两条独立途径查证]` esp-rs 世代（esp-hal 1.2 / esp-rtos 0.4）**没有 TCP/IP 栈 crate**：crates.io 上 `esp-netif` 返回 404（`esp-lwip` / `esp-network` / `esp-tcpip` 同样不存在），`esp-hal` 仓库里没有对应目录，`CHANGELOG.md` 全文 0 次 `netif`、`lwip`，`examples/` 下与网络相关的例子全走 **embassy-net**。

| 层 | crate | 做什么 |
| --- | --- | --- |
| 射频 | `esp-radio` | Wi-Fi 驱动（L2），`esp-wifi-sys-esp32s3` 装预编译 blob |
| 协议 | `embassy-net` | smoltcp：DHCPv4 / IPv4 / TCP / UDP / DNS |

`esp-radio` 的 `wifi` feature **自带 `embassy_net_driver::Driver` 实现**（改名包 `embassy-net-driver-02`），所以「拿到地址」是把两样东西接起来，不是写协议。

**版本钉死 `=1.0.0-beta.1`**（唯一与本仓 `esp-hal 1.2.2` / `esp-rtos 0.4.0` / `esp-config 0.8.0` / `esp-sync 0.3.0` 逐条对上的版本）。beta.2 的 S3 源码与 beta.1 **逐字节相同**（唯一改动是 S31 编译修复），所以换版本当前没有收益。

## 内存：三个决定（S3 上的账）

这一节讲 S3 的三个决定；C6 的对应选择在[下面那一节](#同一套代码，两颗芯片)。

### 驱动的堆：两块内部 DRAM，一块 32 KiB reclaimed + 一块 16 KiB `dram_seg`

`[实测]` `esp-radio` 是闭源 blob，通过 `malloc` 分配，所以符号必须在最终镜像里，`esp-alloc` 提供它们，而这个模块是背后的内存。

- **reclaimed 区**（0x3FCF0000 以上）是二段 bootloader 留给 flash cache 的 DRAM，esp-hal 的链接脚本把它留给堆。但它的大小 = 64 KiB − 数据缓存，默认 64 KiB 缓存时**这段是空的**（本仓既有 ELF 在 0x3FCF0000 以上没有任何符号）。`scripts/iot-xtensa.sh` 把缓存压到 `32KB`，换来 32 KiB。
- **`dram_seg` 里那一半**（先是 32 KiB，2026-10-16 收到 **16 KiB**）只有在 app 自己的堆搬去 PSRAM 之后才付得起（见下条）：那一步把主栈从 63,788 B 抬到 125,788 B，而实测峰值 39,809 B。**这次收到 16 KiB 是另一条账**：blob 的 IRAM 代码要在 `dram_seg` 里留一份同样大的镜像（31,980 B），加上产品自己的静态量之后静态量越过 `RWDATA`、主栈位置只剩 53,272 B（短断言 **12,264 B**），链接直接失败 —— 见「同一套代码，两颗芯片」里的驱动堆那一节。

两块都标 `MemoryCapability::Internal`：驱动的 pbuf 池要走 MAC 的 DMA，那条路到不了 PSRAM。

`ESP_HAL_CONFIG_DATA_CACHE_SIZE` **不能**进 `.cargo/config.toml`：esp-config 把「对本芯片不生效的选项」当未知选项直接失败，而这个选项只在 S2/S3 生效，会把 C6 的构建炸掉。所以它放在 `iot-xtensa.sh` 里，并且**按 `--target` 判定**（无条件 export 时 `moon run iot:check-c6` 直接死在 esp-config 上）。

**代价**：面板与相机 DMA 在没有一半数据缓存的情况下跑。面板整帧是 DMA 写、渲染路径自己的工作集在内部 DRAM，所以量过的那几条路径不读它 —— 但**这是预期不是实测**，`[INPUT]` 的 17–25 ms 最坏 tick 与 AEC 那些数字需要重测。

### app 的堆：搬进 PSRAM，因为没有第三处（S3）

`[实测]` 产品要装下 Wi-Fi，64 KiB 的 Rust 堆必须离开 `dram_seg`，而剩下的两处是：动相机那 150 KiB arena（已经在售的页面，**要人点头**），或者用板上那 8 MB octal PSRAM。`chip_init_at` 顺带把 PSRAM 映射好（`Psram::new(PSRAM::steal(), PsramConfig::default())`；那个 peripheral 是 `Peripherals` 的一个字段，直接取走是 partial move、函数返回不了整个结构体，`steal()` 拿到的是同一个 handle），app 的 `main` 再把堆指过去。

**映射完必须自己验**：写一个步长为 7 的图案再读回，两端各 4 KiB。esp-hal 只保证它把窗口映射出来，不保证这个 build 的 SPI 时序对，而 PSRAM 时序错了不 panic，是「十分钟后随机崩」。所以三种结果分开报：没有窗口 / 有窗口但读不回 / 可用，产品在第二种报错复位。台架实测：`Ready(len: 8388608)`。

`PsramConfig::default()` **不填 core clock**：S3 的 SPI0/SPI1 挂在 APB 上，而 [`set_cpu_clock`] 只动 CPU 不动 APB，所以 CPU 频率不是 SPI 的源时钟，钉一个 core clock 就是拿一个不是源的时钟去除。

### `wifi_force_pd`：补上 esp-radio 没人调的 `init()`（S3）

`[实测]` esp-radio 1.0.0-beta.1 里有一个 `pub(crate) fn init()`，负责开 modem 时钟、给 WiFi 域上电、脉冲复位、取 RTC 唤醒锁 —— **全 crate 没有任何调用者**。本板实测停在 ROM 的 `APB_CTRL.wifi_clk_en = 0xfffce030` 与 `dig_pwc` bit 17（`wifi_force_pd`）= 1。`chip-esp::ungate_radio()` 现在代做这几条写（上游 `init_clocks` 与 `enable_wifi_power_domain` 的 S3 臂），探针在 `take_wifi()` 之前调用，实测 `0xfffce030 → 0xffffdfdf`、`0x20000 → 0`。

**它没有让扫描变稳**（改前 0/0/1，改后 0/1/1/1/1/1 与 1/1/0/1/0/1），所以射频的电源/时钟状态不是扫描抖动的成因。保留是因为谁也不该让驱动跑在上电状态没人设的射频上，不是因为它解决了什么。

## 同一套代码，两颗芯片

`[实测]` 探针与产品用的是**同一份源码**，两颗芯片各自构建各自烧录。差异全部落在 `iot-chip-esp` 之下，调用方看到的是一个入口：

```rust
iot_chip_esp::init_heap(&HEAP, HEAP_BYTES);   // 堆从哪来：这一句不问
iot_chip_esp::ungate_radio();                // 射频上电：这一句不问
let station = board.take_wifi();             // 射频是哪个 part 的：这一句不问
```

具体地，`take_wifi()` 的实现在两块板的板模块里各写一份（板层按构造就知道自己是谁），而 `esp-radio` 的 blob 由板特征选：`lckfb-szpi-esp32s3` 带 `esp-radio/esp32s3`、`esp32c6-devkitc-1` 带 `esp-radio/esp32c6`。`esp-alloc` 的芯片特征同理，**从 `radio` 这个组件 feature 里挪到了芯片 feature 上**（`esp-alloc?/<chip>`），因为「哪个 part 在编译」只有芯片 feature 知道；`scripts/iot-feature-check.sh` 现在把这套组合在两颗芯片上各编译一遍。

### 通用堆：S3 拿 PSRAM，C6 拿一块静态量

`[实测]` C6 **没有 PSRAM**（esp-hal 的 `psram` 模块按 `soc_has_psram` 编译，C6 不在其中），所以「app 的堆放 PSRAM」这条只在 S3 上成立。C6 的做法是芯片层里一块 64 KiB 的 `dram_seg` 静态量 —— 与产品入口原先自己声明的那块是同一块内存、同一节，搬进芯片层只是因为**谁提供内存是 part 的事实，不是入口点该知道的事**。两个 arm 的差别只在内存从哪来，`HeapSource` 只出现在日志里。

`[实测]` 入口点现在只有一句 `init_heap`，它同时管三件以前散在调用方的事：窗口从哪来、`bytes` 是否超出这块 part 能给的、要不到窗口时复位并留下原因（"this module has no PSRAM to map" / "PSRAM mapped and did not read back"，后者附上映射的大小 —— 那个数只在 `init_psram` 里还看得到，所以日志留在那里）。

### 驱动的堆：S3 两块，C6 一块

`[实测]` **两颗芯片现在是同一个总数 49,152 B（48 KiB）、不同的分法**：C6 只有 `dram_seg` 一块 48 KiB，S3 是 32 KiB reclaimed + **16 KiB** `dram_seg`。上板日志一行就是这两块：`[WIFI] driver heap: 16384 B in dram_seg + 32768 B above it (dcache-reclaimed), both internal`。

`[实测]` **S3 的 `dram_seg` 那一半从 32 KiB 收到 16 KiB（2026-10-16）**，理由是上面那条账：blob 的 IRAM 代码要在 `dram_seg` 里留一份同样大的镜像（31,980 B），加上产品自己的静态量之后静态量越过 `RWDATA` 的末尾，主栈位置只剩 53,272 B（短断言 **12,264 B**），链接**失败** —— 把常量改回 32 KiB 可以一行复现。收的是 `dram_seg` 那一半，不是 reclaimed 那半 —— 后者落在 `0x3fcf0000`，在 `dram_seg` 之外，本来就不占主栈。

收完的实测（链接期看符号，运行值看上板日志）：

| | 值 |
| --- | --- |
| 主栈，链接期 | **68,600 B**（列表与键盘落地前 70,008 B；T9 那一版 68,792 B），断言下限之上 **+3,064 B** |
| 主栈峰值，上板 | **37,281 B** of 68,600 B，余 **31,319 B**（54%） |
| 驱动堆 | 49,152 B（16,384 `dram_seg` + 32,768 reclaimed），关联 + DHCP 之后余 **15,820 B**（实测） |

页面的那 2,048 B 是 `nm -S` 差值读出来的两处，不是估的：新静态 `SCAN`（`Channel<CriticalSectionRawMutex, ApList, 1>`）**368 B**，`POOL`（装 `run` future 的 executor pool）**+1,616 B** —— 后者是 `DeviceState` 多了 `Option<ApList>` 之后 future 布局跟着变宽。峰值这一轮是 **37,281 B**（只读版那次 32,353 B），它定在启动那一段 —— 第一次扫描与第一次画列表之间 —— 之后不再抬升（时刻与佐证见「上板读数」），所以这 4,928 B 里既有页面的绘制路径，也有那一轮扫描自己，本仓没有把两者分开量。

四条约束决定了「48 KiB 分两块还是一块」：

- **`#[ram(unstable(dcache_reclaimed))]` 在 C6 上编不出来** —— 那个 procmacro feature 只由 `esp-hal/esp32s3` 打开（`esp-hal/Cargo.toml` 里它是 `esp32s3 = [...]` 的一行；esp32s2 也没开）。所以 reclaimed 那块是 S3 独有的，不是「忘了在 C6 上加」。
- **reclaimed 那块不占主栈，但不是白来的**：它是二级 bootloader 的 flash cache 让出来的 DRAM，esp-hal 的链接脚本把它留给堆，代价是**把数据缓存砍半**（`scripts/iot-xtensa.sh`）—— 换掉的是缓存命中率。
- **C6 上没有第三个地方**。它有 400 KiB 内部 DRAM；64 KiB 给 app 堆、48 KiB 给驱动堆之后，主栈**实测** 247,400 B（断言下限之上 181,864 B），其余是驱动的静态量。
- **余量现在两边一样紧**：C6 的驱动实测用掉 35,028 B（48 KiB 里余 28.7%），S3 用掉 33,332 B（48 KiB 里余 32.2%；用量是 10-15 那次在 64 KiB 预算下量的，预算本身今天上板量到）。**调大 `DYNAMIC_RX_BUFFERS` 是两块板一起付账** —— 文档区间 47–57 KiB 的高端正是 6 个动态 buffer。

**要把那 32 KiB 拿回来，唯一的路是把 blob 的 IRAM 代码送去 flash，本仓做不到。** esp-hal 的脚本里这件事得插在 `.rotext_dummy` 的预留和 `rwtext.x` 的归属规则之间，而两者在**同一个生成文件**里（`ld/esp32s3/esp32s3.x`），不在仓库手上。试过的做法（脚本排在 `linkall.x` 之前去抢那几段）链接是过的、镜像也是坏的：预留段永远落在 `.text` 之前，被送去 flash 的代码只能落在 IROM 原点 `0x42000020`，flash 偏移 `0x20`，和 `.rodata` 的头一页撞上 —— 段表里 `paddr` 就是它的 vaddr，两个段会写到同一片 flash。正路是给 esp-hal 加一个能容纳这件事的钩子（上游），不是本仓自己拼脚本。

### `ungate_radio` 在 C6 上是空操作，而且是查出来的

`[实测，逐条核对 esp-radio 1.0.0-beta.1 源码]` S3 那份是照 `clocks_ll/esp32s3.rs` 与 `common_adapter.rs` 抄的寄存器写。**C6 上 esp-radio 那三个调用全是空的**：

| esp-radio 的调用 | C6 上 |
| --- | --- |
| `init_clocks()` | 空，注释写着 `// done in esp-hal` |
| `reset_wifi_mac()` | 空 |
| `enable_wifi_power_domain()` | 整段 `#[cfg(not(any(soc_has_pmu, esp32c2)))]`，而 C6 有 `soc_has_pmu`、没有 `APB_CTRL` |
| 剩下真正要写的时钟位 | 驱动自己写：`os_adapter/mod.rs` 在启动时调 `enable_wifi(true)`，写 `MODEM_SYSCON.clk_conf1` 与 `MODEM_LPCON.clk_conf` |

所以 C6 的 arm 是一个空函数体加这段表 —— 不是「还没写」，是**写出来的东西会在 C6 上被 cfg 掉**。S3 那份仍然保留，因为它在 S3 上真的改寄存器（`0xfffce030 → 0xffffdfdf`、`dig_pwc 0x20000 → 0`），而且不改扫描也照样能扫到 AP（见下一节）。

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

### 一轮扫描的节奏

产品**只在扫描列表真的在屏上时才问射频**（`DisplayPage::Network`，见「页面何时扫」）；只要在扫，节奏放在 `iot-core::drivers::network`（`SURVEY_PAUSE_MS = 5_000`、`SURVEY_EMPTY_PAUSE_MS = 1_000`、`SURVEY_EMPTY_RETRIES = 3`、空闲退避的 `SURVEY_IDLE_PAUSE_MS = 15_000` / `SURVEY_IDLE_MAX_PAUSE_MS = 30_000` / `SURVEY_IDLE_ROUNDS = 3`，判据是 `survey_pause_after(empty_rounds, idle_rounds)`），因为它和 `MAX_LISTED_APS` 一样是「怎么问射频」的政策，而不是循环的实现细节 —— 而且只有设备能跑到的路，必须由测试钉住。

规则三条：**找到东西就歇满 5 s**；**空轮 1 s 后重试，但连续三个空轮之后就不再加急**（第四个起按 5 s 的间隔）；**连续三轮没人碰过面板就把间隔退到 15 s，再连续三次退到 30 s**（`idle_rounds` 只被点击/滑动/换页清零）。后者是因为一个没人看的列表每 5.8 s 重扫一次，代价全在射频与主栈上，而屏幕上什么都没变。

`[测试]` 三条钉住它（`a_survey_that_found_something_rests_the_whole_pause`、`an_empty_place_stops_being_retried_every_second`、`a_room_nobody_acts_on_is_read_less_and_less_often`）。前两条是补的，因为上板先抓到了错：判据写成 `*empty_rounds <= SURVEY_EMPTY_RETRIES` 时，**非空**轮（计数为 0）也落进快重试那一支，日志立刻是每 1.8 s 一轮而不是 5.8 s。改完重刷，间隔回到 5.8 s，空轮那次重试在 1.76 s 后。

### 列表为什么不跳

`[实测]` 产品镜像 281 行的读数里，同一张列表在相邻两轮之间换过行序：每轮的条数 0 行 17 次 / 1 行 53 次 / 2 行 39 次 / 3 行 31 次 / 4–6 行 4 次，信道 11 的读数 σ≈3.1 dB（−81..−51）、信道 1 是 2.4、信道 6 是 2.1。而 `rank_aps` **本来就按信号从强到弱排** —— 所以「经常跳」不是排序错了，是**每一轮都拿一次性的读数重排**。

政策因此收进 core（`SurveyHistory`，`core/src/drivers/network.rs`），和 `<n>-<m>/8` 一样是「怎么读射频」而不是循环的实现细节：

| 规则 | 取值 | 为什么 |
| --- | --- | --- |
| 迟滞 | 一行要越过 **4 dB** 才动位置（`LIST_HYSTERESIS_DB`） | `bars_for` 每格 10 dB，半格之内换位置在页面上什么都看不出来 |
| 掉轮保留 | 一轮没扫到的行留 **2 轮**（`LIST_MISSED_ROUNDS`） | 单次扫描不是测量（见上）；连着两轮都没扫到才当它走了 |
| 只在画出来变了才 publish | `observe()` 返回 `Some(list)` 才交给平面，否则记 `[WIFI] the list is unchanged, so the page is not redrawn` | 页面的重绘门槛（`DIAG_REPAINT_STEP_MS = 40` + 内容有变）之外再省一次：读数一样就不该重画 |
| 满员顶掉最弱 | `MAX_LISTED_APS = 8`，满了只有强过最弱行 4 dB 的新行才进得来 | 一个新行挤掉一行，和行序抖动是同一件事 |
| 换了网络就重来 | `setup` 每次进关联前 `history.clear()` | 上一次的列表与这一次的链路无关 |

`[测试]` 5 条（band 内不动 / 超 4 dB 上移、掉轮保留、同画面 `None`、满员顶掉最弱、`clear` 之后算一次变化）。

### 页面何时扫

`PagePlane`（`app/src/render.rs` 的 `PAGE`）是页面状态的**唯一共享读数**：渲染循环每 pass 把 `manager.state().page` 发布进去，网络任务用 `PAGE.wait_for_list()` 等「列表在屏上」，并 `select3` 在「扫描间隔到」/「页面**回到**列表」/「staged 凭据就绪」之间醒。

放共享状态而不是加一条消息：页面是状态（幂等、只有最新值有意义），走 channel 就要一个深度 1 的队列、写者侧的变化检测、以及被拒发送时的重试 —— 三个只为绕开「状态被当消息送」而存在的机制。这与 `EffectiveDemand` 同一形状。

为什么门控在**任务**这里而不是让扫描自己判断：扫描是个死循环，它没有「页面」这个输入；把它接进来的是唯一能同时看到页面与射频的那一层。切页回来时 `wait_for_list()` 返回「等过」，所以退避计数被清零，进页立刻重扫一次。

`[实测]` **「回到列表」必须是边沿，不能是电平** —— 这是同一处第二次被上板推翻。第一版把 `wait_for_list()` 放在扫描间隔那一支的第三个分支上，而列表当时**本来就在屏上**：电平检查在第一次 poll 就满足，于是 5 s 的歇息**被整段跳过**。修好前一次 120 s 的读数：**152 轮**、相邻两轮相差 **790 ms**（`survey found` 的时间戳），退避一次都没生效 —— 也就是射频一直在扫，而屏幕上什么都没变。

修法是在 `PagePlane` 上加 `wait_for_return()`：先读当前页，再等它**变化**成列表页，于是「离开又回来」与「一直没离开」不再是一回事。它是**边沿**，`wait_for_list()` 是**电平**，两个都在用：前者给歇息当断路器，后者给循环问「现在该不该扫」。`[测试]` 一条钉住这个区别（`a_rest_on_the_list_is_not_cut_short_by_the_list_being_up`：同一个 plane 上，电平在列表上是 `Ready`，边沿是 `Pending`；先离开再回来才是 `Ready`）。

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

`[查证：ESP-IDF Wi-Fi driver guide]` 另外两条与本页直接相关：

- **连接进行中不能扫**：`esp_wifi_scan_start()` 在连接时**立即失败**（连接优先级更高），官方给的做法是「等一会儿再试」。本仓结构天然满足：扫与连在同一个任务里，凭据一到手就离开扫描循环去 `hold`/`join`（`setup` 与 `hold` 是两个函数），所以不会出现「扫被连接打断」。这也解释了为什么日志里从来没有这类失败。
- **扫得全不全由驻留时间定**：默认 min/max 驻留都是 120 ms，官方明说这两个值可以调来改善扫描 —— 与本仓实测一致（驱动默认 10–20 ms 只看到 1～2 个 AP，`CHANNEL_DWELL` 50–120 ms 看到 1～6 个），见上文「一次不是测量」。

认证这一侧只有一个**产品决定**：有口令 → `Wpa2Personal`，没口令 → `Open`。后者原先写错了 —— 空口令也被塞进 `Wpa2Personal`，而空口令的 WPA2 配置是 AP 会**拒绝**的请求、不是它忽略的字段，所以「列表里点一条 `OPEN` 行 → 免键盘交凭据」在 bsp 层根本走不通（面板与状态机那边是对的）。修法就是照驱动枚举读出来的这一行。`[未实测]` 台架上没有开放网络，这一支只有状态机测试与这次代码审读，**没有真机读数**。

**WPA3-only 与 WPA(1)-only 的网不在支持范围**：`esp-radio 1.0.0-beta.1` 的 `AuthenticationMethodConfig` 只有 `Open` / `Wep` / `Wpa` / `Wpa2Personal` / `WpaWpa2Personal`，没有 WPA3 变体，所以 WPA3-only 的 AP 连不上；WPA2/WPA3 混合模式照常（协商到 WPA2）。

## 分区表

`[实测]` espflash 默认给 esp32s3 写 factory 1 MB，而本仓固件加 Wi-Fi 之前已经 630 KB。`apps/iot/partitions/lckfb-szpi-esp32s3.csv` 给 factory **6 MB**，并带一个 4 KiB 的 `phy_init`。

`phy_init` 原本**故意没写**：esp-radio 的 Xtensa 构建把驱动的标定恢复钩子实现成 `todo!()`（`ld/esp32s3_provides.x` 把 `misc_nvs_restore` 绑到它，blob 里是未定义引用），而驱动看得见标定分区就会走那条路。`[实测]` **这条路走不到** —— 分区在场时 `esp_wifi_init` 不 panic，扫描正常。分区保留，它就是驱动第一次开机写下标定数据的地方。C6 那张也带 `phy_init`，同一个用途。

表按**板子**命名：`scripts/iot-image.sh` 一直按板名找 `partitions/<board>.csv`，而布局本来就是板级事实。6 MB 的槽装不进 espflash 默认的 4 MB，所以每个 S3 烧录/镜像任务都带 `--flash-size 16mb`。**C6 那张不动**（1 MB，实测装得下探针的 597,264 B），但同样写成文件：镜像任务和烧录任务对布局只有一处认知。

## 凭据落在哪（`creds` 分区）

`esp-storage` 0.10 + 自控槽格式，不引 NVS：NVS 是另一套带自己 KV 语义的 blob，而这一处要存的是**一个定长列表**，格式自己拿得住、能一行行读。

**分区**：两张板的表都加 `creds,data,undefined,,0x2000,`，紧跟 `factory` **之后**，既有三条分区的偏移一格未动。`[实测]` 离线复验（`espflash partition-table --to-binary <csv> -o pt.bin` 再 `--to-csv pt.bin`）：S3 `creds` 落在 `0x610000`、C6 落在 `0x110000`，`factory` 仍是 `0x10000`。两个 4 KiB 槽（8 KiB）是**双槽轮写**要的：一次保存只写「上次没写的那一槽」，读的时候取 `sequence` 大的那条 —— 写到一半掉电留下的是另一条的旧副本，不是半条记录。子类型必须写**名字**（`undefined` = `DATA_UNDEFINED` 0x06）：写十六进制会让 `esp-idf-part` 在 `unwrap()` 上 panic（已在不烧板的前提下验过，见待办 5 的 ②）。

**格式**：`core/src/drivers/credentials.rs`。记录 = 12 B 头（magic `VLNC` + 版本 + 条数 + 体长）+ 每条 `1 + 32 + 1 + 64 + 2` B + 4 B CRC，`RECORD_BYTES = 1016`，两条 `const _: () = assert!(...)` 钉住它装得进一个扇区；9 条宿主机测试钉住槽规则、「保存到一半丢电」与坏 CRC 的判读。

**读与写的时机**（都在 `Board::new` 与 `app/src/network.rs`）：

| 时机 | 在哪 | 为什么 |
| --- | --- | --- |
| 开机读表、开库 | `Board::new`，**在射频之前** | 读表要一帧 3 KiB 的缓冲区，而在别的部件起来之前那段还拿得到；分区表读不出来或没有 `creds` 时只记一行日志并交 `None`（**读不到就不保存**，设备照常联网） |
| 开机试哪条 | `link`：先构建输入，再已存列表的**第一条**（最近保存的那条），都没有才回落 `setup` | 已存那条被拒是 `GiveBack` 而不是 `Retry`：面板就在一次点击之外，那是这设备的另一个答案；退避重试是「只有这一条能试」的构建才有的行为 |
| 写 | `hold`：**关联成功那一刻**，每条凭据一次；**只写构建时凭据与面板输入那份**（`Provenance::Named`） | 写的是口令，而口令的答案在手上了；等 DHCP 会把「拿到地址」卡在「口令能不能用」和「下次开机还能不能用」之间。`Refusal::Retry` 的重试路径不会重复写（`kept` 闩住）。**库自己交出来的那份不写**（`Provenance::Stored`：开机自读的第一条、以及点已存行那一条）—— 那份副本就是盘上这一条，写回去只是白烧一个扇区，而开机那条本来就在最前；代价是点已存行连上**不会**把它顶到最前（`remember` 的置顶因此只发生在面板重新打了一次口令那种场合） |
| 擦 | `Store::forget`：长按列表里**已存**的那一行、确认框答「忘记」那一刻，整条记录重写 | 擦掉的是一条口令，不可逆 —— 要再连得从头打一遍，所以只有它问人；断开还能再连，代价小 |

**写盘策略**：S3 用 `FlashStorage::multicore_auto_park()`，C6 直接构造（单核）。本应用**从不启动第二核**，所以默认策略（另一核在跑就 `Err`）今天一样能写；选 auto-park 是为了以后真起第二核时不用回来想起这件事。

**主栈的账**（这是接线撞上的第四个问题，`[实测]`）：`Store` 值 ~1000 B（`SavedNetworks` 981 + 序号 + 槽号），而 `serve`/`link`/`named_by_build`/`setup` 都是同一个任务里的 `async fn`，**每个按值携带它的帧都是主栈自己那一段的静态量**（`__embassy_main::POOL`）。按值直传时 POOL 从 42,144 B 涨到 53,176 B，静态量越过 `RWDATA` 末尾 3,928 B，`ld` 报 `stack.x:13 cannot move location counter backwards`，主栈只剩 53,624 B（断言下限 65,536 B）。改成**在板层装箱一次**（`Option<Box<CredentialStore>>`，1 KB 落在 PSRAM 堆上）之后 POOL 只涨 976 B：`[DISPLAY] main stack 65,944 B`（bench 镜像）/ `66,168 B`（产品镜像），对比 A 阶段的 67,704 / 68,600 B —— 这一整套功能净吃 ~1.8 KB，余量 408 B。**这个余量就是下一次加静态量会撞到的墙**，而撞到的是编译期断言，不是运行时的栈溢出。**再往后（工具条 + 掩码那一轮）**又吃掉一点，`[实测]` 链接期余量变成产品镜像 65,880 B（+344）、bench 镜像 65,560 B（+24）—— bench 那 320 B 差在它自己那批 `log` 的 `Arguments` 表落进 `.data`（`render_loop` 的 `PanelBench` 因此装箱，future 少 28 B 才重新过链接）。

**面板分层那一轮**（`page::Part`/`page::walk` + `stamp::fill_surface`）再量：产品镜像 **+408 B**（构建时给了网络的那一份）、bench 镜像 **+88 B**（`docker run … espressif/idf-rust:esp32s3_1.95.0.0` 里 `nm … | grep _stack_`，余量 = `_stack_start − _stack_end − 65536`）；不给网络、只带 `IOT_BOOT_PAGE=network` 的产品镜像（就是这一轮最后烧在板上那份）是 **+376 B**，两份差的 32 B 在编译进去的那几个字符串上。这一轮**没有新的 `static`**：两个镜像的具名静态量逐字节同名同尺寸（各 308,284 B，`nm --size-sort --print-size` 对齐后无差），新增的全是 `.text`（产品 +15,404 B、bench 更多）。两者那 320 B 的差还是上面那句里的无符号段（`.data` 开头到第一个具名符号之间：产品 9,168 B、bench 9,480 B）—— **所以两个镜像的余量不能互相比较代码**，只能同一个镜像前后比。

**上板读数**（`[实测]`，2026-10-16 夜，S3，三次开机）：

| 次序 | 镜像 | 日志 |
| --- | --- | --- |
| 1 | bench（`IOT_PANEL_BENCH=1` + 构建时凭据），`creds` 擦成空 | `[CREDS] no network saved on this device yet` → bench 在面板上打出 13 个字符 → `[WIFI] associated with "<ssid>" on channel 11` → `[WIFI] this network is saved for the next boot` |
| 2 | 同一 bench 镜像，`creds` 再擦一次 | 同上（这一轮是下面「按面板路径打字」那张表的同一次运行） |
| 3 | **产品镜像**（不带任何构建时凭据） | `[CREDS] 1 saved network(s) on this device` → `[WIFI] this device saved a network; joining "<ssid>"` → `[WIFI] associated with "<ssid>"` → `[WIFI] this network was already saved, so the saved copy was updated` → `[WIFI] phase Connected, address Some(...)` |
| 4 | 同一 bench 镜像，`creds` 擦成空 | 打字关联 → `[WIFI] this network is saved for the next boot`（`*` 出现）→ 工具条 `LEAVE` + 确认 → 长按那一行 + 确认 → `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → `[BENCH] the saved mark is gone from the row the finger held`（`*` 消失），见「工具条、确认框与忘记」 |

第 3 次是这条功能的**验收**：没有手指、没有构建输入，设备自己把上次面板打的口令从 flash 读出来并连上。第 1、2 次证明的是写：**从「空」到「存下来了」在板上真的发生过**（`multicore_auto_park` 在本板真能写）。第 4 次证明的是擦：忘记之后盘上那条真的没了（`the next boot will not join it`），同一轮里列表那一行的 `*` 也当场消失。

`[未实测]`：读串口失败（`Opened::Unreadable`）与写失败（`the part would not keep the network`）两条警告分支都没有台架造法，只有代码审读。

## 特征与分层落点

`wifi` 是**元件 feature（模块轴）**，在 `iot-bsp-esp` 里一个文件一个模块，门控只在 `components/mod.rs` 一处；**两块板的板 feature 都聚合它**（两块板都有射频焊在板上）。`iot-core` 零 cfg：能力经组合点注入（`HasWifi`）。`components/wifi.rs` 本身跨芯片可用 —— 它只拿 `peripherals::WIFI` 与驱动的类型，两颗芯片各编译过一遍。

`Station` 的方法用 RPITIT 返回 `impl Future`，**不要 `+ Send`**：产品把这些 future 与 input/render/control/feed 一起 `join` 在同一个任务栈上，没有 spawn。（`Speaker` 需要 `Send` 是因为它的 feed 在独立的高优先级 executor 上。）

`take_wifi()` 不能在 `Board::new` 里做：**驱动的启动需要调度器已经跑起来**（`esp_rtos::start()` 在 app 的入口里），所以板子交出的是**外设**而不是 station。

`iot-chip-esp` 的两个芯片 feature 各带五个芯片 feature 转发（`esp-hal` / `esp-rtos` / `esp-println` / `esp-backtrace` / `esp-bootloader-esp-idf`），console 另是一条轴（`console-auto` / `console-uart`），app 转发过来。原因是那三个 crate 的 build script 各自要求「恰好一个芯片 feature」和一个 console，平时都由 app 顺手给，于是任何**没有 app 在场**的构建死在别人的 build script 上 —— `iot-feature-check.sh` 第一次真跑到 `wifi` 就抓到了。这就是 `features.md` 规则 5：**用到的每一处都要自己写出来**。

## 产品页：扫描列表 + 4×4 键盘

`DisplayPage::Network` 分两片。**第一片（只读）已落地**：顶部是链路（阶段词、当前 AP 的名字与信号条、拿到的地址），下面是扫描列表。**第二片也落了盘**（输入与状态机 + 面板绘制 + 逐行滑动 + 列表迟滞缓存 + 手势删减 + 选中行反色 + 口令回显 + 开放网络 + 失败分档 + 凭据落盘 + 工具条 + 已存 `*` 标记与长按忘记），见「凭据落在哪」与下面的表。

页面是四个互斥的状态，由 `NetworkSetup.keyboard`、`LinkStatus.phase` 与「有没有一条确认框挂着」决定（草稿留在 `entry` 里跨态保留）：

| 状态 | 画什么 | 手势 |
| --- | --- | --- |
| 列表态（键盘放下） | 三行表头，**一行一个主体**：`NETWORK` + **本机 MAC**（右对齐）/ `AP <名字>` + `ST <phase>` + 信号条 / `IP <addr>` + **该网络的 MAC**（右对齐）；列表页码 `n-m/8` 在**工具条左角**，下面 **3 行 × 47 px** 的列表（**每行两行字：名字在上、该网络自己的 BSSID 在下**，名字画不出时它就是地址、只画一行；开放网络名字行右端画 `OPEN`；**已存网络名后画 `*`**；信号条在名字行右端；**选中行整条反色**），再下面是**一条工具条**（有链路时 `LEAVE`，否则 `SCAN`） | 点行 = 选中该行；**已存网络（画 `*` 的那一行）不弹键盘**，只把那一行的**名字**交给网络任务、由它拿库里的口令去连（库查不到那个名字时，射频回报、页面才升键盘问口令）；**开放网络**同样不弹键盘，空口令直接交凭据；其余行升起键盘。**上/下滑 = 列表滑动一行**；**长按一行 = 问要不要忘记它**；**点工具条** = 有链路时问要不要断开，没链路时立刻重扫（不问）；切页只剩**硬件三连击**。**有链路可离开（`Connecting`/`Connected`）时点行一律不接**：不选中、不升键盘、不交凭据（`OPEN` 行也一样），长按与滑动照旧（**连接框挂着时连它们也不接**：列表没画出来就不可点，见下） |
| 打字态（键盘升起） | **同样三行**表头（`NETWORK` + 本机 MAC / `AP <name>` + `ST <phase>` + 信号条 / `PSK <草稿>`，一种读数一行；本机 MAC 这一态也在）+ **4 列 × 4 行**键盘（键 **80 × 46 px**） | 只有键：字符键多击换候选、`123`/`#&?` 换层、Shift、退格、OK、X、SHOW(回显)；其余意图一律 `Invalid` |
| 确认框（盖在列表上） | **列表与工具条都不画**（`page::Part::hidden`），只画一条 256 × 118 px 的模态框：标题 `LEAVE "<name>"?` / `FORGET "<name>"?`，正文 `THE LINK GOES DOWN` / `THE SAVED KEY IS ERASED`，下面两个答案键 `KEEP` / 动词（`LEAVE`/`FORGET`，各 100 × 44 px） | 只有两个答案键；**其余一切手势与点击都是取消**（`answer_at` 统一裁定） |
| 连接框（盖在列表上，工具条留着） | **列表不画、工具条照画**（`page::Part::hidden`）：同一条 256 × 118 px 的框，**没有答案键**，标题 `JOINING <name>` / `JOINED <name>`，正文 `WAIT FOR THE NETWORK` / `WAIT FOR AN ADDRESS` | 只有工具条：`LEAVE` 仍是唯一出口。**问题与它同时在场时画问题**（`Overlay` 只有一个矩形，问题优先）；键盘升起时不画它 —— 手指当下的事是打字 |

键位（`core/src/keyboard.rs` 的 `CELLS` 表，不是算出来的下标 —— 数字块不按 cell 顺序排）：

```
1  2  3  | SH        ← 右列是四个独立控制键，不是跨行合并
4  5  6  | 123
7  8  9  | <-
X  0  OK | SHOW
```

`1 2 3 / 4 5 6 / 7 8 9` 是拇指记住的块，为此右列两个格子空着也值；`0` 放在手机把它放在的位置（Cancel 与 OK 之间）。右列那四个键在**三层上是同一批 cell**（只有标签随层变），手指记的是格子而不是字。

几何不写魔数，全部从 core `overlay.rs` 的页脚推出来：`FOOTER_ROWS = 8`（全局 FPS 徽标，任何页都不拥有它）、`CONTENT_BOTTOM = PANEL_ROWS - FOOTER_ROWS = 232`，于是 `KEYBOARD_TOP = overlay_row_top(NETWORK_LIST_ROW = 3) = 47`、`KEY_H = (232 − 47)/4 = 46`、`KEY_W = 320/4 = 80`、`LIST_TOP = overlay_row_top(3) = 47`、`LIST_ROW_H = 47`（列表仍是 3 行，从 61 收到 47 让出 42 px）、`TOOLBAR_TOP = 47 + 3×47 = 188`、`TOOLBAR_H = 44`（到 `CONTENT_BOTTOM`）。**列表态与打字态从同一个 y 起**：表头三行之下，列表分三份、键盘分四份，所以键盘升起时页面上的字一动不动，只是多出下面的一块。bsp 里 `const _: () = assert!(...)` 把「页面到哪为止」钉在同一个数上（列表末行、工具条末行、键盘末行都不得越过 `CONTENT_BOTTOM`）—— 徽标挪一行就会被长上来的控件压掉。

决定都放在能被测试打到的层：

| 决定 | 取值 | 为什么 |
| --- | --- | --- |
| 页面何时存在 | 入口点报的接线事实（`iot_app::Link`：`Wired` / `Absent`） | station 由入口点取走（它的启动要调度器），`run` 里已经看不到射频；用 build 事实（`cfg!(feature = "wifi")`）则宿主机冒烟永远到不了这一页，而这一页最需要的验证恰恰是「组合有没有把能力接进来」——`Link` 与 `SpeakerRunner` 同一形状：接线事实由入口点传进来。不是 `Board` 上再开一个方法：那会有两种说法说同一件事，且能互相打架 |
| 扫描列表走哪 | 独立单槽平面（`ScanPlane` / `SCAN`），不进 intent ring | `Intent` 是 `Copy`，每个槽按最宽消息定价：`ApList`（8 × 42 + 1 = 337 B）进 ring 就是 8 槽 ≈ 2.7 KB，而 ring 的窗口是 `size_of::<AudioSample>()`（1,760 B）。`app/src/input.rs` 的尺寸测试直接拦下（`a_capture_window_costs_one_slot_beside_the_ring`） |
| 「没扫过」与「扫了没扫到」 | `DeviceState.aps: Option<ApList>` | 两态在页面上是两句话（`NOT SCANNED` / `NO NETWORKS`）。带自己凭据的构建不去扫，只能说「没扫过」；扫了没扫到是在说这个房间 |
| 列表一行多高 | 3 行 × 47 px（6.0 mm），表头仍是 3 行，行下多一条 44 px 的工具条 | 面板 320×240 = 40.6×30.5 mm（0.127 mm/px）。61 px（7.75 mm）原本达标，但工具条要从这 232 px 里切出 44 px，3 行 × 47 px 是「切完还剩多少就多少」：47 px = 6.0 mm 低于 7 mm 的通行最小触点，与键盘 46 px 同一条**已知并接受的破例**（数字块与列表都是拇指盲按的） |
| 键盘从哪起 | 键盘起点抬到与列表**同一个 y**（第 3 行 = 47 px），表头保留三行 | 原来打字态只有一行表头，为的是把键盘顶上去；上板看下来那三行信息（阶段、当前网络与信号、草稿）都值得留，而 4 行的键盘让出 6 px 仍够按。两态从同一个 y 起还省掉了一次「页面跳一下」 |
| 4×4 而不是 5×3 的 T9 | 数字块 3×3 + 右列 4 个控制键，键 **80 × 46 px**（10.2 × 5.8 mm） | 5 列的 T9 每键只有 64 px（8.1 mm）宽，而 4 列把宽度给到 80 px。**高度 46 px = 5.8 mm，低于 7 mm 的最小触点** —— 这是已知并接受的破例：数字块是拇指盲按的，且 4 行要给到 7 mm 需要 28 px/行以上的空间，面板没有。原 T9 的 64×66 px 是 8.1×8.4 mm，但只有 3 行且右列被 `#&?` 占掉一个数字位 |
| 候选表 | 手机键位：`1`→`.?!,`、`2`→`abc`…`9`→`wxyz`、`0`→空格；层键三层循环 `abc` → `123` → `#&?` | 口令大多是词 + 几个数字 + 一个符号，三层覆盖得住，键位是手指已经会的。`MAX_CANDIDATES = 4` 由编译期 `assert` 钉住：哪个键多了第 5 个候选就编不过。**当前键的候选串画在该键的面上、当前候选反色**（`Mark::Candidate`）——「点到第几个候选了」要看得到 |
| 口令怎么显示 | 掩码全星、**尾部窗口**上限 30 字形（超长时左侧一个 `+` 标出被截断）；回显态画原字符（非可打印画 `?`）；`SHOW` 键兼作 `HIDE`（状态在 `NetworkSetup.revealed`） | 面板是单色反转（`stamp_marked` 先填小矩形再 `stamp_text`），没有第二种强调色。尾窗而不是头窗：刚敲的那个字符在末尾，而口令要改的字符就近在末尾。`SHOW` 与 `HIDE` 同一个键是因为它是**锁存器** —— 一个写着 `SHOW` 而正在显示口令的键，字和做的不一致。30 是画布给的：草稿占整行宽，比原来挤在三列里的 16 字形多出近一倍 |
| 多击 | 第一击立刻上屏；同一键、候选数 > 1、`MULTITAP_WINDOW_MS = 800` 内再点 = 换下一个候选（替换尾部字符）；另一键/退格/层/Shift/窗口过期 = 新字符；候选数 = 1 的重复点 = 新字符 | 第一击立刻上屏，所以打字没有等待；`00` 这种口令靠「单候选重复点 = 新字符」打得出来 |
| Shift | 单击一次性大写（只被字母消费）、窗口内双击锁定、锁定后再点解除；锁定整张键面反色。**Shift 属于整段多击 run，而不是第一次 push** | 手机键盘的惯例；`Connect`/`Cancel` 会结束多击运行。按 run 而不是按首击记：`Z` 是 `9` 的第四击、`S` 是 `7` 的第四击，若 Shift 只随首击落下，3–4 击的字母永远打不出大写 |
| 开放网络 | 列表右端画 `OPEN`（名字止于 x=222，标记右对齐到 274，两条编译期断言钉住不打架），点行**不弹键盘**、直接以空口令交凭据 | 开放网络没有口令可输，弹一个键盘出来只会让人以为要输点什么；`OPEN` 与满格信号条都画在行右端，两串不重叠 |
| 已存网络画 `*` | 名字后一个 `*`（右对齐到 229，在 `OPEN` 词的左侧；名字槽止于 222），来源是**列表自带的每行一位掩码** `ApList.saved: u8` | 名字后加一个字符比在行右端再挤一个词省地方，且与 `OPEN` 不打架（编译期断言钉住三者不重叠）。掩码随 `ApList` 一起走 `SCAN` 平面，那一片因此必须是「新值覆盖旧值」的槽：`SCAN.publish` 丢一次修正就会留下一行错标记，而**下一轮不会自愈**（历史那边已经把修正当成页面在显示的列表，下一轮比对说「列表没变」）—— 见「离开后点回那一行、以及一次丢掉的列表修正」。这是拿「省下在 app 里再落一份 980 B 的已存集合」换来的（S3 静态内存墙，见「凭据落在哪」） |
| 已存的一行点一下就按库里的连 | `pick_row` 读到「`secured` 且那一行掩码已存」就把 `JoinRequest::Held(name)` 落进 `DeviceState.join`（不升键盘、清草稿与回显，口令一个字节都不经过页面）；渲染循环取走发到 `Join` 平面，网络任务用 `SavedNetworks::copy_of(name)` 查库：查到就用那份关联（**不写盘**），查不到就 `BusinessIntent::NoCopy(name)` 回报，页面才升键盘问 | 库里已经有那份口令，再让用户打一遍是让他重打一个设备已经有的秘密。**名字而不是口令**：库只有网络任务一个所有者（`SavedNetworks` 不进 `DeviceState`，S3 静态内存墙，见上一条），所以页面能说的是「哪个网络」而说不出「它的口令是什么」—— 库里没有时只能回报页面，而页面是唯一能问出口令的地方。掩码只决定**先问库还是先升键盘**，答复本身来自库（掩码是上一轮的答复）：掩码旧了的最坏结果是「用户又打了一遍」，不是「连错了网络」。这条把库的两个所有权分开了：库只管口令，页面只管「哪个网络」 |
| 选中行反色 | 整条 band 逐行反转（`stamp_band_fill`），再在上面画名字/标记/信号条 | 面板只有黑白反转。整条反色而不是只反名字，是因为一行里还有信号条与 `OPEN`，只反一部分会让人以为选中的是那半条 |
| 断开从哪发起 | 工具条上的 `LEAVE` 按钮（链路**可离开**时才画），点它弹确认框，答「动词」才置 `DeviceState.leave` 闩锁 | **刻意从「点已连接行」改过来**：那一行既是「这个网络」又是「离开它」的入口，点错了没有退路；工具条是唯一入口、动作唯一，且执行前问一次。可离开与否收在 `LinkStatus::is_leavable()`（`Connecting`/`Connected` 才可离开，`Off`/`Scanning`/`Failed` 只能重扫），工具条按它二选一画；**它只看相位**：`ap` 要关联成功才置上，拿它当条件会把答案变成「关联报告到了没有」。同一个谓词同时决定工具条画哪个词、点行接不接、以及 `LEAVE` 还有没有东西可问。闩锁由渲染循环取走、发布到 `LeavePlane`，网络任务在窗口 `select` 里听见就 `disconnect` 并回报 `Off`；本会话不再自动回连靠 link 的 `auto` 开关（待办 5） |
| 有链路时列表只读 | `pick_row` 开头门控：`link.is_leavable()` 就直接返回（不选中、不升键盘、不交凭据，含 `OPEN` 行）；长按与滑动不过这道门 | 链路在飞的时候点一行有两个意思可给：「换一个网络」和「先断开」，而断开已经收了工具条一个入口。读数是这一行唯一的意思，所以它就没有第二个意思可给 —— 关联前已经升起的草稿键盘不受影响（`Connected` 才清，`Connecting` 不清） |
| 弹窗盖住列表 | core `page::Part`（`Header`/`Content`/`Controls`/`Overlay`，顺序 = 绘制顺序）声明画什么、按什么顺序画，`page::walk` 只把该画的交出来；问题挂着时 `Content` 与 `Controls` 都 `hidden`；表面由 `stamp::fill_surface` **写**实心，而不是把底下翻一遍 | 面板只有一种纸 = 地面翻过来的颜色，所以「反色填充」会把底下已经画上去的字（本来就是纸色）翻成地面色 —— 看起来就是列表的字印在弹窗上，缺的是实心而不是 z 序。而问题里那一行的 band 与弹窗是同一张纸，band 从弹窗两边露出来会读成弹窗的一部分，所以列表整块不画。`Part::hidden` 同时也是命中规则（`intent` 先裁问题、再按 `Part::showing` 裁列表） |
| 打字态不画工具条 | `page::Part::hidden` 里 `Controls` 多一个 `\|\| setup.keyboard`；同一批判据经 `Part::showing` 给 `intent` 用 | 工具条的盒子（y 188–232）与键盘**最后一行**的键盒（y 185–231）是面板上同一条带：两者都在时 `SCAN` 那三个字形正落在结束输入的 `OK` 那一排上（标签行 206），而那时 `intent` 把所有点都发给键 ⇒ 工具条是死像素。**不是 z 序问题，是画了不该画的东西**；根因是「画」与「点」各说一遍，所以命中规则也收进 `page` 一处（`page.rs` 里一条测试把重叠本身钉住 —— 几何一旦分开，这条规则就该重读） |
| 连接中要看得见 | 拆 `Station::connect` 为同步的 `begin_connect()`（`set_config` + 置相位，**失败也置**）与 `await_connected()`（等射频答复），`hold` 在两者之间 `publish(station.status())`；页面画**非模态**连接框，工具条留着 `LEAVE` | 原来整段关联挡在 `connect().await` 里：从按 OK 到地址到手之间页面相位是旧的（工具条画 `SCAN`、`ST` 是上一个词），而 `is_leavable()` 为假 ⇒ `pick_row` 的门在这段窗口是漏的（能点行起新草稿）。最坏实测 **37.6 s**（错口令 + 四步握手超时）。`set_config` 失败也置相位，顺带修了 `NAME`/`DRV` 从不显示。框**非模态**：这一次要的不是确认，是「在做了」加一个出口，而出口就是工具条那个 `LEAVE` |
| 框里的两句话 | 标题 `JOINING`/`JOINED` + 名字，正文 `WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS`；四个词都由 `LinkStatus.ap` 有没有值定（名字取 `shown_ap`，与表头同一个） | 两句话是两种不同的等待，结束方式也不同（一个结束于链路或拒绝，一个结束于地址或「有链路、没地址」）；一个词概括两种，面板就会对「射频还没答复」与「已上网络但没有租约」说同一句。构建输入带来的网络没人打过名字时（`ap` 空）只留词，不编一个名字出来 |
| 长按一行 = 忘记 | `Touch::Hold` → `BusinessIntent::AskForget(ap)`：只有那一行**已存**才弹 `FORGET` 确认框，答「动词」才擦 | 忘记不可逆（口令要从头再打一遍），所以只对它问；长按而不是再给一个按钮，是因为它只对少数行成立，画出来会占掉行右端。确认框里的名字来自那一行的 `ApList` 项，擦除走 `Store::forget`（整条记录重写，双槽轮写照旧） |
| 重扫不问 | 无链路时工具条是 `SCAN`，点它立刻发一个 `RESCAN` 信号给扫描任务 | 扫描是只读的、可再来的，没有不可逆的东西要确认；有链路时**不给 `SCAN`**：链路在手上时 `survey` 根本不会被调到（那时 `setup` 在 `hold` 里等它下来），两个动作从不同时成立，工具条不必二选一地说「而且」 |
| 列表滑一行 | 上/下滑 = `BusinessIntent::Scroll { rows: ±1 }`，`list_top ± 1` 并把末端对齐到最后一个完整窗口 | 一页一页翻时短列表的末页会留空、窗口读数会跳 3；一行一格让每一行都滑得到，`n-m/8` 也是连续的 |
| 列表为什么不跳 | 排序政策在 core（`SurveyHistory`：4 dB 迟滞 + 掉轮留 2 轮 + 只在画出来变了才 publish） | 扫描读数每轮都在抖（信道 11 的 σ≈3.1 dB），拿单轮读数重排就是列表一直跳。机制与证据见「列表为什么不跳」一节 |
| 失败分档 | `NetworkError` 分 `NotFound`（`NOAP`）/`Credentials`（`AUTH`）/`Busy`（`BUSY`）/`InvalidArguments`（`NAME`）/`NoMemory`（`MEM`）/`Driver`（`DRV`）；失败**自动重开键盘**并把词画在打字态中槽 | 4 字形词与阶段词同一套画法（`format_phase` 的四字形测试）。分档表只认驱动枚举里能辩护的那几种，其余落 `Driver` —— **真机映射未实测**，见待办 5④ |
| 页面底色由表面声明 | `DiagnosticsSink::draws_pages()`（默认 `false`）由 `Screen` 覆写为 `true`；`app::render::LightRenderer` 在「表面画页 && 页 == Network」时给纯黑填充 + 背光 100% + 不呼吸 | 由 core 按页派生外观是错的：不是每个光源都有屏幕（C6 那颗 WS2812 也是 light instance 0），而「我画不画页」只有表面自己知道 |
| 命中与绘制同源 | `keyboard::{key_of, key_at, key_box, list_band_at}` 一套几何 | 画在哪和点到哪是同一组函数，改几何不会漏掉命中 |
| 意图只报页面显示的 | `BusinessIntent::Choose(ap)` 带完整的 `AccessPoint`，`Key { key, at_ms }` 带抬指时刻 | 状态层按 BSSID 回查当前列表，不信任页面留下的序号；`at_ms` 是多击判断唯一需要的时钟。`Scroll` 同样不报目标位置：滑一次就是一格，抵到末端与否是状态层自己的事 |

画什么由 `diag_repaint_due` 的 Network 谓词定：阶段、当前 AP、地址、列表、打字态任一有变就重画，包络与触摸计数不重画（`[测试]` 两条，两个方向）。

数字到字形的换算全在 `apps/iot/crates/ui/src/text.rs`（跨页的绘制算术都在那，宿主机可测；本页的 `[测试]` 10 条）：阶段词（失败画六种之一：`NOAP`/`AUTH`/`BUSY`/`NAME`/`MEM`/`DRV`，`MEM` 是「这个构建的堆预算错了」唯一的读数）、点分地址（无地址是 `-`，不是 `0.0.0.0`）、信号条 `#`/`.`（与列表同一把尺子 `bars_for`）、名字（画不出 ASCII 就显示 BSSID；超过 24 字形保留头部并以 `~` 标记，否则折在同一处的两个网络会画成同一行）、请求框的标题与两个答案词、以及连接框的标题与正文（`JOINING`/`JOINED` + 名字、`WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS`；名字超出剩余宽度时折的是名字，`JOINING`/`JOINED` 那个词不动）。

### 手势删减：延迟在分类器里，不在面板

`[测试]` 触摸现在只剩 `Press` / `Tap`（抬指即发） / `Swipe` / `LongPress` / `Ghost`（`GestureEvent::DoubleTap` 与 `TripleTap` 已从词汇表删除，`TouchDiagnostics` 里那两个计数一起去掉）。

**延迟的根因是点击曾经被含在嘴里**：旧的 `TouchGestures` 把一次短抬存进 `chamber`（`PendingTap`），只有在 `DOUBLE_CLICK_WINDOW_MS = 400` 的窗口过期那一次 `tick` 里才发布 —— 也就是**每一次点击都要等最多 400 ms 才被看见**，页面跟着晚 400 ms 反应。删掉触摸多击之后 `Tap` 在**抬指那一次采样**上发布；多击窗口只留给硬件按钮（切页唯一的入口是硬件三连击，那是真按钮，窗口照旧 400 ms）。

`GestureEvent::Tap` 带 `at_ms`（抬指时刻），位置改报**按下沿**（`down_x`/`down_y`）而不是抬指坐标：面板粗糙，抬指坐标会漂；键盘用 `at_ms` 判断 800 ms 内的第二击是「换候选」而不是「又一个字符」。`TouchGestures` 的队列同时从 2 槽放宽到 4 槽（一次采样最多产出「两个触点各一个手势 + 这一次运行的幽灵脉冲」），满了丢最旧的。

### 上板读数（`[实测]`，2026-10-16 夜）

刷到 S3（无凭据构建、`IOT_BOOT_PAGE=network`）：`[IOT] boot page is Network`，启动就在这一页。105 秒的读数（复现见 [`flashing.md`](@/development/iot/flashing.md) 的「启动页与 bench 镜像」：手写的 `IOT_BOOT_PAGE=network` 构建 + `espflash flash`，抓串口用 `espflash monitor`）：

| 量 | 值 |
| --- | --- |
| Network 页重绘 | **每轮扫描 1 次**：`[DISPLAY] Network 1 repaints in 15825 ms, held the executor 18 ms (0%)`，最坏渲染 3 ms、最坏写 15 ms（启动那一窗是 `Network 3 repaints in 7333 ms`，进页 + 头两轮） |
| 扫描节奏 | **5.8 s ×2 → 15.8 s ×3 → 30.8 s**（实测 `survey found` 时间戳：`2.046 → 7.802 → 13.624 → 29.449 → 45.268 → 61.088 → 91.909`），空轮 1 s 后重试（见「一轮扫描的节奏」与「页面何时扫」） |
| 主栈 | 链接期 **68,600 B**（上板自报 `[DISPLAY] main stack 68600 B`），峰值 **37,281 B**（54%），余 **31,319 B** |
| 镜像 | **885,744 B / 6 MB（14.08%）** |

峰值的**时刻**与它的大小一样有用：`1.222 s` 时还是 `0 B`，`2.017 s` 已经是 `37,281 B` —— 也就是第一次扫描加第一次把列表画出来那一段最深，此后整段运行再没抬过。bench 那次可以佐证（打字发生在 `2.1–2.8 s`，而它之后重报的峰值仍是同一个数 `37,297 B of 68,408 B`）：**键盘与打字那条绘制路径不比启动那条深**。

只读那一版在同一台机器上的读数是 **5–8 次重绘 / 2–2.8 s**（`/tmp/network.log`，2026-10-16 17:02：`[DISPLAY] Network 5 repaints in 2324 ms, held the executor 91 ms (3%)`）。差的是这一页自己的底：只读版画的是光源的 fill（颜色走位每几百毫秒越过一次重绘阈值），现在这一页是黑的、静态的，只有内容真变了才重绘，而内容每轮扫描才变一次 —— 而每轮的间隔现在真的按退避走了（见「页面何时扫」里那次 790 ms 的错）。

面板这一侧的**触摸坐标帧一直是错的**（`IDENTITY`：模组报的是控制器自己的 240×320 竖幅窗口，而页面是 320×240 横幅）——症状正是「点某格出的是别格的东西」。修法与依据见[输入与触摸实现记录](@/records/iot/input.md)的「触摸坐标要过一次帧变换」；下面这条手感与整条链路的待验，是在那条帧变换修完之后才值得判的。

### 按面板路径打字（`[实测]`，2026-10-16 深夜）

「点一行 → 输口令 → OK」这条链路里真的碰得到**玻璃的只有一段**：触摸坐标从控制器到 `GestureEvent`。剩下全在页面坐标里，所以可以造一个构建让它**按自己画的格子点自己**：`IOT_PANEL_BENCH` 的构建（`crates/app/src/panel.rs`）让 `credentials_from_build()` 返回 `None`，于是产品开机就走「等键盘给凭据」那一支；`PanelBench` 在渲染循环里**每 pass 发一个 tap**，进的是手指那条路 —— `intent::translate` 加同一个 `INTENT_BUS` 环。

每 pass 一个 tap 而不是一口气发完：环是 8 槽，而一个 tap 只有被**下一 pass** 的 drain 应用，再下一 pass 才按应用后的状态决定下一步。多击因此自然成立 —— tap 的 `at_ms` 取自脚本自己的钟（`keyboard::script_for` 的 `Tap::at_ms`），不是墙上时间，所以「每 pass 一格」的快慢不会把 `Z`（`9` 的第四击）串成别的字符。

跑法（`IOT_PANEL_BENCH` 与 `IOT_BOOT_PAGE=network` 由任务带上 —— 后者是 bench 的一部分而不是顺手给的，因为只有列表在屏上时才扫）：

    IOT_WIFI_SSID=… IOT_WIFI_PSK=… IOT_PORT=/dev/cu.usbmodem1422301 moon run iot:flash-panel-typing-s3

读到的一次开机（S3，口令一个字符都没进日志）：

| 时刻 | 日志 |
| --- | --- |
| 2.11 s | `[BENCH] replaying a credential through the panel: 0 is row 0 of page 0, 28 taps in all` |
| 2.15 s | `[WIFI] the panel named a network: "<ssid>"` —— **键盘打出的口令到达网络任务** |
| 2.78 s | `[WIFI] joining the network the panel named: "<ssid>"` |
| 5.34 s | `[WIFI] associated with "<ssid>" on channel 11` |
| 5.38 s | `[BENCH] the panel's own credential is on the air: 13 characters typed, 28 taps, and the radio has an address` |
| 7.38 s | `[WIFI] phase Connected, address Some([192, 168, 1, 136])` |

**28 击打出 13 个字符**，全落在面板自己画的格子上，射频拿着它关联上并拿到地址 —— 这是「4×4 键位 + 多击候选 + 三层 + Shift 属于整段 run + OK 交出凭据」整套坐标与状态机的端到端证据，而**没有一根手指碰过面板**。口令本身从不进日志，能对照的只有**字符数**（13 = 构建命令行里那个口令的长度）；radio 收下它并关联成功，才说明 13 个字符就是口令本身。

这 28 击用的是**37 ms** 墙上时间（bench 比任何拇指都快），而这一页的重绘门槛是 `DIAG_REPAINT_STEP_MS = 40` 加「这一页画的东西真的变了」——所以那一段窗口里重绘计数很小（`Network 3 repaints in 2255 ms`）不是页面卡：指头两击之间至少隔两百多毫秒，每一击都会自己换来一帧。

**再一次，故意给错口令**（同一条命令，只换 `IOT_WIFI_PSK`）读到：

| 时刻 | 日志 |
| --- | --- |
| 3.54 s | `[WIFI] joining the network the panel named: "<ssid>"` |
| 41.10 s | `[WIFI] left "<ssid>": FourWayHandshakeTimeout at -67 dBm` —— **驱动的原始 reason** |
| 41.10 s | `[WIFI] the radio would not associate: Credentials` |
| 41.12 s | `[BENCH] the radio refused what the panel typed: Credentials after 60 taps` |

于是失败分档里有两件由据称变成**实测**：① 错口令落到 `FourWayHandshakeTimeout`，被 `kind_of_disconnect` 归进 `Credentials`，面板画 `AUTH`；② 从交出凭据到被拒是 **37.6 s** —— 等的是驱动自己那次四步握手的超时，产品这边（面板与列表）照常跑，`ST` 一直是 `CONNECTING`。这里也有一条 bench 自己的教训：第一次扫描只看到邻居 `ap1` 时它报了一句「指名的那条不在列表里」，其实第二个来回就把它带来了 —— 那条从 `error` 降成 `warn` 并改成「还没在列表里」，否则每次正常开机都会先骂一句自己。

bench 自己的算术也钉在宿主机上（`panel.rs` 7 条）：目标在第几页、翻几次页、band 的 y 落在第几行、**最后一击必须落在 `OK` 那一格**、开放网络不被当成要打字的目标、列表还没来时不编脚本，以及**整条 walk 按真实顺序走一遍**（`the_walk_asks_before_it_leaves_forgets_and_looks_around`：有链路时点行不接 → 工具条问一次 → `KEEP` 留住 → 再问一次答动词 → 点回刚离开的那一行（**库那份口令把链路接回来，页面什么都没问**）→ 再断开 → 长按忘记 → 那一行再点一次（这一次读的是**新草稿**）→ 取消键盘 → 最后重扫；顺序本身也钉住：逐动作比 `visited`，少走一个动作或两个动作换了位置都会红）。这些是「几何账」里最容易悄悄错一位的部分，而它们不需要上板。

**还没验的**（软件验不了，只能人点一次面板）：只剩两件 —— 4×4 键、列表行与工具条的**触摸手感**（80 × 46 px 的键、47 px 高的行、44 px 的工具条与确认框答案键，指头按上去才算数）与纯黑底色、背光 100% 要用眼睛看。**「弹窗盖没盖住列表」不再靠眼睛**：那是宿主机可测的事实 —— 问题挂着时 `page::Part::hidden` 不画列表与工具条、`page::walk` 根本不把它们交出来、绘制顺序由 `Part::IN_ORDER` 钉住、`stamp::fill_surface` 逐像素写满整个矩形（`core/src/stamp.rs` 三条测试，其中一条把填充换回「反色」就会红）。整条「点一行 → 输口令 → OK → 关联 → 断开 → 忘记」已经由上面的 bench 走通，不再需要人；宿主机冒烟另覆盖同一套状态机与几何（`iot:smoke-host`：`Row(0)` → `Char(b'4')`, `Char(b'4')`, `Char(b'2')`, `Shift`, `Char(b'8')` 打出 `haT`，凭据到达 `Join` 平面，`NETWORK_TYPED` 到 3）。

### 工具条、确认框与忘记（`[实测]`，2026-10-17）

同一个 `IOT_PANEL_BENCH` 构建，这一次把工具条与忘记走完（`creds` 先擦空；脚本的 `Act` 序列是：打字关联 → 点 `LEAVE` → 答 `KEEP` → 再点 `LEAVE` → 答 `LEAVE` → 长按已存那一行 → 答 `FORGET` → 再点 `SCAN` —— 第五轮在这中间插了一次「点回那一行」，次序见「点已存的一行就用库里的口令」）。S3 一次开机读到（口令与网络名都不进日志）：

| 时刻 | 日志 |
| --- | --- |
| 4.73 s | `[WIFI] this network is saved for the next boot` —— **`*` 标记的来源：这一行进了掩码** |
| — | `[BENCH] tapping the strip, which asks to leave the link it is on` → `[BENCH] the strip asked about leaving, and nothing is down yet` |
| — | `[BENCH] KEEP left the question undone and the link up` —— **答 `KEEP` 后链路还在** |
| — | `[BENCH] the strip asked again; the action's own answer is next` → `[WIFI] the panel asked to leave this network; the link is down` |
| — | `[BENCH] the link is down and the page is back on the list` |
| — | `[BENCH] holding band 0, which asks to forget that network` → `[BENCH] the hold asked to forget "<ssid>"` → `[WIFI] the panel asked to forget "<ssid>"` |
| — | `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → `[BENCH] the saved mark is gone from the row the finger held` —— **`*` 消失** |
| — | `[BENCH] tapping the strip with no link, which asks to look around` → `[WIFI] the panel asked to look around now` → `[BENCH] the strip's own round came back` |

这四步证明的是**同一个掩码**的两端：写入时那一行被标上（`this network is saved for the next boot`），擦除后它不再被标（`the saved mark is gone`）；`holding band 0` 这一句只有在那一行**已存**时才发得出，是掩码在板上真的存在的旁证。断开与重扫走的是工具条与确认框自己的坐标，进的是手指那条路（`intent::translate` 加同一个环）。**这一轮之后 `creds` 是空的**：忘记真的擦了盘，而不是只在页面上抹掉。

### 连上时点行不接、弹窗盖住列表（`[实测]`，2026-10-17 第二轮）

两个缺陷各自收在一处：

- **有链路可离开时点列表行没有反应**。门开在 `pick_row` 开头（`link.is_leavable()` 就直接返回），不选中、不升键盘、不交凭据，`OPEN` 行也一样；长按（忘记）与滑动不受这道门影响。断开因此只剩工具条一个入口。
- **确认框真的盖住列表**。`stamp_fill` 原来是**逐像素把底下翻一遍**而不是写实心 —— 列表的白字（本来就是纸色）被翻成地面色，看起来就是列表的字印在弹窗上，缺的是实心而不是 z 序。现在表面由 `core/src/stamp.rs` 的 `fill_surface` **写**，问题挂着时列表与工具条**都不画**（`page::Part::hidden`）。

bench 的 `Act` 序列在前面加了一步（打字关联 → **点 band 0（链路在飞时）** → 点 `LEAVE` → 答 `KEEP` → …）。S3 一次开机读到：

| 时刻 | 日志 |
| --- | --- |
| 4.66 s | `[WIFI] associated with "<ssid>" on channel 11` → `[WIFI] this network is saved for the next boot` |
| 4.72 s | `[BENCH] the panel's own credential is on the air: 13 characters typed, 28 taps, and the radio has an address` |
| 4.82 s | `[BENCH] tapping band 0 under the link, which must pick nothing` |
| 4.83 s | `[BENCH] the row under the link took no pick, and no keyboard came up` —— **链路在飞时那一行只是读数** |

这一句只有在「点下去会开键盘」的那种实现下才发不出来（把门去掉，宿主机那条 walk 测试立刻红：`a row took a pick while a link was up`）。读回的**不是**「页面上没有选中行」—— 打字那一步本来就选中过一行，而链路升起只清草稿、不清那一行 —— 所以这一句比的是**点之前与点之后一样**，即这一击什么也没改。

弹窗那一半的**结构**在宿主机上，四条各自可测的事实：问题挂着时 `page::Part::hidden` 不画列表与工具条、`page::walk` 根本不把它们交出来、绘制顺序由 `Part::IN_ORDER` 钉住、`fill_surface` 逐像素写满整个矩形（`core/src/stamp.rs` 三条测试，把填充换回「反色」那条就红：`(6,10)` 处 `left=4358 ground / right=61177 ink`）。**画什么、按什么顺序画**这一层这一轮也从面板搬进了 core：`page::walk(setup, draw)` 只把该画的交出来（测试钉住交出的顺序、以及问题挂着时列表与工具条**根本不被交出**），面板剩下的只是 `match` 每个部分长什么样 —— 因为面板是唯一「答错了看不出来」的地方（宿主机编不到它）。**像素本身（弹窗整体观感、色带、对比）仍然只有眼睛能判**。

这一轮的产品镜像（不给网络、`IOT_BOOT_PAGE=network`）也在板上开过：`[CREDS] 0 saved network(s)` → `[IOT] boot page is Network` → 扫到 2 条 → `the list is unchanged, so the page is not redrawn` → 停在那里等面板。同一份页面代码在没有 bench 的那一支上照常跑，`[DISPLAY] main stack 65,912 B`。

### 打字态没有工具条、连接中有一个框（`[实测]`，2026-10-17 第三轮）

两个新报的缺陷，一个在「画」、一个在「相位」，而两处根因都是**同一件事被说了两遍**：

- **打字时工具条压在键盘最后一行上**。工具条的盒子（y 188–232）与键盘第 4 行键盒（y 185–231）重叠 43/46 px，`SCAN` 那三个字形落在 y=206 —— 正是结束输入那一排。而 `intent` 在 `setup.keyboard` 时把所有点都发给键，所以那三个字形**画出来也点不到**。判据收进 `page::Part::hidden`（`Controls` 多一个 `|| setup.keyboard`），命中侧读同一个答案（`Part::showing`），`page.rs` 里一条测试把重叠本身钉住。
- **连接期间页面不说自己在连接**。`Station::connect` 拆成同步的 `begin_connect()`（`set_config` + 置相位）与 `await_connected()`，`hold` 在两者之间 publish 一次状态；页面画一条**非模态**的连接框，工具条留着 `LEAVE`。顺带修了 `set_config` 失败时相位不变（`NAME`/`DRV` 因此从不显示）。

结构侧全在宿主机可测的层：`page::walk(setup, link, draw)`（`Content` 被问题**或被连接框**藏；`Overlay` 二者皆无时不画；两条同时在场画问题）、`text::{format_joining_title, format_joining_body}`（`JOINING`/`JOINED` + 名字、`WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS`，四个词都由 `LinkStatus.ap` 有没有值定，2 条新测试）。

同一个 `IOT_PANEL_BENCH` 构建（先擦空 `creds`，28 击），S3 一次开机读到：

| 时刻 | 日志 |
| --- | --- |
| 2.060 s | `[BENCH] replaying a credential through the panel: 0 is band 0 after 0 slides, 28 taps in all` |
| 2.064 s | `[BENCH] a finger is typing, and the strip is not drawn with the keys` —— **打字态的工具条只是没画** |
| 2.098 s | `[WIFI] the panel named a network: "<ssid>"` |
| 2.100 s | `[WIFI] joining the network the panel named: "<ssid>"` |
| 2.142 s | `[BENCH] an attempt is in flight: the box is over the list and the strip is still offered` —— **连接框在射频答复之前就画出来了** |
| 4.664 s | `[WIFI] associated with "<ssid>" on channel 11` —— **距框出现 2.52 s** |
| 4.685 s | `[WIFI] this network is saved for the next boot` |
| 4.739 s | `[DISPLAY] Network 5 repaints in 4216 ms, held the executor 97 ms (2%), worst render 4 ms, worst write 15 ms` |
| 4.797 s | `[BENCH] the panel's own credential is on the air: 13 characters typed, 28 taps, and the radio has an address` |
| 4.851–6.652 s | 上一轮那条 walk 照旧走完（点行不接 → 工具条问 → `KEEP` → 答动词 → 长按忘记 → 重扫） |

**2.142 s 那一行才是这一轮的读数**：它出现在 `joining` 与 `associated` 之间，也就是**上一轮页面说不上话的那段窗口**（那时 `ST` 还是旧词、工具条画 `SCAN`、`pick_row` 的门是漏的）。同一段窗口在错口令那次实测里是 **37.6 s**，所以这一句不是「快了 2.5 秒」而是「半分钟内页面不再是一片沉默」。

这两句都不是靠眼睛判的：**去掉 `Controls` 那个 `setup.keyboard`，`page.rs` 立刻红 3 条**（`the_page_under_a_keyboard_has_no_strip_and_no_box`、`a_draft_is_the_pages_own_subject_over_an_attempt_too`、`the_walk_leaves_the_strip_out_and_keeps_the_box_in_while_a_finger_types`）；**把 `joining` 的相位条件去掉**（只剩「没在打字」）会连状态层的 24 条一起红；**把长按或滑动那两个 `Part::showing` 门去掉**，`a_hold_on_the_list_under_the_join_box_asks_nothing` / `a_slide_under_the_join_box_does_not_move_the_list` 各红一条（三处都当场验过，改回即绿；改回要用**精确匹配**的替换，别用宽正则 —— 这一轮有一次宽正则把 `joining` 的相位条件一起回滚掉了，正是这几条测试当场抓住的。）

连接框的**几何**不是新数：它复用问题框的 `DIALOG_W/H/X/TOP`（256 × 118，`DIALOG_X = 32`），差的是里面没有答案键、工具条不被它藏。**主栈这张账这一轮没有动**：bench 镜像 `[DISPLAY] main stack 65,624 B`、产品镜像（不给网络）`65,912 B` —— 与上一轮同一对数字，新增的两个 `bool` 落在 `PanelBench` 已有的对齐空隙里（断言下限仍是 65,536 B，两份的余量分别是 88 B 与 376 B）。产品镜像这一次开机：`[CREDS] no network saved on this device yet` → `[IOT] boot page is Network` → 扫到 2 条 → 停在那里等面板（`[DISPLAY] stack peak 38,992 B of 65,912 B`）。

**仍然只能靠眼睛/手指的**：打字态第四行键现在**干净了**（上面不再压着 `SCAN`）、连接框的整体观感与它出现/消失的时机、以及 80 × 46 的键与 44 px 工具条的手感 —— 这三件是像素与触感，不是结构，`[未完成]`。

### 离开后点回那一行、以及一次丢掉的列表修正（`[实测]`，2026-10-17 第四轮）

两个缺陷：一个是报出来的黑屏，另一个是**扫出来**的丢更新 —— 后者不修的话，忘记过一个网络之后，那一行的 `*` 有时会一直留着。

- **离开链路后点回那一行，页面下面全黑**。`pick_row` 的「又点了同一行就留着草稿」判据原来只看 `selected`，而链路升起**清草稿、不清那一行**（见「相位语义」）：链路一落，那一行还是被选中的，再点它被读成「同一行」→ **不建草稿**，却把 `keyboard` 置起 → 面板把键盘区画在空草稿上，键与工具条都不画，**这一页再没有能点的东西**（这就是「全黑」）。判据补上 `entry.is_some()`：没有草稿就没有草稿可留，而「链路丢下的草稿不回来」是故意的（它不会再被交出去），所以那一行再点开的是**新的空草稿**。看不见的那一半写进了文档：`NetworkSetup::keyboard` 与 `pick_row` 都声明了不变量 `keyboard ⇒ entry.is_some()`。
- **忘记之后那一行的 `*` 有时不消失**。`SCAN` 平面原来是容量 1 的 `Channel`，`try_send` 满了就 **`Err` 丢掉**，而丢的偏偏是**新的那一条**：一轮扫描先发一次列表，紧接着（同一毫秒内，忘记正是在这一轮自己的停顿里被执行的）网络任务把「库里已经没它了」重标一遍再发一次 —— 槽里还压着第一条时，第二条被丢掉。页面于是留着一个早就不存在的已存标记，而**后面每一轮都不会再纠**：`SurveyHistory` 已经把那条修正当成「页面正在显示的列表」，下一轮比对说「列表没变，不重画」。修法是把平面换成一个**新值覆盖旧值**的槽（`Signal` 装值 + 一个独立的唤醒信号；唤醒与值分开，因为渲染循环**停泊在唤醒上**，停泊不能把列表取走 —— `take()` 是下一趟开头的事）。

**「下次自动发现这类缺陷」的机制这一轮一起落了**：状态层原来只有散点测试，现在多一层**可达状态穷举** —— `state::tests::reachable::every_state_the_page_can_reach_keeps_its_invariants` 从开机态出发，用 35 个「页面真收得到的动作」（`alphabet()` 里逐个列名：三行的点与长按、工具条、确认框两键、按钮三态、键盘七键、两次滑动、射频的七种相位与三种扫描、以及渲染循环把四个闩交给射频的那四下）做广度优先，落到 **8,800 个状态**（0.90 s，上限 `SWEEP_CAP = 20_000`，并且断言「没触顶」—— 触顶就等于这个测试在宣称一份它没有的覆盖），对**每一个新落到的状态**查四条不变量：

| 不变量 | 破掉之后面板是什么样 |
| --- | --- |
| `keyboard ⇒ entry.is_some()` | 键盘画在空草稿上（这一轮报的那个黑屏） |
| `entry.is_some() ⇒ selected.is_some()` | 草稿没有主人：要交出去时没人知道是给哪个网络 |
| `confirm == Some(Leave) ⇒ link.is_leavable()` | 问「要不要断开」，而已经没有链路可以断 |
| `rescan_wait ⇒ !link.is_leavable()` | 在链路上等一轮扫描，而那时的工具条只会给 `LEAVE` |

穷举的键只写**动作会分支的那些字段**（可读的字节串而不是哈希：哈希碰撞会静默合掉两个状态）；只被绘制读的字段（回显、上次被拒、重扫等待）不进键，但**不变量在每个落点上照查**，所以丢掉的只是「同键状态的另一条分支」，不是覆盖。末尾还有一组**可达性断言**（「键盘真的在草稿上起来过」「真的出现过有行没草稿的态」…），防的是字母表悄悄少了一个动作 —— 比一个状态数的下限准确，也不会被任何合法的裁剪误伤。

`IOT_PANEL_BENCH` 的 walk 也加了**两个动作**，插在「断开」与「忘记」之间：`ReopenTheRowJustLeft`（点回刚离开的那一行，读回「键盘起来、草稿是 **0 个字符**」）与 `DropTheKeyboard`（按 `CANCEL`，读回「回到列表、那一行也不再被选中」）。旧的读回对这一态**会误判成通过**（三元组读回把「键盘起在空草稿上」当成打字态），所以 `read_parts` 多了一条 `draftless_read` 兜底：键盘起在**没有草稿**的槽上就判失败。

板上的读数是 `bench-r7.log`，同一条 walk 的第二次（从当前源码重新编过的那把）是 `bench-r8.log` —— 两次都走完（`creds` 先擦空，S3 一次开机，28 击）：

| 时刻 | 日志 |
| --- | --- |
| 5.023 s | `[BENCH] tapping band 0, the row the link was just taken off` |
| 5.027 s | `[BENCH] the row the link was left from opens a fresh entry: the keyboard is up over 0 characters, and the draft the link dropped is not back` —— **离开后点回同一行：新草稿，键盘起在有东西的槽上** |
| 5.087 s | `[BENCH] the entry's own cancel puts the page back on the list` |
| 5.144 s | `[BENCH] holding band 0, which asks to forget that network` → `[BENCH] the hold asked to forget "<ssid>"` → `[WIFI] the panel asked to forget "<ssid>"` |
| 5.709 s | `[WIFI] survey found 2 access points` → `the list is unchanged, so the page is not redrawn` |
| 5.718 s | `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → **紧接着重标一次再发** |
| 5.768 s | `[BENCH] the saved mark is gone from the row the finger held` —— **修正到了页面上** |
| 5.804–6.403 s | `[BENCH] tapping the strip with no link, which asks to look around` → `[WIFI] the panel asked to look around now` → `[BENCH] the strip's own round came back` —— **整条 walk 走完，没有 `[BENCH] … 120 passes` 的报错** |

`bench-r7.log` 这一次的「一轮扫描 + 一次重标」没有撞上丢包（那一轮扫描说「列表没变」，所以那一轮只发了一次），`bench-r8.log` 也一样，**所以这两次本身都没有复现那个丢更新** —— 复现它的是上一把 `bench-r6.log`：5.901 s `survey found 3 access points`（这一轮会重画）、5.911 s `is forgotten`（重标那一次被丢）、6.653 s `survey found 2 access points` + **`the list is unchanged`**（历史那边已经是「没标记」的列表，可页面不是）、7.788 s `[BENCH] the action's own tap left the saved mark where it was in 120 passes`。同一段窗口里还有 `[INPUT] held the executor 3219 ms of 5008 ms … worst single hold 605 ms (bound 30 ms) … capture 601 ms`：那一次相机抓帧把执行器压了 0.6 s，两条相隔 10 ms 的投递正好都压在槽里。**这个交错现在由宿主机测试钉住**：`render::tests::a_list_published_before_the_last_one_is_taken_replaces_it_rather_than_being_refused`（连发两条、只看得到新的那条）与 `the_wake_a_park_waits_on_does_not_take_the_list_it_woke_for`（停泊不取走列表）。

**这几处都不是靠眼睛判的**：把 `pick_row` 的 `entry.is_some()` 回滚，新状态测试与穷举扫**当场双红**，扫还把复现路径打了出来 ——「the device at boot, a triple click, a scan of two networks, a tap on the first band, the radio connected, the radio reporting nothing on the air, a tap on the first band」；把平面换回「满了就丢」、以及把唤醒接成会取走列表的版本，上面两条平面测试各红一条（两条都当场验过，改回即绿；把唤醒改成「立刻就绪」**不算数** —— 那版没有取走列表，两条测试一条都没红，作数的是**真会取走列表**的那版）。主栈下限仍是 65,536 B，两个镜像这一轮各量了一次：bench 镜像 `65,624 B`（与上一轮同一个数）、产品镜像（不给网络、`IOT_BOOT_PAGE=network`）**`65,944 B`** —— 比上一轮的 65,912 B 多 32 B，是平面那两个 `Signal` 的等待 future 落到渲染循环的 future 里带来的，余量 408 B；产品镜像这次开机：`[CREDS] 0 saved network(s) on this device` → `[IOT] boot page is Network` → 扫到 1–3 条 → 停在列表上等人按（`[DISPLAY] stack peak 38,656 B of 65,944 B`）。

### 点已存的一行就用库里的口令（`[实测]`，2026-10-17 第五轮）

**已存网络（画 `*` 的那一行）点一下 = 用设备自己存在 `creds` 里的那份口令去连**，不再让人重打一遍。这一条把凭据那条路上的**所有权**分开：口令库只有网络任务一个所有者（`SavedNetworks` 从不进 `DeviceState` —— S3 静态内存墙下那 980 B 落不成值，见「凭据落在哪」），页面能说的是「哪个网络」而说不出「它的口令是什么」；于是页面点一下只交**名字**，答复（库里有没有、能不能连）由射频给。

- **请求是同一个平面的两种形状**。`CredentialPlane`/`CREDENTIALS` 改名 `JoinPlane`/`JOIN`，消息由 `Credentials` 变成 `JoinRequest::{Typed(Credentials), Held(Ssid)}` —— 同一个请求「帮多少忙」的两种，还是同一条单槽平面（不新开平面、不新加一条 `select` 臂）。`DeviceState.credential` 因此变成 `DeviceState.join: Option<JoinRequest>`（`take_join()`）：字段还是那一个，宽度只多一个判据字（`Credentials` 已是两个变体里宽的那个），**没有新的 `static`**。
- **库里没有就回报页面**。网络任务在**两个**读这条平面的地方（`setup` 的开头、以及链路已起时 `hold` 的窗口）都走同一个 `asked_for()`：`Typed` 直接用（`Provenance::Named`）；`Held(name)` 走 `SavedNetworks::copy_of(name)` 查库 —— 查到就用那份（`Provenance::Stored`），查不到就发 `BusinessIntent::NoCopy(name)`（走既有 `INTENT_BUS`，与 `Link` 报告同一条路）。页面收到后 `no_copy()`：只对**当前选中的那一行**成立（这期间点了别的行就是更新的问题，旧答复丢掉），升键盘问口令。
- **被拒之后不落到死循环上**。库那份也会被拒（对面改了口令）。那时 `refused` 照旧，而 `reopen_entry` 补一条**选中的行没有草稿就造一个空的**：键盘是拿草稿画的、工具条又被键盘藏起来，所以「有键盘、没草稿」是一页什么都点不到的东西 —— 而这一行偏偏只会被点第二次（点它 → 用库的口令 → 被拒 → 键盘没起来 → 只能再点它）。补空草稿是这个死循环唯一的出口，也是 `reopen_entry` 唯一**新造**草稿的场合。
- **写盘判据跟着分开**（`Provenance`）：构建时凭据与面板输入是设备**学到**的东西，关联成功那一刻写（照旧）；库交出来的那份是设备**已经有**的东西，不写。顺带修掉一处白写：开机自读那份原来每次开机都把同一条内容重写一遍（双槽轮写 = 一次擦写），现在不写；代价是点已存行连上**不会**把它顶到已存列表最前（`remember` 的置顶因此只发生在面板重新打了一次口令那种场合）。

`[测试]`：`network_page` 5 条新测试（已存行落 `Held` 且不升键盘也不动口令、已存的**开放**行照旧直交凭据、`NoCopy` 起键盘且草稿为空、库那份被拒后键盘起在**空草稿**上、答复指向别的行时不动作），`reachable` 穷举多两个动作（`Held` 走到关联、`Held` 走到 `NoCopy`）与两条可达性断言，`key` 多一位（两种形状是两种状态），`drivers::network` 一条 `copy_of` 测试（不是这里就是 `None`，因为空口令是**合法**答案 —— 开放网络）。

**bench 的 walk 顺序因此变了**。原来是 `… LeaveGo → ReopenTheRowJustLeft（点回那一行，读「新草稿」）→ DropTheKeyboard → AskToForget → Forget → Rescan`，现在是：

```
TapRowUnderLink → AskToLeave → KeepTheLink → LeaveAsk → LeaveGo
  → RejoinTheRowJustLeft    ← 点回那一行：链路拿库那份口令自己回来了，页面什么都没问
  → AskToLeave → KeepTheLink → LeaveAsk → LeaveGo    ← 再断一次
  → AskToForget → Forget → ReopenTheRowJustLeft → DropTheKeyboard → Rescan
```

两处挪动都是**页面自己的规矩**逼出来的，不是摆着好看：①「点回那一行」现在读的是**新**行为（链路回来、键盘不起、没有草稿、那一行还带着 `*`），于是读「新草稿」的 `ReopenTheRowJustLeft` 得往后挪；②链路在飞时列表画在「连接框」底下，`network_hold` 与 `network_tap` 都按 `Part::Content.showing` 把长按判成 `Invalid` —— **有链路时长按出不来确认框**，所以忘记必须排在第二次断开之后。walk 的次序本身也钉在宿主机测试里（逐动作断言 `visited`），少走一个动作、或两个动作换了位置都会红。

`[实测]`：同一个 `IOT_PANEL_BENCH` 构建、`creds` 先擦空（28 击打字那一段照旧），S3 一次开机 —— **面板上没有人**，整条（点已存行 → 库那份口令 → 链路回来 → 再断 → 忘记 → 点回那一行 → 重扫）是构建自己按坐标走的：

| 时刻 | 日志 |
| --- | --- |
| 4.71 s | `[WIFI] associated with "<ssid>" on channel 11` → `[WIFI] this network is saved for the next boot` —— 这一份是 **bench 自己打在面板上的口令**（`Provenance::Named`），照旧落盘 |
| 5.13 s | `[BENCH] tapping band 0, the row the link was just taken off and this device has the passphrase for` → `[WIFI] the panel asked for the copy this device keeps of "<ssid>"` —— **页面交的只是名字** |
| 5.89 s | `[WIFI] joining the network the panel named: "<ssid>"` —— 库那份口令上了射频；从报到名字到这一步**没有任何键盘、没有任何草稿** |
| 5.92 s | `[BENCH] the saved row brought the link back up with the copy on the part, and the page asked for nothing` —— **本轮的主判据**；而且这一段**没有** `this network is saved for the next boot`：`Provenance::Stored` 不写盘，板上对得上 |
| 7.50 s | 第二次断开：`the panel asked to leave this network; the link is down`（第一次在 5.02 s）—— **忘记因此落在没有链路的那一侧**（有链路时长按出不来确认框） |
| 8.27 s | `[WIFI] "<ssid>" is forgotten; the next boot will not join it` → `[BENCH] the saved mark is gone from the row the finger held` |
| 8.36 s | `[BENCH] the row the link was left from opens a fresh entry: the keyboard is up over 0 characters, and the draft the link dropped is not back` —— 忘记之后那一行回到「要打字」那一支 |
| 9.11 s | `[BENCH] the strip's own round came back` —— 全程**没有一条** `[BENCH] … 120 passes`（一个动作都没超时） |
| — | `[DISPLAY] main stack 65624 B`（与上一轮同值）、`stack peak 38944 B of 65624 B`；这一轮**没有新的 `static`**，`DeviceState.join` 宽出来的那一点没有落到主栈上 |

产品镜像（不给网络、只带 `IOT_BOOT_PAGE=network`）这一轮是 **65,944 B**（余量 408 B），bench 镜像 **65,624 B** —— 与上一轮**一模一样**：`DeviceState.join` 宽出来的那几个字节在两个镜像的主栈上都没落地。同一把尺子（`nm` 取 `_stack_start − _stack_end`，见「主栈的账」）量与两个镜像在板上自打的 `[DISPLAY] main stack` **一字不差**。

`[实测]`（收尾补量，不是哪一轮）：**不带任何构建输入**的那一份 —— 出货的起始页 Ambient、也是最后烧回板上的那份 —— 是 **65,976 B**（余量 440 B）。`nm` 的符号（`0x3fce9710 − 0x3fcd9558`）与板上开机日志同为 `65976 B`；两次构建只差起始页常量一处，`IOT_BOOT_PAGE=network` 时正是上面那个 65,944 B，差的 32 B 就落在这一处。

`[未实测]`：`NoCopy` 这一格在台架上**造不出来** —— 掩码与库是同一个所有者写的，掩码在就说明那一轮库里确实有；它是「不信掩码、问库」这条规矩的兜底，目前只有宿主机测试。库那份**写失败**、以及「开机自读那条不写盘」同上（见待办 5 的 ⑦）—— 后者只有代码可查（这次开机是 `creds` 擦空之后的面板支）：`hold` 的两处调用点分别递 `Stored`（开机与点行）与 `Named`（构建与面板输入），而 `keep` 只被 `Named` 那一路调用（`app/src/network.rs`）。

## 没有面板的板子怎么配网：开放 AP + 配置页（`[实测]`/`[测试]`）

C6 只带 WS2812 与 Boot 键，而 `LightRenderer::page_ground()` 只在 `light.draws_pages()` 时铺黑 —— WS2812 不画页面，所以 `DisplayPage::Network` 在这块板上**既看不见也按不到**。补上的是一扇前门：本设备起一个**开放**接入点，DHCP 发地址，DNS 把**任何**名字都答成自己（手机的「此网络无互联网」探测因此自己跳到配置页），HTTP 上放一张配置页。面板那扇门不动：S3 仍是「面板为主、页面同时在」，C6 是「只有页面」。

| 层 | 位置 | 是什么 | 谁测 |
| --- | --- | --- | --- |
| 协议 | `iot-core::provision`（`http`/`dns`/`route`/`scan`/`wire` + `assets/` 里两张 `include_str!` 的 HTML 与一个图标） | 请求解析、路由、答复、名字怎么答、列表的 JSON；**全程不分配**（表单解码进调用者的栈缓冲） | 宿主机 **45 条**测试（`provision/*.rs` 的 `#[test]`） |
| 设备 | `iot-app::provision` | 四个循环（DHCP 服务器 / 名字服务器 / 页面 / `embassy-net` runner）接在 socket 上 | 台架（待办 8） |
| 接线 | `iot-app::network` 的 `configure` | 起 AP、把页面与面板 `select` 在一起、凭据交回 `JOIN` 平面 | 台架（待办 8） |

路由：`GET /`（与 `/index.html`）给页面（**带内嵌列表**），`GET /scan` 给 JSON 列表，`GET /rescan` 给同一份列表并**唤醒一轮扫描**，`POST /submit` 收凭据并**内联**回完成页，`GET /done`（与 `/done.html`）给同一张完成页，`GET /favicon.svg` 给图标（缓存一天）、`GET /favicon.ico` 答 `204`，其余**一律 302 回 `BASE_URL`** —— 最后一条不是兜底而是功能：手机探测用的路径（连通性检查、各种探针）拿到的是一次跳转，把用户送到页面的正是这个答案。

射频这一半：`Station` trait 加了 `begin_setup`/`end_setup` 两条，bsp 把它们落成 `Config::AccessPointStation(StationConfig::default(), ap)`（**APSTA**，所以接入点开着的时候扫描照跑）与 `Config::Station(...)`。接入点是 `AuthenticationMethodConfig::Open`（按 `esp-radio 1.0.0-beta.1` 的 `impl Default for AccessPointConfig` 核过），名字来自板子自己的地址（`vanling-<MAC 后两字节>`，`[测试]` 三条钉住格式、去重与补零）而不是构建常量：同一个镜像刷两块板，在一个屋子里就是两个网络。

### 答复先到手机，再结束设置模式

`provision::serve` 返回 `Infallible`，所以在 `configure` 里它与面板那一路（`setup_credential`）`select` 竞争：**谁先说出一个凭据谁结束设置模式**，页面那一路随 `select` 一起被丢掉，接入点连同它的 `Runner` 一起下线。凭据经已有的 `JOIN` 平面走，于是 `hold`、凭据落盘、`Provenance` 全部原样复用 —— 页面与面板在「凭据从哪来」这层是同一条路。

页面在写答复**并等到对端 ACK**（`TcpSocket::flush`，语义按 `embassy-net 0.9.1` 的 `tcp.rs` 与它的文档）之后才把凭据投上 `JOIN`。这不是保守，是一条会**稳定输**的竞态：投上 `JOIN` 就意味 `select` 立刻解析、页面 future 与 AP 的 `Runner` 一起被丢掉，**没有下一次 `interface.poll`** —— 写进 socket 缓冲区、还没上介质的字节会随栈一起消失，手机看到的是一个永不回的请求。`/submit` 因此把完成页内联在 200 里而不是 302 到 `/done`：跳转要靠第二个请求，而第二个请求要等的那套栈已经不在了。

### 空闲退出，与仍然不做的 `/exit`

这一节上一版写的是「AP 一直在，直到有人交出一条凭据」：参考固件（`../xiaozhi-esp32` 的 `78__esp-wifi-connect`）的「定时重开 AP」与 `/exit` 都被判成恒等操作。两档前端落地之后前半条不再成立 —— AP 成了**为手机专门拉起来的一档**，没人看还开着，就等于给射程内任何人一台可以配网的设备；而「重开一遍」仍然没有意义，要收就收掉：

- **`SETUP_IDLE_MS = 180_000`**：从 AP 这一档起来时计时，页面每答一次请求就重置（`provision::page_read()` 是只有最新值有意义的窗口，见 `page_idle`）。正在填表的手机不算空闲。
- **时钟总是武装**（`page_idle()`，不再有 `armed`）：有屏板到期回面板档；无屏板到期回已存网络（`return None`，`link()` 重新关联）；**无屏且无存量网络**的板子也一样收 AP —— 它回不去任何链路，这是 `browsing()` 的离线态。旧版那条「无屏且无存量 → 时钟不启动」的例外在第三版入口里删掉了：有屏板除复位+按住外，还能在离线页按 `AP`；无屏板只有复位+按住。一台出厂设备的开放 AP 不该一直广播（见「回到设置模式：启动页上按住 BOOT」）。
- **`/exit` 仍然不做**：页面上没有它的位置，面板的 `LEAVE`/`CLOSE`/`CANCEL` 是同一件事更完整的说法，而 C6 的入口是启动页上那一下（见「回到设置模式：启动页上按住 BOOT」），不是页面里的一个链接。（页面的两个超时等的是「第一个字节」与「一条请求」，不是等空闲 —— 见台架一节 ④。）

### 设置模式下扫描不再等面板

`[实测]`：`survey` 在设置模式下**无条件**扫（扫描节奏本身没动，见「一轮扫描的节奏」），原来那道「等面板到达再扫」的阻塞门（`PAGE.wait_for_list()`）换成非阻塞的 `PAGE.shows_list()` 加一个上升沿。理由：C6 上没有人会到达那一页，而页面读的正是这条列表。面板到达时 `SurveyHistory::clear()` 仍在跑，所以「到达时看到的是现场」这条性质没变。

列表**两份**，因为有两个读者：面板那份走 `SCAN` 平面（渲染循环 `take` 掉），页面那份走 `provision::LATEST`。页面不能把面板那份取走；`LATEST` 是只有最新值有意义的窗口，所以是 `Mutex<RefCell<...>>` 而不是通道。

### 依赖与版本

`edge-dhcp 0.8.0`（只开 `io`）+ `edge-nal 0.7.0` + `edge-nal-embassy 0.9.0`（只开 `udp` 与 `proto-ipv4`），`embassy-net` 的 feature 补上 `dhcpv4`/`tcp`/`udp`。版本必须与 `embassy-net 0.9` 对齐：`edge-nal-embassy` 0.8.x 是对着 `embassy-net 0.8` 编的，混起来编不过。`udp` 只给 DHCP 用（它经 `edge-nal` 的 `UdpBind` 拿 socket），名字服务器与页面直接走 `embassy-net` 自己的 `UdpSocket`/`TcpSocket`，所以 `edge-nal-embassy` 的 `tcp`/`dns`/`icmp` 全关着 —— 那几样会是「每块板都链接、没有板调用」的代码。HTTP 手写（HTTP/1.0、`Connection: close`、必带 `Content-Length`）：**`picoserve` 被否**，它要 nightly 的 `impl_trait_in_assoc_type`，而本仓钉的是 stable 1.95.0。

`proto-ipv4` 是**这个 crate 自己的** feature，`embassy-net` 那份开着不算数 —— 当初按「`udp` 之外都不要」写，就是被它打掉的，代价与判读见下一节。

两张页以 `include_str!` 进 `.rodata`，本目标的 `.rodata` 落在 DROM（`nm` 里在 `0x3c01xxxx`），也就是 flash 映射区，不吃 `dram_seg`。

### 内存：这功能在 S3 上撞到静态墙，以及它怎么付账（`[实测]`）

同一把尺子（`nm` 取 `_stack_start_cpu0 − _stack_end_cpu0`）、同一条命令（`--bin vanling --no-default-features --features lckfb-szpi-esp32s3,console-auto`）：

| 一步 | POOL | 主栈 |
| --- | --- | --- |
| HEAD 重编（同命令） | 43,336 B | 65,976 B（余量 440 B） |
| 接上设置模式，页面 future 内联 | 50,984 B | **链接失败**：`stack.x:13 cannot move location counter backwards` |
| 页面 future 改 `Box::pin` | 43,552 B | 65,464 B（断言下限 65,536 B，差 72 B） |
| `LATEST` 装箱 + `&Ssid`/`&ApList` | 43,408 B | 65,592 B（余量 56 B） |
| `SurveyHistory` 装箱 | 42,728 B | **66,264 B（余量 728 B）** |

两处装箱都是本记录里既有那条规矩的复用（见「主栈的账」里的 `Store`）：**每个按值携带大值的帧都是主栈自己那一段的静态量**。页面四个循环的 future 连 socket 缓冲区一共 **7,432 B**，`Box::pin` 把它整个搬去堆（S3 是 PSRAM，C6 是那块 64 KiB 的 app 堆）；`SurveyHistory` 约 700 B 同理（`LATEST` 那 344 B 一度也装箱，台架上它以每轮一次的 338 B 分配打死过 S3，现已回到静态量 —— 见下一节）。最后一行比 HEAD **还多 288 B 余量**：这一整套功能净**让出**了约 300 B，C6 与 S3 都过。

**尺子的分辨率要说清**：`nm` 把 `.bss`+`.data` 逐符号求和，第二行与头一行只差 80 B，而位置计数器走了 384 B —— 差额是**节对齐**，不是数据。中途把 `LATEST` 从 344 B 收到 8 B 只换回 128 B 也是同一个道理。所以量这个预算只认 `_stack_end`，不认符号和。

flash 那边是另一笔账，也量了：`moon run iot:image` 出来 S3 是 `997,376 / 6,291,456 B`（15.85%，6 MB 分区），**C6 是 `776,016 / 1,048,576 B`（74.01%，1 MB 工厂分区）**。同一棵树把设置模式摘掉（`git stash` 后同一条命令）量到 C6 是 `706,704 B`（67.40%），所以**这功能的 flash 代价是 69,312 B（+6.61 个百分点）**，两张 HTML 一共 4,725 B，其余落在 smoltcp 的 `tcp`/`udp`（原镜像的网络只用 IP 那一层，DHCP 客户端不碰 TCP）与 `edge-dhcp`/`edge-nal` 上 —— 这是「用 HTTP 服务一张页」的固有代价，不是能省掉的手写部分。

### 台架：从链接到手机，再到四轮修正（`[实测]`，2026-10-19）

C6 先跑通：手机连上 `vanling-1234`、拿到租约、页面**自己**打开（DNS 把任何名字都答成 `192.168.4.1`），再走 `GET /` → `GET /scan`（列出邻居）→ `POST /submit`（一条真口令）→ `phase Connected`；凭据落盘之后**重刷同一个镜像仍然开机即连**。S3 随后暴露了两个只有射频在环里才看得见的问题，另外两条是页面的答复本身太慢。

**① 每轮扫描一次的 338 B 分配会打死 S3**：第一次 `/scan` 之后 panic —— `memory allocation of 338 bytes failed`，堆上只剩 1,632 B。那一笔是列表的一份拷贝（`Box<ApList>`，四个邻居）。修的**不是**把它改小而是**去掉**：页面那份列表是「只有最新值有意义」的窗口，本来就该是静态量，于是回到 `Mutex<RefCell<ApList>>`，每轮扫描的分配归零 —— 代价从「运行时致命」换成「链接时算得清」。同一轮把每个页面 socket 的收发缓冲从 512 收到 256。

**② 一个 socket 就是一条连接，而手机不止一条**：`Connection: close` 意味着每一次 fetch 都是一条新连接，而手机后台同时还跑着连通性探测（`generate_204`）、`mmtls` 与预连接。一条 socket 的页面上 `/scan` 只能等前面几条腾出槽位 —— 40 s 窗口里只答了 2～3 条；加到两条仍是 2～3 条；**三条**才够。第三个 socket 的 `StackResources` 来自**堆**（`Box::leak`），`.bss` 一个字节没涨（`SETUP_SOCKETS` 仍是 5）。

**③ 挂着的 socket 是「正在服务」，不是「可再挂」**：`armable(state)` 只认 `Closed` 与 `TimeWait`；被 `accept()` 出来的 socket 在答复写完之前一直握在循环手里，这期间再 `accept()` 只会拿到 `InvalidState`。一度以为「写答复时也不停挂」能多答几条，实测相反：那会让页面自己的 `/scan`、favicon 与手机探测全被拒。

**④ 两个超时，不是一个**：连接建立起拿到第一个字节之间等 `PENDING_TIMEOUT = 1 s`（手机的预连接与被人放弃的连接就卡在这里），第一个字节之后换成 `CONNECTION_TIMEOUT = 10 s`。挑选顺序也改了：**手上有数据的持有 socket 优先**（`can_recv()`），其次任一持有 socket，最后才收新的 —— 一条真请求不该排在一条空闲连接后面。

**读数**（S3）：`GET /` 14 次、`/scan` 10 次、`POST /submit` 1 次（整轮 6,858 ms，其中页面那一次写 1,900～2,100 ms），日志形如 `… answered Ok, 4468 B of body, in 2111 ms`；读不动的连接印 `a request would not be read after N ms: <err>`（socket 超时与对端 RST 在日志里分不开，所以按耗时读）。页面自己的 `/scan` 重试阶梯是 5 次 × 320 ms，跨的正是 ② 里那几秒排队。

**静态墙**：同一把尺子（`nm` 取 `_stack_start_cpu0 − _stack_end_cpu0`）在这四条之后是 **65,880 B**，下限 65,536 B，**余量 344 B**；渲染循环自己的读数 `[DISPLAY] stack peak 38592 B of 65880 B` 与它一致。之后把设置模式这一支的注释与形状收了一遍（POST 那条路径同时改成不分配），`[DISPLAY] main stack` 现在读 **65,912 B**（余量 376 B），峰值仍是 38,592 B。`[HEAP] … free with setup mode's future up` 每次开机都印：S3 1,632 B、C6 **41,056 B**（那块 64 KiB 窗口用了约三分之一）。

那一轮的 `moon run iot:image` 是 S3 `1,004,848 / 6,291,456 B`（15.97%）、C6 `782,272 / 1,048,576 B`（74.60%）；收完重编是 S3 `1,003,904`（15.96%）、C6 `781,984`（74.58%）—— **反而小了**。差额几乎全在 `percent_decode`：两个调用点都内联它，`#[inline(never)]` 收回一份后 C6 镜像少 1,296 B（`nm` 的符号和少 1,314 B），S3 少 1,216 B。**查 flash 涨没涨要按符号和看，一个被两处内联的小函数能顶掉一次「精简」的全部收益。**

当轮页面 `assets/portal.html` 是 4,468 B（第三版 14,819 B，见下一节），内联 CSS/JS **是刻意的**：一张连接把页面取回，代价是它不能按请求只取一半。

### 刷新七八次就白屏：一次刷新要几条连接（`[实测]`/`[测试]`，2026-10-20）

台架报回来的是「刷七八次页面就白（转圈/报错）」——**是页面下不来，不是手机掉线**。上一轮把每个页面 socket 的收发缓冲从 256 B 抬到 1,460 B，修的是**一页要几个往返**（18 → 4），而这次的三笔账它一笔都没动：

| 账 | 这一轮之前 | 现在 |
| --- | --- | --- |
| 一次刷新要几条连接 | 页面 + `/scan` + 图标（含 `.ico` 的 204）+ 手机自己的连通性探测 = **3～6 条**，对着 3 个页面 socket；多出来的握手被栈**静默拒绝** | **1 条**（页面本身：图标内联成 `data:` URI、首屏列表内嵌进文档）；手机自己的探测仍是它自己的连接，答一次 302 |
| 页面在射频上做什么 | 每 4 s 读一次 `/scan`，而设备对**每一次** `/scan` 都 `RESCAN.request()` —— 页面开着就等于射频一直在扫 | 只在按下「重新扫描」时走 `GET /rescan`；读列表（`/scan`）**不再**唤醒扫描 |
| 首屏等什么 | 文档到了再 XHR 一次列表，列表才变成可点的行 | 列表就在文档里，首屏即可点 |

**为什么「页面自己在扫」是伤**：设置模式的接入点是 APSTA，而逐信道扫描会打断客户端 —— 这正是 ESP-IDF 文档提醒、本记录对照表里标「未复现差别」的那一条。一次刷新 3～6 条连接、每条再叠一次立即重扫，七八次之后手机看到的就是一页没人答的网络。这一轮把两件都去掉，也就把那条 `[据称]` 从「扫描会打断客户端」改成「页面不该自己按扫描」。

**内嵌列表是一个槽，不是模板**：`route::SEED_AT`（`/*scan*/`）标记位置，页面在槽里放一份**自己就能跑**的 `[]`（未替换时是一个空列表，不是语法错误），发送时 `send_page` 把 `LATEST` 的 JSON 写进槽、`Content-Length` 由 `page_len(aps) = PAGE.len() − SEED_SLOT + scan_json_len(aps)` 现算。这是这轮唯一会让「头说多少」与「写出去多少」分家的地方，所以两半都有测试：`the_head_of_the_page_names_the_page_with_the_list_in_it`（对着 `json()` 的实际长度）与 `the_page_leaves_one_slot_for_the_list_and_it_stands_on_its_own`。地址仍是等长替换（`MAC_TOKEN`），列表不是 —— 顺序也因此固定：地址在前、列表在后，两者各出现一次。

**页面第三版**（`portal.html` 14,819 B，内联 CSS/JS 仍是刻意的）：语言切换从页脚搬到**页头右上**的分段控件；密码输入从行内字段改成 `<dialog>` 从底部弹出的表单（带「显示/隐藏」与 `minlength=8`，点背景或 Esc 取消，老浏览器退回 `open` 属性）；每行 = 名字 + **该网络自己的 BSSID**（小字、等宽，同名的 mesh/中继因此能分开）+ 四格信号 + 锁/开锁 + `已保存` 胶囊；列表下面是**「重新扫描」**（`GET /rescan` → 等 1.8 s 让那一轮扫完 → 读 `/scan`）。`done.html` 只换了图标并沿用同一个语言键。

字节的账：页面从 **4,468 → 14,819 B**（+10,351：i18n 词典、对话框、卡片式布局、每行 BSSID、内联图标），但**一次刷新的往返从 3～6 条连接降到 1 条**，且首屏不再等一次 XHR。`/favicon.svg` 仍按名服务（缓存一天），`/favicon.ico` 仍是 204。

**面板列表也补了 BSSID**：手机页每行有地址，面板那 3 行没有 —— 一行只有 47 px，名字（24 字形 = 215 px）已占满宽度，同排再放 17 字形（149 px）放不下。改成**每行两行字**：名字在上、地址在下，两行共 20 px，居中后上下各 13/14 px，47 px 的击点不变（`keyboard::band_lines_top`，几何由一条宿主机测试与一条编译期断言钉住，第二行不许越到下一行）。理由是可用性而不是排版：同名的 mesh 节点与它自己、同名中继，原本只能靠信号条猜；地址与手机页那一行可以逐个字符对照。

地址一律**大写**（`AA:BB:CC:DD:EE:FF`）。三个格式化器原本两套写法：面板表头与地址行是 `mac_text`（大写），手机页每行的 BSSID（`scan` 的 `hex_digit`）与面板上**画不出的 SSID 退回成地址**那一行（`text::write_bssid`）是小写 —— 同一个人手里两块屏对着同一个值看到两种拼法，面板上还可能在**同一条行内**撞上。统一到面板本来就在用的大写；`ap_name`（`vanling-a1b2`）不动，那是接入点的名字而不是地址显示，改大小写等于给设备改名。两个格式化器各补一条**带字母的八位组**的测试（原来的夹具全是全零/全数字地址，大小写根本分不出来）。顺带：名字画不出时名字行**就是**那个地址，那一行不再画第二行 —— 同一个字符串隔一行印两遍是多印的那一遍。

**表头的设备 MAC 换了一行**：它原来占「网络」那行（`AP <名字>` 的位置），于是只要有一行被选中、或一条链路在连，它就让位 —— 正是有人要找它的时候。现在它与 `IP <地址>` 同排（地址在左、MAC 右对齐），第 1 行只属于 `AP <名字>` + 信号条。宽度是算过的：地址从块左缘起、`IP ` 连写（省掉四字形的标签槽，这是这一行唯一的破例）后最长 158 px 止于 168，MAC 17 字形 149 px 起于 171 —— 编译期断言钉住两者不相撞（第一版用标签槽，断言当场判红：差 6 px）。打字态 `PSK <草稿>` 要整行，所以那一态不画 MAC，键盘一放下就回来。同时补上 `diag_repaint_due` 的漏项：`DisplayPage::Network` 原本只比 `link`/`aps`/`setup`、**不比 `mac`** —— MAC 是射频第一份报告之后才到的，而扫描恰恰以「列表没变，不重画」为常态，那一格就可能一直空着。判据加上 `prev.mac != next.mac`，并有一条只改 MAC 的测试要求 `Network` 页重画。

**台架第一次开机**（C6，`[实测]`）：`Get / answered Ok, 15314 B of body, in 72 ms`（4 条 AP 内嵌；`page_len` 现算 = 14,819 − 2 + 497），日志里**没有 `/scan`、没有 `/favicon.svg`** —— 一次刷新就是一条连接，列表不用等 XHR。开机那行 `[HEAP] … free with setup mode's future up` 是这笔账的量尺：抬缓冲前 **33,384 B**，抬到 2,048 B 之后 **32,104 B**，正好少 1,280 B —— 坐实了那块缓冲在设置模式的 future 里（堆）而不是主栈；上一轮同一读数还是 41,056 B。

**同一份日志抓到的真缺陷**：`a request filled the buffer without ending`。Chrome Android 的请求头（UA + client hints + `sec-fetch-*`）700～1000 B，而 `REQUEST_BYTES` 只有 768 B —— 装不下的请求被**静默关掉**，手机侧看到的正是「转一下、白一页、再刷又好」。抬到 **2,048 B**；这块缓冲在设置模式的 future 里（堆），S3 的静态墙一分没涨。判据不靠推理：抬完重抓日志，那句警告消失。

**这一轮的尺寸**：`iot:image` 的 S3 `1,020,848 / 6,291,456 B`（16.23%）、C6 `796,864 / 1,048,576 B`（75.99%）；比上一轮各多 5,456 B 与 5,424 B（页面 +5,155 B 是大头，剩下的约 300 B 是面板每行第二行字的绘制）。`IOT_BOOT_PAGE=network` 的 S3 镜像是 `1,022,736 B`（16.26%），台架那块板烧的是它（开机就落在列表页）。**静态墙没动**：`nm` 取 `_stack_start_cpu0 − _stack_end_cpu0` = **65,816 B**，下限 65,536 B，**余量 280 B**；两种构建、`REQUEST_BYTES` 改前改后都是这个数（缓冲落在堆上），板上开机也印 `[DISPLAY] main stack 65816 B`，峰值 38,672 B。S3 设置模式的堆这次读到 25,560 B（同一读数在 C6 是 32,104 B）。

### 表头按「一行一个主体」重排：两台设备的 MAC 各占一行（`[测试]`，像素待看）

台架要的排法是第 0 行 `NETWORK … mac`、第 1 行 `AP <名字> 信号`、第 2 行 `IP mac`。上一版做不到，而且**不是口味问题，是叠在一起的像素**：`AA:BB:CC:DD:EE:FF` 是 17 字形 = 149 px，右对齐后占 **x=171…319**；阶段词的值从 150 起、最长 4 字形止于 **182**，与 MAC 叠 11 px；列表页码画在 **x=218** 也在同一段。一行 35 格，四项（标题 7 + 阶段 7 + 页码 5 + MAC 17 = 36 格）本来就不够。

于是挪了两样，两样都有几何理由：

- **阶段词搬到第 1 行**（`AP <名字> ST <阶段>`）：它讲的正是「跟这台 AP 的链路到哪一步」，放名字旁边；名字槽 17 字形止于 194，阶段词固定起于 **x=199**（名字长短不影响这一列），最长 7 字形止于 258，信号条 288…319。
- **列表页码搬进工具条左角**：`1-3/6` 起于 x=10，`SCAN`/`LEAVE` 仍居中（约 139…180），两者不相干。它仍由画列表的那一支画（问题框盖住列表时页码也跟着不画），只是位置在条上。

| 行 | 内容 | 宽度（px） |
| --- | --- | --- |
| 0 | `NETWORK` + **本机 MAC** | 标题 10…69，MAC 171…319 |
| 1 | `AP <名字≤17>` + `ST <阶段≤4>` + 信号条 | 名字 46…194，阶段 199…258，信号 288…319 |
| 2 | `IP <地址≤15>` + **所连 AP 的 MAC** | 地址 10…168（`IP ` 连写，省标签槽），MAC 171…319 |
| 工具条 | `n-m/8` … `SCAN`/`LEAVE` | 页码 10…51，按钮词居中 |

两个 MAC **靠行区分**：第 0 行是这块板子（标题行，任何状态都不让位，打字态也在），第 2 行是它连的那台（上一行刚写了那台的名字）。三条编译期断言钉住不相撞（标题 vs 本机 MAC、名字 vs 阶段、地址 vs AP 的 MAC）；阶段词与信号条之间那条断言把 `ST ` 的 3 字形算进去了。

**为什么不把阶段词留在第 0 行**：`ST <阶段>` 的值最短从 150 起，4 字形止于 182，与本机 MAC 的 171 恒叠；把标签槽收窄也只能让 `UP` 这种两字形的词勉强过关，`SCAN`/`JOIN`/`AUTH` 一律越界 —— 所以第 0 行只能二选一，而本机 MAC 是台架要找的东西。

手机页每行小字加了 `BSSID ` 前缀（页头已经有 `Device <本机 MAC>`，两处都不再是一个孤零零的值）。2026-10-21 起这个标签叫 `AP MAC`（见「有链路那格改回 `LEAVE`」）—— 同一个值、同一处小字，只是不再用术语。

**尺寸**：`portal.html` 14,819 → **15,128 B**（+309：前缀与它的注释）。`iot:image` 的 S3 `1,021,728 / 6,291,456 B`（16.24%）、C6 `797,168 / 1,048,576 B`（76.02%），各多 880 B 与 304 B（页面那 309 B 加面板表头的新常量与代码）。`IOT_BOOT_PAGE=network` 的 S3 镜像 **1,023,616 B**（16.27%）。**静态墙一分没动**：`nm` 仍是 **65,816 B**（余量 280 B），板上开机印 `[DISPLAY] main stack 65816 B`；C6 设置模式的堆仍是 32,104 B（这一轮没碰那块缓冲）。

**还没读的**（`[未完成]`）：第 0 行本机 MAC 与标题之间的 100 px 空白读起来是否像「这台设备」、第 1 行阶段词与名字的间距、工具条左角的页码会不会被当成可点区域 —— 这三样只有面板能给答案；手机页的前缀与 `重新扫描` 一并等台架。台架那块 S3 烧的是 `IOT_BOOT_PAGE=network` 且 `creds` 分区里存着一条网络，所以开机直接是**有链路**的状态（正好把第 0 行与第 2 行两个 MAC 都摆出来）；要看设置模式（打字、弹窗）得先擦掉那条已存网络。


### 答完就 `abort()`：RST 把手机还没读走的字节丢掉（`[实测]`，2026-10-20 夜）

台架报回来的是**偶发** `ERR_CONNECTION_RESET`：页面有时不出来，重刷又好。设备侧那几次日志全是 `Get / answered Ok, 15,425 B of body, in 88–196 ms` —— 字节确实写完了，坏在**连接怎么结束**：

- `close()` + `flush()` 只等到**自己的 FIN 被 ACK**（`FinWait2`）。对端还没发 FIN，说明它可能仍在读。
- 紧接着的 `abort()` 把 socket 置 `Closed`；栈看到 `remote_endpoint` 还在就发 **RST**（`flush()` 的 `rst_pending` 判据）。
- RST 让对端内核**丢弃接收队列里还没被应用读走的字节** —— 浏览器于是报 `ERR_CONNECTION_RESET`。

旁证三条：0 B 的答复（`generate_204`、`/mmtls` 的 302）从没失败过（一段就完，没有「还没读走」的窗口）；大页面每次都要多段；两块板共用这段代码，所以两块一起犯。

**第一次修法是错的，而且错得有用**：改成「等对端 FIN 再释放，上限 500 ms」之后，台架立刻报 `ERR_CONNECTION_REFUSED`。等 FIN 的 socket **不在监听**：3 个页面 socket 里躺着一个在等，而手机一次刷新开 6 条连接，页面那条排到第 4 位时栈上没有监听者，SYN 直接被 RST。那几分钟日志正是这个形状：`Get /` 只有 3 条，其余全是 `generate_204` 重试。

**实测把这笔等待算清了**：`flush()` 返回时对端 FIN **已经在**（`TimeWait after 0 ms`，455 次连接里 451 次），所以等它一分钱不花；上限收到 **100 ms**，只有 4/455 撞到（仍 `FinWait2`，那次 RST 落在对端早已不再读的时刻）。验收：**55 次页面请求、0 失败**（`could not accept` 0、`did not finish` 0）。

同一版把 `send` 的 `Ok(0)` 从静默返回改成记 `an answer stopped N B short`：抓到 **3 次对端**中途 reset（正文写到 9,171/15,710 B），以前这些是无声的。`CONNECTION_TIMEOUT` 也从 10 s 收到 **3 s**：只到了半截的请求不该占住 3 个页面 socket 之一十秒（一页本身约 100 ms）。

**一条被推翻的假设**：先在扫描上动过手 —— 页面每答一次就抬 `PAGE_READ`，扫描的暂停见到它就重新开始，理由是逐信道扫描把射频带离接入点信道（一轮 ≈1.15 s、暂停 5 s → 前几轮约 19% 时间不在信道上）。它确实生效：9 次刷新期间**零条 `survey found`**，前后两轮从 31 s 拉到 47 s。但**页面照样失败**，所以扫描不是病因。这段保留（有人在读页面时不该扫），别把它当成这次的修法。

**尺寸**：产品镜像 S3 `1,025,680 / 6,291,456 B`（16.30%），`IOT_BOOT_PAGE=network` 的 S3 镜像 **1,027,424 B**（16.33%）。静态墙这一轮**小了**：产品构建 **65,720 B**（余量 184 B），network 构建 **65,688 B**（余量 152 B）—— 上一轮同一块板印的是 65,816 B，少掉的 96/128 B 没有量到出处（不是等 FIN 的循环：它在设置模式的 future 里，属于堆）。余量只有三位数，记在这里。

### 与 ESP-IDF 的 provisioning 对照（三条取舍）

查的是 ESP-IDF 自己的 Wi-Fi Provisioning 文档（`wifi_prov_mgr`、`wifi_prov_scheme_softap`）。三条我们**与它不一样**：

| 事 | ESP-IDF 的做法 | 本设备 | 代价 |
| --- | --- | --- | --- |
| 什么时候停服务 | 设备**连上之后**才停，并让客户端读到状态 | **收到凭据就停**（`select` 先到者胜，见「答复先到手机，再结束设置模式」） | 口令错时手机被踢下 AP，只能靠设备自己重回设置模式。面板那条路实测 37.6 s 判 `AUTH`（待办 5④），设置模式这一支**未实测** |
| SoftAP 下怎么扫 | 文档提醒逐信道扫描会打断客户端连接，因此提供按信道分组的扫法（`[据称]`，未在本设备上复现差别） | 逐信道扫（APSTA） | 台架几轮里手机没掉线（`[实测]`）；换板子值得重验。页面这一侧已不再自己按扫描（第三版去掉每次 `/scan` 的重扫，见上一节） |
| 凭据怎么传 | protocomm 会话 + 口令校验（PoP）+ 加密 | **开放 AP + 明文表单** | 同一网段任何人都读得到那条口令，也能替设备改一条 —— 已知取舍；启动页进档的 AP 默认开放（面板 `PASS` 可切 WPA2），运行中进档的有屏板默认带口令，所以这条取舍只在启动页那一轮位于默认路径上；要收紧还可以上一次性的 PIN |

## 配网模式变成面板上的一档（`[实测]`/`[测试]`，2026-10-20）

在这之前，设置模式是**隐式**的：没有凭据、按了 `LEAVE`、或者被拒，网络任务就发散进 `unconfigured()`，面板唯一能说的是表头里那个相位词。由此有两件事做不到：**看不出设备什么时候在设置模式**（AP 开着与链路重试画的是同一页），以及**进去就回不来** —— `LEAVE` 之后只剩重扫一遍列表。

这一轮把它变成一档看得见、也退得出来的模式：

| 事 | 之前 | 现在 |
| --- | --- | --- |
| 从链路进设置模式 | 工具条 `LEAVE`，确认框问「离开？」 | 工具条 `LEAVE`，确认框问 `LEAVE "cafe"?` / `THE LINK GOES DOWN`（先叫过一阵 `SETUP`，又改回来了 —— 见「有链路那格改回 `LEAVE`」） |
| 从设置模式出来 | 做不到（`unconfigured` 发散） | 工具条 `CANCEL` → `CANCEL_SETUP` 平面 → `unconfigured` 返回 → `link` 重试它手上的网络 |
| 知道自己在设置模式 | 没有这个字段 | `LinkStatus.setup` / `setup_cancellable`，表头 `ST SETUP` |
| 表头的网络名 | 链路那个 AP（设置模式下没有） | 由 MAC 派生的 `AP vanling-xxxx`（与手机列表里同一个名字，面板不另存一份） |
| 地址行 | 站点的地址（设置模式下没有） | `IP 192.168.4.1` |
| 首次进入 | 无声 | 弹一次 `SETUP MODE` / `AP vanling-xxxx` / `CLOSE`，任意一点关掉；每次开机只弹一次 |
| 声音 | 无 | 进设置模式 `Rise`、连上 `Tap`、被拒 `Error`（复用现有 MIDI 提示音，无 PCM、无端侧 TTS） |
| 硬件键 | Network 页的 Click 是 `Invalid` | 与工具条同一个动词（`LEAVE`/`CANCEL`/`SCAN`） |
| 计数 `n-m/8` | 工具条那一带的左角 | 标题行 `NETWORK` 与 MAC 之间的空档 |

几条规则是这一轮定下来的，都钉在 core 的测试里：

- **`CANCEL` 只在有回头路时出现**：`setup_cancellable` 由调用方算（`link` 看已存列表是否非空，build 指名的那条路径恒为真），而不是网络任务去读库 —— 库在 `unconfigured` 里仍是可变的，而「回到哪」是调用方的问题。没有回头路时工具条退回 `SCAN`：一台从没设置过的设备按 `CANCEL` 只会把自己的 AP 关掉再开，手机看到的是页面消失。
- **设备不把「模式」塞进相位**：`LinkPhase` 仍是射频的（设置模式下是 `Off`/`Scanning`），`setup` 是它旁边的一位；把一个不是相位的状态塞进相位枚举，会让 `Connected`/`Failed` 的每个判据都多一条岔路。
- **首次公告与「每次 AP 起来」分开**：`seen` 每次开机一份；拒绝之后 AP 会再起来一次，那一次不该把框盖回键盘上（错口令要重打）。
- **声音在状态的转移里起**，不在页面里：网络任务是唯一知道「刚连上/刚被拒」的地方，而它只发 `LinkStatus`。

`[测试]` 宿主机钉住的行为：工具条两个动词与 `take_setup_cancel` 只交一次；没有回头路时退回扫描；公告只出现一次、`Dismiss` 关掉它、之后的重入不再弹；`Part::hidden` 在公告期间盖住列表与工具条；Network 页的硬件 Click = `Toolbar`；公告期间任意一点（含长按）= `Dismiss`；`Rise`/`Tap`/`Error` 三个转换；`format_question(Leave)` = `LEAVE "cafe"?`；公告第二行 = `AP vanling-2fc8`（合成 MAC）。

`[实测]` 门禁：`cargo fmt --check`、`moon run iot:test`（core 651、bsp 112、app 34）、`iot:lint`、`iot:check-host`、`iot:check-s3`、`iot:check-c6` 全过。

`[实测]` 静态墙（同一把尺子：`nm` 取 `_stack_start_cpu0 − _stack_end_cpu0`，下限 65,536 B）：

| 树 | 主栈 | 余量 |
| --- | --- | --- |
| HEAD `b61479d` 重编（`git stash` 掉未提交改动） | 66,008 B | 472 B |
| 上一轮动手前（本会话早先的读数） | 65,720 B | 184 B |
| 本轮之后 | **65,688 B** | **152 B** |

所以本功能净吃 **32 B**（65,720 → 65,688），而相对 HEAD 那 320 B 里大部分是尚未提交的门户/RST 那一批。板上读数与 `nm` 一字不差：`[DISPLAY] main stack 65688 B`、`[DISPLAY] stack peak 38520 B of 65688 B`。

flash：`moon run iot:image` 出 S3 **1,031,168 / 6,291,456 B（16.39%）**，比上一轮产品镜像的 1,025,680 B 多 **5,488 B**；C6 同名镜像在 `dist/`。

`[实测]` 板上（产品镜像，`IOT_PORT=/dev/cu.usbmodem1422301`）：

```
[WIFI] this build was given no network and this device has saved none, so the setup access point and the panel are both open until one of them names one
[WIFI] setup mode for "vanling-2fc8" is open
[HEAP] 25560 B free with setup mode's future up
[WIFI] survey found 3 access points
[DISPLAY] main stack 65688 B
```

`[未完成]` 这一轮**没有**做的：

- **无屏板的入口**：入口在启动页上按住 Boot（见「回到设置模式：启动页上按住 BOOT」第三版）；**未做**的是单击入口（C6 的单击仍是光源换色，LED 就是那盏灯）。灯语本身见下一节。
- **像素与手感**：弹窗、`ST SETUP`、`AP`/`IP` 两行、计数的新位置、`CANCEL` 的字宽都要人看一眼；`LEAVE`/`CANCEL` 的触摸要人按一次。
- **设置模式下错口令**：仍是上一轮留下的未测项，这一轮没改它的路径，只是现在退出设置模式不用重启。

### 无屏板的灯语（`[测试]`，2026-10-23）

C6 只有一颗 WS2812，没有屏幕，所以配网状态只能由这颗灯说。灯语定在 core 的状态层（`DeviceState::status_light`），只在「有射频、无屏幕」的板上生效；S3 的屏幕照旧画它自己的页面与框。

| 设备状态 | 灯 | 为什么 |
| --- | --- | --- |
| 从未配网（启动页结束之后） | 琥珀闪 3 下（160 ms 亮 / 160 ms 灭，约 0.8 s），随后页面灯 | 出厂/离线待配网；这是公告，不是模式，亮完就把灯还给页面 |
| AP 已开，等待手机 | 蓝色慢呼吸（2 s，8→48） | 正处于配网模式；慢呼吸表示等待不是错误 |
| 未存盘的凭据正在关联/等地址 | 蓝色快呼吸（0.8 s，16→64） | 手机刚提交的那一份；已存凭据的自动重连不走这条 |
| 关联成功（进入 `Connected`） | 绿色闪 3 下（100 ms 亮 / 100 ms 灭，约 0.5 s） | 成功是事件，不是模式；闪完页面灯接管 |
| 失败：找不到 AP（`NotFound`） | 琥珀闪 3 下 | 人挪一下设备就能解决，不是口令错 |
| 失败：口令被拒等（其余 `Failed`） | 红色闪 3 下（120 ms 亮 / 120 ms 灭，约 0.6 s） | AP 已经拆掉，手机看不到设备；红色表示「被拒绝」 |
| 已配网且在线 | 不覆盖，显示页面自己的灯 | 正常待机 |
| 已配网但暂时掉线 | 不覆盖；一次断网只公告一次 | 「有网络、只是不在线」是事件不是模式；重试不再反复闪，页面灯一直是底 |

优先级是**一次性通知 > AP 配网模式 > 未存盘凭据的连接尝试 > 页面灯**。成功/失败只在相位跳变的那一拍触发；一次断网由 `outage_announced` 锁住，幂等地只公告一次，`Connected` 时解锁，所以重试不会每 5 s 又把灯盖住。已存凭据的自动重连（`saved == true`）从不显示蓝色「连接中」，从开机第一秒起就是产品灯。通知的时钟属于渲染循环（`tick_notice`），状态层只保存当前该画哪一帧，因此宿主机测试能用普通数字驱动它。最后一闪在亮的边沿结束，不再留一段黑场。

接入点的 180 s 空闲时钟到点后，无屏且无存量网络的板子会收掉 AP、回到离线 `browsing`；这一路径在收 AP 后补发一条 `setup = false` 的链路报告，灯从蓝色回到**页面灯**，不再对已经关掉的模式继续说「配网中」。

`[测试]`：`cargo test -p iot-core` 钉住灯的模式与优先级、三种公告的闪法（含最后一闪不留黑场）、`NotFound` 与口令被拒的颜色分档、启动页落页时的一次琥珀公告、启动页按住时不误报离线、重试只公告一次且重连后解锁，以及「状态灯只覆盖第一个光源、有屏板不覆盖」。

`[实测]`（2026-10-23，C6，产品镜像，复位不按键）：启动页在 `00:00:01.848` 结束（1500 ms），随后 `[WIFI] no panel and no saved network; waiting offline`，灯语逐帧如下 ——

| 时刻 | 灯 |
| --- | --- |
| 01.848 | `[LAMP] solid 255 155 0 at 96` — 第一下琥珀 |
| 02.028 | `[LAMP] dark` |
| 02.188 | `[LAMP] solid 255 155 0 at 96` — 第二下 |
| 02.348 | `[LAMP] dark` |
| 02.508 | `[LAMP] solid 255 155 0 at 96` — 第三下 |
| 02.668 | `[LAMP] the page's own light` — 最后一闪在亮的边沿结束，页面灯接管 |

渲染循环只在状态灯**变化**时打 `[LAMP]`（呼吸的页面灯不会刷屏），所以台架上不用眼睛也能数出闪了几下、每下多长：上面的 180/160/160 ms 比设计的 160 ms 各多不到一帧（`STEP_MS = 20`）。

`[实测]`（2026-10-23，C6，目视）：三下琥珀之后确实回到产品灯；单击 BOOT 灯色随页面变化，说明灯没有被状态层长期压住；启动页之后再长按不开 AP。亮度与颜色可以接受。**绿色与红色两档仍未实测**：要一次真实配网（`Connected` 的绿、错口令的 `Credentials` 红），宿主机测试只钉住帧表，不钉住实物。

## 设置模式的两档前端：一次只开一档（`[实测]`/`[测试]`，2026-10-20 夜）

> 上一节那张表里的「首次进入弹一次 `SETUP MODE` / `AP` / `CLOSE`，任意一点关掉」已经被这一节取代：那个框现在就是**接入点档的脸**，只在接入点档开着的时候画。
>
> 表里那行「无屏板开机就是接入点档」也被「回到设置模式：启动页上按住 BOOT」第三版取代：**没有凭据的设备开机是离线的**，不扫描、不广播 AP；接入点档只在复位+按住、离线页的 `AP` 格、面板 `LEAVE` 或被拒之后才起来，180 s 空闲到点就收。两档前端本身没变。

上一轮把设置模式变成面板上的一档，但两个入口**同时在线**：AP 一直开着，面板的列表也一直扫。于是同一根射频在两种角色之间来回，而一台只是放在桌上没人动的设备，一直在广播一个谁都能连的配置页。

这一轮拆成**两档前端、一次只开一档**：

| 事 | 之前 | 现在 |
| --- | --- | --- |
| 有屏的板子进设置模式 | AP 与面板列表同时可用 | 先上面板（`Front::Panel`），AP 不开 |
| 想改用手机 | 直接连 AP | 工具条右格 `AP`：AP 起来，列表被一个框盖住 |
| 想回面板 | 做不到 | 框上 `CLOSE` 或工具条右格 `PANEL`：AP 拆掉，列表回来 |
| 无屏的板子（C6） | AP 一直开 | 开机就是 `Front::AccessPoint`，**开放** AP（见本节后的补记） |
| AP 有没有口令 | 总是开放 | **按入口**：启动页进档开放；运行中从 Network 页进 AP，有屏板默认 WPA2 + 随机 10 位（不记住）；C6 无屏只能开放 |
| 没人看页面 | AP 一直开 | 3 分钟没有页面请求就拆 AP、回到面板 |
| 触摸坏了 | 只能断电重启 | 设置模式下**任意页** Boot 长按 = 切档 |

几条规则是这一轮定下来的：

- **两档不拆代码**：`configure` 的两档共用 `setup_credential`、`SurveyHistory`、列表、页面、`JOIN`/`FORGET`/`CANCEL`；`Front` 只决定现在开着哪一档。两档是**一个前端位 + 一个 AP 开关**，不是两条路径 —— 拆成两条会让「切回来」变成第二次实现。
- **口令由站点生成并持有**：`begin_setup(ssid, secured)` 一次说清「开哪一种」，`status()` 读回 `setup_password`。面板从链路状态里拿到它、画进框里；页面不经过它，日志也不记值（只记位数）。
- **`SetupPass` 是定长窄类型**（`[u8; 10]`），不是 `Ssid`：链路状态每个窗口都要拷一份，32 字节的名字字段装 10 字节的值，就是主任务每帧白付的 22 字节。这一条同时是过墙的手段之一（见下面的表）。
- **字母表去掉 `0/o/1/l`**：`23456789abcdefghjkmnpqrstuvwxyz`，31 个符号 × 10 位 ≈ 49.5 bit。把它从 320 px 的一行抄到手机键盘上，形近字符是唯一会出错的地方。
- **默认口令按入口、不记住**：启动页进档开放；运行中从 `AP`、`LEAVE` 或被拒后进档，有屏板默认 WPA2 + 随机 10 位，框里 `PASS …`，`NO PASS` 可把这一轮改回开放。切走再回来恢复入口默认；C6 无屏只能开放。代价见表。
- **`CLOSE` 与 `CANCEL` 不是一回事**：框上右格 `CLOSE` 只把 AP 拆掉（回到面板档）；工具条左格 `CANCEL` 是退出设置模式、回去连已存的那条网络。后者只在有回头路（`setup_cancellable`）时画；没有已存网络时面板档的左格换成 `SCAN`、接入点档则整条只有 `PANEL` —— 一台空白设备从面板档进接入点档，再从接入点档回面板档，全程不碰「退出」。
- **框里乱点不关 AP**：`answer_at` 对两个按钮之外返回 `None`，让工具条继续命中；这一档里没有「任意一点关掉」，因为手指在框上乱点不该把手机正在填的 AP 拆掉。
- **接入点档不按定时器扫描**：`survey_pause_after(Front::AccessPoint, …)` 返回 `None`（不是零秒，是没有时钟）。进档时扫一轮填页面，之后只有页面自己的 `RESCAN` 要一轮。
- **3 分钟**（`SETUP_IDLE_MS`）：页面每读一次就重置，`CLOSE` 立刻拆。它约束的是接入点档比面板档多出来的那点代价：一个**任何人都能连**的网络在开着。代价是首屏不再广播 AP —— 一台刚开箱的设备在手机列表里是**看不见**的，得先有人按一下面板上的 `AP`；无屏板没有面板可按，入口是复位后在启动页按住 BOOT（见第三版入口一节），所以开箱设备同样不在手机列表里。

几何与文案：

- 工具条两格在 core（`ToolbarCell` / `toolbar_cell` / `toolbar_cell_at`）：`PANEL_COLUMNS/2 - TOOLBAR_GAP` = **152 px** 一格，8 px 的缝从格子里扣而不是加在格子外；`PANEL_COLUMNS` 整除两格、单格 ≥ `TOUCH_MIN` 都是编译期断言。缝不属于任何一格，但**按中间的列切**：`x < PANEL_COLUMNS/2` 是左格，其余是右格，8 px 里没有死区。
- 左格：有回头路时 `CANCEL`，面板档没有回头路时 `SCAN`（一轮在飞时 `WAIT`），接入点档没有回头路时不存在；右格永远是 `AP`/`PANEL` —— 切档写目的地；有链路那一格是例外，`LEAVE` 写的是对链路做了什么（见「有链路那格改回 `LEAVE`」）。
- 框：标题 `SETUP MODE`，两行正文 `AP vanling-xxxx` + `PASS …`（或 `OPEN, NO PASSWORD`），两答 `NO PASS`/`PASS` 与 `CLOSE`。两行正文的底边要压在框内答案之上，这是编译期断言（`DIALOG_BODY_TOP + ROW_PITCH + FONT_H <= answer.y`），`PASS ` + 10 位 ≤ `DIALOG_GLYPHS` 也是。
- 面板档**不画**假的 `AP`/`IP` 行：没有 AP 就没有名字和地址，表头只剩 `ST SETUP` 与列表。

`[测试]` 宿主机钉住的：`LinkStatus.front` 与 `shows_setup_box()`（= 设置模式且接入点档）；`survey_pause_after(AccessPoint, …) == None` 而其余三档仍是三个时钟；两格几何（同宽、缝在格内、`toolbar_cell_at` 两半与边界）；`switch_front`/`take_setup_open` 只在对应档位生效且 `take_*` 只交一次；框的两答与「两答之外返回 `None`」；设置模式下任意页 Boot 长按落在右格；框起时 `Part::hidden` 盖住列表、**不**盖工具条；`format_setup_pass` 的 `PASS k3mq9t7w2x` 与 `OPEN, NO PASSWORD` 两种形态。

`[实测]` 门禁：`cargo fmt --check`、`moon run iot:test`（core **656**、app 34）、`iot:test-bsp`（bsp **113**）、`iot:lint`、`iot:check-host`、`iot:check-s3`、`iot:check-c6`、`moon run iot:image` 全过。

`[实测]` 静态墙（同一把尺子：`nm` 取 `_stack_start_cpu0 − _stack_end_cpu0`，下限 65,536 B）：

| 树 | 主栈 | 余量 |
| --- | --- | --- |
| 上一轮之后 | 65,688 B | 152 B |
| 本轮（两档 + 口令 + 合并的请求平面） | **65,592 B** | **56 B** |

净吃 **96 B**。省下来的两笔是明确的：`CANCEL_SETUP` 并进 `SETUP_ASK`（一个平面、一个 `Signal`，少一个静态和一条 select 分支），`setup_password` 从 `Option<Ssid>` 收成 `Option<SetupPass>`（34 → 11 字节）。板上读数与 `nm` 一字不差：`[DISPLAY] main stack 65592 B`、`stack peak 38696 B of 65592 B`。

`[实测]` 板上的接入点档：为了在没有手指的情况下看到这一档，把 S3 分支的初值**临时**改成 `Front::AccessPoint`（一行，量完即回退；产品仍从面板档起）：

```
[WIFI] this build was given no network and this device has saved none, so setup mode opens
       on the front this build starts with until it names one
[WIFI] setup access point up as vanling-1234 (under a passphrase)
[WIFI] setup mode for "vanling-1234" is open, under a passphrase
[WIFI] setup mode: an access point, a lease for whoever joins it, and the page at http://192.168.4.1/
[WIFI] survey found 1 access points
[DISPLAY] main stack 65592 B
```

（日志里的 AP 名按合成值记作 `vanling-1234`：它由 MAC 的末两字节推出来，真机那两字节不进仓库。进档那一轮扫描是填页面用的，之后没有定时器；3 分钟那一行见下。）

`[实测]` 而**产品**（S3 从面板档起）开机那一屏什么都不广播：同一台板子、同一个空 `creds` 分区，日志里没有 `setup access point up` 这一行，列表照常每 5–6 s 一轮：

```
[WIFI] this build was given no network and this device has saved none, so setup mode opens
       on the front this build starts with until it names one
[WIFI] survey found 3 access points
[DISPLAY] main stack 65592 B
```

这就是「首屏不广播」那句话的实据：一台刚开箱的设备在手机列表里看不见，得先有人在面板上按 `AP`。

`[实测]` 3 分钟自动关档：

```
[WIFI] nobody read the page in 180 s; the access point goes down
```

之后扫描回到面板档的节奏（先是每 5–6 s 一轮，空轮越多越慢），`nm` 与板上主栈不变。

### 真机上的三处：切档格、扫描格、框自己关掉（`[实测]`/`[测试]`，2026-10-20 夜第二版）

产品镜像烧上去，面板上三件事没成立：看不到切档那一格、看不到 `SCAN`、点了 `AP` 之后框起来又自己关掉。三处各有确定的代码路径，不是手感问题：

- **左格不画、右格铺满整条**。`stamp_strip` 原来在 `!setup_cancellable` 时只画右格，而右格那时用 `toolbar_box()` 铺满整条、只写 `AP` —— 于是 `SCAN` 整条消失，切档格看起来也不像一格。这台板子的 `creds` 是空的（没有任何已存网络），`setup_cancellable == false` 正是常态，不是边角。
- **框自己关掉**（这一条当时只找到了其中一半，另一半见下一节）。`SetupAsk` 是一个 `embassy_sync::Signal`：起 AP 要 ~1 s，这期间再点一下 `AP`（或第一下被读成两下），那个 `Front` 请求就留在平面上；下一个读它的是**接入点档**的循环，它立刻把 `Front` 读成「回面板」，框开了就关。请求是**对哪一档**发的，平面本身不记得。
- **修法一：两格永远两格**（工具条 A 案）。左格 = 有回头路时 `CANCEL`；面板档没有回头路时 `SCAN`（一轮在飞时 `WAIT`，与设置模式外同一个词）；接入点档没有回头路时不画左格、右格铺满整条 —— 接入点档没有列表可扫，画一个空半格就是个点不动的控件。右格永远是 `AP`/`PANEL`。`state::toolbar` 读同一组条件：左格 = 取消 / 重扫 / 切档。
- **修法二：请求带上它是在哪一档发的**（`SetupAsk::{Cancel, Front, Pass} { from: Front }`）。渲染循环发布时读当前 `link.front`，两档的循环各自只接 `applies_to(自己的档)` 的请求，其余的丢掉并记一行：`dropped a setup request made on the panel's front; the access point stays up`（另一个方向是 `... made on the access point's front; the box is not open`）。没有时间窗：一条请求要么是这一档发的，要么不是。

`[测试]` 钉住的：`a_setup_request_only_answers_the_front_end_it_was_made_on`（六种组合）；`the_panels_front_keeps_a_cell_for_looking_around_when_there_is_no_way_back`（面板档无回头路时左格 = `SCAN` 且 `rescan_wait` 起来，右格仍是切档）；接入点档那一条改名为 `..._has_only_the_front_switch_on_the_access_points_front`。

`[实测]` 门禁：`cargo fmt --check`、`moon run iot:test`（core **657**、app 35）、`iot:test-bsp`（113）、`iot:lint`、`iot:check-host`、`iot:check-s3`、`iot:check-c6`、`moon run iot:image` 全过；`nm` 仍是 **65,592 B**（余 56 B），因此这一轮只动了几行代码与两个 `Front` 字段，没有碰主栈。烧上去开机：`[DISPLAY] main stack 65592 B`、`stack peak 38696 B`、无 `setup access point up`（产品仍从面板档起）。

`[未完成]` 这一轮**没有**做的：

- **框不再自己关、两格看得见**：前者要人点一次（代码路径与单测都在，但「起 AP 那一秒里再点一下」只有手指能复现）；后者是像素，要人看。
- **像素与手感**：两格的宽度、`AP`/`PANEL`/`SCAN`/`CANCEL`/`NO PASS`/`PASS`/`CLOSE` 的字宽、框里第二行 `PASS …` 的位置、面板档表头空着的 `AP`/`IP` 两行 —— 都要人看一眼；`AP` 那一格、框里两答、以及设置模式下的 Boot 长按要人按一次。
- **C6 的开放 AP**：策略与代码都在（`serve(..., Front::AccessPoint, false)`），`iot:check-c6` 编得过，但 C6 上没有产品路径，**未实测**。
- **无手指的切档走查**：这一轮试过把它加进 `IOT_PANEL_BENCH`（点右格 → 框起来 → 点 `NO PASS` → 点右格回面板，全走一遍 intent 环），**但 bench 变体因此挂不上静态墙**：`vanling-panel` 的 `.data` 比产品多 160 B（符号尺寸一字不差，是 bench 那批 `format_args!` 带出来的对齐空洞），主栈掉到 65,432 B、比下限少 104 B。所以那条走查没有留下 —— 留一段编不过的走查比没有更糟。要补的话，得先让 bench 变体自己付得起那 160 B。

### 每轮扫描都把档位写回面板：站点那份状态不知道自己在哪一档（`[实测]`/`[测试]`，2026-10-21）

上一节把「框自己关」记在 `SetupAsk` 的陈旧请求上；`applies_to` 挡住了跨档那一瞬间的那一次（日志里 05:24 的 `dropped ...`），但**每一轮都发生**的是另一条：

```
[WIFI] setup access point up as vanling-1234 (under a passphrase)
[WIFI] setup mode for "vanling-1234" is open, under a passphrase
        ← 框此刻出现
[WIFI] survey found 2 access points
        ← 状态里的档位回到面板，框消失（射频仍然开着 AP）
```

`setup_credential` 进档先扫一轮，`survey()` 扫完发布的是 `LinkStatus { setup: true, ..station.status() }`。站点那份状态只知道射频那一半（相位、AP、地址、口令），**不知道自己在哪一档**：`front` 是默认的 `Panel`，`setup_cancellable` 是 `false`。于是每轮扫描都把这两格覆盖回去 —— 框被 `shows_setup_box()` 判成「不在接入点档」而消失；有已存网络的板子上 `CANCEL` 也会每 5 秒变成 `SCAN` 一次。这也解释了为什么它是**自己**关的：手指一次也没碰。

修法：`LinkStatus::on_setup_face(front, cancellable)`（core）把档位与回头路一次写好，相位/AP/地址/口令原样透传；网络任务四处发布（面板档、接入点档、扫描成功、扫描失败）都走它，`survey()` 拿的也是同一个 `Face`。

`[实测]` 不用手指的读数：把 S3 的初值临时改成 `Front::AccessPoint`（一行，量完即回），串口只有一次档位变化，扫描之后不再回面板：

```
[WIFI] setup mode is offered on the AccessPoint front
[WIFI] survey found 2 access points
```

换回 `Front::Panel` 的产品镜像开机：没有 `setup access point up`、没有档位变化行，列表每 5–6 s 一轮。渲染循环现在把档位变化记一行 `[WIFI] setup mode is offered on the {front:?} front` —— 这条日志就是「不用眼睛看框」的读数。

`[测试]` `a_publish_from_setup_reports_its_own_face_not_the_stations_default`：两种档 × 可取消与否，覆盖 `front`/`setup_cancellable`/`setup`，并断言相位、AP、地址、口令一字不动。

`[实测]` 门禁：core **658**、app 35、bsp **113**，`iot:lint` / `iot:check-host` / `iot:check-s3` / `iot:check-c6` / `iot:image` 全过；`nm` 65,592 B（余 56 B）不变。

### 名字：`PHONE` → `AP`（同一轮）

`PHONE` 是按「你手里拿着什么」命名的：对笔记本不成立，也不是行业词。Espressif 把这整类叫 *provisioning*（SoftAP 与 WEB 两种传输），WLED、Tasmota、Shelly 一律叫 **AP mode**，而框里第一行本来就是 `AP vanling-xxxx`。于是：工具条右格 `AP`、内部 `Front::AccessPoint`、`PHONE_LABEL` → `AP_LABEL`；`phone_idle` → `page_idle`（它等的是页面请求，不是手机）。「用手机配网」这类叙述照旧 —— 说的是方法，不是档位名。

**仍然只能靠眼睛/手指的**：上面证明的是「档位不再被扫描改回去」，不是「框被画出来了」。所以还欠人一次：点 `AP` 后框应当一直开着、两格宽度与 `AP` 的字宽、框里两答；接入点档整条只有 `CANCEL`，面板档是 `SCAN | AP`。

### 有链路那格改回 `LEAVE`；网页那行改叫 `AP MAC`（`[实测]`/`[测试]`，2026-10-21）

真机上不对的是两句话，不是两处逻辑：

- **`SETUP` 看不出会断网**：有链路时那一格上一轮按「动词写目的地」写成 `SETUP`，可是面板档没有 AP —— 进设置模式就等于离开这条链路，一个只写目的地的词把代价藏起来了。改回 `LEAVE`：框问 `LEAVE "cafe"?`，正文仍是 `THE LINK GOES DOWN`，两答 `KEEP` | `LEAVE`。落点没变（`Hold::Left` → `unconfigured()` 的面板档），出来时左格 `CANCEL` 把已存网络连回来 —— `LEAVE`/`CANCEL` 成对。切档仍写目的地（`AP`/`PANEL`）：不切过去就看不到那边，而离开这件事的意义**就是**代价本身，所以它是一条例外。
- **网页那行的 `BSSID` 改叫 `AP MAC`**：页头已经是 `Device <本机 MAC>`，行里那串是**热点**的 MAC。`AP MAC` 说清是谁的，不用术语也不和页头混。**线上字段名仍是 `"bssid"`** —— 那是协议，不是标签。
- **离开链路回面板那一档，不是回手机上**：`front` 原来在 `link()` 里是 `let mut front = front;`，和任务同寿，只有面板的 `AP`/`PANEL` 会改它 —— 链路起来、链路走后都不复位。于是用手机配过网的设备 `front` 停在 AP 档，之后在面板上按 `LEAVE`，`unconfigured()` 直接把 AP 又拉起来：面板上点的动作，下一个模式开在手机上。`Setup` 的注释只承诺「跨过被拒的凭据」（手机重输口令还留在手机页），从没说过要跨过成功入网。修法是 `front_after_link(start, current, ended)`：`Left`、`Named` 都回 build 的起始档（有屏 = 面板；无屏 = AP），`Refused` 留当前档。两条启动日志也补上了档位（`on the {:?} front`），否则下一次这种问题串口答不出来。
- **`AP MAC` 和地址挤成一条**：标签与值原本是**一个** `textContent`（`'AP MAC ' + bssid`），一个普通空格在 `.72rem` 的灰字里只有 3 px，读起来就是 `AP MACA4:CF:…`。拆成 `.tag` + `.addr` 两个 `span`，间距交给 `gap: .5rem`，地址提到 `--ink`；页头 `Device <MAC>` 的 `<b>` 一并提亮，两行同一套「灰标签 + 亮值」。

`[测试]` `a_leave_names_the_network_the_link_is_on`：`format_question(Leave, link)` = `LEAVE "cafe"?`；链路还没被接受（`Connecting`，没有 `ap`）时只写 `LEAVE`；`Confirm::Leave.network(link)` 就是链路的 AP —— 框后面被反白的行和问句点的是同一个名字。`format_verb` / `format_body` 的词与正文不变（`LEAVE` / `THE LINK GOES DOWN`）。工具条那格现在直接画 `format_verb(Confirm::Leave)`，和框里的答案同一个来源 —— 邀请和按钮不能再各说各的（`the_boxes_words_are_the_verbs_own_and_fit_the_box` 盯着这两处）。

`[实测]` 门禁：core **658** / app 35 / bsp **113**；`iot:lint`、`iot:check-host`、`iot:check-s3`、`iot:check-c6`、`iot:image` 全过；`nm` 仍 **65,592 B**（余 56 B）—— 只是把 `LinkStatus` 多传进两个同步函数，没有跨 `await` 的新状态。

**仍要一次手指/眼睛**：有链路时那格写 `LEAVE`、点下去框是 `LEAVE "cafe"?`、答 `LEAVE` 后回到**面板档**（列表 + 左 `CANCEL` + 右 `AP`，串口应是 `setup mode opens on the Panel front`）；网页那行的 `AP MAC` 与地址之间有一道看得出空隙、地址比标签亮一档。

`[未完成]` `front_after_link` 没有宿主机测试：`network.rs` 挂在 `wifi` 特性下，而 `wifi` 拉起 `iot-bsp-esp/wifi`，宿主机没有能编译它的构建面（`iot:test` 的 `host` 不含 `wifi`）。判据是那次 `LEAVE` 的串口日志与面板上的档位；要把这条也钉进测试，得先有一条假 `Station` 的链路任务台架。

### 刷新不再 `ERR_CONNECTION_REFUSED`：keep-alive、六个监听位与半秒（`[实测]`/`[测试]`，2026-10-21）

C6 台架上手机打开设置页，浏览器报 `ERR_CONNECTION_REFUSED`。复位抓串口（`[实测]`，手机自己回来）：

```
[WIFI] Get /generate_204_<uuid> answered Redirect, 0 B of body, in 0 ms
[WIFI] a request would not be read after 0 ms: ConnectionReset
[WIFI] a request would not be read after 0 ms: ConnectionReset
```

探测那一条答了 302，接着两条连接被手机 reset，整段里**没有一条 `Get /`** —— 要的页面根本没到。「一次刷新只有一条连接」只算了文档自己，没算手机在文档旁边开的那些：预连接与系统探测几乎同时发 SYN，而 `SETUP_SOCKETS = 5` 是 DHCP 1 + DNS 1 + 页面 **3**。第 4 条 SYN 找不到监听者时**栈直接回 RST**，应用层看不到；Chrome 把这个 host 记成拒绝连接，连已经挂上的两条也一起丢掉（那两条 `ConnectionReset` 就是它丢的）。

三处修改：

1. **keep-alive**（`[测试]`）：答复从 `HTTP/1.0 … Connection: close` 改成 `HTTP/1.1 … Connection: keep-alive`，`Response::write_head(out, close)` 里 `close` 是调用方唯一的话；`answer()` 在同一条连接上继续读下一个请求，空闲窗口 `KEEP_ALIVE = 2 s`。手机发 `Connection: close`（`Request::close`，逗号列表里带 `close` 也算）或请求本身命名网络（`Action::Join`，答完就要拆 AP）时才关。刷新因此复用已有连接，不再消耗监听位。**pipelining 不支持**：同一段里多出来的第二个请求随连接缓冲一起丢掉，浏览器不发。
2. **页面监听位 3 → 6**（`PAGE_SOCKETS = 6`，`SETUP_SOCKETS = 2 + PAGE_SOCKETS = 8`）：6 是 Chrome 每 host 的连接上限，盖住「探测 + 预连接 + 页面」这一次突发。
3. **`PENDING_TIMEOUT` 1 s → 500 ms**：一句话都没说的预连接更快把监听位还回来。

**堆的账**（`[实测]`，C6 开机）：`[HEAP] 32048 → 22168 B free with setup mode's future up`，少 9,880 B —— 3 对 1,460 B 收发缓冲是 8,760 B，余下是 `StackResources` 多出的 3 个槽。页面 future 仍装箱在堆上，**静态墙没动**：`nm` 取 `_stack_start_cpu0 − _stack_end_cpu0` = **65,592 B**（下限 65,536 B，余 56 B）。S3 同一读数没有重取（那块板现在存着网络、不进设置模式），按同一差额估 25,560 − 9,880 ≈ 15,680 B。

**尺寸**：`iot:image` 的 S3 `1,049,120 / 6,291,456 B`（16.68%）、C6 `813,424 / 1,048,576 B`（77.57%）。

`[未完成]` 手机侧：C6 已烧这版镜像（开机 `[HEAP] 22168 B`），要人打开页面并连点十次刷新 —— 预期是十条 `Get / answered Ok`、零条 `ConnectionReset`、零条 `would not be read`；S3 的设置模式堆读数也要在那次一并确认。

### 提交：done 页上屏之前 AP 还在——join 之后 2 s（`[实测]`，2026-10-21）

keep-alive 落地后刷新正常了，**提交**那一下手机却报 `ERR_CONNECTION_RESET`。抓串口（C6，复位后 44.8 s），设备侧是一条完全正确的记录：

```
[WIFI] Post /submit answered Ok, 2469 B of body, in 10 ms
[WIFI] the page named a network, so the link is joining it
[WIFI] a request would not be read after 0 ms: ConnectionReset   ×5
[WIFI] joining the network setup named: "…"
[WIFI] setup mode is offered on the Panel front
```

- 没有 `the answer's socket was still FinWait2 after 100 ms`，也没有 `an answer did not finish`：done 页 2,469 B 发出去了，手机 ACK 了，**手机自己的 FIN 也在 7 ms 内到了**（那 100 ms 的 `CLOSE_WAIT` 一次都没用上）；
- 那 5 条 `ConnectionReset` 不是 submit 那条连接：同样形状的 `×5` 在 `generate_204` 的 302 之后、`/mmtls` 的 302 之后各出现过一次，是 Chrome 在导航开始时丢掉自己的池连接；
- 时间差在最后两行：答复到 AP 拆掉只有 **68 ms**（`…853` → `…921`）。ACK 是手机内核收下了字节，不是浏览器画出来了 —— 「配网热点会随之消失」这句承诺在 done 页上屏之前就已经兑现。

改动：`LEAVE_GRACE = 2 s`，`page()` 在 `JOIN.publish` 之前停这一下，AP 多留 2 秒。publish 之后 `pending().await`：这 2 秒里再点一次提交不会再往 join 平面塞第二个凭据，否则链路刚连上就会被那条迟到的请求拽走。代价只有一个定时器，**静态墙不动**（`nm` 仍 65,592 B），C6 的 `[HEAP] 22,168 → 22,048 B`（−120 B），`iot:image-c6` **813,904 / 1,048,576 B**（77.62%）、`iot:image-s3` **1,049,568 / 6,291,456 B**（16.68%）。

**验收**（`[实测]`，手机侧 + 串口）：

```
[WIFI] Post /submit answered Ok, 2469 B of body, in 17 ms
[WIFI] the page named a network, so the link is joining it
        ← 2,058 ms
[WIFI] joining the network setup named: "…"
[WIFI] setup mode is offered on the Panel front
[WIFI] phase Connected, address Some([…]), 0 windows without one
```

done 页正常出现，设备随后加入网络。这 68 ms 究竟坏在「浏览器没画完」还是「Android 把配网门户那次导航作废」，手机侧分不开；两次运行之间只改了这 2 秒，所以按「答完就拆太快」记。同一段串口里页面的 `Get /` 有 4 次答完、另有几次 `an answer stopped … B short: ConnectionReset` —— 那是刷新时 Chrome 把上一条还没写完的加载取消掉，新的加载照常答（GET 会重试，POST 不会，这也是为什么只有提交会露出来）。



`iot-core::drivers::network` 里三条规则是产品决定而不是管道，所以放在能被测试打到的层：

- **扫描结果按 BSSID 去重、按 rssi 排序、截断到 8 条**。同名不同 AP 是两个网络（手机也这么显示），所以身份是 BSSID 而不是名字。
- **SSID 是字节**。802.11 允许任意八位组，面板字库只有可打印 ASCII，所以「这个名字能不能画出来」是一个带退路的判断（画不出就显示 BSSID）。非 ASCII 的名字仍然逐字节保存（连接需要它），只是不可绘制。
- **一轮扫描的节奏**（5 s 一轮、空轮 1 s 后重试到连续三次为止、无人动过就退到 15/30 s）也是同一类政策，见「一轮扫描的节奏」。

`Credentials::from_text` **拒绝**超长口令而不截断 —— 驱动也是拒绝，而截断过的口令对着任何东西都认证不上，读起来却像「密码错了」。

凭据走**构建时环境变量**（`IOT_WIFI_SSID` / `IOT_WIFI_PSK`，`option_env!`），不是 secret store：bench 镜像要说自己连的是哪个网络，而编进去的值就是构建命令行里的值。**产品的凭据在面板输入、存 `creds` 分区**（见「凭据落在哪」），绝不编译进固件。构建输入优先于已存列表：一个被指名了网络的构建只有那一条路可试（要改就改构建），而已存列表是「这设备被设置过」的答案。

### 回到设置模式：启动页上按住 BOOT（`[测试]`/`[实测]`，2026-10-21 第三版）

入口没变：**两块板开机都先画 1.5 s 的启动页，按住 Boot 才进设置模式**。理由照旧 —— 同一根手指在一台工作的设备上已经是页面自己的动词（Ambient 换模式、Speaker 静音），而 C6 要的是一次**故意的仪式**（先复位，再在窗口里按住），不是一条每三分钟能被一次长按打断的旁路。

这一版换掉的是这一按**落到哪**、以及启动页期间**射频做什么**：

| 事 | 上一版 | 这一版 |
| --- | --- | --- |
| 启动页上长按 | `Toolbar(Right)`：有链路就离开，没有就 `Invalid` | `BusinessIntent::RequestSetup`：**只记请求**（`setup_wanted`），不改页、不开 AP |
| 启动页关闭 | 回 `boot_page` | 请求过且 `network_enabled` → **落 `Network`**；否则回 `boot_page` |
| 启动页期间的链路 | 读凭据、关联，长按再把它拆掉 | **一条链路都不建**：`link()` 先等 `PAGE.wait_for_splash_end()` 才读凭据 |
| 请求过之后 | 先关联、再离开 | 跳过关联：`Front::AccessPoint` 直接进设置模式 |
| 判断依据 | `is_leavable()` 看链路 | 与链路无关：窗口内按下就算数，两块板同一个答案 |
| 无凭据开机 | 自动进设置模式、AP 起来 | **离线可用**：不自动进设置模式、不起 AP |
| 这一按的页面 | 框直接叠在启动页上 | 页先落 `Network`，`link()` 之后才拉 AP 档；启动页上不叠任何框 |

几条规则：

- **`SPLASH_MS = 1_500`**。从 3 s 收到 1.5 s：长按阈值是 600 ms，1.5 s 仍容得下一次完整的按住；而手指刚离开复位键就能按到 Boot，再长只是让每次开机都慢一拍。窗口是「在 1.5 s 内**开始**按住」，不是「在 1.5 s 内按完」。
- **判据是按下时刻在窗口内，不是松手在窗口内**：时钟到期时若手指还按着（`InputSource::held()`，含去抖中尚未上报的释放），启动页**等松手再关**（`app/src/input.rs`）。1.4 s 按下、2.1 s 抬起那一下仍然算数，不会在按住的时候被时钟从手指下面抽走。
- **一次性是结构上的**：`Splash` 不在 `CYCLE` 里，`next_page(Splash)` 直接回 `Ambient`，`with_boot_page` 拒绝它，唯一的出口是输入任务到期报的 `BusinessIntent::SplashOver`（总线满时下一拍重试）。开机之后没有任何手势能回到这一页。
- **落点是 `boot_page`，除非请求过**：`with_boot_page()` 只记「启动页关掉之后落在哪」（构建输入 `IOT_BOOT_PAGE`、默认 `Ambient`；未 back 的页退回 `Ambient`）。`SplashOver` 读 `setup_wanted`：请求过且这块板开着网络，就落 `Network`；否则落 `boot_page`。启动页本身不改 `state.page`，所以 `IOT_BOOT_PAGE=network` 的 bench 构建照旧落在网络页。
- **启动页期间一条链路都不建**（`[实测]`）：`link()` 的第一件事是 `PAGE.wait_for_splash_end()`，它没结束时既不读凭据也不碰射频 —— 启动页窗口里不会出现 `joining`/`leaving`。请求过就直接 `Front::AccessPoint` 进设置模式，一次关联也不做；「先连上再拆掉」的写法会在这个窗口里留下两条日志，也会让 AP 档晚 2–3 s 才起来。
- **无凭据 = 离线可用**：`first_saved()` 为空时 `browsing()` 接住这台设备 —— 只在 `PAGE.shows_list()`（面板停在 Network 页）时扫描，页离开就停，`JOIN` 来了才 `hold`（此时才写盘）。不自动进设置模式，也不起 AP；面板页的两格仍在（`SCAN | AP`），点 `AP` 才把 AP 拉起来。C6 没有面板，配网仍要复位后在启动页按住 BOOT，这是它唯一的入口。
- **面板页常驻**：扫描、列表、键盘、`JOIN` 从设置模式的循环里拆了出来（`setup_credential`），`link.setup` 现在表示**页面在设置面上**：面板档不起 AP，接入点档才起。S3 无凭据时三击到 Network 就是活的列表（`browsing()`），左 `SCAN` 右 `AP`，点 `AP` 才拉 AP；点一行 → `JOIN` → `hold`。
- **入口不止一个**：离线页的 `AP` 格、有链路时面板的 `LEAVE`（`Hold::Left`）、已存凭据被拒（`Hold::Refused`）都进设置模式 —— 都是手指自己的动作。
- **空闲时钟总是武装**：`page_idle()` 不再看 `armed`；进 AP 档就起 180 s，页面每读一次重置，退出时有屏板回面板档、无屏板回已存网络。旧版「无屏且无存量 → 时钟不启动」的例外已经删掉：无屏板只有显式的复位+按住，有屏板另有离线页的 `AP` 格，一台出厂设备的开放 AP 不该一直广播。
- **有屏板画三行**（`VANLING` / `HOLD BOOT` / `SETUP`，`bsp-esp` 的 `stamp_splash`），行距 `3 × FONT_H`、按 `width`/`height` 居中。无屏板（C6）没有这页可看，状态照走：Boot 键那一按一样记下请求。

`fast_connect`（这一轮的新能力，`[测试]`）：

- 每个存盘网络多带一个 `LinkHint { bssid: [u8; 6], channel: u8 }`（`NONE` = 未知）。关联时 `Station::begin_connect_hinted` 把它交给驱动：`EspStation` 覆盖成 `config_for(credentials, hint)`，`ScanMethod::Fast` + `with_bssid`/`with_channel`，驱动只去那个信道找那个 BSSID，不再全信道扫一遍。
- **失败回退并重学**：hint 那一轮失败（驱动拒收或关联超时）就 `hint.take()` 并 `continue`，下一次全扫；成功关联后用 `status().ap` 把新的 BSSID/信道写回（`learn`）。`learn` 只在值变了才写盘 —— 同一条网络、同一个信道不产生磨损。
- **与 ESP-IDF 对照**：`wifi_sta_config_t` 也是 `bssid_set` + `bssid` + `channel` 三个事实加 `scan_method = WIFI_FAST_SCAN`；本仓把同样三个事实存在凭据旁边，但不是「记住就信」：AP 换了信道、或同名 mesh 节点换了 BSSID 时，失败回退让 hint 成为一次尝试而不是一条硬约束。这是参考实现没写的那半步。
- **凭据记录升到 v2**：`VERSION = 2`、`ENTRY_BYTES = 108`（hint 占 8 B：6 B BSSID + 1 B 信道 + 1 B 对齐）。**开发阶段不做 v1 兼容**：v1 的记录读成 `Opened::Blank`（等于没存网络），需要重新配一次；`a_version_one_record_is_read_as_nothing` 钉住这个答案。

`[测试]`：`the_splash_page_is_where_the_button_asks_for_setup_mode`、`the_splash_page_answers_nothing_but_the_hold`、`a_splash_long_press_asks_for_setup_mode_on_every_board`、`the_splash_lands_on_the_page_that_offers_setup_when_it_was_asked_for`、`the_splash_lands_on_the_build_page_when_nothing_was_asked`、`a_board_with_no_radio_lands_on_its_page_even_after_the_hold`、`a_long_press_after_the_splash_is_the_pages_own_verb`、可达性穷举里的 `the splash's clock`，以及 hint 那一侧的三条：`a_hint_survives_a_reset_and_a_retyped_password`（改口令不丢 hint）、`learning_a_channel_writes_once_and_only_when_it_moves`、`a_version_one_record_is_read_as_nothing`。

`[实测]` 上板（2026-10-23，产品镜像，两块板各复位一次、不按 BOOT）：C6 在 `00:00:01.848` 打出 `the splash page is over 1500 ms into the input clock`，随后 `[WIFI] no panel and no saved network; waiting offline`；S3 在 `00:00:02.748` 结束（输入时钟起点晚约 1.25 s），`[DISPLAY] main stack 66264 B`，随后走离线 `browsing`。两块板的启动页期间都没有 `joining`/`leaving`，也没有 AP。

`[实测]` 门禁与静态墙：启动页那一轮之后，这一轮的 future 又长过墙 —— 差 **72 B**。收法不是删功能，而是让一个凭据不再跟着每个帧走：`hold` 改成收 `&Credentials`，`Hold::Named` 装 `Box<Credentials>`，`link` 手里那份凭据装箱（每个凭据一次分配，而不是每个窗口一次）。`nm` 读数 `_stack_end_cpu0 = 0x3fcd9658`，`_stack_start_cpu0 − _stack_end_cpu0` = **65,720 B**（下限 65,536，余量 184 B），主任务 `POOL` = 42,704 B。`.cargo/config.toml` 里没有 `--noinhibit-exec`：`iot:build-s3` 自己链接通过。镜像：S3 `1,057,424 / 6,291,456 B`（16.81%）、C6 `820,384 / 1,048,576 B`（78.24%）。

`[未完成]`：

- **按住那条要手指**：复位后的 1.5 s 内按住 BOOT，应落 `Network` 并进 `Front::AccessPoint`，串口里不出现 `joining`，直接是 `setup mode ... is open`。两块板各一次。
- **S3 的三行像素要人看**：`stamp_splash` 的布局有测试（三行都落在面板内、行距与居中），1.5 s 的时序也已实测（`00:00:02.748` 结束），但字模好不好看、这 1.5 s 感觉是否太赶，只有眼睛知道。
- **配网之后的开机时序**：两块板的 `creds` 现在都是空的，`joining`/`associated` 落在启动页之后的那一刻还没有读数，要先各配一次网。

### AP 的默认口令按入口分开；离线页恢复两格；字模换成 Spleen 5×8（`[测试]`/`[实测]`，2026-10-22）

- **AP 的默认口令按入口**：`setup_ap_starts_secured(panel, from_splash)` 决定 `secure_on_entry`。启动页进档开放（`OPEN, NO PASSWORD` + `PASS`）；运行中的 `AP`、`LEAVE`、被拒与命名构建的 `LEAVE` 在有屏板上默认 WPA2 + 随机 10 位（`PASS …` + `NO PASS`）。两个按钮只改这一轮，切走再回恢复入口默认；C6 无屏，始终开放。
- **离线页恢复两格**：`Face::browsing()` 报 `browsing: true`、`setup: false`（`front` 仍是 `Panel`），Network 页因此仍是 `SCAN | AP`，但表头画的是链路相位而不是 `ST SETUP`，长按也不再是切档。`AP` 由 `SETUP_ASK` 接住，交回 `link()` 以 `Front::AccessPoint` 进 `unconfigured()` 才起 AP；离线不自动进档、不广播，180 s 空闲仍由 `page_idle()` 收 AP。
- **字模**：Spleen 5×8（Frederic Cambus，BSD-2-Clause），原生 5×8，`m/M/w/W`、`0`、`\` 同一套；`FONT_H` 8、`OVERLAY_ROW_GAP` 5、节距 9×13 不变。许可证与回归见 `hardware-constraints.md` 的「面板的字模」。
- **上板读数**（2026-10-21 产品镜像，两块板各复位、不按 BOOT）：S3 启动页 `00:00:03.367` 结束，`[DISPLAY] main stack 65720 B` 与 `nm` 一致，随后 `[WIFI] this build was given no network and this device has saved none; it runs offline`；C6 `00:00:03.346` 结束，同样离线、无 AP。两块 `creds` 仍为空。
- **镜像与墙**（2026-10-22）：S3 `1,057,616 / 6,291,456 B`（16.81%），C6 `821,328 / 1,048,576 B`（78.33%）；相对上一版 `+128 B` / `+64 B` 是入口判定与复位那几行的布局差异，未逐项拆分。`_stack_start_cpu0 − _stack_end_cpu0` = **65,688 B**（`0x3fce9710 − 0x3fcd9678`，余量 152 B）。
- **未实测**：正常入口点 `AP` 后的 `[WIFI] setup mode for "vanling-1234" is open, under a passphrase`，与启动页按住 BOOT 后的 `..., open`，各要一次手指；Spleen 在 1× 面板上的观感（小写开口、`m/w` 的密度、降部）只有人眼能给答案；离线页的 `SCAN | AP` 两格、点 `AP` 后的 `PASS`/`NO PASS` 框也要手指点一次。

## 启动页之外的长按不再开 AP；C6 的凭据又能存住并自动重连（`[测试]`，真机待验，2026-10-23）

两处缺陷来自同一类问题：状态用布尔近似，页面又拿近似值当模式。

- **长按在启动页之外开了 AP**：`Face::browsing()` 把离线列表报成 `setup: true`，于是 `link.setup` 从配网结束起一直为真；`intent.rs` 的长按门只看 `link.setup`，任何一次长按都成了 `SwitchFront`。灯仍是琥珀色，因为 `status_light()` 只认 `setup && front == AccessPoint`，于是现象是「没进设置模式，长按却把 AP 起来了」。
- **C6 复位后不自动连**：`iot:flash` 走 `cargo run`，runner 是 `espflash flash --monitor`，没有 `--partition-table`；espflash 的默认表没有 `creds`，`Store::open` 每次开机都读不到分区，设备就永远像「从未配过网」。S3 的 `flash-s3` 与 `iot:image` 都带表，所以只有 C6 中招。

改法：

- `LinkStatus` 新增 `browsing: bool`，`Face::browsing()` 报 `setup: false`、`browsing: true`。`controls::strip` 为 `browsing` 单独画 `SCAN | AP`；表头因此画链路相位而不是 `ST SETUP`。
- `SwitchFront` 接 `setup || browsing`，但仍要求 `panel.is_wired()`；长按门仍是 `link.setup`，所以只有启动页那 1.5 s 和真正的设置模式会响应长按，离线页只有显式点 `AP` 才进档。
- 无面板且无存盘网络时，`link()` 不再进 `browsing()`，而是原地等：没有页面可浏览，也没有格子可点，设置模式只剩启动页这一条入口。C6 的其余链路逻辑与 S3 相同。
- `iot:flash` 改成显式的 `espflash flash --chip esp32c6 --flash-size 8mb --partition-table partitions/esp32c6-devkitc-1.csv --monitor`，并在 `moon.yml` 里写明为什么不能继续用 `cargo run`。

`[测试]`：`a_board_with_no_panel_has_no_front_to_switch_to`（无面板时 `SwitchFront` 不生效）、`an_offline_page_keeps_its_ap_cell_but_a_hold_does_not_raise_one`（`browsing` 的 `AP` 格仍可点，长按不切档）、`controls::tests::the_offline_list_keeps_both_cells`。`iot-core` 691 条测试全过，`check-s3`、`check-c6`、`check-host` 全过。

`[实测]`（2026-10-23，C6，新的 `iot:flash`）：

- 烧录后的分区表第 3 项是 `creds`（`00110000 00002000`），开机打 `[CREDS] no network saved on this device yet`，不再是「表里没有 `creds` 分区」。
- 配网存下一份凭据后复位：`[CREDS] 1 saved network(s)` → `joining ...` → `associated ... channel 11`；第一次 DHCP 30 s 超时，重关联后 `Connected` 并拿到地址。自动重连这条通了。
- 擦掉 `creds` 后再复位：`waiting offline`；启动页之后按键，日志里没有 `setup mode ... is open`，也没有 AP。灯仍是未配网的状态色 —— 无屏板的状态灯覆盖页面灯，所以灯色不能作为这条的判据；日志里的 `page is now Ambient` 说明按键确实到达了状态机。长按门本身由 `a_board_with_no_panel_has_no_front_to_switch_to` 等测试覆盖。

`[未完成]`：S3 的同类复测（当前台架只枚举到 C6）；C6 重新配一次网后，长按 Ambient 应能看到页面灯换档（状态灯让位），这条要人看；以及 `creds` 现在已被测试擦空，C6 复位后会停在未配网状态。

## 待办（`[未完成]`）

1. **两颗芯片的驱动堆余量都只剩约三成**（C6 14,124 B / 49,152 B = 28.7%；S3 15,820 B / 49,152 B = 32.2%，两颗都是实测值）。谁调大 `DYNAMIC_RX_BUFFERS` 或 `STATIC_RX_BUFFERS`，两块板就一起付账 —— 要么在改那两个常量的同一个提交里把两颗都量一次，要么把该板的份额抬到 56 KiB（文档区间的高端：S3 的主栈有余量付这 8 KiB，C6 没有）。
2. **相机的 DVP DMA 为什么能吃掉整轮扫描**：12 轮里 1 轮，没有机制。产品页按「空结果就继续扫」处理，不阻塞交付。
3. **产品页要调 `note_address`**：那是「已连接」唯一的通路，漏了就会一直显示「连接中」。
4. **改缓存之后重测** `[INPUT]` / AEC 的既有数字。
5. **产品页第二片：面板键盘 + 凭据持久化 + 已存标记与忘记**。三片都已落地：键盘与交互见「产品页」，凭据见「凭据落在哪」，已存 `*` 与长按忘记见「离开后点回那一行、以及一次丢掉的列表修正」，点已存行用库里的口令见 ⑦。还剩下的：

    - **① 真机验证**：**整条「点一行 → 输口令 → OK → 关联 → 拿到地址 → 断开 → 忘记」已由 `IOT_PANEL_BENCH` 构建在板上走通**，也含「链路在飞时点那一行没有反应」「打字态不画工具条」「连接框在射频答复之前就画出来」（见「按面板路径打字」「工具条、确认框与忘记」「连上时点行不接、弹窗盖住列表」「打字态没有工具条、连接中有一个框」，全程不需要手指），以及第五轮那条「点回已存的那一行 → 库那份口令自己把链路接回来 → 页面什么都没问」（见「点已存的一行就用库里的口令」）。剩下非得人来的只有两件：4×4 键、列表行与工具条的**触摸手感**（含第四行键现在不再被工具条压着）、纯黑底色与背光 100%、以及连接框的观感与它出现/消失的时机 —— 这些是像素与触感。弹窗**盖没盖住列表**与**连接框挂没挂在那儿**都不是这两件之一 —— 它们是宿主机可测的事实（`Part::hidden`、`Part::showing`、`page::walk`、`Part::IN_ORDER`、`stamp::fill_surface`）。
    - **② 存**（**已落地**，格式、槽规则与写盘策略见「凭据落在哪」）：接线撞上的三件事都在不烧板的前提下定完 —— ① **MD5 条目 espflash 自己会写**：表里前三条 96 B 的 MD5 = `5b7fb69b103d49aee6dd86b70ccdddcf`，`esp-bootloader-esp-idf` 的 `validation` 找的正是它，**不用改**；② **双核写**：本应用从不启动第二核，默认策略今天也能写，仍选 `multicore_auto_park()`（每次写前停另一核、写完放开，几毫秒），换来以后真起第二核不会踩到；③ **多一条 `data` 分区**：子类型必须写**名字**（`creds,data,undefined,,0x2000,` 转出来是 `01 06`），写十六进制会让 `esp-idf-part` 在 `unwrap()` 上 panic。板上三次开机验过（空盘写下来、以及**产品镜像不带任何构建输入自己连回已存网络**，日志见「凭据落在哪」）。
    - **⑥ 已存网络的标识与忘记**（**已落地**）：列表里给已存网络画 `*`、长按那一行 = 忘记（`Touch::Hold` → `BusinessIntent::AskForget(ap)`，只对已存行弹 `FORGET` 确认框）。实现没有走「把名字集经一个平面送进 `DeviceState`」那条路：S3 静态内存墙下 `SavedNetworks`（约 980 B）在 app 里落成值或 `&EMPTY` 都会顶爆，于是**掩码跟着列表自己走**（`ApList.saved: u8`，一位一行），`SurveyHistory::{observe, marked}` 收 `Option<&SavedNetworks>` 以免在 app 里造值。代价是两份列表各自持一份，`SCAN` 平面因此必须投得进去也丢不掉（见第四轮）。擦除走 `Store::forget`，板上验过（见「凭据落在哪」第 4 次）。
    - **③ 细节**（已完成）：选中行整条反色、失败词画在打字态中槽、失败后自动重开键盘并保留草稿、`SHOW`/`HIDE` 回显、开放网络的 `OPEN` 标记与免键盘直连。
    - **④ 失败分档的真机映射**（已实测）：错口令 → 驱动 reason `FourWayHandshakeTimeout` → `NetworkError::Credentials` → 面板画 `AUTH`，从交出凭据到被拒 **37.6 s**（等的是驱动自己那次四步握手的超时）。映射表 `kind_of_disconnect` 只认能辩护的几种（`NoAccessPointFound|BeaconTimeout` → `NotFound`、`AuthenticationFailed|FourWayHandshakeTimeout|HandshakeTimeout` → `Credentials`），其余 `Driver`；`NotFound` 那半仍**未实测**（要一次把 AP 关掉的台架）。
    - **⑤ 开放网络的真机读数**：bsp 的 `Open` 修好之后这一支仍**没有台架证据**（台架上没有开放网络），目前只有状态机测试与代码审读。WPA3-only 的网由驱动版本决定连不上（见「连接策略」），不是待办而是范围。
    - **⑦ 点已存的一行就用库里的口令**（**已落地**，细节与读数见「点已存的一行就用库里的口令」）：页面交**名字**、网络任务查 `SavedNetworks::copy_of` 取口令、查不到回报 `NoCopy` 让页面升键盘；库交出来的那份**不写盘**（`Provenance`）。**只有代码审读**的三处：`NoCopy` 兜底那一格（掩码与库同源，台架上造不出「有掩码但库里没有」）、「库那份写失败」与「开机自读那条不写盘」（这次开机是 `creds` 擦空之后的面板支，走不到它）。

    列表那一条仍然成立：**不能拿一轮扫描的结果当答案** —— 实测单轮在 0～6 个之间跳，页面因此按「空轮 1 s 后重试」处理（见「一轮扫描的节奏」），并把「没扫过」与「扫了没扫到」分开画。
6. `esp-radio` 是 beta：升级版本时重跑本页全部，**两颗芯片都要跑** —— 分芯片的预算差异正是从驱动的实际用量里来的。
7. **把 Wi-Fi blob 的 IRAM 代码送去 flash**：这是 S3 拿回那 32 KiB `dram_seg` 的唯一一条路，而本仓的脚本插不进 esp-hal 生成的那两条规则之间 —— 要先给 esp-hal 开一个能容纳这件事的钩子。见「同一套代码，两颗芯片」里的驱动堆。
8. **设置模式**：**两块板都已在台架上跑通**（C6 全链路；S3 在修掉「每轮 338 B 的分配」「一条 socket 不够」「超时与挑选顺序」三处之后 14 次 `/`、10 次 `/scan` 无 panic —— 见「台架：从链接到手机，再到四轮修正」）。面板上它现在是**明档**：可进（`LEAVE`）、可退（`CANCEL`，仅在设备留着网络时出现）、进得去也看得见（`ST SETUP` + 公告框，见「配网模式变成面板上的一档」）。还没有读数的几件：**口令错时设备是否自己回到设置模式**（面板那条路已知 37.6 s 判 `AUTH`，两档这一支要人拿手机试一次错口令）、一次**开放**接入点的真机读数（台架上的邻居都有口令，待办 5⑤ 同一条）、**无屏板的入口**：复位后在启动页按住 Boot（见「回到设置模式：启动页上按住 BOOT」第三版；上一版那条「有链路时长按 Boot 离开链路」已删除）、以及**手指**：切档那一格、框上的两答、和设置模式下任意页的 Boot 长按。
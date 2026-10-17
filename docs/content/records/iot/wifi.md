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

**两颗芯片的探针都在台架上跑完**（扫描 + 关联 + DHCP）。产品页分两片：**只读的那一片已落地**（`DisplayPage::Network`：阶段 + 当前 AP + 地址 + 扫描列表），**第二片也落了盘**（4×4 键盘、回显键、逐行滑动的列表、列表迟滞与缓存、选中行反色、开放网络、失败分档、扫描门控与退避，以及**工具条**：有链路时一个 `LEAVE` 按钮、无链路时一个 `SCAN` 按钮，前者执行前问一次、后者不问 —— 见「产品页」），**凭据已落盘**（`creds` 分区 + 自控槽格式，开机自己读回来、关联成功即写，见「凭据落在哪」），**已存网络在列表里画 `*`、长按那一行 = 忘记**，**点一下已存的那一行就用设备自己存的那份口令去连**（见「点已存的一行就用库里的口令」）。后四轮报来的缺陷与两处结构修正还在这份记录里。面板这条路有一条**不需要手指**的验法：`IOT_PANEL_BENCH` 构建开机后按面板自己的坐标把口令打一遍，再按工具条与确认框的坐标走完断开、点回那一行、断开、忘记、点回那一行、取消与重扫（见「按面板路径打字」与后四轮的读数）；它证明的是状态机与几何，而**触摸手感与纯黑/背光仍要人点一次面板才算验过**（板上只有 FT6336 产面板坐标，软件合成不了触摸）。状态层另有一层**可达状态穷举**（35 个动作、8,800 个状态、四条不变量，见「离开后点回那一行、以及一次丢掉的列表修正」）。未完成的事在[文末](#待办（[未完成]）)；其余都在下面。

## 台架实测（`[实测]`，2026-10-15）

同一份探针代码，两颗芯片各刷一次，带构建时凭据连本机那个网络（下文写作 `<ssid>`；网络名与口令都只进构建命令行，不落仓库）。S3 用 `-a watchdog-reset` 启动、C6 用普通复位（原因见 `@/development/iot/flashing.md`，命令也在那里）：

| | S3（LCKFB SZPI） | C6（DevKitC-1） |
| --- | --- | --- |
| 扫描 6 轮 | 每轮 0.65–1.0 s，1–6 个 AP | 每轮 0.73–0.80 s，1–4 个 AP |
| **关联** | **2184 / 2203 / 2189 ms**（三次开机） | **3246 / 3261 ms**（两次开机） |
| **DHCP** | **546 / 22 / 44 ms** | **31 / 548 ms** |
| 链接总耗时 | 开机后 9428–9789 ms | 开机后 10325–10995 ms |
| 地址 | `192.168.1.136` | `192.168.1.158` |
| 驱动 C 堆 | **32,204 B 空闲 / 65,536 B**，40 秒内一条不差 | **14,124 B 空闲 / 49,152 B** |
| `phase Connected` | 稳 | 稳 |

> `[实测，2026-10-16 更新]` 表里 S3 的 **65,536 B 是那天的预算**。从今天起两颗芯片都是 **49,152 B**（S3 的 `dram_seg` 那一半从 32 KiB 收到 16 KiB，原因见下面「驱动的堆」那一节）。拿着凭据把 S3 探针重跑一遍（同一段扫描 + 关联 + DHCP）：
>
> - 驱动初始化 + 6 轮扫描在 48 KiB 下通过，没有 `OutOfMemory`；
> - 关联用 **2,228 ms**，此后 **42 ms** 拿到地址 `192.168.1.136`（开机后 9,337 ms 链路起来）；
> - 连上之后驱动堆 **15,820 B 空闲 / 49,152 B**（32.2%），85 秒里 38 次读数一条不差 —— 表里那天 S3 的 32,204 B 是 64 KiB 预算下的读数；
> - 产品镜像走同一条路：`[WIFI] this build joins the network named …`、`associated`、`phase Connected`，无 panic。

两块板看到的是同一批 AP（`<ssid>` 最强 −45…−50 dBm，`ap1`、`ap5`、`ap6`、`ap7`、`aa:00:00:00:00:01` 等），所以两张表之间的差别是 part 的差别，不是环境的差别。

`esp-radio` 官方文档给的 station 堆需求是 **47–57 KiB**（默认 `ControllerConfig`，高端那一档对应 6 个动态 RX buffer，正是本仓的取值）：32 KiB 会让 `esp_wifi_init` 直接 `OutOfMemory`。实测两颗芯片的驱动各用掉 33,332 B（S3）/ 35,028 B（C6）—— **C6 多用 1,696 B**，所以同一份代码在两颗芯片上要有两种给法（总数现在都是 49,152 B），见「同一套代码，两颗芯片」。

周围接入点（S3 侧 12 轮 + 6 轮并集，每行取最强）。**名称与 BSSID 是占位**（`ap1`…、`aa:00:…`），信道、信号、加密是实测值：

| 名称 / BSSID | 信道 | 最强 | 加密 |
| --- | --- | --- | --- |
| **`<ssid>`** | 11 | **−50 dBm** | secured |
| `aa:00:00:00:00:01` | 1 | −78 dBm | secured |
| `ap1` | 6 | −81 dBm | secured |
| `ap2` | 11 | −80 dBm | secured |
| ap3 | 6 | −82 dBm | secured |
| `aa:00:00:00:00:02` | 8 | −84 dBm | secured |
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
| 列表态（键盘放下） | `NETWORK` / `ST <phase>` / `IP <addr>` 三行表头（第三列 `n-m/8`），下面 **3 行 × 47 px** 的列表（名字 + 信号条；开放网络右端画 `OPEN`；**已存网络名后画 `*`**；**选中行整条反色**），再下面是**一条工具条**（有链路时 `LEAVE`，否则 `SCAN`） | 点行 = 选中该行；**已存网络（画 `*` 的那一行）不弹键盘**，只把那一行的**名字**交给网络任务、由它拿库里的口令去连（库查不到那个名字时，射频回报、页面才升键盘问口令）；**开放网络**同样不弹键盘，空口令直接交凭据；其余行升起键盘。**上/下滑 = 列表滑动一行**；**长按一行 = 问要不要忘记它**；**点工具条** = 有链路时问要不要断开，没链路时立刻重扫（不问）；切页只剩**硬件三连击**。**有链路可离开（`Connecting`/`Connected`）时点行一律不接**：不选中、不升键盘、不交凭据（`OPEN` 行也一样），长按与滑动照旧（**连接框挂着时连它们也不接**：列表没画出来就不可点，见下） |
| 打字态（键盘升起） | **同样三行**表头（`NETWORK` + `ST <phase>` / `AP <name>` + 信号条 / `PSK <草稿>`，一种读数一行）+ **4 列 × 4 行**键盘（键 **80 × 46 px**） | 只有键：字符键多击换候选、`123`/`#&?` 换层、Shift、退格、OK、X、SHOW(回显)；其余意图一律 `Invalid` |
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
| 长按一行 = 忘记 | `Touch::Hold` → `Intent::RowHold(band)`：只有那一行**已存**才弹 `FORGET` 确认框，答「动词」才擦 | 忘记不可逆（口令要从头再打一遍），所以只对它问；长按而不是再给一个按钮，是因为它只对少数行成立，画出来会占掉行右端。确认框里的名字来自那一行的 `ApList` 项，擦除走 `Store::forget`（整条记录重写，双槽轮写照旧） |
| 重扫不问 | 无链路时工具条是 `SCAN`，点它立刻发一个 `RESCAN` 信号给扫描任务 | 扫描是只读的、可再来的，没有不可逆的东西要确认；有链路时**不给 `SCAN`**：链路在手上时 `survey` 根本不会被调到（那时 `setup` 在 `hold` 里等它下来），两个动作从不同时成立，工具条不必二选一地说「而且」 |
| 列表滑一行 | 上/下滑 = `BusinessIntent::ListSlide`，`list_top ± 1` 并把末端对齐到最后一个完整窗口 | 一页一页翻时短列表的末页会留空、窗口读数会跳 3；一行一格让每一行都滑得到，`n-m/8` 也是连续的 |
| 列表为什么不跳 | 排序政策在 core（`SurveyHistory`：4 dB 迟滞 + 掉轮留 2 轮 + 只在画出来变了才 publish） | 扫描读数每轮都在抖（信道 11 的 σ≈3.1 dB），拿单轮读数重排就是列表一直跳。机制与证据见「列表为什么不跳」一节 |
| 失败分档 | `NetworkError` 分 `NotFound`（`NOAP`）/`Credentials`（`AUTH`）/`Busy`（`BUSY`）/`InvalidArguments`（`NAME`）/`NoMemory`（`MEM`）/`Driver`（`DRV`）；失败**自动重开键盘**并把词画在打字态中槽 | 4 字形词与阶段词同一套画法（`format_phase` 的四字形测试）。分档表只认驱动枚举里能辩护的那几种，其余落 `Driver` —— **真机映射未实测**，见待办 5④ |
| 页面底色由表面声明 | `DiagnosticsSink::draws_pages()`（默认 `false`）由 `DisplayLight` 覆写为 `true`；`app::render::LightRenderer` 在「表面画页 && 页 == Network」时给纯黑填充 + 背光 100% + 不呼吸 | 由 core 按页派生外观是错的：不是每个光源都有屏幕（C6 那颗 WS2812 也是 light instance 0），而「我画不画页」只有表面自己知道 |
| 命中与绘制同源 | `keyboard::{key_of, key_at, key_box, list_band_at}` 一套几何 | 画在哪和点到哪是同一组函数，改几何不会漏掉命中 |
| 意图只报页面显示的 | `BusinessIntent::Row(u8)` 是**页面上的 band 序号**，`Key { key, at_ms }` 带抬指时刻 | 状态层不该知道滑动滑到哪一条；`at_ms` 是多击判断唯一需要的时钟。`ListSlide` 同样不报目标位置：滑一次就是一格，抵到末端与否是状态层自己的事 |

画什么由 `diag_repaint_due` 的 Network 谓词定：阶段、当前 AP、地址、列表、打字态任一有变就重画，包络与触摸计数不重画（`[测试]` 两条，两个方向）。

数字到字形的换算全在 `bsp-esp/src/camera_readout.rs`（跨页的绘制算术都在那，宿主机可测；本页的 `[测试]` 10 条）：阶段词（失败画六种之一：`NOAP`/`AUTH`/`BUSY`/`NAME`/`MEM`/`DRV`，`MEM` 是「这个构建的堆预算错了」唯一的读数）、点分地址（无地址是 `-`，不是 `0.0.0.0`）、信号条 `#`/`.`（与列表同一把尺子 `bars_for`）、名字（画不出 ASCII 就显示 BSSID；超过 24 字形保留头部并以 `~` 标记，否则折在同一处的两个网络会画成同一行）、请求框的标题与两个答案词、以及连接框的标题与正文（`JOINING`/`JOINED` + 名字、`WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS`；名字超出剩余宽度时折的是名字，`JOINING`/`JOINED` 那个词不动）。

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

结构侧全在宿主机可测的层：`page::walk(setup, link, draw)`（`Content` 被问题**或被连接框**藏；`Overlay` 二者皆无时不画；两条同时在场画问题）、`camera_readout::{format_joining_title, format_joining_body}`（`JOINING`/`JOINED` + 名字、`WAIT FOR THE NETWORK`/`WAIT FOR AN ADDRESS`，四个词都由 `LinkStatus.ap` 有没有值定，2 条新测试）。

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

## 纯逻辑部分（宿主机可测）

`iot-core::drivers::network` 里三条规则是产品决定而不是管道，所以放在能被测试打到的层：

- **扫描结果按 BSSID 去重、按 rssi 排序、截断到 8 条**。同名不同 AP 是两个网络（手机也这么显示），所以身份是 BSSID 而不是名字。
- **SSID 是字节**。802.11 允许任意八位组，面板字库只有可打印 ASCII，所以「这个名字能不能画出来」是一个带退路的判断（画不出就显示 BSSID）。非 ASCII 的名字仍然逐字节保存（连接需要它），只是不可绘制。
- **一轮扫描的节奏**（5 s 一轮、空轮 1 s 后重试到连续三次为止、无人动过就退到 15/30 s）也是同一类政策，见「一轮扫描的节奏」。

`Credentials::from_text` **拒绝**超长口令而不截断 —— 驱动也是拒绝，而截断过的口令对着任何东西都认证不上，读起来却像「密码错了」。

凭据走**构建时环境变量**（`IOT_WIFI_SSID` / `IOT_WIFI_PSK`，`option_env!`），不是 secret store：bench 镜像要说自己连的是哪个网络，而编进去的值就是构建命令行里的值。**产品的凭据在面板输入、存 `creds` 分区**（见「凭据落在哪」），绝不编译进固件。构建输入优先于已存列表：一个被指名了网络的构建只有那一条路可试（要改就改构建），而已存列表是「这设备被设置过」的答案。

## 待办（`[未完成]`）

1. **两颗芯片的驱动堆余量都只剩约三成**（C6 14,124 B / 49,152 B = 28.7%；S3 15,820 B / 49,152 B = 32.2%，两颗都是实测值）。谁调大 `DYNAMIC_RX_BUFFERS` 或 `STATIC_RX_BUFFERS`，两块板就一起付账 —— 要么在改那两个常量的同一个提交里把两颗都量一次，要么把该板的份额抬到 56 KiB（文档区间的高端：S3 的主栈有余量付这 8 KiB，C6 没有）。
2. **相机的 DVP DMA 为什么能吃掉整轮扫描**：12 轮里 1 轮，没有机制。产品页按「空结果就继续扫」处理，不阻塞交付。
3. **产品页要调 `note_address`**：那是「已连接」唯一的通路，漏了就会一直显示「连接中」。
4. **改缓存之后重测** `[INPUT]` / AEC 的既有数字。
5. **产品页第二片：面板键盘 + 凭据持久化 + 已存标记与忘记**。三片都已落地：键盘与交互见「产品页」，凭据见「凭据落在哪」，已存 `*` 与长按忘记见「离开后点回那一行、以及一次丢掉的列表修正」，点已存行用库里的口令见 ⑦。还剩下的：

    - **① 真机验证**：**整条「点一行 → 输口令 → OK → 关联 → 拿到地址 → 断开 → 忘记」已由 `IOT_PANEL_BENCH` 构建在板上走通**，也含「链路在飞时点那一行没有反应」「打字态不画工具条」「连接框在射频答复之前就画出来」（见「按面板路径打字」「工具条、确认框与忘记」「连上时点行不接、弹窗盖住列表」「打字态没有工具条、连接中有一个框」，全程不需要手指），以及第五轮那条「点回已存的那一行 → 库那份口令自己把链路接回来 → 页面什么都没问」（见「点已存的一行就用库里的口令」）。剩下非得人来的只有两件：4×4 键、列表行与工具条的**触摸手感**（含第四行键现在不再被工具条压着）、纯黑底色与背光 100%、以及连接框的观感与它出现/消失的时机 —— 这些是像素与触感。弹窗**盖没盖住列表**与**连接框挂没挂在那儿**都不是这两件之一 —— 它们是宿主机可测的事实（`Part::hidden`、`Part::showing`、`page::walk`、`Part::IN_ORDER`、`stamp::fill_surface`）。
    - **② 存**（**已落地**，格式、槽规则与写盘策略见「凭据落在哪」）：接线撞上的三件事都在不烧板的前提下定完 —— ① **MD5 条目 espflash 自己会写**：表里前三条 96 B 的 MD5 = `5b7fb69b103d49aee6dd86b70ccdddcf`，`esp-bootloader-esp-idf` 的 `validation` 找的正是它，**不用改**；② **双核写**：本应用从不启动第二核，默认策略今天也能写，仍选 `multicore_auto_park()`（每次写前停另一核、写完放开，几毫秒），换来以后真起第二核不会踩到；③ **多一条 `data` 分区**：子类型必须写**名字**（`creds,data,undefined,,0x2000,` 转出来是 `01 06`），写十六进制会让 `esp-idf-part` 在 `unwrap()` 上 panic。板上三次开机验过（空盘写下来、以及**产品镜像不带任何构建输入自己连回已存网络**，日志见「凭据落在哪」）。
    - **⑥ 已存网络的标识与忘记**（**已落地**）：列表里给已存网络画 `*`、长按那一行 = 忘记（`Touch::Hold` → `Intent::RowHold`，只对已存行弹 `FORGET` 确认框）。实现没有走「把名字集经一个平面送进 `DeviceState`」那条路：S3 静态内存墙下 `SavedNetworks`（约 980 B）在 app 里落成值或 `&EMPTY` 都会顶爆，于是**掩码跟着列表自己走**（`ApList.saved: u8`，一位一行），`SurveyHistory::{observe, marked}` 收 `Option<&SavedNetworks>` 以免在 app 里造值。代价是两份列表各自持一份，`SCAN` 平面因此必须投得进去也丢不掉（见第四轮）。擦除走 `Store::forget`，板上验过（见「凭据落在哪」第 4 次）。
    - **③ 细节**（已完成）：选中行整条反色、失败词画在打字态中槽、失败后自动重开键盘并保留草稿、`SHOW`/`HIDE` 回显、开放网络的 `OPEN` 标记与免键盘直连。
    - **④ 失败分档的真机映射**（已实测）：错口令 → 驱动 reason `FourWayHandshakeTimeout` → `NetworkError::Credentials` → 面板画 `AUTH`，从交出凭据到被拒 **37.6 s**（等的是驱动自己那次四步握手的超时）。映射表 `kind_of_disconnect` 只认能辩护的几种（`NoAccessPointFound|BeaconTimeout` → `NotFound`、`AuthenticationFailed|FourWayHandshakeTimeout|HandshakeTimeout` → `Credentials`），其余 `Driver`；`NotFound` 那半仍**未实测**（要一次把 AP 关掉的台架）。
    - **⑤ 开放网络的真机读数**：bsp 的 `Open` 修好之后这一支仍**没有台架证据**（台架上没有开放网络），目前只有状态机测试与代码审读。WPA3-only 的网由驱动版本决定连不上（见「连接策略」），不是待办而是范围。
    - **⑦ 点已存的一行就用库里的口令**（**已落地**，细节与读数见「点已存的一行就用库里的口令」）：页面交**名字**、网络任务查 `SavedNetworks::copy_of` 取口令、查不到回报 `NoCopy` 让页面升键盘；库交出来的那份**不写盘**（`Provenance`）。**只有代码审读**的三处：`NoCopy` 兜底那一格（掩码与库同源，台架上造不出「有掩码但库里没有」）、「库那份写失败」与「开机自读那条不写盘」（这次开机是 `creds` 擦空之后的面板支，走不到它）。

    列表那一条仍然成立：**不能拿一轮扫描的结果当答案** —— 实测单轮在 0～6 个之间跳，页面因此按「空轮 1 s 后重试」处理（见「一轮扫描的节奏」），并把「没扫过」与「扫了没扫到」分开画。
6. `esp-radio` 是 beta：升级版本时重跑本页全部，**两颗芯片都要跑** —— 分芯片的预算差异正是从驱动的实际用量里来的。
7. **把 Wi-Fi blob 的 IRAM 代码送去 flash**：这是 S3 拿回那 32 KiB `dram_seg` 的唯一一条路，而本仓的脚本插不进 esp-hal 生成的那两条规则之间 —— 要先给 esp-hal 开一个能容纳这件事的钩子。见「同一套代码，两颗芯片」里的驱动堆。
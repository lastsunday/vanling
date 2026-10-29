+++
title = "遥测上行（对外发布）"
weight = 40
+++

# 遥测上行（对外发布）

板子把自己的状态发布出去。**传输可以换，数据模型只有一份。** 这份文档是讨论稿：**没有实现，也没有一条 BLE 实测**，唯一引用的硬件数字来自 [Wi-Fi 实现记录](@/records/iot/wifi.md)。

## 边界

三件事必须分开，本文只谈第三件：

| 层 | 谁决定 | 关掉它意味着 |
| --- | --- | --- |
| 采样 | 驱动 / 需求模型 | 本地没有数据 |
| 本地使用 | 产品逻辑 | 页面不显示、不进音频链路 |
| **对外发布** | 本文 | 本地照旧，只是不往外发 |

「采多勤」属于[架构文档的需求模型](@/development/iot/architecture.md)，不在这里。本文只回答「已经采到的状态怎么出去」。

另一条边界：发布的是**状态镜像**，不是事件流。没有顺序、没有事务、没有投递确认，最后一份覆盖前一份。

## 一、数据模型（传输无关）

一份快照，每个槽自带身份和标度：

| 字段 | 宽度 | 说明 |
| --- | --- | --- |
| `sensor_id` | 1 | 板内槽号，接收端用它做缓存键 |
| `type` | 2 | GATT 16-bit UUID，同时定住量纲与 scale |
| `value` | 4 | 定点。**不用 f32** |
| `boot_id` | 2 | 每次上电换一个 |
| `uptime` | 3 | 单位 100 ms，回绕 19.4 天 |

外加一个**披露掩码** `mask`：第 i 位为 1 表示第 i 个槽这一轮在包里。

### 为什么是镜像不是消息

镜像幂等，通道装不住它——用通道装就得补深队列、写方变化检测和失败重试，三套机制都只为「通道存不住一个值」而存在。这是架构文档里已经写过的同一条理由。

### 为什么定点

f32 在两个实现里可能解出不同的数，而这个包没有版本协商的余地。本仓库还多一条：这个 target 的 FPU 是关的，浮点走软件模拟（见[回声消除记录](@/records/iot/aec.md)）。

### 为什么不用绝对时间

板上没有 RTC。`boot_id + uptime` 只够做排序和判重；**绝对时间由接收端在收到时打**。接收端跨 `boot_id` 变化必须清空缓存，因为两个 `boot_id` 的 `uptime` 不可比。

### 窄值怎么扩展

UUID 定住类型和 scale，但没定宽度。规则写死：**无符号零扩展、有符号符号扩展**，接收端照此解。否则同一个 `0x2A6E` 在两个实现里会解出不同的数。

## 二、传输的语义差异

| | BLE 广播 | Zigbee | Thread | Wi-Fi |
| --- | --- | --- | --- | --- |
| 数据模型 | 镜像 | 属性最后值 | 镜像 | 镜像 |
| 寻址 | 无（谁收到算谁的） | device + endpoint + cluster | IPv6 | IP |
| 投递保证 | 无 ACK，靠重复 | MAC ACK + 路由重传 | IP 重试 | TCP |
| 频率控制 | 板子 | 设备侧 reporting interval | 板子 | 板子 |
| 入网 | 无 | permit-join + 网络密钥 | dataset + commissioner | 凭据 |

四条都落在「镜像」上，所以**差别只在投递保证**，只需要两个 sink 形状：

| 形状 | 语义 | 谁用 |
| --- | --- | --- |
| `Mirror` | 覆盖式，失败就丢，下一轮自然补 | BLE 广播、Zigbee 属性上报 |
| `Deliver` | 寻址 + 确认，失败要退避重试 | Thread / Wi-Fi（IP 之上） |

轮播**不属于抽象层**：它是 BLE 的编码约束（31 B 装不下全部），藏在 BLE sink 内部，不往上漏。

## 三、各传输的落点

### BLE 广播

**PDU**：`ADV_NONCONN_IND`（不可连接、不可扫描）。不需要连接、不需要 scan response，31 字节全给载荷；接收端被动扫描即可。

**载荷预算**：flags AD 占 3 B，manufacturer specific data 的头占 4 B（1 长度 + 1 类型 + 2 company id），所以用户数据上限 **24 B**；不带 flags 可以到 27 B。

**一包一个传感器**：

| 字段 | 字节 |
| --- | --- |
| company_id | 2 |
| version | 1 |
| sensor_id | 1 |
| type（GATT UUID） | 2 |
| flags | 1 |
| boot_id | 2 |
| uptime（100 ms） | 3 |
| value | 4 |
| 合计 | **16** |

24 B 里有 8 B 余量，暂不动。

**为什么必须有 timer**：controller 会一直重复最后一份 adv data。所以「换下一个传感器」只能靠**逐时隙重写 adv data**，没有别的办法让空口上的内容变。这不是优化，是模型的前提。

刷新一遍的周期：

```
N_on × k × interval
```

`k` 是同一个传感器连发几个 advertising event（重发次数）。设备插电时 `interval` 取 30–50 ms、`k = 3`，八个槽一轮 0.7–1.2 s。

**顺序**按轮次改变起点并加抖动，否则会和固定扫描间隔的接收端锁相。

**GATT UUID 只允许单值定点特征**：

| UUID | 量 | 编码 |
| --- | --- | --- |
| `0x2A6E` | 温度 | sint16，0.01 °C |
| `0x2A6F` | 湿度 | uint16，0.01 % |
| `0x2A6D` | 气压 | uint32，0.1 Pa |
| `0x2A19` | 电量 | uint8，1 % |

带 flags 位域的复合特征（`0x2A37` 心率、`0x2A5B` CSC 测量）**不进这个格式**——它们不是一个 `value`，塞不进固定 4 字节。

**HCI 三条命令**：`LE Set Advertising Parameters` / `LE Set Advertising Data` / `LE Set Advertising Enable`。`esp-radio` 的 `BleConnector::write` / `read_async` 就是裸字节，手写编码约 50 行，**不需要 `trouble-host`**。

**顺序纪律**：

- 一个 boot 内**不混用** legacy 与 extended 广告命令。混用后 controller 返回 `Command Disallowed`，只有 `HCI_Reset` 能恢复。
- enable 要**幂等**：重复 enable 已开启的 set 同样返回 `Command Disallowed`，自己记状态。
- enabled 时能否直接 `Set Adv Data`——扩展广播规范明确允许，传统广播各家不一。**待测**；不接受就退回 disable → set → enable，代价是一个间隔的空窗。
- **必须读 HCI 事件**。只写不读的实现能跑起来，然后某个时刻静默停摆：Command Complete 和缓冲区回收事件堆满，controller 不再接受新命令。

### Zigbee

属性上报天然就是镜像：cluster + attribute + 最后值，由设备侧的 reporting interval 驱动。所以它和 BLE 共用同一份 `Snapshot` 和同一个 `mask`。

- **有 MAC ACK**，所以是 `Deliver` 形状里最省事的一种：不需要自己写重试。
- **不需要轮播**：没有 31 字节预算，一轮把所有属性报完。
- 复合特征的限制在 ZCL 里同样存在（它有更丰富的数据类型，但我们的 `value` 仍是单值）。

### Thread / Wi-Fi

Thread 只提供网络层（IPv6），**不提供应用数据模型**。所以「支持 Thread」通常不等于多一个 sink，而是**多一条网络接口**——上层复用 Wi-Fi 那条 `Deliver` 路径（MQTT / HTTP）。

这与 BLE 和 Zigbee 的性质不同：那两个各自定义「一条数据长什么样」，IP 这两个不定。

**Zigbee 与 Thread 二选一**：两者都跑在 IEEE 802.15.4 上，射频只有一颗，MAC 是硬实时且信道完全重叠，没有可用的仲裁器。切换只能做成启动时二选一（退出网络 + 重新入网），不是热切换；对于 router 角色，切换等于把网络拓扑拆一次。

## 四、框架落点

现在只该有三样东西：

```rust
// iot-core：传输无关，宿主可测
struct Snapshot { slots: [Slot; N] }
struct Mask(u8);

// 一个方法，关键是这条缝
trait Sink { fn publish(&mut self, snap: &Snapshot); }
```

再加一个 `Signal<Snapshot>` 当跨界点，照 `render.rs` 的契约形状。

**不建** `Plan` / `resolve` / 注册表 / `Route`。今天只有一个 sink，那些层只会在测试里出现。

判据写死，用来决定什么时候才该长出那些层：

> **加第二个 sink 时，改动范围应该只有 `bsp` 加一处 `match`。**

超出去说明缝开错了位置；没超出去说明今天建是白建。这条判据比任何预演可靠，因为它把「何时抽象」交给事实。

**将来的 intent 长什么样**：目标是「一次 intent 指定走哪条路」，但顺序不能反——intent 的取值域由可用的 sink 决定，只有一个 sink 时它只有一个值，那样的枚举不是抽象是占位。所以今天的 intent 只有一句「哪些槽在披露」；`Route` 等第二个 sink 出现时再加，那时它才配拥有第二个变体。

**披露掩码按 sink 分开**，不全局：BLE 是明文，谁都能收；Wi-Fi 到自己的后端。同一次发布，两条路的 `mask` 可以不同。

**失败隔离**：每个 sink 各占一条 `join` 分支，发布方只往各自的 `Signal` 里写。不要写成循环里逐个 `await`——一个 sink 卡住会堵死全部。

## 五、两颗芯片

### S3 的 BT 上电：一条被推翻的推断

曾据 `esp-radio` 的 `init_clocks()` 推断「BT 时钟被清掉了」，进而写「S3 的 BT 电源域没人上电」。**这条是错的**：

- `init_clocks()` 里那三位是 `I2C_CLK` / `UNUSED_BIT12` / `SDIO_HOST`，不是 BT 时钟；S3 的 `enable_bt()` 本身就是空实现。
- `enable_wifi_power_domain()` 在 S3 上（`soc_has_bt`）会脉冲 `WIFIBB/FE/WIFIMAC + BTBB_RST/BTMAC_RST/RW_BTMAC_RST/RW_BTMAC_REG_RST/BTBB_REG_RST`。
- `init()` 由 `RadioRefGuard::new()` 在 refcount 0→1 时触发，而 Wi-Fi 驱动的 `new()` 正是那个 0→1 的调用者。

所以 Wi-Fi 先起时 BT 复位实际已经跑过。本仓库手写的 `ungate_radio()` 只覆盖 Wi-Fi 的三位，但它跑在 `init()` 之前，大概率不缺——**待上板确认**。

### 内存：只有一笔账，不是两笔

| 项 | 数字 | 依据 |
| --- | --- | --- |
| S3 radio 堆总量 | 48 KiB（16 KiB `dram_seg` + 32 KiB dcache reclaimed） | `chip-esp/src/radio/esp32s3.rs` |
| Wi-Fi 跑完剩余 | **15,820 B** | `records/iot/wifi.md`（实测） |
| BLE controller 需要 | 20–30 KiB（典型值） | **待测** |
| controller 任务栈 | 默认 8 KiB | `Config::default()` |

容易搞错的一点：**这 8 KiB 和 controller 自己的堆不是两笔账**。`esp-radio` 的芯片 feature 同时拉进 `esp-rtos` 与 `esp-radio-rtos-driver`，controller 任务经 `esp-rtos` 的 `spawn` 从 `InternalMemory` 分配栈；而 `InternalMemory` 只命中 `esp_alloc::HEAP` 里声明为 `Internal` 的 region——本仓库在这颗芯片上注册的 `Internal` region **只有 radio 那 48 KiB**（app 堆是独立的 `LlffHeap`，在 PSRAM）。所以 15,820 B 要同时装下 controller 的内存和它的任务栈。

### C6

| | C6 |
| --- | --- |
| controller | `npl`（S3 是 `btdm`），两者的 `create_ble_config` 与内存需求不可互相代入 |
| BT 时钟 | `bt_periph_module_enable()` 真的调 `enable_bt(true)`；`ungate_radio()` 是空实现，而且是对的 |
| radio 堆 | 同样 48 KiB，但是**一整块**，不跟主栈抢 |
| 余量 | **待测**，不能拿 S3 的 15,820 B 代入 |
| 产品路径 | 不存在（无 PSRAM、无 LCD_CAM、GPIO 不够） |

### `coex` 不是软 feature

`esp-radio` 的 `coex` 在 `ble_init` 里直接调 `crate::wifi::coex_init`，所以它在代码上依赖 `wifi`。这决定了它的落点：

```toml
bt = ["dep:esp-radio", "esp-radio/ble", ...]                  # 可单独编译
lckfb-szpi-esp32s3 = [..., "bt", "wifi", "esp-radio/coex"]    # 组合
```

代价是 `check-features` 要加一条**组合条目**——这是整个脚本里唯一不能「单独跑」的项。它不是两个软 feature，是「同一根天线上的两个能力同时在场」这个硬件事实。

### 802.15.4 的底层已经有了，协议栈没有

`esp-radio` 有 `ieee802154` feature，即射频驱动已经存在；但 Zigbee 与 Thread 的协议栈不在这个 crate 里，需要额外的 C 库。所以「加 Zigbee」的代价不在射频，在协议栈、设备兼容性和认证。

## 六、被否的方案

| 方案 | 为什么否 |
| --- | --- |
| 多传感器打包进一包 | 装不下全部；且开关只能表达成「位为 0」，与「没发」难以区分 |
| 多个广播集（BLE 5.0） | 不需要 timer，但 controller 与接收端支持不一，空口占用叠加 |
| `trouble-host` / 直接依赖 `bt-hci` | 只要三条 HCI 命令，`BleConnector` 的接口就是 `&[u8]` |
| 队列 + 定频发送 | 广播无 ACK，队列给不了投递保证；排队发出去的前几条都是过期状态 |
| `Plan` / `resolve` / 注册表 | 只有一个 sink 时，这些层只会在测试里出现 |
| 运行时切 Zigbee / Thread | 退网 + 重入 + 邻居重建，不是热插拔；router 角色等于拆一次网络 |
| 用 MAC 当设备 ID | 手机侧地址会轮换，设备 ID 应放在载荷里 |

## 七、未决（全部要上板）

1. **S3 coex 下够不够**：15,820 B 要同时装 controller 内存与 8 KiB 任务栈，典型需求 20–30 KiB。
2. **C6 的等价余量**是多少。
3. **enabled 状态下能否直接 `LE Set Adv Data`**：决定有没有空窗。
4. **Wi-Fi 扫描时广播间隔的实际抖动**：决定 coex 下这条广播能不能用。

## 八、后续入口

不一次上 coex，分两次跑，才能把两个变量分开：

| 探针 | 相位 | 量什么 |
| --- | --- | --- |
| BLE-only | refcount 0→1，`init()` 会跑 | radio 堆与任务栈落在哪、广播能否被手机看到 |
| Wi-Fi + coex | refcount 2→3，`init()` 不跑 | 同一条广播要多少堆、扫描时的间隔抖动 |

两次读数一比，「电源域」和「内存」就分开了——否则 coex 一崩，分不清是哪一个。

C6 上做第一次（干净基线），S3 上两次都做（对照）。

读数出来之后，实测部分进 `records/iot/`，本文件保留推理与取舍。

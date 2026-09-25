+++
title = "Cargo Feature 判据与内聚"
weight = 10
+++

# Cargo Feature 判据与内聚

## 背景

`apps/iot` 曾定义过一组"能力 feature"：`iot-core` 的 `led`/`button`、`iot-app` 的 `breath`/`button`。它们在 `drivers/mod.rs`、`state.rs`、`render.rs`（双 render 循环）、`main.rs`、`bsp` 里以 `#[cfg(feature=...)]` 散射，最终被整体删除。

删除不是权宜之计，而是发现它们不满足任何引入判据：vanling 单产品、单板，每种 build 需要的能力集由**板**（硬件轴）完全决定，软件轴从未存在过"两种配置"。

## 硬 feature vs 软 feature

Cargo 没有官方的硬/软分类，但生态实践（esp-hal、embassy、bevy、smithy-rs RFC-0015）划出的两类 feature 用例正好对应我们说的两个轴：

| | 硬 feature（硬件轴） | 软 feature（能力/行为轴） |
|---|---|---|
| 含义 | 选中目标：chip / board / ABI / toolchain | 同一块固定硬件上，编译进哪些可选能力/行为/依赖 |
| 语义 | 互斥，一次只开一个（由物理现实决定） | 加法式（union），任意组合应合法 |
| 例 | `esp32c6`、`esp32c6-devkitc-1` | `wifi`、codec、`bt-stack`；esp-radio 的 `wifi`/`ble`/`csi` |
| 生态参照 | esp-hal chip feature、embassy 按芯片拆分、cargo #2980 | Cargo Book "Features examples"、bevy profiles、pyo3 `std`/`abi3` |

判定：**换芯片/换板/换 ABI 才变 = 硬 feature；同一硬件上换产品档/换体积目标才变 = 软 feature 候选**。

## 软 feature 引入四判据（全中才引入）

1. **可选依赖门控** —— 省下一个完整后端/驱动栈的编译、flash、RAM（如 tls、codec、radio 栈）。
2. **官方 SKU 档位** —— 同一硬件出真实变体（lite/free），下游连 API 都不该见。
3. **大幅二进制瘦身** —— 可选大表/算法（如 unicode 表、编解码格式）真占 flash。
4. **运行支持开关** —— std/no_std、`rt`、日志通道、`unstable`/nightly 门控。

尤其注意：行为开关若在 opt-level≥2 下与常量折叠等价、且只有一个配置在售，就不该用 feature（编译器验证参考 nullderef *Why you shouldn't obsess about Rust features*）。反模式清单见文末。

## 内聚六规则

Cargo feature 是**加法语义**（union，开启一个不应关闭另一个），且是**库级全局**的（同一依赖图内无法"此实例开、彼实例关"）。因此内聚即把每个 feature 当作**可裁剪的模块轴**，而不是散落的 ifdef：

1. **一个 feature = 一个模块**：cfg 只在 `mod` 声明处门控一次，能力代码全部住在 owning 文件；off 即文件不存在。触碰多个文件的 feature 不内聚，把该层抽成一个模块。
2. **消费者零 cfg**：关闭时以 stub/no-op 提供同一 API，或把能力经 trait 在组合点注入；任何调用点出现 `#[cfg(feature=...)]` 即内聚泄漏。
3. **单一事实源在 Cargo.toml**：声明 + `dep:` 命名 + 组合关系都写在 `[features]`；bin/test 用 `required-features` 整体跳过。
4. **分层金字塔不平铺**：叶子 = 能力或 `dep:`；集合 feature 只组合叶子；元 feature（`full`/default）只写 `["..."]`，绝不直接门控代码。
5. **矩阵强制合法组合**：新增/改动软 feature，必须保证每个子集独立编译且测试通过（`cargo hack --feature-powerset` / `cargo fc`）；子集单独无意义 = 该 feature 不是独立单元。
6. **命名与文档承载角色**：能力/集合/硬件用不同词根；feature 清单与用途集中到文档页；禁用空的"假 feature"（优先 `dep:` 或模块背书）。

## 反面清单（本次删除项即反例）

- `led = []`、`button = ["led"]`、`breath = []`：空 feature，纯 cfg 开关，无依赖、无模块背书（违反 3/6）。
- cfg 同时出现在 `drivers/mod.rs`、`state.rs`、`render.rs`（双 render 循环）、`main.rs`、`bsp`（违反 1/2）。
- 唯一消费者是单产品单板，软件状态不存在（不满足四判据；参考常量折叠等价论证）。
- 无任何 feature 组合矩阵验证（违反 5）。

## 现状

`apps/iot` 的 feature 分两个轴：

```
iot-core     — 无 [features]
iot-bsp-esp  — 元件 feature（button / ft6336 / pca9557 / ws2812 / st7789 / display-light）
               板 feature（esp32c6-devkitc-1 / lckfb-szpi-esp32s3）＝ 元件聚合
iot-app      — esp32c6（chip 别名）/ esp32c6-devkitc-1（default）/ lckfb-szpi-esp32s3
```

- **元件 feature 是模块轴**：一个 feature = 一个元件模块（`components/` 里 `mod` 处门控一次、off 即文件不存在），板 feature 只聚合接线所需的元件，不散装依赖。空 feature（如 `button = []`）靠模块背书合法化（判据 rule 1/6 的「模块背书」），不属于当初删除的纯 cfg 开关。
- **两轴命名判别**（rule 6「不同词根」的落地）：硬件轴用芯片/板产品名作词根（`esp32c6`、`esp32s3`、`esp32c6-devkitc-1`、`lckfb-szpi-esp32s3`），模块轴用元件裸名（`button`、`ft6336`、`pca9557`、`st7789`、`ws2812`、`display-light`）。二者从名称即可分明互斥/加法语义，不加 `cmp-`/`board-`/`chip-` 前缀；真撞名后（如「板名恰等于元件名」）才考虑前缀。
- **板 feature 是硬件轴**：互斥，一次只开一个，由物理板型决定；`iot-core` 仍零 cfg，能力经组合点（板接线）经 trait 注入。
- CI 固定 `--no-default-features --features <board>`，构建的即该板全功能固件。将来引入任何能力/行为轴软 feature，都必须重新满足四判据 + 六规则，并补齐矩阵验证。
+++
title = "面板双缓冲：测过，否决"
description = "ST7789 面板加一块 PSRAM 暂存区后帧率 24→28 fps，但核心占用 40%→84%、Wi-Fi 关联窗口出现 674 ms 的一帧；本板能放双缓冲的只有内部 DRAM，只剩 16.7 KB。真因是灯呼吸驱动整屏重绘，已解耦为运行时 `GlassBreath`。"
weight = 25
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 面板双缓冲：测过，否决

`apps/iot` 的面板路径能不能靠双缓冲把绘制和 SPI 传输重叠起来。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

依据标注沿用本域约定：**`[测试]`** 有单元测试，**`[实测]`** 有硬件测量并附复现，**`[据称]`** 未复现只留痕。

## 结论

`[实测]` **不做。** 面板保持单缓冲：模型是板上那块 153,600 B 的 DRAM arena（与相机环共用），画完直接投递并等 SPI 传完。把同一帧另拷一块 PSRAM 暂存区能换来 +16% 帧率，代价是协调器核心占用翻倍，并在 Wi-Fi 关联窗口出现一帧 **674 ms**——可见冻结，且每次重连都会复现。

`[实测]` 这次试验还找到了真正的浪费：整屏重绘的触发者不是页面，而是**灯的呼吸**。把呼吸从灯里解耦后，普通页稳态核心占用从 40% 降到 0–1%（见下文「呼吸重绘」），不需要双缓冲。

## 两条路径

- **单缓冲（现状）**：paint 写 DRAM 模型（Menu 页 19–20 ms），投递后协调器空等 SPI（15–16 ms）。等待是空闲，其他任务照跑。
- **暂存双缓冲（本次试验）**：模型仍留 DRAM，另在 PSRAM 堆上分配 153,600 B 暂存区；每帧按 damage 把改动像素拷进暂存区后投递，下一帧 paint 与上一帧 wire 重叠。相机页例外——环就是面板帧，仍直接投递并等。

## 实测（同一棵树、同一页、每 2 s 一窗）

Menu demo 页（构建期未设 `IOT_BOOT_PAGE`），`[DISPLAY]` 稳态三窗：

| 指标 | 单缓冲 | 暂存双缓冲 |
| --- | --- | --- |
| repaints / 2 s | 47–49 | 56–58 |
| 帧率 | ≈24 fps | ≈28 fps |
| `held the executor` | 40%（16.8 ms/帧） | 82–84%（29.6 ms/帧） |
| `worst render` | 19–20 ms | 31–33 ms |
| `worst wait` | 19–22 ms | 0 ms |
| `worst write` | 15–16 ms | 16–17 ms |
| `longest chunk` | 1 ms | 2 ms |
| 关联窗口最大单帧 | 25 ms | **674 ms** |
| `_stack_end` / 主栈 | `0x3fcd5598` / 82,296 B | 相同 |

> 25 ms 取自同一构建 `IOT_LOG_LEVEL=debug` 的首窗（`info` 构建的首窗在达标时不打印），其余数字都是 `info` 构建。

- 拷贝本身 `[实测]`：`worst render` 差值 12–13 ms，即 153,600 B 写进 PSRAM 的代价（写未命中要先把行读进来，约 12 MB/s 量级）。
- 674 ms 那一帧 `[实测]`：出现在 Wi-Fi 关联的同一窗口；同窗口单缓冲最大 25 ms。停顿成因没有逐层归因，最可能是 PSRAM 与 flash 共用 MSPI——`camera.md` 已记过同类 600 ms 级停顿。
- 净效果：帧间隔 41 ms → 35 ms（−6 ms）。省下的 20 ms 等待是**空闲**，换来的 13 ms 拷贝是**占用**，所以帧率涨 16% 而核心多花 40 个百分点，留给定时器与 Wi-Fi worker 的只剩 18%。
- `worst write` 与 chunk 略涨：DMA 从 PSRAM 取数比 DRAM 慢。

## 为什么放不进快内存

`[实测]` 两块整屏需要 307,200 B。本板内部 DRAM：`dram_seg` 余量为 0，主栈从 82,296 B 到这个任务的下限 65,536 B 只剩 **16,760 B**。PSRAM 放得下（512 KiB 堆，两块占 59%），但按上面的数字，PSRAM 的写带宽和 MSPI 争用正是问题本身。

条带双缓冲（LVGL 的部分缓冲语义）是理论上唯一的替代：16 KB 只够约 34 行，且 painter 要改成按条带绘制并重写裁剪/布局，收益上限是「paint 19 ms 与 wire 16 ms 重叠」，与整屏双缓冲相同。没有实现。

## 要提升面板帧率，先压 paint

`[实测]` 单缓冲每帧 = paint 19–20 ms + wire 15–16 ms。双缓冲能动的只有后半段，而不走 PSRAM 就动不了。前半段没有这个限制：Menu demo 的 paint 是 19 ms（`fill` 只占 1–2 ms），把它压下来是唯一不需要额外内存的路。

## 局部重绘：框架自己算 damage

`[实测]` 页面不再指定重绘矩形。布局后框架给每个节点算两份指纹：`structure`（节点数 + 每个节点的 kind、矩形、裁剪）和 `content`（`Style` 逐字段哈希、文本字节、custom 槽的 `state`）。上一帧只留 `FramePrint { structure, content: [u32; 64], len }`（约 262 B，放栈上）；结构变了就整帧，否则只重绘 content 变了的节点矩形（与裁剪求交）。`Ui::custom_state(style, slot, state)` 让页面用一个 `u32` 说明「这个槽这次画的是不是同一张图」；`Ui::custom` 表示页面不描述像素，快照一动就重画该槽。

`[实测]` partial 帧先把 damage 铺回底色（页面文字靠反色地面绘制，同一块画两遍会翻转），再按树序重画与 damage 相交的节点，所以后画的卡片和 scrim 会自动盖回去。弹窗的每列声明 `Style::cover()`（整块矩形都由自己上色），框架据此跳过被它完全盖住的节点；不声明时卡片后面的页面文字每帧都要重新排版，弹窗滚轮只有 ≈35 fps，声明后回到 50 fps。`Picker` 的 scrim 仍是卡片四周条带，卡片 body 重绘不碰混合。

`[实测]` 同一块 S3、`320×240`、debug 构建、每 2 s 一窗（临时 bench 驱动滚轮，收工前已删）：

| 指标 | 整帧基线 | 弹窗滚轮（一列 + 页脚） | 内联小时轮（滚轮 + 读数行） |
| --- | --- | --- | --- |
| repaints / 2 s | 38 | 91–99 | 100–101 |
| 帧率 | ≈19 fps | ≈45–50 fps | ≈50 fps |
| `held the executor` | 66% | 46% | 26–29% |
| `worst render` | 36 ms | 10–15 ms | 5–9 ms |
| `worst write` | 10 ms | 4–5 ms | 3–4 ms |

50 fps 是 `STEP_MS = 20` 的上限，不是渲染能力；整帧那列是同一 bench 的基线。帧率之外没有别的收益：像素与整帧重绘逐点相同（测试对比同一帧的 partial 与整帧结果）。

## 呼吸重绘：真因不在缓冲

> 以下三节记录 2026-11 中旬的实现；入口现在是 `DeviceState.glow` + `LightingView.breath`（`GlowScope` 三档 + `GlowPattern` 三档），背光独立、0 即全黑。见 `development/iot/architecture.md`。

`[实测]` 屏幕每步重绘的触发者不是页面，而是灯：`ColorOutput::step` → `Screen::set_rgb` → `fill` + `page_dirty`，于是灯呼吸一次，整屏就重画一帧（19 ms 绘制 + 15 ms 上线）。Camera、Network 这类自带 `ground()` 的页面反而静止，正好证明问题出在底色跟着灯走。

`[实测]` 把「灯」和「屏」解耦后，普通页稳态（Menu，`IOT_LOG_LEVEL=debug`）每窗 2–4 次重绘、`held the executor` 0–1%、`worst render` 17–21 ms、`worst write` 15–16 ms；改动前同一页 47–49 次/2 s、40%。

屏幕呼吸现在是显式能力 `GlassBreath { period_ms, hue, saturation, min_value, max_value }`（`shared/model/light.rs`）。`DeviceState.glass_breath` 是全局（报警）入口，`PageSnapshot.glow` 是页面私有入口，投影在 `FrameSnapshot.glass_breath` 合成（全局优先）。默认 `None`：进页或换灯色只铺一次静止底色，`step()` 返回 `Idle`。自带 `ground()` 的页面（Camera、Network）仍由页面自己铺满，报警呼吸不覆盖它们。

示例页 `lighting`（`PageId::Lighting`，原名 `breath`）用 `CALM`（3 s 暖白）/`ALERT`（800 ms 红）两个预设、`SLOWER`/`FASTER`（±500 ms，500–5000 ms）、`DARKER`/`BRIGHTER`（包络两端同步 ±32，下限 `min ≥ 0`、峰值 `max ≥ 32`，上下滑同义）和 `EVERY PAGE`（把当前参数写进全局，再点清除）演示与编辑参数。烧 `IOT_BOOT_PAGE=lighting` 实测：每 2 s 18–26 次重绘（≈12 fps，受 `REPAINT_STEP=12` 的颜色阈值限制，不是节流上限）、`held` 9–14%、`worst render` 11–13 ms、`worst write` 15–16 ms。

`[实测]` 当时的背光路径：`Off` 0、静止 100、玻璃呼吸随相位在 `BACKLIGHT_FLOOR_PCT..=100` 之间、`OnChange` 钳到 12%。现在背光只取 `Backlight`，不再随呼吸走，0 即全黑。灯（C6 的 WS2812）仍按原样呼吸，屏幕不再跟随。让背光跟随灯的亮度（而非玻璃呼吸）没有做：默认 boot 呼吸 `max_brightness=80` 会把整屏默认压到约 72%，风险大于收益。

## 复现

两版都用 `--no-default-features --features lckfb-szpi-esp32s3,console-auto` 构建，烧同一块 S3（`lckfb-szpi-esp32s3`）：

```sh
IOT_PORT=/dev/cu.usbmodem1442301 moon run iot:flash-s3
espflash monitor --port /dev/cu.usbmodem1442301 --non-interactive
```

读每 2 s 一行的 `[DISPLAY] … repaints in … held the executor …%, worst render … ms (fill …), worst wait … ms, worst write … ms (longest chunk … ms)`；开机第一行 `[DISPLAY] backlight dark until the first frame` 证明在跑面板路径。判定门槛沿用试验前的约定：`worst write` ≤17 ms、无花屏、Wi-Fi 关联窗口无 600 ms 级卡顿——第三条未过，故否决。

暂存版的改动点（供重试时对照）：`virtual_components/screen/{mod,paint,panel}.rs` 加 PSRAM 暂存区与 `copy_from`，`core/drivers/surface.rs` 加 `FrameDamage::copy_from`。

呼吸的两组数字用同一块 S3 量，`IOT_LOG_LEVEL=debug` 让稳态窗口也打印。构建期取值不进 moon 的缓存键，直接跑脚本：

```sh
cd apps/iot
IOT_LOG_LEVEL=debug bash ../../scripts/iot-xtensa.sh build --release \
  -p iot-app --bin vanling --target xtensa-esp32s3-none-elf \
  -Z build-std=core,alloc \
  --no-default-features --features lckfb-szpi-esp32s3,console-auto
# 示例页：同一个构建加 IOT_BOOT_PAGE=lighting（原 breath）
```

`IOT_LOG_LEVEL` 缺省为 `info` 时，未超预算的窗口不打印（`over_budget` 只看 25% 与 20 ms），所以普通页「没有行」本身就是达标信号。

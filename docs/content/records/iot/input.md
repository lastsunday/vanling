+++
title = "输入与触摸实现记录"
weight = 60
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 输入与触摸实现记录

`apps/iot` 的 button / touch / motion / capture 四路轮询、手势识别与页面循环。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

复现方式：从 `apps/iot/` 烧默认固件，串口读 `[INPUT] page is now`（每次页切换一行）与 `[INPUT] held the executor …`。触摸路径**必须由人手按屏幕**——FT6336 的触控寄存器无法从主机伪造注入，所以这一段的证据全部来自人与硬件的组合。

---

## 页面循环在硬件上是对的，而 2 秒窗口看不出页序

`[实测]` 复现：烧默认固件，请人按屏幕依次点完 `Ambient → Attitude → Audio → Speaker → Camera` 再绕回，重复若干分钟；串口读 `[INPUT] page is now`。

一轮 46 次跃迁的判据：

- 连续重复页 **0** 次
- 偏离 `DisplayPage::CYCLE` 的跃迁 **0** 次
- 五个页全部出现
- 一次物理点击 = 一次跃迁（重复页为 0 即证明）

`[测试]` `every_page_the_board_can_back_is_shown_on_the_way_round`（`core/src/state/mod.rs`）早就在 host 上钉住了次序表，所以这条实测的价值不在于「次序对不对」，而在于**驱动、识别、意图、状态、渲染整条链在硬件上确实只走一格**。

`[实测]` 一次**错误判断**值得记下来，因为它改变了我读日志的方法：`[DISPLAY]` 的成本报告是 **2 秒窗口**，只反映窗口结束时的页。同一个窗口里 `Ambient → Attitude → Audio` 完全可能发生，而我从「窗口末是 Audio」读成了「跳过了 Attitude」，据此说过页面在乱跳。**人是准的，2 秒窗口不是**——同一个可疑现象，人眼观察直接推翻了我的日志结论。

因此加了一行 `[INPUT] page is now <page>`，每次页切换一行。两个细节是踩出来的：

- **按变化打，不按操作打。** 采集源每个 cadence tick 都发一次 `Intent::Operation`（每 20 ms 一次），所以「每操作一行」等于每 20 ms 一行。
- **打页名，不打整个事件。** `AudioSample` 内含 200 列 envelope，展开后一行 1.6 KB，一轮抓取就是几十 MB 噪声。

这行日志同时让「一次手势是否被判成多个操作」变成可证伪的——重复页计数就是判据。

---

## 四路轮询里只有一路可以被关

`[实测]` `[INPUT] held the executor … worst single hold … worst source: …` 一行给出整次 tick 的耗时、单次最长持有与四路各自的最差耗时。实测稳定形态：`button 0 ms, touch 1 ms, motion 3 ms, capture 16–20 ms`。capture 是最大的一项，因为折叠一个样本要跑六阶 IIR。

`[测试]` 交互路径按设计不可关（`poll_plan_of` 对 `DemandField::Interaction` 直接返回 `active: true`）。实测地板 **10.87%**（全部平面 `Off` 的构建，8 个 5 s 窗口，离散 0.1 点）——剩下的全是 button 与 touch 的轮询。

`[实测]` 节流把这一侧让出来的执行器，动画时钟会拿走：只把 motion 批到 30 s 时 input 侧从 49.5% 降到 35.5%（等价于旧的 `IOT_BUDGET=thrift`，见[音频实现记录](@/records/iot/audio.md)），而 `[DISPLAY]` 那一侧从 45.3% 升到 58.5%，设备总量 94.8% → 94.0%。所以「input 侧便宜了」不等于「设备便宜了」，两者之和才是设备的占用。

`[实测]` 而且 `[DEMAND]` 的 poll 侧现在就跟得上：按实际节奏缩放后默认构建 497_300 对实测 494_600（+0.6%），thrift 340_104 对 354_800（−4.1%）。缩放之前 thrift 报的是 497_300，高了 40%。

`[实测]` 节流对最坏单次持有的作用：默认构建 22–23 ms（capture 每次都 due），thrift 构建 17–19 ms，个别 motion/capture 都没到期的窗口只有 1 ms。`MAX_SINGLE_HOLD_MS = 30` 在两种构建下都没有越界。

`[测试]` 采集那一路不许被节流（`Es7210Rx::accepts_slower_poll()` 答 `false`），因为环只有 `sample()` 会排空——见[音频实现记录](@/records/iot/audio.md)。

`[实测]` 把所有平面关掉后这个地板仍在，因为它与需求无关：**一个什么都不给的设备，按下按钮仍然必须被响应。** 所以它是理论下限，不是调优空间。

`[实测]` 采集那一路在 `capture 0 ms` 时不是「变便宜」，而是**没有环可排空**——见[音频实现记录](@/records/iot/audio.md)里那条失效链。读这行时 `0 ms` 与 `1 ms` 的含义完全不同，前者是故障。

---

## 触摸坐标要过一次帧变换，`IDENTITY` 一直是错的

`[据称]` 这块模组的数字转换器报的是**面板控制器自己的 240×320 竖幅窗口**，而面板被写成 320×240 横幅（`MADCTL = MV|MX`）：两者差一次**转置 + 镜像**，正确的净变换是

```
raw(rx 0..240, ry 0..320)  →  fb(x, y) = (ry, 239 − rx)
```

在此之前给 FT6336 的是 `TouchMap::IDENTITY`，于是同一次点按被读成 `(rx, ry) = (239 − fy, fx)`：**列由手指的纵向位置决定、行由横向位置决定，列还是镜像的**。点 `7`（`pqrs`，格心 `(96,133)`）命中 `(106,96)`，也就是 `2`（`abc`）那一格；列表的 band 由 `fx` 决定（`(fx−47)/61`，纵向完全不参与）；灯页的物理上下滑被判成 `Left`/`Right` —— 改的是颜色，不是亮度。

依据两条，互相独立：

- 板厂对这块面板的触摸配置（wiki 第 9/11 章；例程 `08-lcd_lvgl` 与 `14-handheld` 是同一份，第三方 LVGL 移植博客亦然）：`.x_max = BSP_LCD_V_RES(240)`、`.y_max = BSP_LCD_H_RES(320)`、`swap_xy = 1`、`mirror_x = 1`、`mirror_y = 0`，配同一个显示配置（`swap_xy(true)` + `mirror(true, false)`，也就是 `MV|MX` = 本仓的 `MADCTL_LANDSCAPE = 0x60`）。
- 上游 `esp_lcd_touch.c` 的变换顺序是**先镜像（用 `x_max`/`y_max`，所以作用在原始轴上）再交换**，而 `esp_lcd_touch_ft5x06.c` 只把寄存器原样交出去；两条合起来正是 `(ry, 240 − rx)`。

所以这一侧的取值是 `bsp-esp::components::ft6336::LANDSCAPE_MAP`：`TouchMap { swap_xy: true, mirror_x: false, mirror_y: true }`。它名字里的 `mirror_y` 是**交换之后**的 `y`（`TouchMap::map` 先交换再镜像），跟板厂嘴里的 `mirror_x` 是同一件事；本仓用 `239 − rx` 而不是板厂的 `240 − rx`，因为后者把最上面一行落到了 framebuffer 之外。

`[测试]` 两条在 `bsp-esp/src/components/ft6336.rs`，跑在 `iot:test-bsp`（为此把 `ft6336` 加进该任务的 feature 列表——它是 bsp 里到那时唯一没有宿主机覆盖的驱动）：

- `a_finger_lands_where_the_panel_shows_it`：把「手指在 (96,133)」按逆变换写成原始坐标喂进 `read_sample`，断言交出来的还是 (96,133)。喂进去的一侧是照着 `LANDSCAPE_MAP` 反推的，所以把常量改回 `IDENTITY`，这条立刻红。
- `the_panels_own_frame_is_transposed_and_mirrored`：`(0,0)→(0,239)`、`(239,0)→(0,0)`、`(0,319)→(319,239)`、`(239,319)→(319,0)` 四个角。转置在任一轴的中段看不出来，只有角能钉住它。

这个错能活到今天，是因为**在此之前上板验过的触摸只有方向无关的点击**（页循环：点哪儿都算一次跃迁），而列表的每一行是整宽的一条——横向错位只有键盘的**列**会一眼暴露。

**还没验的**（软件合成不了触摸，只有 FT6336 产面板坐标）：点 `abc` 那一格必须出 `abc`、点 `7` 那一格必须出 `7`，灯页物理上下滑必须改亮度。

`[实测]` 这一版已经烧上 S3（bench 镜像，`IOT_PORT=/dev/cu.usbmodem1422301`）：`App/part. size: 880,112/6,291,456 bytes`、开机 `[IOT] boot page is Network`、无 panic，`[DISPLAY] Network 1 repaints in 5901 ms, held the executor 18 ms (0%)`——**所以板上跑的就是这一版**，上面那条只欠一次手指，不欠一次烧录。
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

`[测试]` `every_page_the_board_can_back_is_shown_on_the_way_round`（`core/src/state.rs`）早就在 host 上钉住了次序表，所以这条实测的价值不在于「次序对不对」，而在于**驱动、识别、意图、状态、渲染整条链在硬件上确实只走一格**。

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
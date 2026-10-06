+++
title = "音频实现记录"
weight = 40
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 音频实现记录

`apps/iot` 采集路径的实现判断与被否方案。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

每条判断标注依据：**`[测试]`** 有对应单元测试，**`[实测]`** 有硬件测量并附复现方式，**`[据称]`** 未复现、仅留痕。

---

## 静音必须读作"没变化"，否则整条 diff 链空转

`[测试]` `a_silent_capture_stops_reading_as_changed_once_the_ring_has_wrapped`（`core/src/drivers/audio.rs`）。

窗口写满之后 `cursor` 每轮都在推进，`#[derive(PartialEq)]` 于是把每一次推进都算成一次变化。实测：600 次连续静音轮询中，比较相等 **0 次**。

后果不是显示抖动，而是整条渲染 diff 管线在一个安静房间上按渲染节拍（20 ms）反复重跑，每轮都要重画整帧。诊断派生的 `Diagnostics` 内嵌 1.6 KB 音频包络，每次比较都是一次按值拷贝。

所以 `AudioEnvelope` 手写 `PartialEq`，只比**面板画得出来的那部分**：已提交列数与被绘制的列值，不比写指针。判据是「渲染层能看见的东西变了没有」——写指针变了而画面没变，不算变化。

---

## 采集积压判据的三档深度来自实测

`[测试]` `the_depth_a_healthy_capture_settles_at_is_not_called_a_backlog`（`core/src/drivers/audio.rs`）。

一次按时到达的轮询只等约一个轮询周期的音频；一个比链路固定落后一个节拍的循环，则**永远**比那个稳态再多深一个周期。因此把「两周期」当积压线，等于让一条从不积压的环每轮都报警。

实测中环一直健康时面板报出的深度是 `7_708 / 7_934 / 8_188` 字节（环容量 `24_576`）。`CaptureBacklog` 的告警/清除两档就落在这里面，一档一个轮询周期。

`[据称]` 该面板曾把整条 ring 判为已耗尽并重建 DMA，中途丢掉正在播的音频。未定位触发条件，仅留痕。

---

## dBA 表对原始表：让两次测量是同一个测量

`[测试]` `a_low_tone_drives_the_raw_columns_but_not_the_weighted_twin`、`a_mid_band_tone_reads_the_same_through_both_columns`、`spl_is_dbfs_with_the_microphones_own_reference_back`（`core/src/drivers/audio.rs`）。

A 计权 1 kHz 处为 0 dB，62.5 Hz 处约 −26 dB。于是两套列并存：面板的 scope 与 PK/RMS 行读**未加权**（那是 codec 交出来的样子），角落读数读 A 加权（那是能和别处 dB(A) 比的数字）。

`AWeight` 只跑一遍级联，包络的峰值列与加权列分别吸收**同一批**样本。第二遍级联在系数与零状态下只能得到同一个答案，而实测那让采集路径的每样本成本翻倍。

---

## 量化噪声会喂出 65 dBA 的假底噪

`[测试]` `a_quiet_floor_reads_quiet_instead_of_feeding_a_limit_cycle`（`core/src/drivers/audio.rs`）。

A 计权在低频几乎平坦，而滤波器极点靠近单位圆。定标器状态位宽足够宽时，量化噪声被自身放大并循环：一块只听得到自己 codec 底噪的板子会稳定读出约 65 dBA。

`STATE_FRACTION` 的宽度按这个上限反推——安静底噪（几十 LSB）必须读成几十 LSB，而不是一个自持的几百。

---

## 0 dBFS = 102 dB SPL 是这一颗麦克风的标定

`[实测]` 推导链与复测方法见[硬件约束记录](@/records/iot/hardware-constraints.md)。

核心测 dBFS，偏移量归板级所有（`SPL_OFFSET_DECIBELS` 在 `bsp-esp`）。`core` 不能依赖携带该器件的板，所以 `spl()` 把偏移量当参数收，板级传自己的值进来。

`[测试]` `spl_is_dbfs_with_the_microphones_own_reference_back` 顺带钉住两个读数的差值必须**全部**是那个偏移：中间多出任何东西，两个列就已经不是同一个测量了。
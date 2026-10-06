+++
title = "IoT 实现记录"
weight = 10
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# IoT 实现记录

Vanling 自有 ESP32 固件（`apps/iot`）的实现判断记录。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

对应的系统说明在[开发文档 / IoT 固件](@/development/iot/_index.md)。

| 章节                                                    | 内容                                                   |
| ------------------------------------------------------- | ------------------------------------------------------ |
| [放音实现记录](@/records/iot/playback.md)               | `Cp0Disabled` 调查、被否方案、feed 节拍归属、ISR 拆分  |
| [硬件约束记录](@/records/iot/hardware-constraints.md)   | 麦克风前端写序与标定、运动判定、栈容量由来             |
| [摄像头实现记录](@/records/iot/camera.md)             | GC2145 视野算术、bins 与标量实测、PSRAM 不可用由来、面板接线 |
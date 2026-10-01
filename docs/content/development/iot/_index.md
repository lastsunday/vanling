+++
title = "IoT 固件"
weight = 250
sort_by = "weight"
+++

# IoT 固件

Vanling 自有 ESP32 固件（`apps/iot`）的分层、组合与 Cargo feature 判据。

- [Cargo Feature 判据与内聚](@/development/iot/features.md) — 硬/软 feature 定义、引入四判据、内聚六规则、本次删除决策记录
- [分层与组合](@/development/iot/architecture.md) — 分层、三轴正交组合、渲染插拔、新增板/芯片流程
- [运动语义框架](@/development/iot/motion.md) — 数据面/语义面双平面、三 family 仲裁、能力声明、QMI8658A 寄存器绑定、新增来源
- [音频计量语义](@/development/iot/audio.md) — 采集契约、屏幕读数与刻度、对数映射的弦误差、释放斜率反推
- [放音语义](@/development/iot/playback.md) — 放音契约、一次性声音的相位、两种声音从哪来、一臂 20 ms 的 DMA 环、ES8311 寄存器行
- [固件安装](@/development/iot/flashing.md) — 浏览器一键安装、esptool/GUI、批量烧录与调试
- [无硬件仿真与回归冒烟](@/development/iot/emulation.md) — 宿主 harness + esp-emu、决策矩阵、console UART 约束
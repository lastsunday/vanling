+++
title = "共享客户端协议"
weight = 302
+++

# 共享客户端协议

Flutter App 与 IoT 固件是 server 的**同类客户端**：同为"base + 外设能力"的设备抽象，只是技术栈不同（Dart 手机端 vs Rust 嵌入式）。两者遵守同一份协议规格，本节即该规格的权威描述。

- Flutter App：手机设备（麦克风/扬声器/屏幕 + USB 外设遥测，Android 先行）
- IoT 固件：esp32c6 板级设备（板载传感器/OLED/音频外设，Cargo features 组合）

> 服务端协议与管线参考 `development/server/`（`websocket-protocol.md`、`architecture.md`、`dialogue-flow.md`）。

---

## 1. 客户端模型

```
客户端 = base（必选） + 外设能力（可选 feature 任意组合）
base   = 网络 / 配置 / HTTP 短事务遥测 + device_key / OTA
外设   = 语音（Opus + WS + 唤醒词） / 遥测（传感器/USB 外设） / 显示（OLED/LCD/屏幕渲染） / MCP 工具面
```

- 身份：客户端以 **device 身份**接入（OTA/激活流程签发 JWT，`token_type = device`），与 server 的 `device` 实体绑定，遥测归属设备维度。
- 能力协商：`hello` 携带当前能力集，server 据此组装管线节点（数据型不挂 VAD/ASR/LLM/TTS）。
- 传送通道：语音与指令走 WS（长连接，仅语音型/混合型需要）；遥测走 HTTP 短事务（任何客户端，与语音可并存）。

## 2. 语音会话（WS）

双方均需实现 `development/server/websocket-protocol.md` 的协议：

- `hello`：可带 `capabilities`（当前外设能力集）与 `audio_params`（Opus 采样率/帧长）
- `listen[start|stop|detect|text]`、`abort`、二进制 Opus 帧上行/下行
- 消费 `stt` / `llm`（含 `emotion`）/ `tts`（`sentence_start` 带字幕）/ `error` 文本帧
- `mcp`：device MCP 工具面（见 §5）

## 3. 会话状态与反馈（Attention 状态机）

会话状态（Attention）是唯一由协议驱动的事实源；输出通道为其可插拔投影，可取舍、可组合。

**作用域**：Attention 状态机仅存在于编译启用 `voice` 外设（语音型）的客户端。无语音设备没有会话状态机，只有设备本地页 + HTTP 数据路径，本协议对其为空。

### 输出通道

| 通道 | 说明 |
| --- | --- |
| `Display` | 以分页渲染（见下）；`chat` 页仅语音型存在，其余页本地自决 |
| `Led` | 状态色 + `mic_live`；采集音频须明确指示（隐私底线）；idle 与全状态形成对比基准 |
| `Audio` | 双子态：本地耳音 cue + TTS 引导语；与视觉 cue 同步；无屏设备至少以 Audio 指示状态转变 |
| `Vib` | 同状态机投影（含 timeout/error 告警）；适合可穿戴/触觉 |
| `Web` | 复用配网/遥测 HTTP 面：静态页 + REST 状态 + WS/SSE 实时；headless 设备以其为"头" |
| `None` | headless 空实现（合法通道） |

> `chat` 相关状态（listening/thinking/speaking）仅语音型客户端存在；其余状态/通道任何设备可选。
> 物理动作（机器人级体语，如 QUBI）为研究范畴，不在本协议范围。

### 全流程

输入通道（`Button`/`Touch`/`Mic`/`Web`，`Sensor` 仅进遥测、`Motion` 未来）→ 事件化（`listen_*`/`text`/`abort`/`close`）→ Attention 状态机（唯一事实源）→ 分发到输出通道，经输出通道回声（`mic_live`、`listening`）闭合。交互优先级：用户输入 > TTS > 告警 > 通知 > 媒体（AVS Interaction Model）。`microphone off` 为独立常驻隐私态，高于交互态；输入事件集不新增协议。客户端实现遵循 reducer 折叠（协议帧 ∪ 本地事件），chat 页消费投影而非原始帧（页面实现见 [`pages`](pages.md)）。

### Display 渲染形态：分页模型

显示按 **page 分页处理**：每个 page 拥有独立的内容模型与输入源，前台唯一、可切换。`status bar`（网络/电量/时钟）为栈外常驻层，`overlay`（通知/告警）为瞬时覆盖层。协议只驱动 `chat` 页，其余页本地自决——跨端只统一名词，不统一规则。

| Page | 内容模型 | 输入源 | 处理范式 |
| --- | --- | --- | --- |
| `chat` | 会话五态 + 消息 | 协议帧（唯一，仅语音型） | 帧→状态机→投影 |
| `home` | 遥测快照 | 本地轮询 / MCP 直查 | 读→渲染 |
| `config` | 配网/配置表单 | 本地事件/按钮 | 传统 UI |
| `ota` | 进度状态 | 本地事件 | 传统 UI |
| `settings` | 设置项 | 本地事件 | 传统 UI |

### chat 页（规范）

`chat` 仅语音型客户端存在，是唯一由服务器帧驱动的页面。帧是**输入事件**而非回放帧，不做"帧→像素"的逐帧直绘；由会话状态机重建语义，渲染器读取状态投影绘制。实现上页面**消费折叠后的投影而非逐帧直读**（帧 → reducer → 投影，见 [`pages`](pages.md)）。

**状态集**（单轴，与 `websocket-protocol.md` 会话状态机对齐）：

| 状态 | 进入 | 载荷 |
| --- | --- | --- |
| `idle` | 断线/超时/会话结束 | — |
| `connecting` | 建立 WS，hello 前 | — |
| `listening` | `listen_start` / `detect` | `message { role: user, text, is_interim }` |
| `thinking` | 本 turn LLM 处理中 | `message { role: assistant, text, is_interim }` + `emotion` |
| `speaking` | `tts.start` | `subtitle { text, speaking }` + `emotion` |

**转换规则**（最小集）：

- `stt` partial → 就地更新 `user` 消息（`is_interim=true`），不追加
- `llm` 流式 → 就地更新 `assistant` 消息（`is_interim=true`），`emotion` 随帧覆盖
- `tts.start` → `speaking`；`tts.sentence_start` 落 `subtitle {text, speaking=true}`
- `tts.stop` → 回 `listening`（realtime）/ `idle`（auto/manual）
- `abort`（wake_word_detected）→ 回 `listening` + 清空当前 `message`/`subtitle`（重算，不追帧）
- `close`/超时 → `idle` + 清空
- 幂等：同状态重复转换 = no-op（防闪烁）
- `speaking` 内可叠加本地媒体时钟做字幕/音频线性同步（播放性子模块），不改变事件驱动的主模型

**不进契约**：滚动/聚焦/防抖/动画/熄屏等渲染行为各端自决。

## 4. 遥测双路径

| 方向 | 触发 | 通道 | 归属 |
| --- | --- | --- | --- |
| ① 落盘采集 | 客户端主动、周期 | HTTP 短事务 + `device_key` | server 状态仓库（DB），离线可答 |
| ② 会话读取 | server 主动、语音会话内 | MCP `home_state` 本地工具 / device MCP 实时直查 | 会话内即席回答 |

- ①：Flutter（含 USB 外设采集）与 IoT（板载传感器）同构上报。
- ②：`home_state` 工具查询状态仓库（读 ① 数据）；device MCP `tools/call` 当场读传感器。
- 遥测**不承载于**语音 Session 管线，也不赖 WS 长连接。

## 5. MCP 工具面

`self.*` 工具同构，LLM 不关心工具运行于手机还是板子：

- Flutter：通知/定位/USB 外设控制
- IoT：灯/传感器/OLED/音频

## 6. 共同行为保证

- 重连：OTA → hello 周期，处理后 30s 空闲断开
- 冻结期：TTS 开始 250ms 内不抢答（`barge_in_lockout_ms`）
- 遥测降频：语音会话突发流量期间低优先

## 7. 测试归一

server 侧 WS 端到端测试作为两个客户端实现的共同回归基线；协议变更需双端同步。
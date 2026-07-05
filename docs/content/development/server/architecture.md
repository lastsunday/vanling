+++
title = "核心架构"
weight = 200
+++

# 核心架构

## 会话概览

- 使用session + round进行管理，session为一次的连接，round为每轮对话。
- session为抽象的会话，不绑定某种协议（Websocket等）,通过accept_frame和output_frame作为frame信息的入口和出口，并抽象帧结构，如Hello,Listen,LLM,TTS等。
- 为了便于frame的收集以及对话的连续开启，中断，采用双round模式，分别为shadow round和running round。

```txt
ws -> input proxy -> session -> output proxy -> ws

input proxy <- record frame
output proxy <- record frame
```

### Session

#### 入口和出口

```txt
accept_frame

output_frame
  output_controller
```

TODO

#### 抽象Frame

TODO

#### 状态机

- on_idle
  new_round

- on_ready
  new_round
  stop_round

- on_listening

- on_speaking

TODO

### Round

TODO

### Output Controller

TODO

### 活动时间

TODO

### 中断

TODO

### Frame收集

TODO

+++
title = "服务端实现记录"
weight = 20
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# 服务端实现记录

Rust 服务端（`apps/server`）的调查与演进记录。**AI 产出，未经人类审阅**，见[上级说明](@/records/_index.md)。

系统现在是什么样，见[开发文档 / 服务端](@/development/server/_index.md)。

| 记录 | 内容 |
| --- | --- |
| [TODO 分流记录](@/records/server/todo-triage.md) | 为什么删掉独立 TODO、每一类去了哪、哪些结论已过期或不再成立、唯一留下的守门测试 |
| [管道演进记录](@/records/server/pipeline-evolution.md) | 单链 + 观察者取代了哪些结构、被替换的形态对照 |
| [TTS 播放卡顿与音量归一化](@/records/server/audio-debug.md) | 实时 TTS 播放几秒后卡顿与电流声的调查与根因；音量归一化两套方案的实测对比，以及它现在没接入管线 |

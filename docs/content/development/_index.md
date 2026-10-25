+++
title = "开发文档"
weight = 30
sort_by = "weight"
+++

# 开发文档

按"要动哪一部分"组织的系统说明：这里只写**现在是什么样**。怎么变成这样、被否过什么，在[开发记录](@/records/_index.md)。

阅读路径：

1. [服务端](@/development/server/_index.md) — 先核心架构，再管道与对话流程，最后按需要查协议和模型部署。
2. [客户端](@/development/clients/_index.md) — 跨端语义只有一份，其余按端展开。
3. [IoT 固件](@/development/iot/_index.md) — 先分层，再 feature 判据，然后才是烧录与各路语义。

按需查阅：

- [调试](@/development/debugging/_index.md) — ASR 的基准与调试入口、TTS 音质评估
- [模型下载器](@/development/downloader.md) — manifest、下载与校验
- [相关项目](@/development/related-project/_index.md) — 上游实现里值得对照的调用链
- [资源](@/development/resource/_index.md) — 测试样本的实测属性

## 多语文档

改动 `docs/` 下中文页面后，需同步翻译为 `.en.md`，并执行 `git hash-object <source.md>` 更新其 front matter 中的 `source_file_hash` 与 `translated_at`。

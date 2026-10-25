+++
title = "TODO 分流记录"
weight = 401
sort_by = "weight"
+++

<!-- doc-audience: ai -->

# TODO 分流记录

## 问题

`development/server/TODO.md` 曾是服务端的开工入口：一张 86 行的表，自带宽高优先级列。它与 `roadmap/_index.md` 是同一根轴的两份副本：

- 两边都有优先级列，且互相打架；`roadmap` 的列说明写着"优先级由人工评估"，TODO 却没写来源。
- 位置列天生腐烂。`api/src/ws/default_listener.rs`、`api/src/mcp/mcp_host.rs` 这些路径在重构后已不存在，行却还挂着。
- 18 条"建议: 新 crate"写在无人裁决的表里，等于隐形批准。

文档不是状态机。待办需要跟踪的是"独立变化的状态"，所以它该由能自动报告状态的东西承载。

## 分流去向

86 行数据按四类处理：

| 类 | 行数 | 去向 |
| --- | --- | --- |
| 未完成 | 73 | 仍然成立的并入 [roadmap](@/roadmap/_index.md) 对应模块表，净新增 46 行；其余是与 roadmap 已有行同义、或已由架构解决的重复项 |
| 已完成 | 11 | git log；roadmap 里已有对应的 ✅ 行，不重复搬 |
| 丢弃 | 2 | 本页"被丢弃的结论" |
| 建议: 新 crate | 18 | [依赖候选](@/discussions/dependency-candidates.md)，标"未批准" |

## 被丢弃的结论

| 项 | 丢弃原因 |
| --- | --- |
| Hann 窗 crossfade 防 click（512 samples） | 输入 PCM 本身连续，chunk 边界不产生不连续；加窗只会改变能量包络。2026-07-23 否 |
| 首包静音裁剪 + 40 ms preroll | 与两阶段流式架构不兼容：首包是在 token 到达时立即发出的，裁剪需要先看到后续音频，等于把首包推迟到句末。2026-07-23 否 |

## 已过期、不再成立的项

这些行留在 TODO 里会误导，核查后确认不成立：

| 项 | 核查结果 |
| --- | --- |
| MCP 认证 | 已实现。`component/mcp/mod.rs` 的 `/mcp` 路由挂了 `get_auth_layer()`，security 配置另有 per-principal 限流 |
| 音频热路径克隆 | 原 `default_listener.rs` 已删除。现 `OpusDecodeNode` 每 20 ms 解码一次、每 VAD 窗切一次 `to_vec`（60 次/秒），不是原描述的热路径 |
| 时钟溢出 | `service/src/session/` 已无 `Local::now()` 参与超时计算 |
| VAD 非 16 kHz | `OpusDecodeNode` 把所有配置采样率经 rubato FFT 统一到 16 kHz，链上没有第二个音频入口，VAD 拿不到非 16 kHz 输入 |
| MCP 锁顺序 ABBA | 原 `api/src/mcp/mcp_host.rs` 已不存在，现组件没有跨实例锁；已作为"重新引入多端聚合前复查"写入 roadmap，不再保留为缺陷行 |

## 没有转成测试的项

原计划把三条转成 `#[ignore]` 测试，实际只留下一条：

| 项 | 处置 |
| --- | --- |
| Opus 采样率为 0 | 已加 `opus_configure_zero_sample_rate_does_not_panic`（`api/tests/pipeline_nodes_test.rs`，`#[ignore]`）。`Configure(sample_rate = 0)` 会在 `ropus::Decoder::new(0, …)` 的 `unwrap` 处 panic，测试断言"不应 panic"，修好后去掉 `#[ignore]` 即可 |
| VAD 非 16 kHz | 不成立（见上），没有可断言的失败路径 |
| TTS 尾部 hold buffer | 是功能缺失而非行为缺陷，已作为 roadmap 的"尾部 Hold Buffer"行保留；写一条失败的 ignored 测试不能说明何时算修好 |

## 留下的规则

- **待办只有一处优先级权威**：`roadmap/_index.md`。代理开工读它，不再有第二张表。
- **能被断言的行为缺陷**用 `#[ignore = "原因"]` 测试承载；测试会随重构一起报告 bug 还在不在。
- **"为什么放弃"** 进本目录，不进 roadmap。
- **新依赖**先过[依赖候选](@/discussions/dependency-candidates.md)，由人批准后才动 `Cargo.toml`。

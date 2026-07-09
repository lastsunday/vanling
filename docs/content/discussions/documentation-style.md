+++
title = "协议文档三层模型与人机分工"
weight = 100
+++

# 协议文档三层模型与人机分工

## 背景

AGENTS.md §6.1 定义了协议/流程类文档的编写规则——分概览层、图示层、参考层，其中概览层必须由人手写。但原有规则只给了指令，没有说明"为什么"。

在后续讨论中，我们对"为什么概览层必须人手写"进行了更深入的分析，发现其核心原因不是通常认为的"人类更准确"或"人类署名建立信任"，而是另一个更本质的机制。

## 讨论

### 核心发现：思维链暴露

人手写概览的真正价值在于——人类写作的每个选择（选哪几条消息、按什么顺序、略过哪些分支、用多粗的粒度）都在**暴露作者对这个系统的思维模型**。

另一个人类读者沿这条推理路径走一遍，就能重建作者的认知地图。这种**认知对齐**是理解和信任的真正来源。

这不是"人类比 AI 更精确"的问题——手写概览本身可以是不精确的、有瑕疵的。关键是有人真实地做了取舍判断，并且这些判断痕迹留在了文档里。

### AI 为什么无法替代

Anthropic 的研究表明，AI 的 chain-of-thought（CoT）本质上是 **post-hoc rationalization**——它生成的是"听起来合理的推理路径"，而不是反映内部实际计算过程的思维链（见 Tracing the Thoughts of a Large Language Model, Anthropic, 2025）。当 AI 写概览时，不存在一个真实的"作者做了取舍"的过程可以追溯。

### 认知回路的丢失

Tandem Health（2026）在临床笔记场景中观察到类似现象：*"When a clinician writes a note manually, the act of writing is itself a form of verification. Each sentence requires active recall and deliberate choice of language. When an AI assistant generates the note, that cognitive loop is bypassed, and with it goes some of the felt certainty."*

下笔即推理。写作行为本身就是验证过程。把这个过程交给 AI，转交的不只是"写"的动作，还有读者对推理过程的可见性。

### 结论

| 层 | 作者 | 为什么 |
|------|------|--------|
| 概览 | 人手写 | 暴露思维链，建立认知对齐 |
| 图示 | AI 从概览生成 | 视觉翻译是机械工作，不涉及取舍判断 |
| 参考 | AI 草稿 + 人验证 | 细节填写 AI 更快，但具体值必须对照产品验证 |

## 参考资料

- Anthropic (2025). *Tracing the thoughts of a large language model*. https://www.anthropic.com/research/tracing-thoughts-language-model
- Anthropic (2025). *Attribution Graphs*. https://transformer-circuits.pub/2025/attribution-graphs/methods.html
- Tandem Health (2026). *Building trust in AI-generated clinical notes*. https://tandemhealth.ai/resources/knowledge/building-trust-in-ai-generated-clinical-notes
- Parshakov et al. (2025). *Human-AI Collaboration in Content Evaluation*. arXiv:2504.10961
- Zhou & Fang (2026). *Understanding user trust in AI-generated content*. Online Information Review, 50(1), 171-188.

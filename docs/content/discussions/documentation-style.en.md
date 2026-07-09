+++
title = "Protocol Documentation Three-Layer Model and Human-AI分工"
weight = 100
[extra]
source_file_hash = "9ae07a4c7d7c7920db52c3ca4546ba66176c202b"
translated_at = "2026-06-28T18:00:00Z"
+++

# Protocol Documentation Three-Layer Model and Human-AI Division of Labor

## Background

AGENTS.md §6.1 defines the writing rules for protocol/process documentation — it is divided into an overview layer, a diagram layer, and a reference layer, with the overview layer required to be written by humans. However, the original rules only gave instructions without explaining "why."

In subsequent discussions, we conducted a deeper analysis of "why the overview layer must be written by hand" and found that the core reason is not the commonly assumed "humans are more accurate" or "human attribution builds trust," but rather a more fundamental mechanism.

## Discussion

### Core Finding: Chain-of-Thought Exposure

The real value of a human-written overview lies in the fact that every choice a human makes while writing (which messages to pick, in what order, which branches to omit, how coarse the granularity should be) **exposes the author's mental model of the system**.

When another human reader follows this reasoning path, they can reconstruct the author's cognitive map. This **cognitive alignment** is the true source of understanding and trust.

This is not a question of "humans being more precise than AI" — hand-written overviews can themselves be imprecise or flawed. The key is that a real person made judgment calls, and the traces of those judgments remain in the document.

### Why AI Cannot Replace This

Research from Anthropic shows that AI chain-of-thought (CoT) is fundamentally **post-hoc rationalization** — it generates a "plausible-sounding reasoning path" rather than reflecting the internal computational process of the model (see Tracing the Thoughts of a Large Language Model, Anthropic, 2025). When an AI writes an overview, there is no real "author's decision-making" process to trace.

### Loss of the Cognitive Loop

Tandem Health (2026) observed a similar phenomenon in clinical note-taking: *"When a clinician writes a note manually, the act of writing is itself a form of verification. Each sentence requires active recall and deliberate choice of language. When an AI assistant generates the note, that cognitive loop is bypassed, and with it goes some of the felt certainty."*

Writing is reasoning. The act of writing is itself a verification process. Delegating this to AI transfers not just the "writing" action, but also the reader's visibility into the reasoning process.

### Conclusion

| Layer | Author | Why |
|-------|--------|-----|
| Overview | Human-written | Exposes chain-of-thought, establishes cognitive alignment |
| Diagram | AI-generated from overview | Visual translation is mechanical, involves no judgment calls |
| Reference | AI draft + human verification | AI is faster at filling in details, but specific values must be verified against the product |

## References

- Anthropic (2025). *Tracing the thoughts of a large language model*. https://www.anthropic.com/research/tracing-thoughts-language-model
- Anthropic (2025). *Attribution Graphs*. https://transformer-circuits.pub/2025/attribution-graphs/methods.html
- Tandem Health (2026). *Building trust in AI-generated clinical notes*. https://tandemhealth.ai/resources/knowledge/building-trust-in-ai-generated-clinical-notes
- Parshakov et al. (2025). *Human-AI Collaboration in Content Evaluation*. arXiv:2504.10961
- Zhou & Fang (2026). *Understanding user trust in AI-generated content*. Online Information Review, 50(1), 171-188.

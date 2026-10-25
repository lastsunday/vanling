+++
title = "Debugging"
weight = 300
sort_by = "weight"
[extra]
source_file_hash = "adb115cf87fef00c7df5ed11c66866daee491d81"
translated_at = "2026-10-25T00:00:00Z"
+++

# Debugging

Diagnostic tools and benchmarks: they answer "what does this stage actually output right now" and do not describe system design.

- [ASR Speech Recognition](@/development/debugging/asr-debug.en.md) — debugging entry point and benchmarks for the sherpa-onnx recognizer
- [TTS Audio Quality Evaluation](@/development/debugging/tts-tools.en.md) — synthesize → decode → three-axis scoring, for comparing models and tuning speaking rate

The semantics of VAD and turn detection live in [Server / VAD and Turn Detection](@/development/server/vad.en.md).

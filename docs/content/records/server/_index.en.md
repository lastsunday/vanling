+++
title = "Server Implementation Records"
weight = 20
sort_by = "weight"
[extra]
source_file_hash = "e8586853f34670b318467bd2e89545fe8e3a5146"
translated_at = "2026-10-25T00:00:00Z"
+++

<!-- doc-audience: ai -->

# Server Implementation Records

Investigations and evolution of the Rust server (`apps/server`). **AI-produced, not human-reviewed** — see the [parent note](@/records/_index.en.md).

For what the system is now, see [development docs / Server](@/development/server/_index.en.md).

| Record | Contents |
| --- | --- |
| [TODO triage record](@/records/server/todo-triage.en.md) | Why the standalone TODO went away, where each class went, which conclusions expired or no longer hold, and the single guard test left behind |
| [Pipeline evolution record](@/records/server/pipeline-evolution.en.md) | What the single chain + observer replaced, and the before/after comparison |
| [TTS playback stutter and loudness normalisation](@/records/server/audio-debug.en.md) | Investigation and root cause of stutter plus static a few seconds into real-time TTS; measured comparison of two loudness approaches, and why it is not wired into the pipeline |

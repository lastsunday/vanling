+++
title = "Dependency Candidates"
weight = 30
sort_by = "weight"
[extra]
source_file_hash = "0213b2fa6af7a9e7f63c1f83d7700abc39d207aa"
translated_at = "2026-10-25T00:00:00Z"
+++

# Dependency Candidates

External dependencies mentioned while the server evolved but **not yet approved**. This page records candidates and open questions; it is not a selection decision. Per [AGENTS.md](https://github.com/anomalyco/vanling/blob/main/AGENTS.md), adding a crate requires human agreement first.

| Candidate | Serves | Status | Open question |
| --- | --- | --- | --- |
| `nanoid` | Short IDs for Invite Codes | Not adopted; invite codes themselves are unimplemented | Is a short ID worth one more dependency, given its collision odds |
| `tower-governor` + `governor` | Rate limiting (GCRA) | A hand-rolled fixed window + login-failure lock already covers the feature | Would GCRA's smoothness justify replacing the current implementation |
| `qdrant` | Personal memory / RAG | Not adopted; `rig-core` already supports several vector backends | Self-hosted or embedded, and who carries the operational cost |
| `wasmtime` | Plugin system | Not adopted | WASI P2 ABI stability and per-plugin runtime cost |
| `rumqttc` | MQTT gateway / device-to-device calls | Not adopted; WebSocket is the only transport today | Add MQTT as a second transport, or stay WebSocket-only |
| `rodio` | Music playback | Not adopted | Playback on the server or the device, and how server playback coexists with TTS output |
| `tokio-cron-scheduler` | Timer / reminder / alarm | Not adopted | Is in-memory single-node scheduling enough, and must jobs survive a restart |
| `csm.rs` (AGPL-3.0) | Conversational prosody TTS | Not adopted | AGPL compatibility with this project's licence is unassessed |
| `voirs-emotion` | Emotion-adaptive tone | Not adopted; `sherpa-onnx` already ships SER models | It overlaps the existing SER work; is only a mapping layer missing |
| `webrtc-rs` | WebRTC real-time audio/video | Not adopted | Add it as a third transport, and who deploys ICE/TURN |
| `aec3` | Server-side echo cancellation | Not adopted; AEC currently runs on the device | With AEC already on the device, what does a second pass buy |
| `polyvoice` | Speaker diarization | Not adopted; `sherpa-onnx` has speaker capabilities too | It overlaps; maybe only the model list needs extending |
| `web_socket_channel` | Flutter App WebSocket | Not adopted; App-side WS is not wired in yet | Has the Flutter transport choice been made |

These are **not** new dependencies, only capability extensions of ones already present, so they need no approval here:

| Existing dependency | Capabilities it can extend to |
| --- | --- |
| `sherpa-onnx` | Piper/VITS models (multilingual TTS), ECAPA-TDNN/WeSpeaker (voiceprint), speaker embeddings (voice cloning), SER (emotion) |
| `rig-core` | Vector-store backends (RAG / memory), Agent/Chain/Router (orchestration) |
| `rmcp` | Tool market, per-agent tool filtering, remote MCP |

Layered turn detection and VAD tuning mentioned LiveKit `v1-mini` and OpenAI's configuration pattern; those are model/configuration references rather than crates, and whether they become Layer 2/3 is still open. See [Roadmap](@/roadmap/_index.en.md) Session / Listener.

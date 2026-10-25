+++
title = "TTS Playback Stutter and Loudness Normalisation"
weight = 402
[extra]
source_file_hash = "5cfc65e65e30c7356186c5900cfb7d17861a438f"
translated_at = "2026-10-25T00:00:00Z"
+++

<!-- doc-audience: ai -->

# TTS Playback Stutter and Loudness Normalisation

## Symptom

Real-time WebSocket TTS playback: fine at first, then obvious **stuttering** and **static** a few seconds in. WAV files exported offline are perfectly clean.

## Investigation

### 1. Rule out the server audio pipeline first

WAVs produced by the session integration test play cleanly → the TTS → resample → Opus → WebSocket chain itself is fine. The problem is either the **send cadence** or **client consumption**.

### 2. Rule out each tunable

| Change | Effect |
| --- | --- |
| `opus-rs` → `opus` crate (C binding) | None |
| Output sample rate 16000 → 24000 Hz | None |
| `sherpa-onnx::LinearResampler` → `rubato::FftFixedIn` | None |
| `Application::LowDelay` → `Application::Audio` + FEC | None |
| `Signal::Auto` → `Signal::Voice` + `Bandwidth::Superwideband` | None |
| VBR → CBR (`set_vbr(false)`) | None |
| Old pacing: elapsed relative time → absolute target time | None |

No encoder setting helped, which means the distortion is not introduced by encoding; the player simply is not getting data on time.

### 3. Client code

Three facts under `apps/server-ui/public/test/device/js/core/audio/`:

- `player.js` calls `_opus_decode` **synchronously through WASM**, blocking the JS main thread while decoding;
- `player.js` pulls packets from the queue in batches (inner loop up to 99 packets);
- `stream-context.js` schedules through the Web Audio API chained style: `source.start(max(scheduledEndTime, currentTime))`, 120 ms per segment.

Once the main thread stalls on a long decode, `scheduledEndTime` is in the past and the next segment cannot attach — audible as a gap plus noise.

## Root cause

TTS first runs roughly 13 s of inference for a whole sentence, then slices the PCM into 228 frames (60 ms each) that all arrive at the sender at once. The old pacing derived absolute targets from `audio_start_time` (the TTS::Start instant):

```
target = audio_start_time + paced_index × 60ms
```

By the time the frames arrive 13 s later, every `target` is in the past, so `now > target` always holds → never sleep → **all 228 frames burst out within 10 ms**. The client receives 200+ packets at once, decodes them in one blocking pass for over a second, and the Web Audio schedule breaks.

Offline WAVs are unaffected because there is no real-time scheduling constraint: every frame is decoded and concatenated in order, so a blocking decode has no audible consequence.

## Fix

The idea is to replace "absolute deadline" with "relative beat": after the first frame, space every frame a fixed interval from **the current instant**, and drop missed beats instead of catching up. Send cadence then depends only on real consumer progress, not on when TTS started inferring.

The current implementation is the output loop in `service/src/session/mod.rs` (the `audio_pacer`): on the first audio message it lazily creates `interval_at(now + frame_duration, frame_duration)`, then awaits `pacer.tick()` per frame; any non-audio message clears the pacer so the next audio segment starts a fresh beat. Frame length comes from `AudioSpec::frame_duration_ms`, not a hardcoded 60 ms.

Two differences from the original patch worth knowing if you touch this: the original set `MissedTickBehavior::Skip` explicitly to discard backlog, the current code uses the default policy; and the original sent the first frame immediately, whereas the first `tick()` now waits one frame duration. With audio arriving continuously the two are equivalent, but **the burst path no longer has backlog protection**.

Result: the initial burst went from 228 frames to about one, steady state is one frame per packet, client batch decoding dropped from 200+ frames to 1, and main-thread blocking from >1 s to <1 ms.

## Symptom two: inconsistent loudness

In mixed Chinese/English speech the trailing English is clearly louder, which sounds disjointed.

### Approach 1: fixed-parameter DRC (abandoned)

A conventional feed-forward compressor with peak-detector envelope and optional soft knee. Parameters came from grid search plus EBU R128 metrics: threshold −28 dB, ratio 6, knee 0, attack 5 ms, release 80 ms, makeup 8 dB. LRA dropped from 9.58 to 5.66 LU, but Crest Factor fell from 15.4 to 6.2 dB — loudness evened out, dynamics gone, perceived as "muffled".

### Approach 2: adaptive RMS gain normalisation

`adaptive_normalize()` (`api/src/util/compressor.rs`) in six steps:

| Step | What it does |
| --- | --- |
| Frame RMS | 200 ms window, 10 ms hop |
| Target loudness | p30 over all frame RMS values |
| Frame gain | `target_rms / frame_rms`, clamped to ±12 dB |
| Gain smoothing | attack 5 ms / release 300 ms, per-sample IIR |
| Global compensation | match the original RMS, then +3 dB |
| Soft limiting | −0.5 dBFS ceiling to prevent clipping |

Metric comparison:

| Metric | Raw | Adaptive |
| --- | --- | --- |
| LRA | 9.08 LU | 1.59 LU (−82%) |
| LUFS | −26.90 | −24.14 (about 3 dB louder) |
| Crest Factor | 15.3 dB | 14.7 dB (dynamics preserved) |

Total overhead for 10 s of audio is about 10 ms (O(n) framing, sorting ~1000 frames, per-sample IIR) — negligible next to seconds of TTS inference.

## Current state

`adaptive_normalize()` is **not called by the pipeline** (the repository has the definition and no callers); `pcm_compress()`, `grid_search_compressor()` and `evaluate_compressed()` are likewise only used inside `compressor.rs`. The comparison helpers of the time, `test_compare_raw_vs_processed` and `test_grid_search_compressor`, no longer exist, so the numbers above are measurements from that period and cannot be reproduced with the current code. Reintroducing the approach requires writing a runnable analysis case first.

## Key files

| File | Role |
| --- | --- |
| `apps/server/service/src/session/mod.rs` | `audio_pacer`: per-frame send cadence (where the fix lives) |
| `apps/server/api/src/component/tts/model/matcha/mod.rs` | MatchaTts audio generation |
| `apps/server/api/src/util/compressor.rs` | `adaptive_normalize()` / `pcm_compress()` / `evaluate_compressed()`, currently uncalled |
| `apps/server-ui/public/test/device/js/core/audio/player.js` | Client Opus decoding and batched dequeue |
| `apps/server-ui/public/test/device/js/core/audio/stream-context.js` | Web Audio API chained scheduling |
